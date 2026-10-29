use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use clab_core::svc::SvcSimulationFile;
use clap::{Args, ValueEnum};
use serde_json::json;

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
    /// Read one signal by its listed name; repeatable. Requires --json.
    #[arg(long)]
    signal: Vec<String>,
    /// Emit JSON for --list/--match or --signal (HDF5 only).
    #[arg(long)]
    json: bool,
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
    if args.json {
        return inspect_json(
            &file,
            &args.signal,
            args.list || args.r#match.is_some(),
            args.r#match.as_deref(),
        );
    }
    if !args.signal.is_empty() {
        bail!("--signal requires --json");
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

/// Prints either the signal catalog or the axis plus the named signals' values.
fn inspect_json(path: &Path, names: &[String], list: bool, match_text: Option<&str>) -> Result<()> {
    let is_hdf5 = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "h5" | "hdf5"));
    if !is_hdf5 {
        bail!("--json supports .h5 or .hdf5 files only");
    }
    if list == !names.is_empty() {
        bail!("--json needs exactly one of --list/--match or --signal");
    }
    let file = CanonicalFile::open(path)
        .with_context(|| format!("failed to inspect HDF5 '{}'", path.display()))?;
    let document = if list {
        let rows = match match_text {
            Some(text) => file.matching_signals(text),
            None => file.signals().iter().collect(),
        };
        let signals: Vec<_> = rows
            .into_iter()
            .map(|signal| {
                json!({
                    "name": signal.safe_name,
                    "component": signal.component,
                    "unit": signal.unit,
                    "source_path": signal.source_path,
                })
            })
            .collect();
        let summary = file.summary();
        json!({
            "summary": {
                "source_file": summary.source_file,
                "source_kind": summary.source_kind.as_str(),
                "schema_version": summary.schema_version,
                "samples": summary.sample_count,
                "signals": summary.signal_count,
                "test_metadata": (summary.source_kind == SourceKind::Measurement)
                    .then(|| file.test_info().len()),
            },
            "signals": signals,
        })
    } else {
        let signals = names
            .iter()
            .map(|name| {
                let signal = file.read_signal(name, None, None)?;
                Ok(json!({
                    "name": signal.info.safe_name,
                    "unit": signal.info.unit,
                    "values": signal.values,
                }))
            })
            .collect::<Result<Vec<_>>>()?;
        json!({ "axis": file.read_axis()?, "signals": signals })
    };
    println!("{document}");
    Ok(())
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
    let file = CanonicalFile::open(path)
        .with_context(|| format!("failed to inspect HDF5 '{}'", path.display()))?;
    let summary = file.summary();
    if let Some(warning) = summary.legacy_warning() {
        eprintln!("warning: {warning}");
    }
    println!("source file: {}", summary.source_file);
    println!("schema version: {}", summary.schema_version);
    println!("source kind: {}", summary.source_kind.as_str());
    println!("samples: {}", summary.sample_count);
    println!("signals: {}", summary.signal_count);
    if summary.source_kind == SourceKind::Measurement {
        println!("test metadata: {}", file.test_info().len());
    }
    if signals {
        let rows = match match_text {
            Some(text) => file.matching_signals(text),
            None => file.signals().iter().collect(),
        };
        println!("SIGNAL\tCOMPONENT\tUNIT\tSOURCE");
        for signal in rows {
            println!(
                "{}\t{}\t{}\t{}",
                signal.safe_name, signal.component, signal.unit, signal.source_path
            );
        }
    }
    Ok(())
}
