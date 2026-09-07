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
        InputKind::Res => clab_converter::convert_res(input, &output),
        InputKind::Acd => clab_converter::measurement_hdf5::import_acd(input, &output),
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

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["clab", "convert", "input.res", "-o", "output.h5"]).unwrap();
        assert!(format!("{cli:?}").contains("Convert"));
    }
}
