use clab_license::ed25519_dalek::SigningKey;
use clab_license::{Claims, ErrorCode, Token, sign};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

const KEY_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const MAX_KEY: usize = 64;
const MAX_INSTALL_ID: usize = 64;
const MAX_HOSTNAME: usize = 255;

pub enum Failure {
    Denied(ErrorCode),
    Internal(rusqlite::Error),
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Internal(error)
    }
}

impl From<ErrorCode> for Failure {
    fn from(code: ErrorCode) -> Self {
        Self::Denied(code)
    }
}

pub struct Listed {
    pub id: i64,
    pub person: String,
    pub revoked: bool,
    pub hostname: Option<String>,
    pub activated_at: Option<i64>,
}

struct Row {
    id: i64,
    person: String,
    revoked: bool,
    install_id: Option<String>,
}

/// Licenses, their seats and the signing of tokens. The only place that decides who may run.
pub struct Store {
    connection: Connection,
    signing_key: SigningKey,
    grace_secs: u64,
}

fn normalize(key: &str) -> String {
    key.trim().to_ascii_uppercase()
}

/// Keys are stored hashed, so a leaked database does not leak usable keys.
fn hash(key: &str) -> String {
    format!("{:x}", Sha256::digest(normalize(key).as_bytes()))
}

fn bounded(value: &str, max: usize) -> Result<(), ErrorCode> {
    if value.is_empty() || value.len() > max {
        Err(ErrorCode::InvalidRequest)
    } else {
        Ok(())
    }
}

fn new_key() -> String {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes).expect("system randomness");
    let chars: Vec<char> = bytes
        .iter()
        .map(|byte| KEY_ALPHABET[usize::from(byte % 32)] as char)
        .collect();
    let groups: Vec<String> = chars
        .chunks(5)
        .map(|group| group.iter().collect())
        .collect();
    format!("CLAB-{}", groups.join("-"))
}

impl Store {
    pub fn open(path: &Path, signing_key: SigningKey, grace_days: u64) -> rusqlite::Result<Self> {
        Self::with(Connection::open(path)?, signing_key, grace_days)
    }

    fn with(
        connection: Connection,
        signing_key: SigningKey,
        grace_days: u64,
    ) -> rusqlite::Result<Self> {
        // The admin commands run next to the server and write the same file.
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch(
            "create table if not exists licenses (
                id integer primary key autoincrement,
                key_hash text not null unique,
                person text not null,
                revoked integer not null default 0,
                install_id text,
                hostname text,
                activated_at integer
            )",
        )?;
        Ok(Self {
            connection,
            signing_key,
            grace_secs: grace_days * 24 * 60 * 60,
        })
    }

    /// Returns the id and the key; the key is not recoverable later.
    pub fn create(&self, person: &str) -> rusqlite::Result<(i64, String)> {
        let key = new_key();
        self.connection.execute(
            "insert into licenses (key_hash, person) values (?1, ?2)",
            params![hash(&key), person],
        )?;
        Ok((self.connection.last_insert_rowid(), key))
    }

    pub fn revoke(&self, id: i64) -> rusqlite::Result<bool> {
        Ok(self
            .connection
            .execute("update licenses set revoked = 1 where id = ?1", [id])?
            > 0)
    }

    /// Frees the seat, for example after a machine change.
    pub fn unbind(&self, id: i64) -> rusqlite::Result<bool> {
        Ok(self.connection.execute(
            "update licenses set install_id = null, hostname = null, activated_at = null where id = ?1",
            [id],
        )? > 0)
    }

    pub fn list(&self) -> rusqlite::Result<Vec<Listed>> {
        self.connection
            .prepare(
                "select id, person, revoked, hostname, activated_at from licenses order by id",
            )?
            .query_map([], |row| {
                Ok(Listed {
                    id: row.get(0)?,
                    person: row.get(1)?,
                    revoked: row.get(2)?,
                    hostname: row.get(3)?,
                    activated_at: row.get(4)?,
                })
            })?
            .collect()
    }

    pub fn activate(
        &self,
        key: &str,
        install_id: &str,
        hostname: &str,
        now: u64,
    ) -> Result<Token, Failure> {
        bounded(key, MAX_KEY)?;
        bounded(install_id, MAX_INSTALL_ID)?;
        bounded(hostname, MAX_HOSTNAME)?;
        let row = self.usable(key)?;
        if row
            .install_id
            .as_deref()
            .is_some_and(|holder| holder != install_id)
        {
            return Err(ErrorCode::SeatInUse.into());
        }
        self.connection.execute(
            "update licenses set install_id = ?1, hostname = ?2, activated_at = coalesce(activated_at, ?3) where id = ?4",
            params![install_id, hostname, now as i64, row.id],
        )?;
        Ok(self.issue(&row, install_id, now))
    }

    pub fn validate(&self, key: &str, install_id: &str, now: u64) -> Result<Token, Failure> {
        let row = self.held(key, install_id)?;
        Ok(self.issue(&row, install_id, now))
    }

    pub fn deactivate(&self, key: &str, install_id: &str) -> Result<(), Failure> {
        let row = self.held(key, install_id)?;
        self.unbind(row.id)?;
        Ok(())
    }

    fn find(&self, key: &str) -> rusqlite::Result<Option<Row>> {
        self.connection
            .query_row(
                "select id, person, revoked, install_id from licenses where key_hash = ?1",
                [hash(key)],
                |row| {
                    Ok(Row {
                        id: row.get(0)?,
                        person: row.get(1)?,
                        revoked: row.get(2)?,
                        install_id: row.get(3)?,
                    })
                },
            )
            .optional()
    }

    /// A known, unrevoked license.
    fn usable(&self, key: &str) -> Result<Row, Failure> {
        match self.find(key)? {
            Some(row) if !row.revoked => Ok(row),
            _ => Err(ErrorCode::InvalidKey.into()),
        }
    }

    /// A usable license whose seat belongs to this installation.
    fn held(&self, key: &str, install_id: &str) -> Result<Row, Failure> {
        bounded(key, MAX_KEY)?;
        bounded(install_id, MAX_INSTALL_ID)?;
        let row = self.usable(key)?;
        if row.install_id.as_deref() == Some(install_id) {
            Ok(row)
        } else {
            Err(ErrorCode::NotActivated.into())
        }
    }

    fn issue(&self, row: &Row, install_id: &str, now: u64) -> Token {
        sign(
            &self.signing_key,
            &Claims {
                license_id: row.id,
                licensee: row.person.clone(),
                install_id: install_id.to_owned(),
                issued_at: now,
                valid_until: now + self.grace_secs,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clab_license::verify;

    const NOW: u64 = 1_000;
    const DAY: u64 = 86_400;

    fn store() -> Store {
        Store::with(
            Connection::open_in_memory().unwrap(),
            SigningKey::from_bytes(&[9; 32]),
            7,
        )
        .unwrap()
    }

    fn denied<T>(result: Result<T, Failure>) -> ErrorCode {
        match result {
            Err(Failure::Denied(code)) => code,
            _ => panic!("expected a denial"),
        }
    }

    #[test]
    fn activation_issues_a_verifiable_token_with_the_grace_period() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        let token = store
            .activate(&key, "install-a", "ada-pc", NOW)
            .ok()
            .unwrap();
        let claims = verify(&store.signing_key.verifying_key(), &token).unwrap();
        assert_eq!(
            claims,
            Claims {
                license_id: id,
                licensee: "Ada".into(),
                install_id: "install-a".into(),
                issued_at: NOW,
                valid_until: NOW + 7 * DAY,
            }
        );
    }

    #[test]
    fn keys_are_matched_without_case_or_padding_and_stored_hashed() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert!(
            store
                .activate(&format!(" {} ", key.to_lowercase()), "a", "pc", NOW)
                .is_ok()
        );
        let stored: String = store
            .connection
            .query_row("select key_hash from licenses", [], |row| row.get(0))
            .unwrap();
        assert!(!stored.contains("CLAB"));
    }

    #[test]
    fn repeating_activation_on_the_same_install_works() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert!(store.activate(&key, "a", "pc", NOW).is_ok());
        assert!(store.activate(&key, "a", "pc", NOW + 5).is_ok());
    }

    #[test]
    fn one_seat_per_key() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert!(store.activate(&key, "a", "pc", NOW).is_ok());
        assert_eq!(
            denied(store.activate(&key, "b", "laptop", NOW)),
            ErrorCode::SeatInUse
        );
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert_eq!(
            denied(store().activate("CLAB-NOPE", "a", "pc", NOW)),
            ErrorCode::InvalidKey
        );
    }

    #[test]
    fn oversized_fields_are_rejected() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        let long = "x".repeat(MAX_HOSTNAME + 1);
        assert_eq!(
            denied(store.activate(&key, "a", &long, NOW)),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            denied(store.activate(&key, "", "pc", NOW)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn validation_refreshes_the_token_for_the_holder_only() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "pc", NOW).ok().unwrap();
        let token = store.validate(&key, "a", NOW + DAY).ok().unwrap();
        let claims = verify(&store.signing_key.verifying_key(), &token).unwrap();
        assert_eq!(claims.valid_until, NOW + 8 * DAY);
        assert_eq!(
            denied(store.validate(&key, "b", NOW)),
            ErrorCode::NotActivated
        );
    }

    #[test]
    fn validation_needs_an_activation() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert_eq!(
            denied(store.validate(&key, "a", NOW)),
            ErrorCode::NotActivated
        );
    }

    #[test]
    fn revoking_stops_activation_and_validation() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "pc", NOW).ok().unwrap();
        assert!(store.revoke(id).unwrap());
        assert_eq!(
            denied(store.validate(&key, "a", NOW)),
            ErrorCode::InvalidKey
        );
        assert_eq!(
            denied(store.activate(&key, "a", "pc", NOW)),
            ErrorCode::InvalidKey
        );
    }

    #[test]
    fn deactivating_frees_the_seat() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "pc", NOW).ok().unwrap();
        assert!(store.deactivate(&key, "a").is_ok());
        assert!(store.activate(&key, "b", "laptop", NOW).is_ok());
    }

    #[test]
    fn only_the_holder_can_deactivate() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "pc", NOW).ok().unwrap();
        assert_eq!(denied(store.deactivate(&key, "b")), ErrorCode::NotActivated);
    }

    #[test]
    fn unbinding_by_the_admin_frees_the_seat() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "pc", NOW).ok().unwrap();
        assert!(store.unbind(id).unwrap());
        assert!(store.activate(&key, "b", "laptop", NOW).is_ok());
        assert!(!store.unbind(999).unwrap());
    }

    #[test]
    fn generated_keys_are_unique_and_well_formed() {
        let (a, b) = (new_key(), new_key());
        assert_ne!(a, b);
        assert_eq!(a.len(), "CLAB-XXXXX-XXXXX-XXXXX-XXXXX".len());
    }

    #[test]
    fn listing_shows_the_seat_holder() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store.activate(&key, "a", "ada-pc", NOW).ok().unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed[0].hostname.as_deref(), Some("ada-pc"));
        assert_eq!(listed[0].activated_at, Some(NOW as i64));
    }
}
