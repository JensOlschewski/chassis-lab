use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_converter::identity::Identity;
use clap::Args;

#[derive(Debug, Args)]
#[command(
    override_usage = "clab convert --domain <DOMAIN> [--test <TEST>] [OPTIONS] <INPUT>",
    after_help = "Domains and tests:\n  svc            no test\n  knc            --test vertical\n\nExample:\n  clab convert run.res -d knc -t vertical -o converted/"
)]
pub(crate) struct ConvertArgs {
    /// Adams RES simulation or Lommel ACD measurement input.
    input: PathBuf,
    /// Output HDF5 file, or a directory that receives the derived filename.
    /// A path ending in `/` is a directory and is created if missing.
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,
    /// Replace an existing output file.
    #[arg(long)]
    force: bool,
    /// Domain the data belongs to: svc or knc (required). Stored in the HDF5 file.
    #[arg(short, long, value_name = "DOMAIN")]
    domain: String,
    /// Test within the domain: required for knc (vertical), not allowed for svc.
    #[arg(short, long, value_name = "TEST", required_if_eq("domain", "knc"))]
    test: Option<String>,
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
        input.with_extension("h5")
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
    let identity = Identity::new(&args.domain, args.test.as_deref())?;
    let output = resolve_output(kind, &args.input, args.output.as_deref());
    if let (Some(requested), Some(parent)) = (args.output.as_deref(), output.parent())
        && is_directory_request(requested)
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory '{}'", parent.display()))?;
    }
    validate_output(&args.input, &output, args.force)?;

    let warnings = match kind {
        InputKind::Res => clab_converter::convert_res(&args.input, &output, args.force, &identity),
        InputKind::Acd => clab_converter::measurement_hdf5::import_acd(
            &args.input,
            &output,
            args.force,
            &identity,
        ),
    }
    .with_context(|| format!("failed to generate '{}'", output.display()))?;
    for warning in warnings {
        eprintln!("warning: {warning}");
    }

    println!("generated {} ({})", output.display(), kind.output_type());
    Ok(())
}

/// An existing directory, or a path written with a trailing separator.
fn is_directory_request(output: &Path) -> bool {
    let text = output.to_string_lossy();
    output.is_dir() || text.ends_with('/') || text.ends_with(std::path::MAIN_SEPARATOR)
}

fn resolve_output(kind: InputKind, input: &Path, output: Option<&Path>) -> PathBuf {
    match output {
        Some(output) if is_directory_request(output) => {
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
        let cli = Cli::try_parse_from([
            "clab",
            "convert",
            "input.res",
            "-d",
            "knc",
            "-t",
            "vertical",
            "-o",
            "output.h5",
        ])
        .unwrap();
        assert!(format!("{cli:?}").contains("Convert"));
        // --domain is required, also for svc.
        assert!(Cli::try_parse_from(["clab", "convert", "input.res"]).is_err());
        assert!(Cli::try_parse_from(["clab", "convert", "input.res", "--domain", "svc"]).is_ok());
    }
}
