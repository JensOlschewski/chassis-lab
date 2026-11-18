mod http;
mod store;

use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use clab_license::ed25519_dalek::SigningKey;
use clab_license::encode_public_key;
use clap::{Parser, Subcommand};
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use store::Store;

#[derive(Parser)]
#[command(about = "License server for clab")]
struct Cli {
    /// Directory holding `licenses.db` and `signing.key`.
    #[arg(long, default_value = "data", global = true)]
    data: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Answer license requests.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8080")]
        listen: String,
        /// How long a token stays valid without a new online check.
        #[arg(long, default_value_t = 1)]
        grace_days: u64,
        /// Pass `false` to stop new installations from registering; existing ones keep working.
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        open_registration: bool,
    },
    /// Create the signing key. Refuses to replace an existing one.
    Keygen,
    /// Create a license for a person and print its key once.
    Create {
        #[arg(long)]
        person: String,
    },
    /// Make a license stop working at the next online check, and block its machine.
    Revoke { id: i64 },
    /// Undo `revoke`: the license works again and its machine is unblocked.
    Unrevoke { id: i64 },
    /// Set the person a license belongs to.
    Name { id: i64, person: String },
    /// Free the seat of a license, for example after a machine change.
    Unbind { id: i64 },
    /// Issue a new key for a lost or leaked one and print it once. The old key stops working and
    /// the seat is freed.
    Rekey { id: i64 },
    /// Remove a license for good, for example one created by mistake. Prefer `revoke`. Its
    /// machine block goes too.
    Delete { id: i64 },
    /// Show all licenses.
    List,
}

/// Unix seconds as `YYYY-MM-DD HH:MM` in UTC (Hinnant's civil-from-days).
fn utc(seconds: i64) -> String {
    let (days, rest) = (seconds.div_euclid(86_400), seconds.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3_600,
        rest % 3_600 / 60
    )
}

fn key_path(data: &Path) -> PathBuf {
    data.join("signing.key")
}

fn load_key(data: &Path) -> Result<SigningKey> {
    let path = key_path(data);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("no signing key at {}; run `keygen` first", path.display()))?;
    let seed: [u8; 32] = BASE64
        .decode(text.trim())
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
        .with_context(|| format!("{} is not a signing key", path.display()))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn keygen(data: &Path) -> Result<()> {
    std::fs::create_dir_all(data)?;
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed)?;
    let path = key_path(data);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .with_context(|| {
            format!(
                "{} exists; keep it, clients trust its public key",
                path.display()
            )
        })?;
    writeln!(file, "{}", BASE64.encode(seed))?;
    println!(
        "{}",
        encode_public_key(&SigningKey::from_bytes(&seed).verifying_key())
    );
    Ok(())
}

fn open(data: &Path, grace_days: u64) -> Result<Store> {
    Ok(Store::open(
        &data.join("licenses.db"),
        load_key(data)?,
        grace_days,
    )?)
}

fn person(store: &Store, id: i64) -> Result<String> {
    match store.list()?.into_iter().find(|license| license.id == id) {
        Some(license) => Ok(license.person),
        None => bail!("no license {id}"),
    }
}

async fn shutdown() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut terminate = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = cli.data;
    match cli.command {
        Command::Serve {
            listen,
            grace_days,
            open_registration,
        } => {
            let app = http::router(open(&data, grace_days)?, open_registration);
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            let registration = if open_registration { "open" } else { "closed" };
            eprintln!(
                "listening on {listen}, grace {grace_days} days, registration {registration}"
            );
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown())
                .await?;
        }
        Command::Keygen => keygen(&data)?,
        Command::Create { person } => {
            let (id, key) = open(&data, 0)?.create(&person)?;
            println!("license {id} for {person}\n{key}");
        }
        Command::Revoke { id } => {
            let store = open(&data, 0)?;
            let person = person(&store, id)?;
            store.revoke(id, http::now())?;
            println!("revoked license {id} ({person}); its machine is blocked");
        }
        Command::Unrevoke { id } => {
            let store = open(&data, 0)?;
            let person = person(&store, id)?;
            store.unrevoke(id)?;
            println!("license {id} ({person}) works again; its machine is unblocked");
        }
        Command::Name { id, person: name } => {
            let store = open(&data, 0)?;
            person(&store, id)?;
            store.rename(id, &name)?;
            println!("license {id} belongs to {name}");
        }
        Command::Unbind { id } => {
            let store = open(&data, 0)?;
            let person = person(&store, id)?;
            store.unbind(id)?;
            println!("freed the seat of license {id} ({person})");
        }
        Command::Rekey { id } => {
            let store = open(&data, 0)?;
            let person = person(&store, id)?;
            if let Some(key) = store.rekey(id)? {
                println!("license {id} for {person}; the old key no longer works\n{key}");
            }
        }
        Command::Delete { id } => {
            let store = open(&data, 0)?;
            let person = person(&store, id)?;
            store.delete(id)?;
            println!("deleted license {id} ({person})");
        }
        Command::List => {
            let rows: Vec<[String; 9]> = open(&data, 0)?
                .list()?
                .into_iter()
                .map(|license| {
                    let or_dash = |text: Option<String>| text.unwrap_or_else(|| "-".to_owned());
                    [
                        license.id.to_string(),
                        license.person,
                        match (license.revoked, license.refused > 0) {
                            (true, true) => "revoked, still asking",
                            (true, false) => "revoked",
                            (false, _) => "active",
                        }
                        .to_owned(),
                        if license.auto { "auto" } else { "manual" }.to_owned(),
                        or_dash(license.hostname),
                        or_dash(license.last_ip),
                        or_dash(license.last_seen.map(utc)),
                        or_dash(license.version),
                        match (license.refused, license.refused_at) {
                            (refused, Some(at)) if refused > 0 => {
                                format!("{} ok, {refused} refused ({})", license.ok, utc(at))
                            }
                            _ => format!("{} ok", license.ok),
                        },
                    ]
                })
                .collect();
            let widths: [usize; 9] = std::array::from_fn(|column| {
                rows.iter().map(|row| row[column].len()).max().unwrap_or(0)
            });
            for row in &rows {
                let cells: Vec<String> = row
                    .iter()
                    .zip(widths)
                    .map(|(cell, width)| format!("{cell:<width$}"))
                    .collect();
                println!("{}", cells.join("  ").trim_end());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::utc;

    #[test]
    fn formats_unix_seconds_as_utc() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(1_709_209_140), "2024-02-29 12:19");
        assert_eq!(utc(1_794_830_400), "2026-11-16 12:00");
    }
}
