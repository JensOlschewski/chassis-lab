use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::input_hdf5::{Hdf5Input, open_hdf5_input};
use clap::Args;

#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    /// Normalized HDF5 simulation or measurement file (.h5 or .hdf5).
    file: PathBuf,
    /// List all available signals or channels.
    #[arg(long)]
    signals: bool,
    /// Filter signals or channels by case-insensitive catalog text.
    #[arg(long)]
    r#match: Option<String>,
}

pub(crate) fn inspect(args: InspectArgs) -> Result<()> {
    if !args.file.is_file() {
        bail!("input file does not exist: {}", args.file.display());
    }
    let extension = args
        .file
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("h5" | "hdf5") => inspect_hdf5(
            &args.file,
            args.signals || args.r#match.is_some(),
            args.r#match.as_deref(),
        ),
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
