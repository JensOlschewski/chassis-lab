use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "clab", version, about = "ChassisLab vehicle analysis CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate configuration and its referenced inputs.
    Check {
        /// TOML case configuration.
        config: PathBuf,
    },
    /// Inspect a supported input without writing output.
    Inspect {
        /// Input file. RES inspection is currently supported.
        file: PathBuf,
        /// List all available RES signals.
        #[arg(long)]
        signals: bool,
    },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(2);
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Check { config } => check(&config),
        Command::Inspect { file, signals } => inspect(&file, signals),
    }
}

fn check(path: &Path) -> Result<()> {
    let config = clab_core::config::load_config(path).map_err(|error| {
        anyhow::anyhow!("failed to load configuration '{}': {error}", path.display())
    })?;
    if !config.simulation_hdf5.is_file() {
        bail!(
            "simulation HDF5 does not exist: {}",
            config.simulation_hdf5.display()
        );
    }
    if let Some(validation) = &config.validation {
        clab_core::svc_reference::load_svc_reference(&validation.reference).map_err(|error| {
            anyhow::anyhow!(
                "failed to load SVC reference '{}': {error}",
                validation.reference.display()
            )
        })?;
    }
    if let Some(vertical) = &config.vertical {
        for path in [&vertical.measurement_acd, &vertical.simulation_hdf5] {
            if !path.is_file() {
                bail!("vertical input does not exist: {}", path.display());
            }
        }
    }
    println!("configuration is valid: {}", config.path.display());
    Ok(())
}

fn inspect(path: &Path, signals: bool) -> Result<()> {
    if !path.is_file() {
        bail!("input file does not exist: {}", path.display());
    }
    let is_res = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("res"));
    if !is_res {
        bail!("RES inspection currently requires a .res input");
    }
    let result = adams_res_converter::parser::parse_file(path)
        .with_context(|| format!("failed to parse '{}'", path.display()))?;
    println!(
        "{} samples, {} signals",
        result.sample_count,
        result.signal_count()
    );
    if signals {
        println!("ID\tENTITY\tCOMPONENT\tUNITS");
        for (entity, component) in result.values() {
            println!(
                "{}\t{}\t{}\t{}",
                component.adams_id,
                entity.source_path.as_deref().unwrap_or(&entity.name),
                component.name,
                component.units.as_deref().unwrap_or("")
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_check_command() {
        let cli = Cli::try_parse_from(["clab", "check", "case.toml"]).unwrap();
        assert!(format!("{cli:?}").contains("Check"));
    }
}
