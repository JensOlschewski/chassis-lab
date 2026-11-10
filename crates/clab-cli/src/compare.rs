use std::{
    collections::{BTreeMap, BTreeSet},
    env, fmt, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
use clab_core::canonical_hdf5::{CanonicalFile, SourceKind};
use clap::Args;
use serde::Serialize;
use sha2::Digest;

use crate::{
    analysis::{
        display,
        knc::{self, KncTest},
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
    domain: &'static str,
    test: Option<&'static str>,
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

/// Curves of one KnC input file. Metrics of a test are merged across its
/// files; plots are not, so a longitudinal B0 run is never overlaid on B1.
struct KncRun {
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
    /// Longitudinal runs pair by brake state, every other test has one run.
    fn pairs_with(&self, other: &Self, test: KncTest) -> bool {
        test != KncTest::Longitudinal || self.conditions.brakes() == other.conditions.brakes()
    }
}

/// Condition document of a test: one object per run, an array for several.
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
    path: &'a Path,
}

enum ComparedDomain {
    Svc(Vec<svc::CanonicalMetric>),
    Knc {
        test: KncTest,
        metrics: Vec<serde_json::Value>,
        runs: Vec<KncRun>,
    },
}

impl ComparedDomain {
    /// KnC test of the domain; `None` for SVC.
    fn test(&self) -> Option<KncTest> {
        match self {
            Self::Svc(_) => None,
            Self::Knc { test, .. } => Some(*test),
        }
    }

    fn title(&self) -> &'static str {
        self.test().map_or("SVC", KncTest::title)
    }

    /// Report domain name: `svc` or `knc`.
    fn domain(&self) -> &'static str {
        if self.test().is_some() { "knc" } else { "svc" }
    }
}

/// Keys of `Input::domains` in console order; KnC tests compare separately.
const DOMAIN_KEYS: [&str; 4] = ["svc", "knc-vertical", "knc-roll", "knc-longitudinal"];

fn knc_key(test: KncTest) -> &'static str {
    match test {
        KncTest::Vertical => "knc-vertical",
        KncTest::Roll => "knc-roll",
        KncTest::Longitudinal => "knc-longitudinal",
    }
}

struct Input {
    config: clab_core::config::CaseConfig,
    domains: BTreeMap<&'static str, ComparedDomain>,
    sources: Vec<PathBuf>,
}

/// Per-input identity and conditions shared by console, JSON and CSV output.
#[derive(Serialize)]
struct InputSummary {
    label: String,
    path: PathBuf,
    conditions: BTreeMap<&'static str, TestConditions>,
}

/// Conditions of every run of one test: serialized as an object for a single
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
    fn lines(&self) -> Vec<String> {
        self.0.iter().flat_map(knc::Conditions::lines).collect()
    }

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
    /// Per KnC test, keyed like `Input::domains`.
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
                .domains
                .values()
                .filter_map(|domain| match domain {
                    ComparedDomain::Knc { test, runs, .. } => Some((
                        test.name(),
                        TestConditions(runs.iter().map(|run| run.conditions.clone()).collect()),
                    )),
                    ComparedDomain::Svc(_) => None,
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
        knc: KncTest::ALL
            .into_iter()
            .filter_map(|test| test.config(&baseline.config).map(|knc| (test, knc)))
            .map(|(test, knc)| (knc_key(test), knc.tolerances.clone()))
            .collect(),
    });
    let mut failed = false;
    let mut incomplete = false;
    let mut rows = Vec::new();
    let mut candidates = Vec::new();
    let mut plots = BTreeMap::new();
    let mut knc_candidates: BTreeMap<&str, Vec<_>> = BTreeMap::new();
    let mut candidate_sources = Vec::new();
    let mut unavailable_domains = Vec::new();
    let mut warnings: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (index, path) in args.candidates.iter().enumerate() {
        let candidate = &loaded[index];
        let mut metrics = Vec::new();
        let mut domains = Vec::new();
        for (name, base_domain) in &baseline.domains {
            let Some(candidate_domain) = candidate.domains.get(name) else {
                incomplete = true;
                unavailable_domains.push((index, *name));
                domains.push(serde_json::json!({"domain": base_domain.domain(), "test": base_domain.test().map(KncTest::name), "status": "not compared"}));
                continue;
            };
            match (base_domain, candidate_domain) {
                (ComparedDomain::Svc(base), ComparedDomain::Svc(candidate_values)) => {
                    let compared = compare_metrics(
                        base,
                        candidate_values,
                        tolerances.as_ref().map(|value| &value.svc),
                    );
                    failed |= compared.iter().any(|metric| metric.status == "failed");
                    incomplete |= compared.iter().any(|metric| metric.status == "missing");
                    metrics.extend(compared);
                    domains.push(
                        serde_json::json!({"domain": "svc", "test": null, "status": "compared"}),
                    );
                }
                (
                    ComparedDomain::Knc {
                        test: base_test,
                        metrics: base_metrics,
                        runs: base_runs,
                    },
                    ComparedDomain::Knc {
                        test,
                        metrics: candidate_metrics,
                        runs: candidate_runs,
                    },
                ) if base_test == test => {
                    let compared = compare_knc_metrics(
                        *test,
                        base_metrics,
                        candidate_metrics,
                        tolerances.as_ref().map(|value| &value.knc[name]),
                    );
                    failed |= compared.iter().any(|metric| metric.status == "failed");
                    incomplete |= compared.iter().any(|metric| metric.status == "missing");
                    metrics.extend(compared);
                    domains.push(serde_json::json!({"domain": "knc", "test": test.name(), "status": "compared", "conditions": runs_json(candidate_runs)}));
                    let label = display_label(&labels, index + 1);
                    let skipped = not_compared(base_metrics, candidate_metrics);
                    if !skipped.is_empty() {
                        warnings.entry(*name).or_default().push(display::tag_warning(
                            "knc",
                            Some(test.name()),
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
                            .filter(|run| base_run.pairs_with(run, *test))
                        {
                            let (base_conditions, candidate_conditions) =
                                (&base_run.conditions, &candidate_run.conditions);
                            let differences = base_conditions.differences(candidate_conditions);
                            if !differences.is_empty() {
                                warnings.entry(*name).or_default().push(display::tag_warning(
                                    "knc",
                                    Some(test.name()),
                                    format!(
                                        "test conditions differ ({}): {} {} vs {} {}; results compare different physical cases",
                                        differences.join(", "),
                                        display_label(&labels, 0),
                                        base_conditions.summary(),
                                        label,
                                        candidate_conditions.summary(),
                                    ),
                                ));
                            }
                            knc_candidates
                                .entry(test.name())
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
                                    path: path.as_path(),
                                });
                        }
                    }
                }
                _ => bail!(
                    "incompatible comparison domains between '{}' and '{}'",
                    args.baseline.display(),
                    path.display()
                ),
            }
        }
        if baseline.domains.len() == 1
            && candidate.domains.len() == 1
            && baseline.domains.keys().next() != candidate.domains.keys().next()
        {
            bail!(
                "cannot compare different tests: '{}' is {}, '{}' is {}",
                args.baseline.display(),
                baseline.domains.keys().next().unwrap(),
                path.display(),
                candidate.domains.keys().next().unwrap(),
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
        let mut entry = serde_json::json!({"input": provenance(path)?, "case": {"name": candidate.config.name, "loadcase": candidate.config.loadcase}, "metrics": metrics, "domains": domains});
        if let Some(label) = &labels[index + 1] {
            entry["label"] = label.as_str().into();
        }
        candidates.push(entry);
    }
    if args.debug {
        for test in KncTest::ALL {
            let key = knc_key(test);
            let Some(ComparedDomain::Knc { runs, .. }) = baseline.domains.get(key) else {
                continue;
            };
            if runs.iter().any(|run| !run.branch.measurement)
                || knc_candidates.get(test.name()).is_some_and(|candidates| {
                    candidates
                        .iter()
                        .any(|candidate| !candidate.run.branch.measurement)
                })
            {
                warnings.entry(key).or_default().push(display::tag_warning(
                    "knc",
                    Some(test.name()),
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
        &baseline.domains,
        &rows,
        &unavailable_domains,
        args.tolerances,
        &warnings,
    );
    let status = comparison_status(failed, incomplete, &rows, args.tolerances);
    let progress = ReportProgress::new(args.output.is_some(), args.json);
    let mut plot_jobs = Vec::new();
    if args.output.is_some() {
        for test in KncTest::ALL {
            let Some(ComparedDomain::Knc { runs, .. }) = baseline.domains.get(knc_key(test)) else {
                continue;
            };
            let candidates = knc_candidates
                .get(test.name())
                .map_or(&[][..], Vec::as_slice);
            for run in runs {
                let paired: Vec<_> = candidates
                    .iter()
                    .filter(|candidate| run.pairs_with(candidate.run, test))
                    .collect();
                if test == KncTest::Longitudinal && paired.is_empty() {
                    continue;
                }
                let title = if test == KncTest::Longitudinal {
                    format!("{} {}", test.title(), run.conditions.setup_code())
                } else {
                    test.title().to_owned()
                };
                plot_jobs.push((title, (test, run, paired)));
            }
        }
    }
    let workers = knc::plot_worker_limit(plot_jobs.len());
    let jobs = plot_jobs
        .iter()
        .map(|(title, job)| (title.as_str(), job))
        .collect();
    let rendered = progress.plot_phases(jobs, |(test, base_run, candidates_for_test)| {
        let (test, base_run) = (*test, *base_run);
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
            candidates_for_test
                .iter()
                .map(|candidate| candidate.footer.clone()),
        );
        if let Some(warnings) = warnings.get(knc_key(test)) {
            footer.extend(warnings.iter().map(|warning| format!("WARNING: {warning}")));
        }
        footer.push(format!("Created by: {}", report_creator()));
        footer.push(format!("clab version: {}", env!("CARGO_PKG_VERSION")));
        let debug = debug_info(
            args.debug,
            &args.config,
            &std::iter::once(args.baseline.as_path())
                .chain(candidates_for_test.iter().map(|candidate| candidate.path))
                .collect::<Vec<_>>(),
        )?;
        let candidates = candidates_for_test
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
            test,
            (&base_run.curves, &base_run.branch, &base_run.corner_mass_kg),
            &candidates,
            (
                &display_label(&labels, 0),
                &format!("plots/knc/{}/comparison", test.name()),
            ),
            base_run.conditions.brakes(),
            knc::PlotOptions {
                label: None,
                footer: &footer.join("\n"),
                debug: debug.as_deref(),
                workers,
                setup: (test == KncTest::Longitudinal).then_some(setup.as_str()),
            },
            &progress,
        )
    })?;
    for group in rendered {
        plots.extend(group);
    }
    let mut baseline_entry = serde_json::json!({"input": provenance(&args.baseline)?, "case": {"name": baseline.config.name, "loadcase": baseline.config.loadcase}});
    if let Some(label) = &labels[0] {
        baseline_entry["label"] = label.as_str().into();
    }
    let report = serde_json::json!({
        "schema_version": 1, "command": "compare",
        "domain": baseline.domains.values().next().map_or("", ComparedDomain::domain),
        "domains": baseline.domains.values().map(|domain| serde_json::json!({
            "domain": domain.domain(),
            "test": domain.test().map(KncTest::name),
            "conditions": match domain {
                ComparedDomain::Knc { runs, .. } => runs_json(runs),
                ComparedDomain::Svc(_) => serde_json::Value::Null,
            },
        })).collect::<Vec<_>>(),
        "baseline": baseline_entry,
        "candidates": candidates, "status": status,
        "warnings": warnings.values().flatten().collect::<Vec<_>>(),
    });
    if let Some(output) = &args.output {
        let csv = metrics_csv(&inputs, &rows)?;
        let knc_details = baseline
            .domains
            .values()
            .filter_map(ComparedDomain::test)
            .map(|test| format!("knc-{}.json", test.name()))
            .collect::<Vec<_>>();
        let detail = serde_json::to_string_pretty(&report)? + "\n";
        let mut files = vec![
            ("report.json", detail.clone().into_bytes()),
            ("metrics.csv", csv.into_bytes()),
        ];
        if baseline.domains.contains_key("svc") {
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
            let file = CanonicalFile::open(path)?;
            match (
                file.summary().domain.as_deref(),
                file.summary().test.as_deref(),
            ) {
                (Some("svc"), _) => clab_core::config::load_config_with_input(config, path, None)
                    .map(|case| (case, true, false)),
                (Some("knc"), Some(test @ ("vertical" | "roll" | "longitudinal"))) => {
                    clab_core::config::load_config_with_input(config, path, Some(test))
                        .map(|case| (case, false, true))
                }
                (Some(domain), test) => bail!(
                    "unsupported domain '{domain}'{} in '{}'; supported: svc, knc vertical, knc roll, knc longitudinal",
                    test.map(|test| format!(" test '{test}'"))
                        .unwrap_or_default(),
                    path.display()
                ),
                (None, _) => {
                    // Converted before domain and test were stored: infer from the config.
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
    for (test, unit, source) in &units {
        validate_comparison_input(source)?;
        if let Some(knc) = test.config(unit)
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
        for (test, unit, source) in &units {
            paths.push(source.as_path());
            if let Some(static_input) = test
                .config(unit)
                .and_then(|knc| knc.static_simulation_hdf5.as_ref())
            {
                paths.push(static_input);
            }
        }
        for path in paths {
            if let Some(warning) = CanonicalFile::open(path)?.summary().legacy_warning() {
                eprintln!("warning: {warning}");
            }
        }
    }
    let mut domains = BTreeMap::new();
    let mut sources = Vec::new();
    let mut svc_corner_mass_kg: Option<BTreeMap<String, f64>> = None;
    if include_svc && case.svc_source_kind.is_some() {
        let values = match case.svc_source_kind {
            Some(clab_core::config::SourceKind::Simulation) if is_hdf5(&case.simulation_json) => {
                svc::evaluate_hdf5_case(&case)?
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
        domains.insert("svc", ComparedDomain::Svc(svc::canonical_metrics(&values)));
    }
    let mut knc_reports = Vec::new();
    for (test, unit, source) in &units {
        let report = knc::analyze_input(unit, *test, source, svc_corner_mass_kg.as_ref(), None)?;
        sources.push(source.clone());
        if let Some(static_input) = test
            .config(unit)
            .and_then(|knc| knc.static_simulation_hdf5.as_ref())
        {
            sources.push(static_input.clone());
        }
        knc_reports.push((*test, source.as_path(), report));
    }
    knc::ensure_distinct_setups(
        knc_reports
            .iter()
            .map(|(test, source, report)| (*test, *source, report)),
    )?;
    for test in KncTest::ALL {
        let mut reports: Vec<_> = knc_reports
            .iter_mut()
            .filter(|(candidate, ..)| *candidate == test)
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
                    curves: detail["curves"].clone(),
                    conditions: report.conditions.clone(),
                    branch: report.branch.clone(),
                    corner_mass_kg: report.corner_mass_kg.clone(),
                })
            })
            .collect::<Result<_>>()?;
        domains.insert(
            knc_key(test),
            ComparedDomain::Knc {
                test,
                metrics,
                runs,
            },
        );
    }
    Ok(Input {
        config: case,
        domains,
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
                domain: "svc",
                test: None,
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
    test: KncTest,
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
                domain: "knc",
                test: Some(test.name()),
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
        "domain",
        "test",
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
                .test
                .and_then(|test| input.conditions.get(test))
                .map(TestConditions::csv_cells)
                .unwrap_or_default()
        };
        let [base_setup, base_phase, base_offset] = cells(base);
        let [setup, phase, offset] = cells(other);
        csv.write_record([
            safe_cell(&base.path.display().to_string()),
            safe_cell(&candidate.display().to_string()),
            safe_cell(metric.domain),
            metric.test.map(safe_cell).unwrap_or_default(),
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

/// Rows of one domain grouped per metric (one value per candidate), in console order.
fn grouped_metrics<'a>(
    compared: &ComparedDomain,
    candidates: &[PathBuf],
    rows: &'a [(PathBuf, Metric)],
) -> GroupedMetrics<'a> {
    let (domain, domain_test) = (compared.domain(), compared.test().map(KncTest::name));
    let mut grouped: GroupedMetrics<'_> = Vec::new();
    for (candidate, metric) in rows
        .iter()
        .filter(|(_, metric)| metric.domain == domain && metric.test == domain_test)
    {
        let index = candidates
            .iter()
            .position(|path| path == candidate)
            .expect("candidate row must have an input");
        if let Some((_, _, values)) = grouped
            .iter_mut()
            .find(|(test, name, _)| *test == metric.test && *name == metric.metric.as_str())
        {
            values.push((index, metric));
        } else {
            grouped.push((metric.test, &metric.metric, vec![(index, metric)]));
        }
    }
    grouped.sort_by_key(|(test, name, _)| {
        crate::analysis::display::console_metric_order(domain, *test, name)
    });
    grouped
}

/// `--json` twin of `print_metrics`: one section line per compared domain.
fn json_metrics(
    inputs: &[InputSummary],
    candidates: &[PathBuf],
    domains: &BTreeMap<&'static str, ComparedDomain>,
    rows: &[(PathBuf, Metric)],
    unavailable_domains: &[(usize, &'static str)],
    _use_tolerances: bool,
    warnings: &BTreeMap<&str, Vec<String>>,
) {
    for key in DOMAIN_KEYS {
        let Some(compared) = domains.get(key) else {
            continue;
        };
        let (domain, test) = (compared.domain(), compared.test().map(KncTest::name));
        let metrics = grouped_metrics(compared, candidates, rows)
            .into_iter()
            .filter(|(test, name, _)| display::show_metric(domain, *test, name))
            .map(|(test, name, values)| {
                let mut per_candidate = vec![serde_json::Value::Null; candidates.len()];
                for (index, metric) in &values {
                    let compared = !unavailable_domains
                        .iter()
                        .any(|(unavailable, domain)| unavailable == index && *domain == key);
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
                    "name": display::qualified_metric_name(domain, test, name),
                    "unit": values[0].1.unit,
                    "baseline": values[0].1.baseline_value,
                    "allowed_difference": values.iter().find_map(|(_, metric)| metric.allowed_difference),
                    "candidates": per_candidate,
                })
            })
            .collect();
        let domain_rows: Vec<_> = rows
            .iter()
            .filter(|(_, metric)| metric.domain == domain && metric.test == test)
            .collect();
        let (passed, failed, missing, tolerance_missing) = outcome_counts(&domain_rows);
        display::print_json(&display::section_json(
            compared.title(),
            (domain, test),
            metrics,
            warnings.get(key).map_or(&[][..], Vec::as_slice),
            serde_json::json!({"inputs": inputs, "result": {
                "passed": passed, "failed": failed, "missing": missing,
                "tolerance_missing": tolerance_missing, "total": domain_rows.len(),
            }}),
        ));
    }
}

fn print_metrics(
    inputs: &[InputSummary],
    candidates: &[PathBuf],
    domains: &BTreeMap<&'static str, ComparedDomain>,
    rows: &[(PathBuf, Metric)],
    unavailable_domains: &[(usize, &'static str)],
    use_tolerances: bool,
    warnings: &BTreeMap<&str, Vec<String>>,
) {
    for key in DOMAIN_KEYS {
        let Some(compared) = domains.get(key) else {
            continue;
        };
        let (title, domain) = (compared.title(), compared.domain());
        let domain_test = compared.test().map(KncTest::name);
        let in_domain = |metric: &Metric| metric.domain == domain && metric.test == domain_test;
        println!();
        let grouped = grouped_metrics(compared, candidates, rows);
        println!("{title}");
        for (index, input) in inputs.iter().enumerate() {
            let role = if index == 0 {
                "baseline".to_owned()
            } else {
                format!("candidate_{index}")
            };
            println!("{role}: {} | {}", input.label, input.path.display());
            if let Some(conditions) = domain_test.and_then(|test| input.conditions.get(test)) {
                for line in conditions.lines() {
                    println!("{line}");
                }
            }
        }
        println!(
            "running {} metrics",
            grouped
                .iter()
                .filter(|(test, name, _)| {
                    crate::analysis::display::show_metric(domain, *test, name)
                })
                .count()
        );
        for (test, metric_name, values) in grouped
            .into_iter()
            .filter(|(test, name, _)| crate::analysis::display::show_metric(domain, *test, name))
        {
            let name = crate::analysis::display::qualified_metric_name(domain, test, metric_name);
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
                    !unavailable_domains
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
                    (metric.domain == domain && metric.test == test && metric.metric == metric_name)
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
        for _ in unavailable_domains
            .iter()
            .filter(|(_, unavailable)| *unavailable == key)
        {
            println!("  {key} ... {}", colored_status("not compared"));
        }
        let domain_rows: Vec<_> = rows
            .iter()
            .filter(|(_, metric)| in_domain(metric))
            .collect();
        let (passed, failed, missing, tolerance_missing) = outcome_counts(&domain_rows);
        if use_tolerances {
            println!(
                "comparison result: {}. {passed} ok; {failed} failed; {missing} metric missing; {tolerance_missing} tolerance missing; {} total",
                if failed > 0 {
                    "failed"
                } else if missing > 0
                    || unavailable_domains
                        .iter()
                        .any(|(_, unavailable)| *unavailable == key)
                {
                    "incomplete"
                } else {
                    "passed"
                },
                domain_rows.len()
            );
        } else {
            println!(
                "comparison result: informational. {} compared; {missing} metric missing; {} total",
                domain_rows.len() - missing,
                domain_rows.len()
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
                domain: "svc",
                test: None,
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
