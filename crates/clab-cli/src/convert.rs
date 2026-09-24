use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct ConvertArgs {
    /// Adams RES simulation or Lommel ACD measurement input.
    input: PathBuf,
    /// Output HDF5 file. An existing directory receives derived output filename.
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
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
            Self::Res => "canonical simulation HDF5",
            Self::Acd => "canonical measurement HDF5",
        }
    }
}

pub(crate) fn convert(args: ConvertArgs) -> Result<()> {
    let kind = InputKind::from_path(&args.input)?;
    let output = resolve_output(kind, &args.input, args.output.as_deref());
    validate_output(&args.input, &output, args.force)?;

    match kind {
        InputKind::Res => clab_converter::convert_res(&args.input, &output, args.force),
        InputKind::Acd => {
            clab_converter::measurement_hdf5::import_acd(&args.input, &output, args.force)
        }
    }
    .with_context(|| format!("failed to generate '{}'", output.display()))?;

    println!("generated {} ({})", output.display(), kind.output_type());
    Ok(())
}

fn resolve_output(kind: InputKind, input: &Path, output: Option<&Path>) -> PathBuf {
    match output {
        Some(output) if output.is_dir() => {
            let derived = kind.default_output(input);
            let filename = derived.file_name().expect("derived output has a filename");
            output.join(filename)
        }
        Some(output) => output.to_path_buf(),
        None => kind.default_output(input),
    }
}

fn validate_output(input: &Path, output: &Path, force: bool) -> Result<()> {
    if output.exists() && std::fs::canonicalize(input)? == std::fs::canonicalize(output)? {
        bail!(
            "output path must not be the same as input: {}",
            output.display()
        );
    }
    let extension = output.extension().and_then(|extension| extension.to_str());
    if !matches!(extension, Some("h5" | "hdf5")) {
        bail!(
            "output file must use .h5 or .hdf5 extension: {}",
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
