use clab_license::ed25519_dalek::SigningKey;
use clab_license::{
    ActivateRequest, Claims, ErrorCode, RegisterRequest, RegisterResponse, Token, ValidateRequest,
    sign,
};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::Duration;

const KEY_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const MAX_KEY: usize = 64;
const MAX_INSTALL_ID: usize = 64;
const MAX_FINGERPRINT: usize = 128;
const MAX_HOSTNAME: usize = 255;
const MAX_VERSION: usize = 32;

/// `user_version` of the current schema. Older databases are migrated on open.
const SCHEMA_VERSION: i64 = 2;

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
    pub auto: bool,
    pub hostname: Option<String>,
    pub last_ip: Option<String>,
    pub last_seen: Option<i64>,
    pub version: Option<String>,
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

fn migrate(connection: &Connection) -> rusqlite::Result<()> {
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
    let version: i64 = connection.query_row("pragma user_version", [], |row| row.get(0))?;
    if version < SCHEMA_VERSION {
        connection.execute_batch(
            "begin;
            alter table licenses add column fingerprint text;
            alter table licenses add column last_ip text;
            alter table licenses add column last_seen integer;
            alter table licenses add column version text;
            alter table licenses add column auto integer not null default 0;
            create index licenses_fingerprint on licenses (fingerprint);
            create table blocked_fingerprints (
                fingerprint text primary key,
                license_id integer not null,
                blocked_at integer not null
            );
            pragma user_version = 2;
            commit;",
        )?;
    }
    Ok(())
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
        migrate(&connection)?;
        Ok(Self {
            connection,
            signing_key,
            grace_secs: grace_days * 24 * 60 * 60,
        })
    }

    /// A named license for a manual grant. Returns the id and the key; the key is not recoverable later.
    pub fn create(&self, person: &str) -> rusqlite::Result<(i64, String)> {
        let key = new_key();
        self.connection.execute(
            "insert into licenses (key_hash, person) values (?1, ?2)",
            params![hash(&key), person],
        )?;
        Ok((self.connection.last_insert_rowid(), key))
    }

    /// Stops the license and blocks its machine, so a reinstall does not help.
    pub fn revoke(&self, id: i64, now: u64) -> rusqlite::Result<bool> {
        self.connection.execute(
            "insert or ignore into blocked_fingerprints (fingerprint, license_id, blocked_at)
             select fingerprint, id, ?2 from licenses where id = ?1 and fingerprint is not null",
            params![id, now as i64],
        )?;
        Ok(self
            .connection
            .execute("update licenses set revoked = 1 where id = ?1", [id])?
            > 0)
    }

    /// Undoes `revoke`: the license works again and its machine is unblocked.
    pub fn unrevoke(&self, id: i64) -> rusqlite::Result<bool> {
        self.connection.execute(
            "delete from blocked_fingerprints where license_id = ?1",
            [id],
        )?;
        Ok(self
            .connection
            .execute("update licenses set revoked = 0 where id = ?1", [id])?
            > 0)
    }

    pub fn rename(&self, id: i64, person: &str) -> rusqlite::Result<bool> {
        Ok(self.connection.execute(
            "update licenses set person = ?2 where id = ?1",
            params![id, person],
        )? > 0)
    }

    /// Frees the seat, for example after a machine change.
    pub fn unbind(&self, id: i64) -> rusqlite::Result<bool> {
        Ok(self.connection.execute(
            "update licenses set install_id = null, hostname = null, activated_at = null where id = ?1",
            [id],
        )? > 0)
    }

    /// The key then behaves like an unknown one. A machine block stays.
    pub fn delete(&self, id: i64) -> rusqlite::Result<bool> {
        Ok(self
            .connection
            .execute("delete from licenses where id = ?1", [id])?
            > 0)
    }

    pub fn list(&self) -> rusqlite::Result<Vec<Listed>> {
        self.connection
            .prepare(
                "select id, person, revoked, auto, hostname, last_ip, last_seen, version
                 from licenses order by id",
            )?
            .query_map([], |row| {
                Ok(Listed {
                    id: row.get(0)?,
                    person: row.get(1)?,
                    revoked: row.get(2)?,
                    auto: row.get(3)?,
                    hostname: row.get(4)?,
                    last_ip: row.get(5)?,
                    last_seen: row.get(6)?,
                    version: row.get(7)?,
                })
            })?
            .collect()
    }

    /// First start of an installation. A machine that registered before gets its
    /// license back on the new install ID, with a new key.
    pub fn register(
        &self,
        request: &RegisterRequest,
        ip: Option<&str>,
        now: u64,
    ) -> Result<RegisterResponse, Failure> {
        bounded(&request.install_id, MAX_INSTALL_ID)?;
        bounded(&request.fingerprint, MAX_FINGERPRINT)?;
        bounded(&request.hostname, MAX_HOSTNAME)?;
        bounded(&request.version, MAX_VERSION)?;
        if self.blocked(&request.fingerprint)? {
            return Err(ErrorCode::InvalidKey.into());
        }
        let key = new_key();
        let known: Option<(i64, String)> = self
            .connection
            .query_row(
                "select id, person from licenses
                 where fingerprint = ?1 and auto = 1 and revoked = 0 order by id desc limit 1",
                [&request.fingerprint],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let (id, person) = match known {
            Some((id, person)) => {
                self.connection.execute(
                    "update licenses set key_hash = ?1, install_id = ?2, hostname = ?3, last_ip = ?4,
                     last_seen = ?5, version = ?6 where id = ?7",
                    params![hash(&key), request.install_id, request.hostname, ip, now as i64, request.version, id],
                )?;
                (id, person)
            }
            None => {
                self.connection.execute(
                    "insert into licenses (key_hash, person, auto, install_id, hostname, fingerprint,
                     activated_at, last_seen, last_ip, version)
                     values (?1, ?2, 1, ?3, ?2, ?4, ?5, ?5, ?6, ?7)",
                    params![
                        hash(&key),
                        request.hostname,
                        request.install_id,
                        request.fingerprint,
                        now as i64,
                        ip,
                        request.version
                    ],
                )?;
                (
                    self.connection.last_insert_rowid(),
                    request.hostname.clone(),
                )
            }
        };
        let row = Row {
            id,
            person,
            revoked: false,
            install_id: Some(request.install_id.clone()),
        };
        Ok(RegisterResponse {
            key,
            token: self.issue(&row, &request.install_id, now),
        })
    }

    /// Binds a manually issued key to an installation.
    pub fn activate(
        &self,
        request: &ActivateRequest,
        ip: Option<&str>,
        now: u64,
    ) -> Result<Token, Failure> {
        bounded(&request.key, MAX_KEY)?;
        bounded(&request.install_id, MAX_INSTALL_ID)?;
        bounded(&request.fingerprint, MAX_FINGERPRINT)?;
        bounded(&request.hostname, MAX_HOSTNAME)?;
        bounded(&request.version, MAX_VERSION)?;
        if self.blocked(&request.fingerprint)? {
            return Err(ErrorCode::InvalidKey.into());
        }
        let row = self.usable(&request.key)?;
        if row
            .install_id
            .as_deref()
            .is_some_and(|holder| holder != request.install_id)
        {
            return Err(ErrorCode::SeatInUse.into());
        }
        self.connection.execute(
            "update licenses set install_id = ?1, hostname = ?2, fingerprint = ?3, last_ip = ?4,
             last_seen = ?5, version = ?6, activated_at = coalesce(activated_at, ?5) where id = ?7",
            params![
                request.install_id,
                request.hostname,
                request.fingerprint,
                ip,
                now as i64,
                request.version,
                row.id
            ],
        )?;
        Ok(self.issue(&row, &request.install_id, now))
    }

    pub fn validate(
        &self,
        request: &ValidateRequest,
        ip: Option<&str>,
        now: u64,
    ) -> Result<Token, Failure> {
        bounded(&request.hostname, MAX_HOSTNAME)?;
        bounded(&request.version, MAX_VERSION)?;
        let row = self.held(&request.key, &request.install_id)?;
        self.connection.execute(
            "update licenses set hostname = ?1, last_ip = ?2, last_seen = ?3, version = ?4 where id = ?5",
            params![request.hostname, ip, now as i64, request.version, row.id],
        )?;
        Ok(self.issue(&row, &request.install_id, now))
    }

    pub fn deactivate(&self, key: &str, install_id: &str) -> Result<(), Failure> {
        let row = self.held(key, install_id)?;
        self.unbind(row.id)?;
        Ok(())
    }

    fn blocked(&self, fingerprint: &str) -> rusqlite::Result<bool> {
        self.connection.query_row(
            "select exists(select 1 from blocked_fingerprints where fingerprint = ?1)",
            [fingerprint],
            |row| row.get(0),
        )
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

    fn activation(key: &str, install_id: &str, hostname: &str) -> ActivateRequest {
        ActivateRequest {
            key: key.into(),
            install_id: install_id.into(),
            fingerprint: format!("fp-{hostname}"),
            hostname: hostname.into(),
            version: "1.0".into(),
        }
    }

    fn validation(key: &str, install_id: &str) -> ValidateRequest {
        ValidateRequest {
            key: key.into(),
            install_id: install_id.into(),
            hostname: "pc".into(),
            version: "1.0".into(),
        }
    }

    fn registration(install_id: &str, fingerprint: &str, hostname: &str) -> RegisterRequest {
        RegisterRequest {
            install_id: install_id.into(),
            fingerprint: fingerprint.into(),
            hostname: hostname.into(),
            version: "1.0".into(),
        }
    }

    fn verified(store: &Store, token: &Token) -> Claims {
        verify(&store.signing_key.verifying_key(), token).unwrap()
    }

    #[test]
    fn activation_issues_a_verifiable_token_with_the_grace_period() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        let token = store
            .activate(&activation(&key, "install-a", "ada-pc"), None, NOW)
            .ok()
            .unwrap();
        assert_eq!(
            verified(&store, &token),
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
        let padded = format!(" {} ", key.to_lowercase());
        assert!(
            store
                .activate(&activation(&padded, "a", "pc"), None, NOW)
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
        assert!(
            store
                .activate(&activation(&key, "a", "pc"), None, NOW)
                .is_ok()
        );
        assert!(
            store
                .activate(&activation(&key, "a", "pc"), None, NOW + 5)
                .is_ok()
        );
    }

    #[test]
    fn one_seat_per_key() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert!(
            store
                .activate(&activation(&key, "a", "pc"), None, NOW)
                .is_ok()
        );
        assert_eq!(
            denied(store.activate(&activation(&key, "b", "laptop"), None, NOW)),
            ErrorCode::SeatInUse
        );
    }

    #[test]
    fn unknown_keys_are_refused() {
        assert_eq!(
            denied(store().activate(&activation("CLAB-NOPE", "a", "pc"), None, NOW)),
            ErrorCode::InvalidKey
        );
    }

    #[test]
    fn oversized_fields_are_rejected() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        let long = "x".repeat(MAX_HOSTNAME + 1);
        assert_eq!(
            denied(store.activate(&activation(&key, "a", &long), None, NOW)),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            denied(store.activate(&activation(&key, "", "pc"), None, NOW)),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            denied(store.register(&registration("a", "", "pc"), None, NOW)),
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn validation_refreshes_the_token_for_the_holder_only() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        let token = store
            .validate(&validation(&key, "a"), None, NOW + DAY)
            .ok()
            .unwrap();
        assert_eq!(verified(&store, &token).valid_until, NOW + 8 * DAY);
        assert_eq!(
            denied(store.validate(&validation(&key, "b"), None, NOW)),
            ErrorCode::NotActivated
        );
    }

    #[test]
    fn validation_needs_an_activation() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        assert_eq!(
            denied(store.validate(&validation(&key, "a"), None, NOW)),
            ErrorCode::NotActivated
        );
    }

    #[test]
    fn validation_records_what_the_machine_reports() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), Some("10.0.0.1"), NOW)
            .ok()
            .unwrap();
        let mut request = validation(&key, "a");
        request.hostname = "renamed".into();
        store
            .validate(&request, Some("10.0.0.2"), NOW + DAY)
            .ok()
            .unwrap();
        let listed = &store.list().unwrap()[0];
        assert_eq!(listed.hostname.as_deref(), Some("renamed"));
        assert_eq!(listed.last_ip.as_deref(), Some("10.0.0.2"));
        assert_eq!(listed.last_seen, Some((NOW + DAY) as i64));
    }

    #[test]
    fn revoking_stops_activation_and_validation() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert!(store.revoke(id, NOW).unwrap());
        assert_eq!(
            denied(store.validate(&validation(&key, "a"), None, NOW)),
            ErrorCode::InvalidKey
        );
        assert_eq!(
            denied(store.activate(&activation(&key, "a", "pc"), None, NOW)),
            ErrorCode::InvalidKey
        );
    }

    #[test]
    fn deactivating_frees_the_seat() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert!(store.deactivate(&key, "a").is_ok());
        assert!(
            store
                .activate(&activation(&key, "b", "laptop"), None, NOW)
                .is_ok()
        );
    }

    #[test]
    fn only_the_holder_can_deactivate() {
        let store = store();
        let (_, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert_eq!(denied(store.deactivate(&key, "b")), ErrorCode::NotActivated);
    }

    #[test]
    fn unbinding_by_the_admin_frees_the_seat() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert!(store.unbind(id).unwrap());
        assert!(
            store
                .activate(&activation(&key, "b", "laptop"), None, NOW)
                .is_ok()
        );
        assert!(!store.unbind(999).unwrap());
    }

    #[test]
    fn a_deleted_license_is_unknown() {
        let store = store();
        let (id, key) = store.create("Ada").unwrap();
        store
            .activate(&activation(&key, "a", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert!(store.delete(id).unwrap());
        assert!(store.list().unwrap().is_empty());
        assert_eq!(
            denied(store.validate(&validation(&key, "a"), None, NOW)),
            ErrorCode::InvalidKey
        );
        assert!(!store.delete(id).unwrap());
    }

    #[test]
    fn generated_keys_are_unique_and_well_formed() {
        let (a, b) = (new_key(), new_key());
        assert_ne!(a, b);
        assert_eq!(a.len(), "CLAB-XXXXX-XXXXX-XXXXX-XXXXX".len());
    }

    #[test]
    fn registering_creates_a_license_named_after_the_host() {
        let store = store();
        let registered = store
            .register(&registration("a", "fp-1", "ada-pc"), Some("10.0.0.1"), NOW)
            .ok()
            .unwrap();
        let claims = verified(&store, &registered.token);
        assert_eq!(
            (claims.licensee.as_str(), claims.install_id.as_str()),
            ("ada-pc", "a")
        );
        let listed = &store.list().unwrap()[0];
        assert!(listed.auto);
        assert_eq!(listed.last_ip.as_deref(), Some("10.0.0.1"));
        // The returned key works for validation.
        let refreshed = store.validate(&validation(&registered.key, "a"), None, NOW + DAY);
        assert!(refreshed.is_ok());
    }

    #[test]
    fn registering_again_from_one_machine_reuses_its_license_with_a_new_key() {
        let store = store();
        let first = store
            .register(&registration("a", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        let second = store
            .register(&registration("b", "fp-1", "pc"), None, NOW + DAY)
            .ok()
            .unwrap();
        assert_eq!(store.list().unwrap().len(), 1);
        assert_ne!(first.key, second.key);
        assert_eq!(
            denied(store.validate(&validation(&first.key, "a"), None, NOW)),
            ErrorCode::InvalidKey
        );
        assert!(
            store
                .validate(&validation(&second.key, "b"), None, NOW)
                .is_ok()
        );
    }

    #[test]
    fn a_revoked_machine_cannot_register_or_activate_again() {
        let store = store();
        let registered = store
            .register(&registration("a", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        let id = verified(&store, &registered.token).license_id;
        assert!(store.revoke(id, NOW).unwrap());
        assert_eq!(
            denied(store.register(&registration("fresh", "fp-1", "pc"), None, NOW)),
            ErrorCode::InvalidKey
        );
        let (_, other_key) = store.create("Ada").unwrap();
        let mut request = activation(&other_key, "fresh", "pc");
        request.fingerprint = "fp-1".into();
        assert_eq!(
            denied(store.activate(&request, None, NOW)),
            ErrorCode::InvalidKey
        );
        assert!(
            store
                .register(&registration("x", "fp-2", "other"), None, NOW)
                .is_ok()
        );
    }

    #[test]
    fn unrevoking_restores_the_license_and_unblocks_the_machine() {
        let store = store();
        let registered = store
            .register(&registration("a", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        let id = verified(&store, &registered.token).license_id;
        store.revoke(id, NOW).unwrap();
        assert!(store.list().unwrap()[0].revoked);
        assert!(store.unrevoke(id).unwrap());
        assert!(!store.list().unwrap()[0].revoked);
        let again = store
            .register(&registration("b", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        assert_eq!(verified(&store, &again.token).license_id, id);
        assert!(!store.unrevoke(99).unwrap());
    }

    #[test]
    fn deleting_a_license_keeps_the_machine_block() {
        let store = store();
        let registered = store
            .register(&registration("a", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        let id = verified(&store, &registered.token).license_id;
        store.revoke(id, NOW).unwrap();
        store.delete(id).unwrap();
        assert_eq!(
            denied(store.register(&registration("b", "fp-1", "pc"), None, NOW)),
            ErrorCode::InvalidKey
        );
    }

    #[test]
    fn a_license_without_a_fingerprint_revokes_without_a_block() {
        let store = store();
        let (id, _) = store.create("Ada").unwrap();
        assert!(store.revoke(id, NOW).unwrap());
        assert!(store.unrevoke(id).unwrap());
    }

    #[test]
    fn renaming_a_license_changes_the_licensee() {
        let store = store();
        let registered = store
            .register(&registration("a", "fp-1", "pc"), None, NOW)
            .ok()
            .unwrap();
        let id = verified(&store, &registered.token).license_id;
        assert!(store.rename(id, "Ada").unwrap());
        let token = store
            .validate(&validation(&registered.key, "a"), None, NOW)
            .ok()
            .unwrap();
        assert_eq!(verified(&store, &token).licensee, "Ada");
        assert!(!store.rename(999, "x").unwrap());
    }

    #[test]
    fn an_old_database_is_migrated_without_losing_licenses() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "create table licenses (
                    id integer primary key autoincrement,
                    key_hash text not null unique,
                    person text not null,
                    revoked integer not null default 0,
                    install_id text,
                    hostname text,
                    activated_at integer
                );
                insert into licenses (key_hash, person, install_id, hostname, activated_at)
                    values ('old', 'Ada', 'a', 'pc', 5);",
            )
            .unwrap();
        let store = Store::with(connection, SigningKey::from_bytes(&[9; 32]), 7).unwrap();
        let listed = store.list().unwrap();
        assert_eq!((listed[0].person.as_str(), listed[0].auto), ("Ada", false));
        assert!(store.revoke(1, NOW).unwrap());
        let version: i64 = store
            .connection
            .query_row("pragma user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn opening_twice_does_not_migrate_twice() {
        let dir = std::env::temp_dir().join(format!(
            "clab-license-migrate-{}",
            clab_license::new_install_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("licenses.db");
        let key = || SigningKey::from_bytes(&[9; 32]);
        Store::open(&path, key(), 7)
            .ok()
            .unwrap()
            .create("Ada")
            .unwrap();
        assert_eq!(
            Store::open(&path, key(), 7)
                .ok()
                .unwrap()
                .list()
                .unwrap()
                .len(),
            1
        );
    }
}
