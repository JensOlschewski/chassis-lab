use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::input_hdf5::{Hdf5Input, open_hdf5_input};
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
        /// Input file (.res, .h5, or .hdf5).
        file: PathBuf,
        /// List all available signals or channels.
        #[arg(long)]
        signals: bool,
        /// Filter signals or channels by case-insensitive catalog text.
        #[arg(long)]
        r#match: Option<String>,
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
        Command::Inspect {
            file,
            signals,
            r#match,
        } => inspect(&file, signals, r#match.as_deref()),
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

fn inspect(path: &Path, signals: bool, match_text: Option<&str>) -> Result<()> {
    if !path.is_file() {
        bail!("input file does not exist: {}", path.display());
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("res") => inspect_res(path, signals || match_text.is_some(), match_text),
        Some("h5" | "hdf5") => inspect_hdf5(path, signals || match_text.is_some(), match_text),
        _ => bail!("unsupported input extension; expected .res, .h5, or .hdf5"),
    }
}

fn inspect_res(path: &Path, signals: bool, match_text: Option<&str>) -> Result<()> {
    let result = clab_converter::parser::parse_file(path)
        .with_context(|| format!("failed to parse '{}'", path.display()))?;
    println!(
        "{} samples, {} signals",
        result.sample_count,
        result.signal_count()
    );
    if signals {
        println!("ID\tENTITY\tCOMPONENT\tUNITS");
        for (entity, component) in result.values() {
            let catalog = format!(
                "{}\t{}\t{}\t{}",
                component.adams_id,
                entity.source_path.as_deref().unwrap_or(&entity.name),
                component.name,
                component.units.as_deref().unwrap_or("")
            );
            if !matches_catalog(&catalog, match_text) {
                continue;
            }
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

fn inspect_hdf5(path: &Path, signals: bool, match_text: Option<&str>) -> Result<()> {
    match open_hdf5_input(path)
        .with_context(|| format!("failed to inspect HDF5 '{}'", path.display()))?
    {
        Hdf5Input::Simulation(file) => {
            let summary = file.summary();
            println!("source file: {}", summary.source_file);
            println!("format version: {}", summary.format_version);
            println!("samples: {}", summary.sample_count);
            println!("signals: {}", summary.signal_count);
            println!("entities: {}", summary.entity_count);
            if signals {
                println!("ENTITY\tCOMPONENT\tUNIT\tADAMS ID");
                let signals = match match_text {
                    Some(text) => file.matching_signals(text),
                    None => file.signals().iter().collect(),
                };
                for signal in signals {
                    println!(
                        "{}\t{}\t{}\t{}",
                        signal.entity(),
                        signal.component,
                        signal.unit,
                        signal.adams_id
                    );
                }
            }
        }
        Hdf5Input::Measurement(file) => {
            let summary = file.summary();
            println!("source file: {}", summary.source_file);
            println!("format version: {}", summary.format_version);
            println!("samples: {}", summary.sample_count);
            println!("channels: {}", summary.channel_count);
            println!("test metadata: {}", summary.test_info_count);
            if let Some(encoding) = &summary.encoding {
                println!("encoding: {encoding}");
            }
            if let Some(dialect) = &summary.dialect {
                println!("dialect: {dialect}");
            }
            if signals {
                println!("ORIGINAL NAME\tUNIT\tOFFSET\tCOLUMN\tDESCRIPTION");
                let channels = match match_text {
                    Some(text) => file.matching_channels(text),
                    None => file.channels().iter().collect(),
                };
                for channel in channels {
                    println!(
                        "{}\t{}\t{}\t{}\t{}",
                        channel.original_name,
                        channel.unit,
                        channel.offset,
                        channel.column,
                        channel.description
                    );
                }
            }
        }
    }
    Ok(())
}

fn matches_catalog(catalog: &str, match_text: Option<&str>) -> bool {
    match_text.is_none_or(|text| catalog.to_lowercase().contains(&text.to_lowercase()))
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
