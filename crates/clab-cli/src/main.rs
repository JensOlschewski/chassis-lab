use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::input_hdf5::{Hdf5Input, open_hdf5_input};
use clap::{Parser, Subcommand};

mod vertical;

#[derive(Debug, Parser)]
#[command(name = "clab", version, about = "ChassisLab vehicle analysis CLI")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate a configured analysis domain.
    Validate {
        #[command(subcommand)]
        domain: ValidateDomain,
    },
    /// Validate configuration and its referenced inputs.
    Check {
        /// TOML case configuration.
        config: PathBuf,
    },
    /// Inspect a supported input without writing output.
    Inspect {
        /// Normalized HDF5 simulation or measurement file (.h5 or .hdf5).
        file: PathBuf,
        /// List all available signals or channels.
        #[arg(long)]
        signals: bool,
        /// Filter signals or channels by case-insensitive catalog text.
        #[arg(long)]
        r#match: Option<String>,
    },
    /// Convert a supported input to HDF5.
    Convert {
        /// RES simulation result or ACD measurement input.
        input: PathBuf,
        /// Output file. Defaults to INPUT.simulation.h5 or INPUT.measurement.h5.
        #[arg(short, long, value_name = "OUTPUT")]
        output: Option<PathBuf>,
        /// Replace an existing output file.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Subcommand)]
enum ValidateDomain {
    /// Compare normalized vertical measurement and simulation trajectories.
    Vertical {
        /// TOML case configuration.
        config: PathBuf,
        #[arg(long)]
        plot_output: Option<PathBuf>,
        #[arg(long)]
        json_output: Option<PathBuf>,
        #[arg(long)]
        csv_output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
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
        Command::Validate { domain } => match domain {
            ValidateDomain::Vertical {
                config,
                plot_output,
                json_output,
                csv_output,
                force,
            } => vertical::validate(
                &config,
                plot_output.as_deref(),
                json_output.as_deref(),
                csv_output.as_deref(),
                force,
            ),
        },
        Command::Check { config } => check(&config),
        Command::Inspect {
            file,
            signals,
            r#match,
        } => inspect(&file, signals, r#match.as_deref()),
        Command::Convert {
            input,
            output,
            force,
        } => convert(&input, output.as_deref(), force),
    }
}

#[derive(Clone, Copy)]
enum InputKind {
    Res,
    Acd,
}

impl InputKind {
    fn from_path(path: &Path) -> Result<Self> {
        if !path.is_file() {
            bail!(
                "input file does not exist or is not a file: {}",
                path.display()
            );
        }
        let extension = path.extension().ok_or_else(|| {
            anyhow::anyhow!("input file has no extension; supported extensions are .res and .acd")
        })?;
        let extension = extension.to_str().ok_or_else(|| {
            anyhow::anyhow!("input file extension is not valid UTF-8; expected .res or .acd")
        })?;
        if extension.eq_ignore_ascii_case("res") {
            Ok(Self::Res)
        } else if extension.eq_ignore_ascii_case("acd") {
            Ok(Self::Acd)
        } else {
            bail!("unsupported input extension '.{extension}'; expected .res or .acd");
        }
    }

    fn default_output(self, input: &Path) -> PathBuf {
        match self {
            Self::Res => input.with_extension("simulation.h5"),
            Self::Acd => input.with_extension("measurement.h5"),
        }
    }

    fn output_type(self) -> &'static str {
        match self {
            Self::Res => "simulation HDF5",
            Self::Acd => "measurement HDF5",
        }
    }
}

fn convert(input: &Path, explicit_output: Option<&Path>, force: bool) -> Result<()> {
    let kind = InputKind::from_path(input)?;
    let output = explicit_output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| kind.default_output(input));
    validate_output(input, &output, force)?;

    match kind {
        InputKind::Res => clab_converter::convert_res(input, &output, force),
        InputKind::Acd => clab_converter::measurement_hdf5::import_acd(input, &output, force),
    }
    .with_context(|| format!("failed to generate '{}'", output.display()))?;

    println!("generated {} ({})", output.display(), kind.output_type());
    Ok(())
}

fn validate_output(input: &Path, output: &Path, force: bool) -> Result<()> {
    if output.exists() && std::fs::canonicalize(input)? == std::fs::canonicalize(output)? {
        bail!(
            "output path must not be the same as input: {}",
            output.display()
        );
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        bail!(
            "output parent directory does not exist: {}",
            parent.display()
        );
    }
    if output.is_dir() {
        bail!("output path is a directory: {}", output.display());
    }
    if output.exists() && !force {
        bail!(
            "output file already exists: {}; pass --force to replace it",
            output.display()
        );
    }
    Ok(())
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
        for path in [&vertical.measurement_hdf5, &vertical.simulation_hdf5] {
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
        Some("h5" | "hdf5") => inspect_hdf5(path, signals || match_text.is_some(), match_text),
        _ => bail!(
            "unsupported input extension; clab inspect accepts only .h5 or .hdf5; convert raw .res or .acd inputs first with clab convert"
        ),
    }
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

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn parses_check_command() {
        let cli = Cli::try_parse_from(["clab", "check", "case.toml"]).unwrap();
        assert!(format!("{cli:?}").contains("Check"));
    }

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["clab", "convert", "input.res", "-o", "output.h5"]).unwrap();
        assert!(format!("{cli:?}").contains("Convert"));
    }
}
