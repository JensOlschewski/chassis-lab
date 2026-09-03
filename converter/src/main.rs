use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;

use adams_res_converter::{hdf5_writer, parser};

#[derive(Debug, Parser)]
#[command(
    name = "adams-res",
    version,
    about = "Convert an Adams/Car text result file (.res) to HDF5"
)]
struct Cli {
    /// Adams/Car text result file
    input: PathBuf,

    /// Output HDF5 file; defaults to INPUT with the .h5 extension
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,

    /// Replace an existing output file
    #[arg(short, long)]
    force: bool,

    /// List every available value series instead of creating an HDF5 file
    #[arg(short, long)]
    list: bool,
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("error: {error:#}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<()> {
    validate_input(&cli.input)?;

    if cli.list {
        let result = parser::parse_file(&cli.input)?;
        print_values(&result);
        return Ok(());
    }

    let output = cli.output.unwrap_or_else(|| cli.input.with_extension("h5"));

    if output.exists() && !cli.force {
        bail!(
            "output file '{}' already exists; pass --force to replace it",
            output.display()
        );
    }

    let result = parser::parse_file(&cli.input)?;
    hdf5_writer::write_file(&output, &cli.input, &result)
        .with_context(|| format!("failed to write '{}'", output.display()))?;

    println!(
        "Converted {} events and {} signals to {}",
        result.sample_count,
        result.signal_count(),
        output.display()
    );

    Ok(())
}

fn print_values(result: &adams_res_converter::model::AdamsResult) {
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

fn validate_input(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!(
            "input file '{}' does not exist or is not a file",
            path.display()
        );
    }

    let is_res = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("res"));

    if !is_res {
        bail!(
            "input file '{}' must have the .res extension",
            path.display()
        );
    }

    Ok(())
}
