//! Token format and wire types shared by `clab` and the license server.

use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::fmt;

pub use ed25519_dalek;

/// What the server vouches for. The token is only issued for a valid license.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub license_id: i64,
    pub licensee: String,
    pub install_id: String,
    pub issued_at: u64,
    /// The end of the grace period: an unreachable server is tolerated until then.
    pub valid_until: u64,
}

/// `payload` is the JSON of [`Claims`]; `signature` signs exactly those bytes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Token {
    pub payload: String,
    pub signature: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TokenError {
    Malformed,
    Signature,
}

impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "license token is malformed",
            Self::Signature => "license token signature is invalid",
        })
    }
}

impl std::error::Error for TokenError {}

pub fn sign(key: &SigningKey, claims: &Claims) -> Token {
    let payload = serde_json::to_string(claims).expect("claims serialize");
    let signature = BASE64.encode(key.sign(payload.as_bytes()).to_bytes());
    Token { payload, signature }
}

pub fn verify(key: &VerifyingKey, token: &Token) -> Result<Claims, TokenError> {
    let signature = BASE64
        .decode(&token.signature)
        .ok()
        .and_then(|bytes| Signature::from_slice(&bytes).ok())
        .ok_or(TokenError::Malformed)?;
    key.verify(token.payload.as_bytes(), &signature)
        .map_err(|_| TokenError::Signature)?;
    serde_json::from_str(&token.payload).map_err(|_| TokenError::Malformed)
}

/// The public key as shown by the server and embedded in clients.
pub fn encode_public_key(key: &VerifyingKey) -> String {
    BASE64.encode(key.as_bytes())
}

pub fn decode_public_key(text: &str) -> Option<VerifyingKey> {
    let bytes: [u8; 32] = BASE64.decode(text.trim()).ok()?.try_into().ok()?;
    VerifyingKey::from_bytes(&bytes).ok()
}

/// A random ID that names one installation; it carries no machine data.
pub fn new_install_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("system randomness");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// First start of an installation: the server creates a license for it.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub install_id: String,
    /// Salted hash of the OS machine ID; only the hash leaves the machine.
    pub fingerprint: String,
    pub hostname: String,
    pub version: String,
}

/// The key is needed later for `validate`; the server stores it only hashed.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub key: String,
    pub token: Token,
}

/// Binds a manually issued key to this installation.
#[derive(Debug, Serialize, Deserialize)]
pub struct ActivateRequest {
    pub key: String,
    pub install_id: String,
    pub fingerprint: String,
    pub hostname: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateRequest {
    pub key: String,
    pub install_id: String,
    pub hostname: String,
    pub version: String,
}

/// Used for `deactivate`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SeatRequest {
    pub key: String,
    pub install_id: String,
}

/// Why the server refused. HTTP 403 means the license is not usable and a client
/// drops its token; any other status only means the server could not answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// Unknown or revoked key.
    InvalidKey,
    /// Another installation holds the seat.
    SeatInUse,
    /// The key is not bound to this installation.
    NotActivated,
    /// A field is missing or too long (HTTP 400).
    InvalidRequest,
    /// The server accepts no new registrations (HTTP 403).
    RegistrationClosed,
    /// Too many registrations from one address (HTTP 429).
    RateLimited,
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidKey => "the license key is unknown or revoked",
            Self::SeatInUse => "the license is already activated on another machine",
            Self::NotActivated => "the license is not activated on this machine",
            Self::InvalidRequest => "the request was rejected as invalid",
            Self::RegistrationClosed => "the server accepts no new installations",
            Self::RateLimited => "too many requests from this address; try again later",
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorCode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn claims() -> Claims {
        Claims {
            license_id: 7,
            licensee: "Ada".into(),
            install_id: "abc".into(),
            issued_at: 100,
            valid_until: 200,
        }
    }

    #[test]
    fn verifies_what_was_signed() {
        let token = sign(&key(1), &claims());
        assert_eq!(verify(&key(1).verifying_key(), &token), Ok(claims()));
    }

    #[test]
    fn rejects_a_changed_payload() {
        let mut token = sign(&key(1), &claims());
        token.payload = token.payload.replace("200", "999");
        assert_eq!(
            verify(&key(1).verifying_key(), &token),
            Err(TokenError::Signature)
        );
    }

    #[test]
    fn rejects_another_signer() {
        let token = sign(&key(2), &claims());
        assert_eq!(
            verify(&key(1).verifying_key(), &token),
            Err(TokenError::Signature)
        );
    }

    #[test]
    fn rejects_a_malformed_signature() {
        let mut token = sign(&key(1), &claims());
        token.signature = "not base64!".into();
        assert_eq!(
            verify(&key(1).verifying_key(), &token),
            Err(TokenError::Malformed)
        );
    }

    #[test]
    fn public_key_round_trips() {
        let public = key(3).verifying_key();
        assert_eq!(decode_public_key(&encode_public_key(&public)), Some(public));
        assert_eq!(decode_public_key("short"), None);
    }

    #[test]
    fn install_ids_differ() {
        assert_ne!(new_install_id(), new_install_id());
        assert_eq!(new_install_id().len(), 32);
    }
}
