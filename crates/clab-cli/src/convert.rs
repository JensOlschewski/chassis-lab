use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct ConvertArgs {
    /// SVC RES, vertical RES, or ACD measurement input.
    input: PathBuf,
    /// Output file. Defaults to INPUT.json for SVC RES, INPUT.simulation.h5 for
    /// vertical RES, or INPUT.measurement.h5 for ACD.
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
}

#[derive(Clone, Copy)]
enum InputKind {
    SvcRes,
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
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let stem = stem.to_ascii_lowercase();
            if stem == "svc" || stem.ends_with("_svc") || stem.ends_with("-svc") {
                Ok(Self::SvcRes)
            } else {
                Ok(Self::Res)
            }
        } else if extension.eq_ignore_ascii_case("acd") {
            Ok(Self::Acd)
        } else {
            bail!("unsupported input extension '.{extension}'; expected .res or .acd");
        }
    }

    fn default_output(self, input: &Path) -> PathBuf {
        match self {
            Self::SvcRes => input.with_extension("json"),
            Self::Res => input.with_extension("simulation.h5"),
            Self::Acd => input.with_extension("measurement.h5"),
        }
    }

    fn output_type(self) -> &'static str {
        match self {
            Self::SvcRes => "SVC JSON",
            Self::Res => "simulation HDF5",
            Self::Acd => "measurement HDF5",
        }
    }
}

pub(crate) fn convert(args: ConvertArgs) -> Result<()> {
    let kind = InputKind::from_path(&args.input)?;
    let output = args
        .output
        .unwrap_or_else(|| kind.default_output(&args.input));
    if matches!(kind, InputKind::SvcRes)
        && !output
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
    {
        bail!(
            "SVC RES output must use .json extension: {}",
            output.display()
        );
    }
    validate_output(&args.input, &output, args.force)?;

    match kind {
        InputKind::SvcRes => clab_converter::convert_svc_res(&args.input, &output, args.force),
        InputKind::Res => clab_converter::convert_res(&args.input, &output, args.force),
        InputKind::Acd => {
            clab_converter::measurement_hdf5::import_acd(&args.input, &output, args.force)
        }
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

#[cfg(test)]
mod tests {
    use crate::Cli;
    use clap::Parser;

    #[test]
    fn parses_convert_command() {
        let cli = Cli::try_parse_from(["clab", "convert", "input.res", "-o", "output.h5"]).unwrap();
        assert!(format!("{cli:?}").contains("Convert"));
    }
}
