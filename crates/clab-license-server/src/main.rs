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
        #[arg(long, default_value_t = 7)]
        grace_days: u64,
    },
    /// Create the signing key. Refuses to replace an existing one.
    Keygen,
    /// Print the public key that clients embed.
    PublicKey,
    /// Create a license for a person and print its key once.
    Create {
        #[arg(long)]
        person: String,
    },
    /// Make a license stop working at the next online check.
    Revoke { id: i64 },
    /// Free the seat of a license, for example after a machine change.
    Unbind { id: i64 },
    /// Show all licenses.
    List,
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
        Command::Serve { listen, grace_days } => {
            let app = http::router(open(&data, grace_days)?);
            let listener = tokio::net::TcpListener::bind(&listen).await?;
            eprintln!("listening on {listen}, grace {grace_days} days");
            axum::serve(listener, app)
                .with_graceful_shutdown(shutdown())
                .await?;
        }
        Command::Keygen => keygen(&data)?,
        Command::PublicKey => println!("{}", encode_public_key(&load_key(&data)?.verifying_key())),
        Command::Create { person } => {
            let (id, key) = open(&data, 0)?.create(&person)?;
            println!("license {id} for {person}\n{key}");
        }
        Command::Revoke { id } => {
            if !open(&data, 0)?.revoke(id)? {
                bail!("no license {id}");
            }
        }
        Command::Unbind { id } => {
            if !open(&data, 0)?.unbind(id)? {
                bail!("no license {id}");
            }
        }
        Command::List => {
            let now = http::now();
            for license in open(&data, 0)?.list()? {
                let seat = match (&license.hostname, license.activated_at) {
                    (Some(host), Some(at)) => {
                        format!("{host}, {} days ago", (now as i64 - at).max(0) / 86_400)
                    }
                    _ => "free".to_owned(),
                };
                let state = if license.revoked { "revoked" } else { "active" };
                println!(
                    "{:>4}  {:<24} {:<8} {seat}",
                    license.id, license.person, state
                );
            }
        }
    }
    Ok(())
}
