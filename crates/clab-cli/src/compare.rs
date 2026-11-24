use std::{
    collections::{BTreeMap, BTreeSet},
    env, fmt, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
use clab_core::canonical_hdf5::SourceKind;
use clap::Args;
use serde::Serialize;
use sha2::Digest;

use crate::{
    analysis::{
        OpenFiles, display,
        knc::{self, KncSubtest},
        svc,
    },
    analyze::{absolute_path, debug_info, write_report_bundle},
    progress::ReportProgress,
};

#[derive(Debug, Args)]
pub(crate) struct CompareArgs {
    /// Shared TOML configuration: mappings, units, conventions, physics, and setup.
    config: PathBuf,
    /// Baseline SVC JSON, normalized HDF5, or run TOML manifest.
    baseline: PathBuf,
    /// Candidate SVC JSON, normalized HDF5, or run TOML manifests.
    #[arg(required = true, num_args = 1..)]
    candidates: Vec<PathBuf>,
    /// Directory for report bundle.
    #[arg(short = 'r', long = "report")]
    output: Option<PathBuf>,
    /// Legend name per input in order, baseline first; repeat once per input.
    /// An empty name keeps the default ("Baseline", "Curve N").
    #[arg(short, long = "label")]
    labels: Vec<String>,
    /// Apply validation tolerances from configuration.
    #[arg(short, long)]
    tolerances: bool,
    /// Replace report bundle already in output directory.
    #[arg(long)]
    force: bool,
    /// Include KnC rate-filter diagnostic plots in the report (requires --report).
    #[arg(long, requires = "output")]
    debug: bool,
    /// Print one JSON line per comparison section instead of console text.
    #[arg(long)]
    json: bool,
}

#[derive(Debug)]
pub(crate) struct ToleranceFailure;
impl fmt::Display for ToleranceFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("comparison failed tolerance checks")
    }
}
impl std::error::Error for ToleranceFailure {}

#[derive(Clone, Serialize)]
struct Metric {
    test: &'static str,
    subtest: Option<&'static str>,
    metric: String,
    unit: String,
    baseline_value: Option<f64>,
    candidate_value: Option<f64>,
    difference: Option<f64>,
    absolute_tolerance: Option<f64>,
    relative_percent: Option<f64>,
    allowed_difference: Option<f64>,
    status: &'static str,
}

/// Curves of one KnC input file. Metrics of a subtest are merged across its
/// files; plots are not, so a longitudinal B0 run is never overlaid on B1.
struct KncRun {
    source: PathBuf,
    curves: serde_json::Value,
    conditions: knc::Conditions,
    branch: clab_core::config::VerticalBranch,
    /// This series' own `ride_frequency` mass map (same source as
    /// `knc::Reports::corner_mass_kg`): baseline and candidate
    /// resolve it from their own SVC run, so it travels per series,
    /// not shared.
    corner_mass_kg: BTreeMap<String, f64>,
}

impl KncRun {
    /// Longitudinal runs pair by brake state, every other subtest has one run.
    fn pairs_with(&self, other: &Self, subtest: KncSubtest) -> bool {
        subtest != KncSubtest::Longitudinal || self.conditions.brakes() == other.conditions.brakes()
    }
}

/// Condition document of a subtest: one object per run, an array for several.
fn runs_json(runs: &[KncRun]) -> serde_json::Value {
    match runs {
        [run] => run.conditions.json(),
        runs => runs.iter().map(|run| run.conditions.json()).collect(),
    }
}

/// A candidate run paired with a baseline run, with its plot footer line.
struct CandidateRun<'a> {
    label: String,
    run: &'a KncRun,
    footer: String,
}

enum ComparedTest {
    Svc(Vec<svc::CanonicalMetric>),
    Knc {
        subtest: KncSubtest,
        metrics: Vec<serde_json::Value>,
        runs: Vec<KncRun>,
    },
}

impl ComparedTest {
    /// KnC subtest of the test; `None` for SVC.
    fn subtest(&self) -> Option<KncSubtest> {
        match self {
            Self::Svc(_) => None,
            Self::Knc { subtest, .. } => Some(*subtest),
        }
    }

    fn title(&self) -> &'static str {
        self.subtest().map_or("SVC", KncSubtest::title)
    }

    /// Report test name: `svc` or `knc`.
    fn test(&self) -> &'static str {
        if self.subtest().is_some() {
            "knc"
        } else {
            "svc"
        }
    }
}

/// Keys of `Input::tests` in console order; KnC tests compare separately.
const TEST_KEYS: [&str; 4] = ["svc", "knc-vertical", "knc-roll", "knc-longitudinal"];

fn knc_key(subtest: KncSubtest) -> &'static str {
    match subtest {
        KncSubtest::Vertical => "knc-vertical",
        KncSubtest::Roll => "knc-roll",
        KncSubtest::Longitudinal => "knc-longitudinal",
    }
}

struct Input {
    config: clab_core::config::CaseConfig,
    tests: BTreeMap<&'static str, ComparedTest>,
    sources: Vec<PathBuf>,
}

/// Per-input identity and conditions shared by console, JSON and CSV output.
#[derive(Serialize)]
struct InputSummary {
    label: String,
    path: PathBuf,
    conditions: BTreeMap<&'static str, TestConditions>,
}

/// Conditions of every run of one subtest: serialized as an object for a single
/// run, as an array for several.
#[derive(Clone)]
struct TestConditions(Vec<knc::Conditions>);

impl Serialize for TestConditions {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0.as_slice() {
            [one] => one.serialize(serializer),
            many => many.serialize(serializer),
        }
    }
}

impl TestConditions {
    /// CSV cells; the setups of several runs are joined with ` / `.
    fn csv_cells(&self) -> [String; 3] {
        let mut cells: [Vec<String>; 3] = Default::default();
        for run in &self.0 {
            for (column, cell) in cells.iter_mut().zip(run.csv_cells()) {
                column.push(cell);
            }
        }
        cells.map(|column| column.join(" / "))
    }
}

struct ComparisonTolerances {
    svc: clab_core::config::ValidationTolerances,
    /// Per KnC subtest, keyed like `Input::tests`.
    knc: BTreeMap<&'static str, clab_core::config::ValidationTolerances>,
}

type GroupedMetrics<'a> = Vec<(Option<&'a str>, &'a str, Vec<(usize, &'a Metric)>)>;

const MAX_LABEL_CHARS: usize = 64;

/// A trimmed label, `None` when blank so the default applies.
pub(crate) fn parse_label(label: &str) -> Result<Option<String>> {
    let label = label.trim();
    if label.chars().count() > MAX_LABEL_CHARS {
        bail!("label '{label}' is longer than {MAX_LABEL_CHARS} characters");
    }
    if label.chars().any(char::is_control) {
        bail!(
            "label '{}' contains control characters",
            label.escape_debug()
        );
    }
    Ok((!label.is_empty()).then(|| label.to_owned()))
}

/// Explicit label per input (baseline first), `None` where the default applies.
fn parse_labels(labels: &[String], inputs: usize) -> Result<Vec<Option<String>>> {
    if !labels.is_empty() && labels.len() != inputs {
        bail!(
            "expected {inputs} --label values (one per input, baseline first), got {}",
            labels.len()
        );
    }
    let mut parsed = labels
        .iter()
        .map(|label| parse_label(label))
        .collect::<Result<Vec<_>>>()?;
    parsed.resize(inputs, None);
    let mut seen = std::collections::BTreeSet::new();
    for index in 0..inputs {
        let label = display_label(&parsed, index);
        if !seen.insert(label.clone()) {
            bail!("label '{label}' is used by more than one input");
        }
    }
    Ok(parsed)
}

fn display_label(labels: &[Option<String>], index: usize) -> String {
    labels[index].clone().unwrap_or_else(|| match index {
        0 => "Baseline".to_owned(),
        _ => format!("Curve {index}"),
    })
}

/// A run's `[meta] name` as label; `None` when absent (`undefined`) or unusable.
pub(crate) fn meta_label(name: &str) -> Option<String> {
    parse_label(name)
        .ok()
        .flatten()
        .filter(|name| name != "undefined")
}

/// Explicit labels, else run names; a run name clashing with another label
/// falls back to its default.
fn resolve_labels(explicit: Vec<Option<String>>, names: &[&str]) -> Vec<Option<String>> {
    let mut labels = explicit;
    let derived: Vec<bool> = labels.iter().map(Option::is_none).collect();
    for (label, name) in labels.iter_mut().zip(names) {
        if label.is_none() {
            *label = meta_label(name);
        }
    }
    loop {
        let shown: Vec<_> = (0..labels.len())
            .map(|index| display_label(&labels, index))
            .collect();
        let clashes: Vec<_> = (0..labels.len())
            .filter(|&index| {
                derived[index]
                    && labels[index].is_some()
                    && shown.iter().filter(|label| **label == shown[index]).count() > 1
            })
            .collect();
        if clashes.is_empty() {
            return labels;
        }
        for index in clashes {
            labels[index] = None;
        }
    }
}

pub(crate) fn compare(args: CompareArgs) -> Result<()> {
    let explicit = parse_labels(&args.labels, args.candidates.len() + 1)?;
    let baseline = load_input(&args.config, &args.baseline)?;
    let loaded = args
        .candidates
        .iter()
        .map(|path| load_input(&args.config, path))
        .collect::<Result<Vec<_>>>()?;
    let names: Vec<_> = std::iter::once(&baseline)
        .chain(&loaded)
        .map(|input| input.config.name.as_str())
        .collect();
    let labels = resolve_labels(explicit, &names);
    let inputs: Vec<_> = std::iter::once((&args.baseline, &baseline))
        .chain(args.candidates.iter().zip(&loaded))
        .enumerate()
        .map(|(index, (path, input))| InputSummary {
            label: display_label(&labels, index),
            path: path.clone(),
            conditions: input
                .tests
                .values()
                .filter_map(|test| match test {
                    ComparedTest::Knc { subtest, runs, .. } => Some((
                        subtest.name(),
                        TestConditions(runs.iter().map(|run| run.conditions.clone()).collect()),
                    )),
                    ComparedTest::Svc(_) => None,
                })
                .collect(),
        })
        .collect();
    let tolerances = args.tolerances.then(|| ComparisonTolerances {
        svc: baseline
            .config
            .validation
            .as_ref()
            .map(|v| v.tolerances.clone())
            .unwrap_or_else(empty_tolerances),
        knc: KncSubtest::ALL
            .into_iter()
            .filter_map(|subtest| subtest.config(&baseline.config).map(|knc| (subtest, knc)))
            .map(|(subtest, knc)| (knc_key(subtest), knc.tolerances.clone()))
            .collect(),
    });
    let mut failed = false;
    let mut incomplete = false;
    let mut rows = Vec::new();
    let mut candidates = Vec::new();
    let mut plots = BTreeMap::new();
    let mut knc_candidates: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    let mut candidate_sources = Vec::new();
    let mut unavailable_tests = Vec::new();
    let mut warnings: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (index, path) in args.candidates.iter().enumerate() {
        let candidate = &loaded[index];
        let mut metrics = Vec::new();
        let mut tests = Vec::new();
        for (name, base_test) in &baseline.tests {
            let Some(candidate_test) = candidate.tests.get(name) else {
                incomplete = true;
                unavailable_tests.push((index, *name));
                tests.push(serde_json::json!({"test": base_test.test(), "subtest": base_test.subtest().map(KncSubtest::name), "status": "not compared"}));
                continue;
            };
            match (base_test, candidate_test) {
                (ComparedTest::Svc(base), ComparedTest::Svc(candidate_values)) => {
                    let compared = compare_metrics(
                        base,
                        candidate_values,
                        tolerances.as_ref().map(|value| &value.svc),
                    );
                    failed |= compared.iter().any(|metric| metric.status == "failed");
                    incomplete |= compared.iter().any(|metric| metric.status == "missing");
                    metrics.extend(compared);
                    tests.push(
                        serde_json::json!({"test": "svc", "subtest": null, "status": "compared"}),
                    );
                }
                (
                    ComparedTest::Knc {
                        subtest: base_subtest,
                        metrics: base_metrics,
                        runs: base_runs,
                    },
                    ComparedTest::Knc {
                        subtest,
                        metrics: candidate_metrics,
                        runs: candidate_runs,
                    },
                ) if base_subtest == subtest => {
                    let compared = compare_knc_metrics(
                        *subtest,
                        base_metrics,
                        candidate_metrics,
                        tolerances.as_ref().map(|value| &value.knc[name]),
                    );
                    failed |= compared.iter().any(|metric| metric.status == "failed");
                    incomplete |= compared.iter().any(|metric| metric.status == "missing");
                    metrics.extend(compared);
                    tests.push(serde_json::json!({"test": "knc", "subtest": subtest.name(), "status": "compared", "conditions": runs_json(candidate_runs)}));
                    let label = display_label(&labels, index + 1);
                    let skipped = not_compared(base_metrics, candidate_metrics);
                    if !skipped.is_empty() {
                        warnings.entry(*name).or_default().push(display::tag_warning(
                            "knc",
                            Some(subtest.name()),
                            format!(
                                "{} metrics not compared, the stored setups of {} and {} do not both cover them: {}",
                                skipped.len(),
                                display_label(&labels, 0),
                                label,
                                skipped.join(", "),
                            ),
                        ));
                    }
                    for base_run in base_runs {
                        for candidate_run in candidate_runs
                            .iter()
                            .filter(|run| base_run.pairs_with(run, *subtest))
                        {
                            let (base_conditions, candidate_conditions) =
                                (&base_run.conditions, &candidate_run.conditions);
                            let differences = base_conditions.differences(candidate_conditions);
                            if !differences.is_empty() {
                                warnings.entry(*name).or_default().push(display::tag_warning(
                                    "knc",
                                    Some(subtest.name()),
                                    format!(
                                        "subtest conditions differ ({}): {} {} vs {} {}; results compare different physical cases",
                                        differences.join(", "),
                                        display_label(&labels, 0),
                                        base_conditions.summary(),
                                        label,
                                        candidate_conditions.summary(),
                                    ),
                                ));
                            }
                            knc_candidates
                                .entry(subtest.name())
                                .or_default()
                                .push(CandidateRun {
                                    label: label.clone(),
                                    run: candidate_run,
                                    footer: format!(
                                        "{label}: {} | {} | {} | {}",
                                        candidate.config.name,
                                        candidate.config.loadcase,
                                        candidate_conditions.summary(),
                                        absolute_path(path)?.display()
                                    ),
                                });
                        }
                    }
                }
                _ => bail!(
                    "incompatible comparison tests between '{}' and '{}'",
                    args.baseline.display(),
                    path.display()
                ),
            }
        }
        if baseline.tests.len() == 1
            && candidate.tests.len() == 1
            && baseline.tests.keys().next() != candidate.tests.keys().next()
        {
            bail!(
                "cannot compare different tests: '{}' is {}, '{}' is {}",
                args.baseline.display(),
                baseline.tests.keys().next().unwrap(),
                path.display(),
                candidate.tests.keys().next().unwrap(),
            );
        }
        rows.extend(metrics.iter().cloned().map(|metric| (path.clone(), metric)));
        candidate_sources.extend(candidate.sources.iter().cloned());
        let metrics = display::console_ordered(
            &serde_json::to_value(&metrics)?
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        let mut entry = serde_json::json!({"input": provenance(path)?, "case": {"name": candidate.config.name, "loadcase": candidate.config.loadcase}, "metrics": metrics, "tests": tests});
        if let Some(label) = &labels[index + 1] {
            entry["label"] = label.as_str().into();
        }
        candidates.push(entry);
    }
    if args.debug {
        for subtest in KncSubtest::ALL {
            let key = knc_key(subtest);
            let Some(ComparedTest::Knc { runs, .. }) = baseline.tests.get(key) else {
                continue;
            };
            if runs.iter().any(|run| !run.branch.measurement)
                || knc_candidates
                    .get(subtest.name())
                    .is_some_and(|candidates| {
                        candidates
                            .iter()
                            .any(|candidate| !candidate.run.branch.measurement)
                    })
            {
                warnings.entry(key).or_default().push(display::tag_warning(
                    "knc",
                    Some(subtest.name()),
                    knc::DEBUG_RATE_WARNING,
                ));
            }
        }
    }
    let show = if args.json {
        json_metrics
    } else {
        print_metrics
    };
    show(
        &inputs,
        &args.candidates,
        &baseline.tests,
        &rows,
        &unavailable_tests,
        args.tolerances,
        &warnings,
    );
    let status = comparison_status(failed, incomplete, &rows, args.tolerances);
    let progress = ReportProgress::new(args.output.is_some(), args.json);
    let mut plot_jobs: Vec<(&str, Vec<_>)> = Vec::new();
    if args.output.is_some() {
        for subtest in KncSubtest::ALL {
            let Some(ComparedTest::Knc { runs, .. }) = baseline.tests.get(knc_key(subtest)) else {
                continue;
            };
            let candidates = knc_candidates
                .get(subtest.name())
                .map_or(&[][..], Vec::as_slice);
            for run in runs {
                let paired: Vec<_> = candidates
                    .iter()
                    .filter(|candidate| run.pairs_with(candidate.run, subtest))
                    .collect();
                if subtest == KncSubtest::Longitudinal && paired.is_empty() {
                    continue;
                }
                match plot_jobs.last_mut() {
                    Some((title, group)) if *title == subtest.title() => {
                        group.push((subtest, run, paired));
                    }
                    _ => plot_jobs.push((subtest.title(), vec![(subtest, run, paired)])),
                }
            }
        }
    }
    let workers = knc::plot_worker_limit(plot_jobs.iter().map(|(_, group)| group.len()).sum());
    // One progress job per subtest; its brake states render in turn.
    let rendered = progress.plot_phases(plot_jobs, |group| {
        group
            .into_iter()
            .map(|(subtest, base_run, candidates_for_subtest)| {
                let mut footer = vec![format!(
                    "Config: {}",
                    absolute_path(&args.config)?.display()
                )];
                footer.push(format!(
                    "{}: {} | {} | {} | {}",
                    display_label(&labels, 0),
                    baseline.config.name,
                    baseline.config.loadcase,
                    base_run.conditions.summary(),
                    absolute_path(&args.baseline)?.display()
                ));
                footer.extend(
                    candidates_for_subtest
                        .iter()
                        .map(|candidate| candidate.footer.clone()),
                );
                if let Some(warnings) = warnings.get(knc_key(subtest)) {
                    footer.extend(warnings.iter().map(|warning| format!("WARNING: {warning}")));
                }
                footer.push(format!("Created by: {}", report_creator()));
                footer.push(format!("clab version: {}", env!("CARGO_PKG_VERSION")));
                let debug = debug_info(
                    args.debug,
                    &args.config,
                    &std::iter::once(base_run.source.as_path())
                        .chain(
                            candidates_for_subtest
                                .iter()
                                .map(|candidate| candidate.run.source.as_path()),
                        )
                        .collect::<Vec<_>>(),
                )?;
                let candidates = candidates_for_subtest
                    .iter()
                    .map(|candidate| {
                        (
                            candidate.label.clone(),
                            candidate.run.curves.clone(),
                            candidate.run.branch.clone(),
                            candidate.run.corner_mass_kg.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let setup = base_run.conditions.setup_code();
                knc::compare_plots(
                    &baseline.config,
                    subtest,
                    (&base_run.curves, &base_run.branch, &base_run.corner_mass_kg),
                    &candidates,
                    (
                        &display_label(&labels, 0),
                        &format!("plots/knc/{}", subtest.name()),
                    ),
                    base_run.conditions.brakes(),
                    knc::PlotOptions {
                        label: None,
                        footer: &footer.join("\n"),
                        debug: debug.as_deref(),
                        workers,
                        setup: (subtest == KncSubtest::Longitudinal).then_some(setup.as_str()),
                    },
                    &progress,
                )
            })
            .collect::<Result<Vec<_>>>()
    })?;
    for group in rendered.into_iter().flatten() {
        plots.extend(group);
    }
    let mut baseline_entry = serde_json::json!({"input": provenance(&args.baseline)?, "case": {"name": baseline.config.name, "loadcase": baseline.config.loadcase}});
    if let Some(label) = &labels[0] {
        baseline_entry["label"] = label.as_str().into();
    }
    let report = serde_json::json!({
        "schema_version": 1, "command": "compare",
        "test": baseline.tests.values().next().map_or("", ComparedTest::test),
        "tests": baseline.tests.values().map(|test| serde_json::json!({
            "test": test.test(),
            "subtest": test.subtest().map(KncSubtest::name),
            "conditions": match test {
                ComparedTest::Knc { runs, .. } => runs_json(runs),
                ComparedTest::Svc(_) => serde_json::Value::Null,
            },
        })).collect::<Vec<_>>(),
        "baseline": baseline_entry,
        "candidates": candidates, "status": status,
        "warnings": warnings.values().flatten().collect::<Vec<_>>(),
    });
    if let Some(output) = &args.output {
        let csv = metrics_csv(&inputs, &rows)?;
        let knc_details = baseline
            .tests
            .values()
            .filter_map(ComparedTest::subtest)
            .map(|subtest| format!("knc-{}.json", subtest.name()))
            .collect::<Vec<_>>();
        let detail = serde_json::to_string_pretty(&report)? + "\n";
        let mut files = vec![
            ("report.json", detail.clone().into_bytes()),
            ("metrics.csv", csv.into_bytes()),
        ];
        if baseline.tests.contains_key("svc") {
            files.push(("svc.json", detail.into_bytes()));
        }
        for knc_detail in &knc_details {
            files.push((
                knc_detail,
                (serde_json::to_string_pretty(&report)? + "\n").into_bytes(),
            ));
        }
        files.extend(plots.iter().map(|(name, pdf)| (name.as_str(), pdf.clone())));
        let mut inputs = vec![args.config.as_path(), args.baseline.as_path()];
        inputs.extend(baseline.sources.iter().map(PathBuf::as_path));
        inputs.extend(args.candidates.iter().map(PathBuf::as_path));
        inputs.extend(candidate_sources.iter().map(PathBuf::as_path));
        progress.phase("Writing report bundle", "Writing report bundle…");
        write_report_bundle(output, files, args.force, &inputs)?;
        progress.done("Writing report bundle");
        progress.finish();
        if args.json {
            display::print_json(&serde_json::json!({"report": output}));
        } else {
            println!("report: {}", output.display());
        }
    }
    if args.json {
        display::print_json(&serde_json::json!({"comparison": {
            "baseline": args.baseline,
            "candidates": args.candidates.len(),
            "status": status,
        }}));
    } else {
        println!(
            "comparison: baseline {}; {} candidate(s); {}",
            args.baseline.display(),
            args.candidates.len(),
            status
        );
    }
    if failed {
        bail!(ToleranceFailure);
    }
    Ok(())
}

fn load_input(config: &Path, path: &Path) -> Result<Input> {
    validate_comparison_input(path)?;
    let mut files = OpenFiles::default();
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .map(str::to_ascii_lowercase);
    let (case, include_svc, include_knc) = match extension.as_deref() {
        Some("toml") => {
            clab_core::config::load_config_and_run(config, path).map(|case| (case, true, true))
        }
        Some("json") => clab_core::config::load_config_with_input(config, path, None)
            .map(|case| (case, true, false)),
        Some("h5" | "hdf5") => {
            let file = files.open(path)?;
            match (
                file.summary().test.as_deref(),
                file.summary().subtest.as_deref(),
            ) {
                (Some("svc"), _) => clab_core::config::load_config_with_input(config, path, None)
                    .map(|case| (case, true, false)),
                (Some("knc"), Some(subtest @ ("vertical" | "roll" | "longitudinal"))) => {
                    clab_core::config::load_config_with_input(config, path, Some(subtest))
                        .map(|case| (case, false, true))
                }
                (Some(test), subtest) => bail!(
                    "unsupported test '{test}'{} in '{}'; supported: svc, knc vertical, knc roll, knc longitudinal",
                    subtest.map(|subtest| format!(" subtest '{subtest}'"))
                        .unwrap_or_default(),
                    path.display()
                ),
                (None, _) => {
                    // Converted before test and subtest were stored: infer from the config.
                    if let Some(warning) = file.summary().legacy_warning() {
                        eprintln!("warning: {warning}");
                    }
                    let svc_case =
                        clab_core::config::load_config_with_input(config, path, None).ok();
                    match svc_case {
                        Some(svc_case)
                            if file.summary().source_kind == SourceKind::Simulation
                                && svc::supports_hdf5(&svc_case, &file) =>
                        {
                            Ok((svc_case, true, false))
                        }
                        _ => clab_core::config::load_config_with_input(
                            config,
                            path,
                            Some("vertical"),
                        )
                        .map(|case| (case, false, true)),
                    }
                }
            }
        }
        _ => bail!("compare accepts run TOML, SVC JSON, or normalized HDF5 input"),
    }
    .map_err(|error| {
        anyhow::anyhow!("failed to load compare input '{}': {error}", path.display())
    })?;
    let units = if include_knc {
        knc::knc_units(&case)
    } else {
        Vec::new()
    };
    if include_svc && case.svc_source_kind.is_some() {
        validate_comparison_input(&case.simulation_json)?;
    }
    for (subtest, unit, source) in &units {
        validate_comparison_input(source)?;
        if let Some(knc) = subtest.config(unit)
            && knc.source_kind == clab_core::config::SourceKind::Simulation
            && let Some(static_input) = &knc.static_simulation_hdf5
        {
            validate_comparison_input(static_input)?;
        }
    }
    if matches!(extension.as_deref(), Some("toml")) {
        let mut paths = Vec::new();
        if include_svc && is_hdf5(&case.simulation_json) {
            paths.push(case.simulation_json.as_path());
        }
        for (subtest, unit, source) in &units {
            paths.push(source.as_path());
            if let Some(static_input) = subtest
                .config(unit)
                .and_then(|knc| knc.static_simulation_hdf5.as_ref())
            {
                paths.push(static_input);
            }
        }
        for path in paths {
            if let Some(warning) = files.open(path)?.summary().legacy_warning() {
                eprintln!("warning: {warning}");
            }
        }
    }
    let mut tests = BTreeMap::new();
    let mut sources = Vec::new();
    let mut svc_corner_mass_kg: Option<BTreeMap<String, f64>> = None;
    if include_svc && case.svc_source_kind.is_some() {
        let values = match case.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) if is_hdf5(&case.simulation_json) => {
                svc::evaluate_hdf5_case(&case, &mut files)?
            }
            Some(clab_core::config::SourceKind::Simulation) => svc::evaluate_case(&case)?,
            Some(clab_core::config::SourceKind::Measurement) => {
                clab_core::svc::load_svc_reference(
                    &case.simulation_json,
                    &case.svc_measurement_signals,
                )
                .map_err(|error| anyhow::anyhow!(error.to_string()))?
                .values
            }
            None => unreachable!(),
        };
        svc_corner_mass_kg = Some(
            ["front_left", "front_right", "rear_left", "rear_right"]
                .into_iter()
                .filter_map(|corner| {
                    values
                        .get(&format!("{corner}_mass_kg"))
                        .map(|mass| (corner.to_string(), *mass))
                })
                .collect(),
        );
        sources.push(case.simulation_json.clone());
        tests.insert("svc", ComparedTest::Svc(svc::canonical_metrics(&values)));
    }
    let mut knc_reports = Vec::new();
    for (subtest, unit, source) in &units {
        let report = knc::analyze_input(
            unit,
            *subtest,
            source,
            svc_corner_mass_kg.as_ref(),
            None,
            &mut files,
        )?;
        sources.push(source.clone());
        if let Some(static_input) = subtest
            .config(unit)
            .and_then(|knc| knc.static_simulation_hdf5.as_ref())
        {
            sources.push(static_input.clone());
        }
        knc_reports.push((*subtest, source.as_path(), report));
    }
    knc::ensure_distinct_setups(
        knc_reports
            .iter()
            .map(|(subtest, source, report)| (*subtest, *source, report)),
    )?;
    for subtest in KncSubtest::ALL {
        let mut reports: Vec<_> = knc_reports
            .iter_mut()
            .filter(|(candidate, ..)| *candidate == subtest)
            .map(|(.., report)| report)
            .collect();
        if reports.is_empty() {
            continue;
        }
        knc::drop_resolved_placeholders(&mut reports);
        let metrics = reports
            .iter()
            .flat_map(|report| report.metrics.clone())
            .collect();
        let runs = reports
            .iter()
            .map(|report| {
                let detail: serde_json::Value = serde_json::from_str(&report.detail)?;
                Ok(KncRun {
                    source: report.input.clone(),
                    curves: detail["curves"].clone(),
                    conditions: report.conditions.clone(),
                    branch: report.branch.clone(),
                    corner_mass_kg: report.corner_mass_kg.clone(),
                })
            })
            .collect::<Result<_>>()?;
        tests.insert(
            knc_key(subtest),
            ComparedTest::Knc {
                subtest,
                metrics,
                runs,
            },
        );
    }
    Ok(Input {
        config: case,
        tests,
        sources,
    })
}

fn validate_comparison_input(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("comparison input does not exist: {}", path.display());
    }
    Ok(())
}

fn is_hdf5(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|v| v.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("h5" | "hdf5")
    )
}

fn compare_metrics(
    baseline: &[svc::CanonicalMetric],
    candidate: &[svc::CanonicalMetric],
    tolerances: Option<&clab_core::config::ValidationTolerances>,
) -> Vec<Metric> {
    baseline
        .iter()
        .zip(candidate)
        .map(|(baseline, candidate)| {
            debug_assert_eq!(baseline.name, candidate.name);
            debug_assert_eq!(baseline.unit, candidate.unit);
            let base = baseline.value;
            let candidate = candidate.value;
            let absolute = tolerances.and_then(|value| value.absolute.get(baseline.name).copied());
            let relative_percent =
                tolerances.and_then(|value| value.relative_percent.get(baseline.name).copied());
            let relative = base.zip(relative_percent).map(|(v, p)| v.abs() * p / 100.0);
            let allowed = match (absolute, relative) {
                (Some(a), Some(r)) => Some(a.min(r)),
                (Some(v), None) | (None, Some(v)) => Some(v),
                _ => None,
            };
            let difference = base.zip(candidate).map(|(a, b)| (b - a).abs());
            let status = match (base, candidate, allowed, difference) {
                (Some(_), Some(_), Some(limit), Some(diff)) if diff <= limit => "passed",
                (Some(_), Some(_), Some(_), Some(_)) => "failed",
                (Some(_), Some(_), None, _) => "informational",
                _ => "missing",
            };
            Metric {
                test: "svc",
                subtest: None,
                metric: baseline.name.into(),
                unit: baseline.unit.into(),
                baseline_value: base,
                candidate_value: candidate,
                difference,
                absolute_tolerance: absolute,
                relative_percent,
                allowed_difference: allowed,
                status,
            }
        })
        .collect()
}

fn metrics_by_name(metrics: &[serde_json::Value]) -> BTreeMap<&str, &serde_json::Value> {
    metrics
        .iter()
        .filter_map(|metric| metric["metric"].as_str().map(|name| (name, metric)))
        .collect()
}

/// Metrics that either side lists as `not_covered` by its stored setup: the
/// other side's value has nothing to compare with.
fn not_compared<'a>(
    baseline: &'a [serde_json::Value],
    candidate: &'a [serde_json::Value],
) -> Vec<&'a str> {
    let uncovered = |metrics: &'a [serde_json::Value]| {
        metrics_by_name(metrics)
            .into_iter()
            .filter(|(_, metric)| knc::is_not_covered(metric))
            .map(|(name, _)| name)
            .collect::<BTreeSet<_>>()
    };
    uncovered(baseline)
        .union(&uncovered(candidate))
        .copied()
        .collect()
}

fn compare_knc_metrics(
    subtest: KncSubtest,
    baseline: &[serde_json::Value],
    candidate: &[serde_json::Value],
    tolerances: Option<&clab_core::config::ValidationTolerances>,
) -> Vec<Metric> {
    let skipped = not_compared(baseline, candidate);
    let baseline = metrics_by_name(baseline);
    let candidate = metrics_by_name(candidate);
    baseline
        .keys()
        .chain(candidate.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|name| !skipped.contains(name))
        .map(|name| {
            let baseline = baseline.get(name);
            let candidate = candidate.get(name);
            let base = baseline.and_then(|metric| metric["value"].as_f64());
            let candidate_value = candidate.and_then(|metric| metric["value"].as_f64());
            let absolute = tolerances.and_then(|value| value.absolute.get(name).copied());
            let relative_percent =
                tolerances.and_then(|value| value.relative_percent.get(name).copied());
            let relative = base
                .zip(relative_percent)
                .map(|(value, percent)| value.abs() * percent / 100.0);
            let allowed_difference = match (absolute, relative) {
                (Some(absolute), Some(relative)) => Some(absolute.min(relative)),
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            };
            let difference = base
                .zip(candidate_value)
                .map(|(left, right)| (right - left).abs());
            let status = match (base, candidate_value, allowed_difference, difference) {
                (Some(_), Some(_), Some(limit), Some(difference)) if difference <= limit => {
                    "passed"
                }
                (Some(_), Some(_), Some(_), Some(_)) => "failed",
                (Some(_), Some(_), None, _) => "informational",
                _ => "missing",
            };
            Metric {
                test: "knc",
                subtest: Some(subtest.name()),
                metric: name.into(),
                unit: baseline
                    .or(candidate)
                    .and_then(|metric| metric["unit"].as_str())
                    .unwrap_or_default()
                    .into(),
                baseline_value: base,
                candidate_value,
                difference,
                absolute_tolerance: absolute,
                relative_percent,
                allowed_difference,
                status,
            }
        })
        .collect()
}

fn metrics_csv(inputs: &[InputSummary], rows: &[(PathBuf, Metric)]) -> Result<String> {
    let mut csv = csv::Writer::from_writer(Vec::new());
    csv.write_record([
        "baseline",
        "candidate",
        "test",
        "subtest",
        "metric",
        "unit",
        "baseline_value",
        "candidate_value",
        "difference",
        "absolute_tolerance",
        "relative_percent",
        "allowed_difference",
        "status",
        "baseline_label",
        "candidate_label",
        "baseline_setup",
        "baseline_phase",
        "baseline_offset_mm",
        "candidate_setup",
        "candidate_phase",
        "candidate_offset_mm",
    ])?;
    for (candidate, metric) in rows {
        let base = &inputs[0];
        let other = inputs[1..]
            .iter()
            .find(|input| input.path == *candidate)
            .expect("comparison row has a candidate input");
        let cells = |input: &InputSummary| {
            metric
                .subtest
                .and_then(|subtest| input.conditions.get(subtest))
                .map(TestConditions::csv_cells)
                .unwrap_or_default()
        };
        let [base_setup, base_phase, base_offset] = cells(base);
        let [setup, phase, offset] = cells(other);
        csv.write_record([
            safe_cell(&base.path.display().to_string()),
            safe_cell(&candidate.display().to_string()),
            safe_cell(metric.test),
            metric.subtest.map(safe_cell).unwrap_or_default(),
            safe_cell(&metric.metric),
            safe_cell(&metric.unit),
            option_number(metric.baseline_value),
            option_number(metric.candidate_value),
            option_number(metric.difference),
            option_number(metric.absolute_tolerance),
            option_number(metric.relative_percent),
            option_number(metric.allowed_difference),
            safe_cell(metric.status),
            safe_cell(&base.label),
            safe_cell(&other.label),
            base_setup,
            base_phase,
            base_offset,
            setup,
            phase,
            offset,
        ])?;
    }
    Ok(String::from_utf8(csv.into_inner()?)?)
}

/// Rows of one test grouped per metric (one value per candidate), in console order.
fn grouped_metrics<'a>(
    compared: &ComparedTest,
    candidates: &[PathBuf],
    rows: &'a [(PathBuf, Metric)],
) -> GroupedMetrics<'a> {
    let (test, test_subtest) = (compared.test(), compared.subtest().map(KncSubtest::name));
    let mut grouped: GroupedMetrics<'_> = Vec::new();
    for (candidate, metric) in rows
        .iter()
        .filter(|(_, metric)| metric.test == test && metric.subtest == test_subtest)
    {
        let index = candidates
            .iter()
            .position(|path| path == candidate)
            .expect("candidate row must have an input");
        if let Some((_, _, values)) = grouped.iter_mut().find(|(subtest, name, _)| {
            *subtest == metric.subtest && *name == metric.metric.as_str()
        }) {
            values.push((index, metric));
        } else {
            grouped.push((metric.subtest, &metric.metric, vec![(index, metric)]));
        }
    }
    grouped.sort_by_key(|(subtest, name, _)| {
        crate::analysis::display::console_metric_order(test, *subtest, name)
    });
    grouped
}

/// `--json` twin of `print_metrics`: one section line per compared test.
fn json_metrics(
    inputs: &[InputSummary],
    candidates: &[PathBuf],
    tests: &BTreeMap<&'static str, ComparedTest>,
    rows: &[(PathBuf, Metric)],
    unavailable_tests: &[(usize, &'static str)],
    _use_tolerances: bool,
    warnings: &BTreeMap<&str, Vec<String>>,
) {
    for key in TEST_KEYS {
        let Some(compared) = tests.get(key) else {
            continue;
        };
        let (test, subtest) = (compared.test(), compared.subtest().map(KncSubtest::name));
        let metrics = grouped_metrics(compared, candidates, rows)
            .into_iter()
            .filter(|(subtest, name, _)| display::show_metric(test, *subtest, name))
            .map(|(subtest, name, values)| {
                let mut per_candidate = vec![serde_json::Value::Null; candidates.len()];
                for (index, metric) in &values {
                    let compared = !unavailable_tests
                        .iter()
                        .any(|(unavailable, test)| unavailable == index && *test == key);
                    per_candidate[*index] = if compared {
                        serde_json::json!({
                            "value": metric.candidate_value,
                            "difference": metric.difference,
                            "status": metric.status,
                        })
                    } else {
                        serde_json::json!({"value": null, "difference": null, "status": "not compared"})
                    };
                }
                serde_json::json!({
                    "name": display::qualified_metric_name(test, subtest, name),
                    "unit": values[0].1.unit,
                    "baseline": values[0].1.baseline_value,
                    "allowed_difference": values.iter().find_map(|(_, metric)| metric.allowed_difference),
                    "candidates": per_candidate,
                })
            })
            .collect();
        let test_rows: Vec<_> = rows
            .iter()
            .filter(|(_, metric)| metric.test == test && metric.subtest == subtest)
            .collect();
        let (passed, failed, missing, tolerance_missing) = outcome_counts(&test_rows);
        display::print_json(&display::section_json(
            compared.title(),
            (test, subtest),
            metrics,
            warnings.get(key).map_or(&[][..], Vec::as_slice),
            serde_json::json!({"inputs": inputs, "result": {
                "passed": passed, "failed": failed, "missing": missing,
                "tolerance_missing": tolerance_missing, "total": test_rows.len(),
            }}),
        ));
    }
}

fn print_metrics(
    _inputs: &[InputSummary],
    candidates: &[PathBuf],
    tests: &BTreeMap<&'static str, ComparedTest>,
    rows: &[(PathBuf, Metric)],
    unavailable_tests: &[(usize, &'static str)],
    use_tolerances: bool,
    warnings: &BTreeMap<&str, Vec<String>>,
) {
    for key in TEST_KEYS {
        let Some(compared) = tests.get(key) else {
            continue;
        };
        let (title, test) = (compared.title(), compared.test());
        let test_subtest = compared.subtest().map(KncSubtest::name);
        let in_test = |metric: &Metric| metric.test == test && metric.subtest == test_subtest;
        println!();
        let grouped = grouped_metrics(compared, candidates, rows);
        println!("{title}");
        println!(
            "running {} metrics",
            grouped
                .iter()
                .filter(|(subtest, name, _)| {
                    crate::analysis::display::show_metric(test, *subtest, name)
                })
                .count()
        );
        for (subtest, metric_name, values) in grouped.into_iter().filter(|(subtest, name, _)| {
            crate::analysis::display::show_metric(test, *subtest, name)
        }) {
            let name = crate::analysis::display::qualified_metric_name(test, subtest, metric_name);
            let baseline_value = values[0].1.baseline_value;
            let unit = &values[0].1.unit;
            let mut fields = vec![format!("baseline {}", display_number(baseline_value))];
            let mut metrics = vec![None; candidates.len()];
            for (index, metric) in values {
                metrics[index] = Some(metric);
            }
            let mut labels = Vec::new();
            for (index, metric) in metrics.into_iter().enumerate() {
                let metric = metric.filter(|_| {
                    !unavailable_tests
                        .iter()
                        .any(|(unavailable, name)| *unavailable == index && *name == key)
                });
                fields.push(format!(
                    "candidate_{} {}",
                    index + 1,
                    display_number(metric.and_then(|metric| metric.candidate_value))
                ));
                fields.push(format!(
                    "diff_{} {}",
                    index + 1,
                    display_number(metric.and_then(|metric| metric.difference))
                ));
                let status = metric.map(|metric| metric.status).unwrap_or("missing");
                if use_tolerances {
                    let label = match status {
                        "passed" => "ok",
                        "failed" => "FAILED",
                        "informational" => "tolerance missing",
                        other => other,
                    };
                    labels.push(colored_status(label));
                } else if !use_tolerances && status == "missing" {
                    labels.push(colored_status("missing"));
                }
            }
            if use_tolerances
                && let Some(limit) = rows.iter().find_map(|(_, metric)| {
                    (metric.test == test
                        && metric.subtest == subtest
                        && metric.metric == metric_name)
                        .then_some(metric.allowed_difference)
                        .flatten()
                })
            {
                fields.push(format!("tolerance <= {limit:.8} {unit}"));
            }
            let labels = labels.join(" ");
            let separator = if labels.is_empty() { "" } else { " " };
            println!("  {name} ...{separator}{labels} ({})", fields.join("; "));
        }
        for _ in unavailable_tests
            .iter()
            .filter(|(_, unavailable)| *unavailable == key)
        {
            println!("  {key} ... {}", colored_status("not compared"));
        }
        let test_rows: Vec<_> = rows.iter().filter(|(_, metric)| in_test(metric)).collect();
        let (passed, failed, missing, tolerance_missing) = outcome_counts(&test_rows);
        if use_tolerances {
            println!(
                "comparison result: {}. {passed} ok; {failed} failed; {missing} metric missing; {tolerance_missing} tolerance missing; {} total",
                if failed > 0 {
                    "failed"
                } else if missing > 0
                    || unavailable_tests
                        .iter()
                        .any(|(_, unavailable)| *unavailable == key)
                {
                    "incomplete"
                } else {
                    "passed"
                },
                test_rows.len()
            );
        } else {
            println!(
                "comparison result: informational. {} compared; {missing} metric missing; {} total",
                test_rows.len() - missing,
                test_rows.len()
            );
        }
        if let Some(warnings) = warnings.get(key) {
            display::print_warnings(warnings);
        }
    }
}

fn outcome_counts(rows: &[&(PathBuf, Metric)]) -> (usize, usize, usize, usize) {
    rows.iter().fold(
        (0, 0, 0, 0),
        |(passed, failed, missing, tolerance_missing), (_, metric)| match metric.status {
            "passed" => (passed + 1, failed, missing, tolerance_missing),
            "failed" => (passed, failed + 1, missing, tolerance_missing),
            "missing" => (passed, failed, missing + 1, tolerance_missing),
            "informational" => (passed, failed, missing, tolerance_missing + 1),
            _ => unreachable!(),
        },
    )
}

fn display_number(value: Option<f64>) -> String {
    value
        .map(|value| format!("{value:.8}"))
        .unwrap_or_else(|| "missing".into())
}

fn colored_status(status: &str) -> String {
    render_status(status, std::io::stdout().is_terminal())
}

fn render_status(status: &str, terminal: bool) -> String {
    let color = match status {
        "ok" => display::GREEN,
        "FAILED" => display::RED,
        "missing" | "not compared" => "33",
        "tolerance missing" => display::CYAN,
        _ => return status.into(),
    };
    display::paint_if(status, color, terminal)
}

fn option_number(value: Option<f64>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn empty_tolerances() -> clab_core::config::ValidationTolerances {
    clab_core::config::ValidationTolerances {
        absolute: BTreeMap::new(),
        relative_percent: BTreeMap::new(),
    }
}

fn comparison_status(
    failed: bool,
    incomplete: bool,
    _rows: &[(PathBuf, Metric)],
    use_tolerances: bool,
) -> &'static str {
    if !use_tolerances {
        return "informational";
    }
    if failed {
        "failed"
    } else if incomplete {
        "incomplete"
    } else {
        "passed"
    }
}

pub(crate) fn safe_cell(value: &str) -> String {
    if matches!(
        value.trim_start().as_bytes().first(),
        Some(b'=' | b'+' | b'-' | b'@')
    ) {
        format!("'{value}")
    } else {
        value.into()
    }
}
fn report_creator() -> String {
    let user = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".into());
    let host = env::var("HOSTNAME").unwrap_or_else(|_| {
        Command::new("hostname")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "unknown-host".into())
    });
    format!("{user}@{host}")
}
fn provenance(path: &Path) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"path": path, "sha256": format!("{:x}", sha2::Sha256::digest(fs::read(path)?))}),
    )
}

#[cfg(test)]
mod tests {
    use super::{Metric, comparison_status, parse_labels, render_status, resolve_labels};
    use std::path::PathBuf;

    fn labels(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_labels_with_defaults_for_blank_or_missing() {
        assert_eq!(parse_labels(&[], 2).unwrap(), vec![None, None]);
        assert_eq!(
            parse_labels(&labels(&[" foo ", ""]), 2).unwrap(),
            vec![Some("foo".into()), None]
        );
    }

    #[test]
    fn rejects_invalid_labels() {
        let message = |values: &[&str], inputs| {
            parse_labels(&labels(values), inputs)
                .unwrap_err()
                .to_string()
        };
        assert!(message(&["foo"], 2).contains("expected 2 --label"));
        assert!(message(&["foo", "foo"], 2).contains("more than one input"));
        assert!(message(&["Curve 1", ""], 2).contains("more than one input"));
        assert!(message(&["", "Baseline"], 2).contains("more than one input"));
        assert!(message(&["a\nb", "c"], 2).contains("control characters"));
        assert!(message(&[&"x".repeat(65), "c"], 2).contains("longer than 64"));
    }

    #[test]
    fn resolves_run_names_below_explicit_labels() {
        let some = |value: &str| Some(value.to_owned());
        assert_eq!(
            resolve_labels(
                vec![None, some("x"), None],
                &["run a", "run b", "undefined"]
            ),
            vec![some("run a"), some("x"), None]
        );
        assert_eq!(
            resolve_labels(vec![None, None, None], &["same", "same", "other"]),
            vec![None, None, some("other")]
        );
        assert_eq!(
            resolve_labels(vec![some("foo"), None], &["n", "foo"]),
            vec![some("foo"), None]
        );
    }

    #[test]
    fn colors_console_statuses_only_for_terminals() {
        assert_eq!(render_status("ok", false), "ok");
        assert_eq!(render_status("FAILED", false), "FAILED");
        assert_eq!(render_status("ok", true), "\x1b[32mok\x1b[0m");
        assert_eq!(render_status("FAILED", true), "\x1b[31mFAILED\x1b[0m");
        assert_eq!(render_status("missing", true), "\x1b[33mmissing\x1b[0m");
    }

    #[test]
    fn incomplete_precedes_all_informational_but_not_failure() {
        let rows = vec![(
            PathBuf::from("candidate"),
            Metric {
                test: "svc",
                subtest: None,
                metric: "metric".into(),
                unit: "mm".into(),
                baseline_value: None,
                candidate_value: None,
                difference: None,
                absolute_tolerance: None,
                relative_percent: None,
                allowed_difference: None,
                status: "informational",
            },
        )];
        assert_eq!(comparison_status(false, true, &rows, true), "incomplete");
        assert_eq!(comparison_status(true, true, &rows, true), "failed");
        assert_eq!(comparison_status(true, true, &rows, false), "informational");
    }
}
