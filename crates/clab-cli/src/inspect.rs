use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::input_hdf5::{Hdf5Input, open_hdf5_input};
use clab_core::svc_simulation_json::SvcSimulationFile;
use clap::{Args, ValueEnum};

use crate::signals;

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Mapping {
    Svc,
    Knc,
}

#[derive(Debug, Args)]
pub(crate) struct InspectArgs {
    /// Normalized SVC JSON or HDF5 simulation/measurement file.
    file: Option<PathBuf>,
    /// Show canonical mapping requirements instead of inspecting a file.
    #[arg(long, value_enum)]
    mapping: Option<Mapping>,
    /// List all available signals or channels.
    #[arg(long)]
    list: bool,
    /// Filter signals or channels by case-insensitive catalog text.
    #[arg(long)]
    r#match: Option<String>,
    /// Restrict KnC mapping output to one test.
    #[arg(long)]
    test: Option<String>,
    /// Emit editable TOML mapping skeleton.
    #[arg(long)]
    template: bool,
}

pub(crate) fn inspect(args: InspectArgs) -> Result<()> {
    if let Some(mapping) = args.mapping {
        if args.file.is_some() {
            bail!("--mapping cannot be combined with FILE");
        }
        if args.list || args.r#match.is_some() {
            bail!("--list and --match require FILE");
        }
        return signals::mapping(
            Some(match mapping {
                Mapping::Svc => signals::Domain::Svc,
                Mapping::Knc => signals::Domain::Knc,
            }),
            args.test.as_deref(),
            args.template,
        );
    }
    let Some(file) = args.file else {
        bail!("FILE is required unless --mapping is provided");
    };
    if args.test.is_some() || args.template {
        bail!("--test and --template require --mapping");
    }
    if !file.is_file() {
        bail!("input file does not exist: {}", file.display());
    }
    let extension = file
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("json") => inspect_svc_json(
            &file,
            args.list || args.r#match.is_some(),
            args.r#match.as_deref(),
        ),
        Some("h5" | "hdf5") => inspect_hdf5(
            &file,
            args.list || args.r#match.is_some(),
            args.r#match.as_deref(),
        ),
        _ => bail!("unsupported input extension; clab inspect accepts .json, .h5, or .hdf5"),
    }
}

fn inspect_svc_json(path: &Path, signals: bool, match_text: Option<&str>) -> Result<()> {
    let file = SvcSimulationFile::open(path)
        .with_context(|| format!("failed to inspect SVC JSON '{}'", path.display()))?;
    println!("source file: {}", file.source_file());
    println!("format version: 1");
    println!("samples: {}", file.sample_count());
    println!("signals: {}", file.signals().count());
    if signals {
        println!("ENTITY\tCOMPONENT\tUNIT\tADAMS ID");
        let matches = match match_text {
            Some(text) => file.matching_signals(text),
            None => file.signals().collect(),
        };
        for signal in matches {
            let entity = if signal.source_path.is_empty() {
                &signal.entity
            } else {
                &signal.source_path
            };
            println!(
                "{}\t{}\t{}\t{}",
                entity, signal.component, signal.unit, signal.adams_id
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
            if match_text.is_none() {
                println!("format version: {}", summary.format_version);
            }
            println!("samples: {}", summary.sample_count);
            if match_text.is_none() {
                println!("channels: {}", summary.channel_count);
                println!("test metadata: {}", summary.test_info_count);
                if let Some(encoding) = &summary.encoding {
                    println!("encoding: {encoding}");
                }
                if let Some(dialect) = &summary.dialect {
                    println!("dialect: {dialect}");
                }
            }
            if signals {
                let channels = match match_text {
                    Some(text) => file.matching_channels(text),
                    None => file.channels().iter().collect(),
                };
                let channel_width = channels
                    .iter()
                    .map(|channel| channel.group_name.len())
                    .max()
                    .unwrap_or(0)
                    .max("CHANNEL".len());
                let unit_width = channels
                    .iter()
                    .map(|channel| channel.unit.len())
                    .max()
                    .unwrap_or(0)
                    .max("UNIT".len());
                if match_text.is_some() {
                    for channel in &channels {
                        let values = file.read_channel(&channel.group_name, None)?.values;
                        let (count, minimum, maximum) =
                            values.into_iter().filter(|value| value.is_finite()).fold(
                                (0usize, f64::INFINITY, f64::NEG_INFINITY),
                                |(count, minimum, maximum), value| {
                                    (count + 1, minimum.min(value), maximum.max(value))
                                },
                            );
                        if channels.len() > 1 {
                            println!("channel: {}", channel.group_name);
                        }
                        if count == 0 {
                            println!("raw: no finite samples");
                        } else {
                            println!("raw: {minimum:.6}..{maximum:.6}");
                            println!("offset: {:.6}", channel.offset);
                            println!(
                                "normalized: {:.6}..{:.6}",
                                minimum - channel.offset,
                                maximum - channel.offset
                            );
                        }
                    }
                    println!();
                    println!(
                        "{:<channel_width$}  {:<unit_width$}  {:>14}  DESCRIPTION",
                        "CHANNEL", "UNIT", "SOURCE COLUMN"
                    );
                } else {
                    println!(
                        "{:<channel_width$}  {:<unit_width$}  {:>14} {:>14}  DESCRIPTION",
                        "CHANNEL", "UNIT", "OFFSET", "SOURCE COLUMN"
                    );
                }
                for channel in channels {
                    let description = if channel.original_name == channel.group_name {
                        channel.description.clone()
                    } else if channel.description.is_empty() {
                        format!("source name: {}", channel.original_name)
                    } else {
                        format!(
                            "source name: {}; {}",
                            channel.original_name, channel.description
                        )
                    };
                    if match_text.is_some() {
                        println!(
                            "{:<channel_width$}  {:<unit_width$}  {:>14}  {}",
                            channel.group_name, channel.unit, channel.column, description
                        );
                    } else {
                        println!(
                            "{:<channel_width$}  {:<unit_width$}  {:>14.6} {:>14}  {}",
                            channel.group_name,
                            channel.unit,
                            channel.offset,
                            channel.column,
                            description
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
