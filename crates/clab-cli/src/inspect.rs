use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use clab_core::svc::SvcSimulationFile;
use clab_core::view::ViewLayout;
use clap::{Args, ValueEnum};
use serde_json::{Value, json};

use crate::{signals, solution};

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
    /// Show a solver solution in the terminal. An optional view file (JSON)
    /// picks the bodies and points to draw.
    #[arg(long, value_name = "FILE", num_args = 0..=1)]
    view: Option<Option<PathBuf>>,
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
    let kind = detect(&file)?;
    if let Kind::Solution(solution) = &kind {
        if args.list || args.r#match.is_some() || !args.signal.is_empty() {
            bail!("--list, --match and --signal apply to HDF5 files, not solver solutions");
        }
        return inspect_solution(solution, args.json, args.view);
    }
    if args.view.is_some() {
        bail!("--view applies to solver solutions only");
    }
    if args.json {
        return inspect_json(
            &file,
            &kind,
            &args.signal,
            args.list || args.r#match.is_some(),
            args.r#match.as_deref(),
        );
    }
    if !args.signal.is_empty() {
        bail!("--signal requires --json");
    }
    let signals = args.list || args.r#match.is_some();
    match kind {
        Kind::SvcJson => inspect_svc_json(&file, signals, args.r#match.as_deref()),
        Kind::Hdf5 => inspect_hdf5(&file, signals, args.r#match.as_deref()),
        Kind::Solution(_) => unreachable!("handled above"),
    }
}

fn inspect_solution(solution: &Value, json: bool, view: Option<Option<PathBuf>>) -> Result<()> {
    if json && view.is_some() {
        bail!("--json and --view cannot be combined");
    }
    if let Some(layout) = view {
        let layout = layout.map(|path| ViewLayout::load(&path)).transpose()?;
        return view_solution(solution, layout.as_ref());
    }
    let summary = solution::render_summary(solution)?;
    if json {
        println!("{}", json!({ "kind": "solution", "summary": summary }));
    } else {
        print!("{summary}");
    }
    Ok(())
}

/// Opens the terminal viewer; shared by `inspect --view` and `solve --view`.
pub(crate) fn view_solution(solution: &Value, layout: Option<&ViewLayout>) -> Result<()> {
    let frames = clab_core::view::frames_from_solution(solution, layout)?;
    crate::view::run_viewer(frames)
}

/// What a file given to `inspect` holds.
enum Kind {
    Hdf5,
    SvcJson,
    /// A solver solution, already parsed.
    Solution(Value),
}

/// HDF5 is told by extension. A `.json` is a solver solution if it has the
/// solver's `time` and `v_hat`/`bodies` fields, otherwise it is normalized SVC JSON
/// (whose reader reports any further problem).
fn detect(path: &Path) -> Result<Kind> {
    match extension(path).as_deref() {
        Some("h5" | "hdf5") => Ok(Kind::Hdf5),
        Some("json") => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("cannot read '{}'", path.display()))?;
            Ok(match serde_json::from_str::<Value>(&text) {
                Ok(value)
                    if value.get("time").is_some()
                        && (value.get("v_hat").is_some() || value.get("bodies").is_some()) =>
                {
                    Kind::Solution(value)
                }
                _ => Kind::SvcJson,
            })
        }
        _ => bail!("unsupported input extension; clab inspect accepts .json, .h5, or .hdf5"),
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
}

/// Prints either the signal catalog or the axis plus the named signals' values.
fn inspect_json(
    path: &Path,
    kind: &Kind,
    names: &[String],
    list: bool,
    match_text: Option<&str>,
) -> Result<()> {
    if !matches!(kind, Kind::Hdf5) {
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
                "domain": summary.domain,
                "test": summary.test,
                "setup": summary.setup,
                "phase": summary.phase,
                "offset_mm": summary.offset_mm,
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
    let file = SvcSimulationFile::open(path)?;
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
    if let Some(domain) = &summary.domain {
        println!("domain: {domain}");
    }
    if let Some(test) = &summary.test {
        println!("test: {test}");
    }
    if let Some(setup) = &summary.setup {
        println!("setup: {setup}");
    }
    if let Some(phase) = &summary.phase {
        println!("phase: {phase}");
    }
    if let Some(offset) = summary.offset_mm {
        println!("offset: {offset} mm");
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(name: &str, text: &str) -> Result<Kind> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(name);
        std::fs::write(&path, text).unwrap();
        detect(&path)
    }

    #[test]
    fn json_with_solver_fields_is_a_solution() {
        assert!(matches!(
            kind_of("a.json", r#"{"time": [0.0], "v_hat": []}"#).unwrap(),
            Kind::Solution(_)
        ));
        assert!(matches!(
            kind_of("a.json", r#"{"time": [0.0], "bodies": []}"#).unwrap(),
            Kind::Solution(_)
        ));
    }

    #[test]
    fn other_json_is_left_to_the_svc_reader() {
        for text in [r#"{"schema_version": 1}"#, r#"{"time": [0.0]}"#, "not json"] {
            assert!(matches!(kind_of("a.json", text).unwrap(), Kind::SvcJson));
        }
    }

    #[test]
    fn hdf5_is_told_by_extension_and_others_are_refused() {
        assert!(matches!(kind_of("a.H5", "").unwrap(), Kind::Hdf5));
        assert!(matches!(kind_of("a.hdf5", "").unwrap(), Kind::Hdf5));
        assert!(kind_of("a.txt", "").is_err());
    }
}
