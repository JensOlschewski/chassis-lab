//! License gate and the `clab license` commands.
//!
//! The server decides who may run `clab`. This module keeps the signed token it
//! hands out, checks it with the server on every run, and tolerates an
//! unreachable server until the token's `valid_until`.

use anyhow::{Context, Result, anyhow, bail};
use clab_license::ed25519_dalek::VerifyingKey;
use clab_license::{
    ActivateRequest, Claims, ErrorBody, ErrorCode, RegisterRequest, RegisterResponse, SeatRequest,
    Token, ValidateRequest, decode_public_key, new_install_id, verify,
};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SERVER: &str = "https://clab.license.mariecolette.de";
const PUBLIC_KEY: &str = "MK1RsG6OxxmXlJOTpn/cAPdz8m/UAOb/f5uB9pJjZBs=";
const FINGERPRINT_SALT: &str = "clab-license-v1";
const NO_LICENSE: &str = "no valid license: run 'clab license activate <KEY>'";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Subcommand)]
pub enum LicenseCommand {
    /// Bind a license key to this installation.
    Activate { key: String },
    /// Show the license of this installation.
    Status,
    /// Release the license so another machine can use it.
    Deactivate,
}

/// What this installation keeps between runs. `token` is gone once the server
/// refused the license.
#[derive(Debug, Serialize, Deserialize)]
struct State {
    install_id: String,
    key: String,
    token: Option<Token>,
}

struct Store {
    path: PathBuf,
}

impl Store {
    fn standard() -> Result<Self> {
        let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let base = if cfg!(windows) {
            env("APPDATA")
        } else if cfg!(target_os = "macos") {
            env("HOME").map(|home| home.join("Library/Application Support"))
        } else {
            env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|home| home.join(".config")))
        };
        let base = base.context("cannot find the user configuration directory")?;
        Ok(Self {
            path: base.join("clab/license.json"),
        })
    }

    /// A missing or unreadable file means the installation has no license yet.
    fn load(&self) -> Option<State> {
        serde_json::from_slice(&std::fs::read(&self.path).ok()?).ok()
    }

    fn save(&self, state: &State) -> Result<()> {
        let dir = self
            .path
            .parent()
            .context("license path has no directory")?;
        std::fs::create_dir_all(dir)?;
        let temporary = self.path.with_extension("json.tmp");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        options
            .open(&temporary)
            .and_then(|mut file| file.write_all(&serde_json::to_vec_pretty(state)?))
            .and_then(|()| std::fs::rename(&temporary, &self.path))
            .with_context(|| format!("cannot write {}", self.path.display()))
    }

    fn remove(&self) -> Result<()> {
        match std::fs::remove_file(&self.path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

/// `Denied` is the server's verdict (HTTP 403): the license is not usable.
/// `Unreachable` covers every other outcome, including refused requests such as
/// a rate limit, and decides nothing.
#[derive(Debug)]
enum Refusal {
    Denied(ErrorCode),
    Unreachable(String),
}

trait Server {
    fn register(&self, request: &RegisterRequest) -> Result<RegisterResponse, Refusal>;
    fn activate(&self, request: &ActivateRequest) -> Result<Token, Refusal>;
    fn validate(&self, request: &ValidateRequest) -> Result<Token, Refusal>;
    fn deactivate(&self, request: &SeatRequest) -> Result<(), Refusal>;
}

struct Http(ureq::Agent);

impl Http {
    fn new() -> Self {
        Self(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(REQUEST_TIMEOUT))
                .build()
                .into(),
        )
    }

    fn post(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<ureq::http::Response<ureq::Body>, Refusal> {
        let mut response = self
            .0
            .post(format!("{SERVER}{path}"))
            .send_json(body)
            .map_err(|error| Refusal::Unreachable(error.to_string()))?;
        let status = response.status().as_u16();
        if status < 300 {
            return Ok(response);
        }
        let code = response
            .body_mut()
            .read_json::<ErrorBody>()
            .ok()
            .map(|body| body.error);
        Err(match (status, code) {
            (403, Some(code)) => Refusal::Denied(code),
            (_, Some(code)) => Refusal::Unreachable(code.to_string()),
            _ => Refusal::Unreachable(format!("HTTP {status}")),
        })
    }

    fn json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &impl Serialize,
    ) -> Result<T, Refusal> {
        self.post(path, body)?
            .body_mut()
            .read_json()
            .map_err(|error| Refusal::Unreachable(error.to_string()))
    }
}

impl Server for Http {
    fn register(&self, request: &RegisterRequest) -> Result<RegisterResponse, Refusal> {
        self.json("/v1/register", request)
    }

    fn activate(&self, request: &ActivateRequest) -> Result<Token, Refusal> {
        self.json("/v1/activate", request)
    }

    fn validate(&self, request: &ValidateRequest) -> Result<Token, Refusal> {
        self.json("/v1/validate", request)
    }

    fn deactivate(&self, request: &SeatRequest) -> Result<(), Refusal> {
        self.post("/v1/deactivate", request).map(drop)
    }
}

/// A salted hash of the OS machine ID. Only the hash leaves the machine.
fn fingerprint() -> Result<String> {
    let id = machine_id()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(format!("{FINGERPRINT_SALT}:{id}").as_bytes())
    ))
}

#[cfg(target_os = "linux")]
fn machine_id() -> Result<String> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
        .context("cannot read the machine ID (/etc/machine-id)")
}

#[cfg(target_os = "macos")]
fn machine_id() -> Result<String> {
    let output = command_output("ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])?;
    parse_ioreg(&output).context("cannot read the platform UUID")
}

#[cfg(windows)]
fn machine_id() -> Result<String> {
    let output = command_output(
        "reg",
        &[
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
        ],
    )?;
    parse_reg(&output).context("cannot read the MachineGuid")
}

#[cfg(any(target_os = "macos", windows))]
fn command_output(program: &str, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("cannot run {program}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `"IOPlatformUUID" = "XXXXXXXX-..."`
#[cfg(any(target_os = "macos", test))]
fn parse_ioreg(output: &str) -> Option<String> {
    let line = output
        .lines()
        .find(|line| line.contains("IOPlatformUUID"))?;
    let value = line.split('=').nth(1)?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.to_owned())
}

/// `    MachineGuid    REG_SZ    xxxxxxxx-...`
#[cfg(any(windows, test))]
fn parse_reg(output: &str) -> Option<String> {
    let line = output.lines().find(|line| line.contains("MachineGuid"))?;
    line.split_whitespace().last().map(str::to_owned)
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

fn denied(code: ErrorCode) -> anyhow::Error {
    anyhow!(code).context(NO_LICENSE)
}

fn unreachable(reason: String) -> anyhow::Error {
    anyhow!(
        "the license server gave no answer ({reason}): check the internet connection or try again later"
    )
}

struct Gate<S> {
    store: Store,
    server: S,
    public_key: VerifyingKey,
    fingerprint: fn() -> Result<String>,
    hostname: String,
    now: u64,
}

impl<S: Server> Gate<S> {
    /// Returns the claims of a usable license, or the reason there is none.
    fn require(&self) -> Result<Claims> {
        match self.store.load() {
            Some(state) => self.check(state).map(|(claims, _)| claims),
            None => self.register(),
        }
    }

    /// Asks the server on every call, so a revocation takes effect at once. An
    /// unreachable server is tolerated until the token's `valid_until`. The flag
    /// tells whether the server answered.
    fn check(&self, mut state: State) -> Result<(Claims, bool)> {
        let Some(token) = state.token.clone() else {
            bail!(NO_LICENSE);
        };
        let claims = self.accept(&state.install_id, &token).context(NO_LICENSE)?;
        let request = ValidateRequest {
            key: state.key.clone(),
            install_id: state.install_id.clone(),
            hostname: self.hostname.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        match self.server.validate(&request) {
            Ok(token) => {
                let claims = self.accept(&state.install_id, &token)?;
                state.token = Some(token);
                self.store.save(&state)?;
                Ok((claims, true))
            }
            Err(Refusal::Denied(code)) => {
                state.token = None;
                self.store.save(&state)?;
                Err(denied(code))
            }
            Err(Refusal::Unreachable(_)) if self.now < claims.valid_until => Ok((claims, false)),
            Err(Refusal::Unreachable(reason)) => Err(unreachable(reason)
                .context("the license check is overdue and the grace period has ended")),
        }
    }

    fn accept(&self, install_id: &str, token: &Token) -> Result<Claims> {
        let claims = verify(&self.public_key, token)?;
        if claims.install_id != install_id {
            bail!("the license belongs to another installation");
        }
        Ok(claims)
    }

    /// First run: the server creates a license for this installation.
    fn register(&self) -> Result<Claims> {
        let install_id = new_install_id();
        let request = RegisterRequest {
            install_id: install_id.clone(),
            fingerprint: (self.fingerprint)()?,
            hostname: self.hostname.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        let response =
            self.server
                .register(&request)
                .map_err(|refusal| match refusal {
                    Refusal::Denied(code) => denied(code),
                    Refusal::Unreachable(reason) => unreachable(reason)
                        .context("the first run registers this installation online"),
                })?;
        let claims = self.accept(&install_id, &response.token)?;
        self.store.save(&State {
            install_id,
            key: response.key,
            token: Some(response.token),
        })?;
        Ok(claims)
    }

    fn activate(&self, key: &str) -> Result<Claims> {
        let install_id = self
            .store
            .load()
            .map_or_else(new_install_id, |state| state.install_id);
        let key = key.trim().to_owned();
        let request = ActivateRequest {
            key: key.clone(),
            install_id: install_id.clone(),
            fingerprint: (self.fingerprint)()?,
            hostname: self.hostname.clone(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        };
        let token = self
            .server
            .activate(&request)
            .map_err(|refusal| match refusal {
                Refusal::Denied(code) => anyhow!(code),
                Refusal::Unreachable(reason) => unreachable(reason),
            })?;
        let claims = self.accept(&install_id, &token)?;
        self.store.save(&State {
            install_id,
            key,
            token: Some(token),
        })?;
        Ok(claims)
    }

    fn deactivate(&self) -> Result<()> {
        let state = self
            .store
            .load()
            .context("this installation has no license")?;
        let request = SeatRequest {
            key: state.key,
            install_id: state.install_id,
        };
        match self.server.deactivate(&request) {
            // A refused license holds no seat that needs releasing.
            Ok(()) | Err(Refusal::Denied(_)) => self.store.remove(),
            Err(Refusal::Unreachable(reason)) => Err(unreachable(reason)),
        }
    }

    /// Asks the server; when it is unreachable, reports what the stored token says.
    fn status(&self) -> String {
        let Some(state) = self.store.load() else {
            return "not licensed yet: the next analysis registers this installation".to_owned();
        };
        if state.token.is_none() {
            return NO_LICENSE.to_owned();
        }
        match self.check(state) {
            Ok((claims, true)) => format!(
                "licensed to {} (license {}), confirmed by the server",
                claims.licensee, claims.license_id
            ),
            Ok((claims, false)) => {
                let left = claims.valid_until.saturating_sub(self.now);
                format!(
                    "licensed to {} (license {}), server unreachable: works offline for another {}d {}h",
                    claims.licensee,
                    claims.license_id,
                    left / 86_400,
                    left % 86_400 / 3_600
                )
            }
            Err(error) => format!("{error:#}"),
        }
    }
}

fn gate() -> Result<Gate<Http>> {
    Ok(Gate {
        store: Store::standard()?,
        server: Http::new(),
        public_key: decode_public_key(PUBLIC_KEY).context("embedded license key is invalid")?,
        fingerprint,
        hostname: hostname(),
        now: unix_now(),
    })
}

/// Called before every command that needs a license.
pub fn require() -> Result<()> {
    if cfg!(feature = "skip-license-gate") {
        return Ok(());
    }
    gate()?.require().map(drop)
}

pub fn run(command: LicenseCommand) -> Result<()> {
    let gate = gate()?;
    match command {
        LicenseCommand::Activate { key } => {
            let claims = gate.activate(&key)?;
            println!("activated for {}", claims.licensee);
        }
        LicenseCommand::Status => println!("{}", gate.status()),
        LicenseCommand::Deactivate => {
            gate.deactivate()?;
            println!("license released");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clab_license::ed25519_dalek::SigningKey;
    use clab_license::sign;
    use std::cell::{Cell, RefCell};

    const DAY: u64 = 86_400;
    const START: u64 = 1_000_000;

    fn signer() -> SigningKey {
        SigningKey::from_bytes(&[5; 32])
    }

    fn token(install_id: &str, issued_at: u64) -> Token {
        token_by(&signer(), install_id, issued_at)
    }

    fn token_by(key: &SigningKey, install_id: &str, issued_at: u64) -> Token {
        sign(
            key,
            &Claims {
                license_id: 3,
                licensee: "Ada".into(),
                install_id: install_id.into(),
                issued_at,
                valid_until: issued_at + 7 * DAY,
            },
        )
    }

    /// An unset answer behaves like an unreachable server.
    #[derive(Default)]
    struct Fake {
        /// The key the server hands out; its token is bound to the requesting install ID.
        register: RefCell<Option<Result<String, Refusal>>>,
        activate: RefCell<Option<Result<Token, Refusal>>>,
        validate: RefCell<Option<Result<Token, Refusal>>>,
        deactivate: RefCell<Option<Result<(), Refusal>>>,
        calls: Cell<usize>,
    }

    impl Fake {
        fn answer<T>(&self, slot: &RefCell<Option<Result<T, Refusal>>>) -> Result<T, Refusal> {
            self.calls.set(self.calls.get() + 1);
            slot.borrow_mut()
                .take()
                .unwrap_or_else(|| Err(Refusal::Unreachable("offline".into())))
        }
    }

    impl Server for Fake {
        fn register(&self, request: &RegisterRequest) -> Result<RegisterResponse, Refusal> {
            self.answer(&self.register).map(|key| RegisterResponse {
                key,
                token: token(&request.install_id, START),
            })
        }

        fn activate(&self, _: &ActivateRequest) -> Result<Token, Refusal> {
            self.answer(&self.activate)
        }

        fn validate(&self, _: &ValidateRequest) -> Result<Token, Refusal> {
            self.answer(&self.validate)
        }

        fn deactivate(&self, _: &SeatRequest) -> Result<(), Refusal> {
            self.answer(&self.deactivate)
        }
    }

    struct Fixture {
        gate: Gate<Fake>,
        _dir: tempfile::TempDir,
    }

    fn fixture(now: u64) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        Fixture {
            gate: Gate {
                store: Store {
                    path: dir.path().join("clab/license.json"),
                },
                server: Fake::default(),
                public_key: signer().verifying_key(),
                fingerprint: || Ok("fp".to_owned()),
                hostname: "pc".into(),
                now,
            },
            _dir: dir,
        }
    }

    fn saved(gate: &Gate<Fake>, token: Option<Token>) {
        gate.store
            .save(&State {
                install_id: "inst".into(),
                key: "CLAB-KEY".into(),
                token,
            })
            .unwrap();
    }

    fn message(error: &anyhow::Error) -> String {
        format!("{error:#}")
    }

    #[test]
    fn the_first_run_registers() {
        let f = fixture(START);
        *f.gate.server.register.borrow_mut() = Some(Ok("CLAB-NEW".into()));
        assert_eq!(f.gate.require().unwrap().licensee, "Ada");
        let state = f.gate.store.load().unwrap();
        assert_eq!(state.key, "CLAB-NEW");
        assert_eq!(f.gate.server.calls.get(), 1);
    }

    #[test]
    fn a_first_run_offline_fails_clearly_and_saves_nothing() {
        let f = fixture(START);
        let error = f.gate.require().unwrap_err();
        assert!(message(&error).contains("first run registers"), "{error:#}");
        assert!(f.gate.store.load().is_none());
    }

    #[test]
    fn a_refused_registration_reports_no_license() {
        let f = fixture(START);
        *f.gate.server.register.borrow_mut() = Some(Err(Refusal::Denied(ErrorCode::InvalidKey)));
        let error = f.gate.require().unwrap_err();
        assert!(message(&error).starts_with(NO_LICENSE), "{error:#}");
        assert!(f.gate.store.load().is_none());
    }

    #[test]
    fn every_run_asks_the_server_and_stores_the_new_token() {
        let f = fixture(START + 10);
        saved(&f.gate, Some(token("inst", START)));
        *f.gate.server.validate.borrow_mut() = Some(Ok(token("inst", START + 10)));
        assert_eq!(f.gate.require().unwrap().issued_at, START + 10);
        assert_eq!(f.gate.server.calls.get(), 1);
        let stored = f.gate.store.load().unwrap().token.unwrap();
        assert!(stored.payload.contains(&(START + 10).to_string()));
    }

    #[test]
    fn a_refusal_drops_the_token_for_good() {
        let f = fixture(START + DAY);
        saved(&f.gate, Some(token("inst", START)));
        *f.gate.server.validate.borrow_mut() = Some(Err(Refusal::Denied(ErrorCode::InvalidKey)));
        let error = f.gate.require().unwrap_err();
        assert!(message(&error).starts_with(NO_LICENSE), "{error:#}");
        assert!(f.gate.store.load().unwrap().token.is_none());
        assert!(f.gate.require().is_err());
        assert_eq!(f.gate.server.calls.get(), 1);
    }

    #[test]
    fn an_unreachable_server_is_tolerated_until_valid_until() {
        let f = fixture(START + 6 * DAY);
        saved(&f.gate, Some(token("inst", START)));
        assert!(f.gate.require().is_ok());
        assert!(f.gate.store.load().unwrap().token.is_some());

        let f = fixture(START + 7 * DAY);
        saved(&f.gate, Some(token("inst", START)));
        let error = f.gate.require().unwrap_err();
        assert!(message(&error).contains("grace period"), "{error:#}");
    }

    #[test]
    fn a_token_from_another_signer_is_rejected() {
        let f = fixture(START);
        let forged = token_by(&SigningKey::from_bytes(&[6; 32]), "inst", START);
        saved(&f.gate, Some(forged));
        let error = f.gate.require().unwrap_err();
        assert!(message(&error).starts_with(NO_LICENSE), "{error:#}");
        assert_eq!(f.gate.server.calls.get(), 0);
    }

    #[test]
    fn a_token_of_another_installation_is_rejected() {
        let f = fixture(START);
        saved(&f.gate, Some(token("someone-else", START)));
        assert!(f.gate.require().is_err());
    }

    #[test]
    fn a_refreshed_token_for_another_installation_is_rejected() {
        let f = fixture(START + DAY);
        saved(&f.gate, Some(token("inst", START)));
        *f.gate.server.validate.borrow_mut() = Some(Ok(token("someone-else", START + DAY)));
        assert!(f.gate.require().is_err());
    }

    #[test]
    fn activation_keeps_the_install_id_and_stores_the_key() {
        let f = fixture(START);
        saved(&f.gate, None);
        *f.gate.server.activate.borrow_mut() = Some(Ok(token("inst", START)));
        assert_eq!(f.gate.activate("  CLAB-MINE ").unwrap().licensee, "Ada");
        let state = f.gate.store.load().unwrap();
        assert_eq!(
            (state.install_id.as_str(), state.key.as_str()),
            ("inst", "CLAB-MINE")
        );
        assert!(f.gate.require().is_ok());
    }

    #[test]
    fn a_refused_activation_changes_nothing() {
        let f = fixture(START);
        saved(&f.gate, Some(token("inst", START)));
        *f.gate.server.activate.borrow_mut() = Some(Err(Refusal::Denied(ErrorCode::SeatInUse)));
        let error = f.gate.activate("CLAB-MINE").unwrap_err();
        assert!(message(&error).contains("another machine"), "{error:#}");
        assert_eq!(f.gate.store.load().unwrap().key, "CLAB-KEY");
    }

    #[test]
    fn deactivation_removes_the_state_only_when_the_server_answered() {
        let f = fixture(START);
        saved(&f.gate, Some(token("inst", START)));
        assert!(f.gate.deactivate().is_err());
        assert!(f.gate.store.load().is_some());
        *f.gate.server.deactivate.borrow_mut() = Some(Ok(()));
        f.gate.deactivate().unwrap();
        assert!(f.gate.store.load().is_none());
        assert!(f.gate.deactivate().is_err());
    }

    #[test]
    fn status_asks_the_server_and_falls_back_to_the_token() {
        let f = fixture(START + DAY);
        assert!(f.gate.status().starts_with("not licensed yet"));
        saved(&f.gate, Some(token("inst", START)));
        assert_eq!(
            f.gate.status(),
            "licensed to Ada (license 3), server unreachable: works offline for another 6d 0h"
        );
        *f.gate.server.validate.borrow_mut() = Some(Ok(token("inst", START + DAY)));
        assert_eq!(
            f.gate.status(),
            "licensed to Ada (license 3), confirmed by the server"
        );
        *f.gate.server.validate.borrow_mut() = Some(Err(Refusal::Denied(ErrorCode::InvalidKey)));
        assert!(f.gate.status().starts_with(NO_LICENSE));
        assert_eq!(f.gate.status(), NO_LICENSE);
    }

    #[cfg(unix)]
    #[test]
    fn the_state_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let f = fixture(START);
        saved(&f.gate, None);
        let mode = std::fs::metadata(&f.gate.store.path)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn the_embedded_public_key_decodes() {
        assert!(decode_public_key(PUBLIC_KEY).is_some());
    }

    #[test]
    fn machine_ids_are_parsed_from_platform_output() {
        let ioreg = "  | \"IOPlatformUUID\" = \"AAAA-BBBB\"\n";
        assert_eq!(parse_ioreg(ioreg).as_deref(), Some("AAAA-BBBB"));
        let reg = "HKEY...\\Cryptography\n    MachineGuid    REG_SZ    1234-abcd\n";
        assert_eq!(parse_reg(reg).as_deref(), Some("1234-abcd"));
        assert_eq!(parse_reg("nothing"), None);
    }

    #[test]
    fn the_fingerprint_is_a_stable_hash() {
        if let Ok(first) = fingerprint() {
            assert_eq!(first.len(), 64);
            assert_eq!(first, fingerprint().unwrap());
        }
    }
}
