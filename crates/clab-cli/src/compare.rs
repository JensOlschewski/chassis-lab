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
    /// Apply validation tolerances from configuration.
    #[arg(short, long)]
    tolerances: bool,
    /// Directory for deterministic report bundle.
    #[arg(short = 'r', long = "report")]
    output: Option<PathBuf>,
    /// Replace report bundle already in output directory.
    #[arg(long)]
    force: bool,
    /// Include KnC rate-filter diagnostic plots in the report (requires --report).
    #[arg(long, requires = "output")]
    debug: bool,
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

enum ComparedDomain {
    Svc(Vec<svc::CanonicalMetric>),
    Knc {
        test: KncTest,
        curves: serde_json::Value,
        metrics: Vec<serde_json::Value>,
        branch: clab_core::config::VerticalBranch,
        /// This series' own `ride_frequency` mass map (same source as
        /// `knc::Reports::corner_mass_kg`): baseline and candidate
        /// resolve it from their own SVC run, so it travels per series,
        /// not shared.
        corner_mass_kg: BTreeMap<String, f64>,
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
const DOMAIN_KEYS: [&str; 3] = ["svc", "knc-vertical", "knc-roll"];

fn knc_key(test: KncTest) -> &'static str {
    match test {
        KncTest::Vertical => "knc-vertical",
        KncTest::Roll => "knc-roll",
    }
}

struct Input {
    config: clab_core::config::CaseConfig,
    domains: BTreeMap<&'static str, ComparedDomain>,
    sources: Vec<PathBuf>,
}

struct ComparisonTolerances {
    svc: clab_core::config::ValidationTolerances,
    /// Per KnC test, keyed like `Input::domains`.
    knc: BTreeMap<&'static str, clab_core::config::ValidationTolerances>,
}

type GroupedMetrics<'a> = Vec<(Option<&'a str>, &'a str, Vec<(usize, &'a Metric)>)>;

pub(crate) fn compare(args: CompareArgs) -> Result<()> {
    let baseline = load_input(&args.config, &args.baseline)?;
    let tolerances = args.tolerances.then(|| ComparisonTolerances {
        svc: baseline
            .config
            .validation
            .as_ref()
            .map(|v| v.tolerances.clone())
            .unwrap_or_else(empty_tolerances),
        knc: knc_configs(&baseline.config)
            .into_iter()
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
    for (index, path) in args.candidates.iter().enumerate() {
        let candidate = load_input(&args.config, path)?;
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
                        ..
                    },
                    ComparedDomain::Knc {
                        test,
                        curves: candidate_curves,
                        metrics: candidate_metrics,
                        branch: candidate_branch,
                        corner_mass_kg: candidate_mass,
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
                    domains.push(serde_json::json!({"domain": "knc", "test": test.name(), "status": "compared"}));
                    knc_candidates.entry(test.name()).or_default().push((
                        format!("Curve {}", index + 1),
                        candidate_curves.clone(),
                        candidate_branch.clone(),
                        candidate_mass.clone(),
                        format!(
                            "Curve {}: {} | {} | {}",
                            index + 1,
                            candidate.config.name,
                            candidate.config.loadcase,
                            absolute_path(path)?.display()
                        ),
                        path.as_path(),
                    ));
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
                "incompatible comparison domains between '{}' and '{}'",
                args.baseline.display(),
                path.display()
            );
        }
        rows.extend(metrics.iter().cloned().map(|metric| (path.clone(), metric)));
        candidate_sources.extend(candidate.sources.iter().cloned());
        candidates.push(serde_json::json!({"input": provenance(path)?, "case": {"name": candidate.config.name, "loadcase": candidate.config.loadcase}, "metrics": metrics, "domains": domains}));
    }
    let mut warnings = BTreeMap::new();
    if args.debug {
        for test in [KncTest::Vertical, KncTest::Roll] {
            let key = knc_key(test);
            let Some(ComparedDomain::Knc { branch, .. }) = baseline.domains.get(key) else {
                continue;
            };
            if !branch.measurement
                || knc_candidates.get(test.name()).is_some_and(|candidates| {
                    candidates.iter().any(|candidate| !candidate.2.measurement)
                })
            {
                warnings.insert(
                    key,
                    vec![display::tag_warning(
                        "knc",
                        Some(test.name()),
                        knc::DEBUG_RATE_WARNING,
                    )],
                );
            }
        }
    }
    print_metrics(
        &args.baseline,
        &args.candidates,
        &baseline.domains,
        &rows,
        &unavailable_domains,
        args.tolerances,
        &warnings,
    );
    let status = comparison_status(failed, incomplete, &rows, args.tolerances);
    let progress = ReportProgress::new(args.output.is_some());
    let plot_tests = [KncTest::Vertical, KncTest::Roll]
        .into_iter()
        .filter(|test| args.output.is_some() && baseline.domains.contains_key(knc_key(*test)))
        .collect::<Vec<_>>();
    let workers = knc::plot_worker_limit(plot_tests.len());
    let jobs = plot_tests
        .into_iter()
        .map(|test| {
            let candidates = knc_candidates.remove(test.name()).unwrap_or_default();
            (test.title(), (test, candidates))
        })
        .collect();
    let rendered = progress.plot_phases(jobs, |(test, candidates_for_test)| {
        let Some(ComparedDomain::Knc {
            curves: base,
            branch: base_branch,
            corner_mass_kg: base_mass,
            ..
        }) = baseline.domains.get(knc_key(test))
        else {
            anyhow::bail!("missing KnC {} baseline", test.name());
        };
        let mut footer = Vec::new();
        if let Some(config) = test.config(&baseline.config) {
            footer.push(format!("KnC setup: {}", config.setup.summary()));
        }
        footer.push(format!(
            "Config: {}",
            absolute_path(&args.config)?.display()
        ));
        footer.push(format!(
            "Baseline: {} | {} | {}",
            baseline.config.name,
            baseline.config.loadcase,
            absolute_path(&args.baseline)?.display()
        ));
        footer.extend(
            candidates_for_test
                .iter()
                .map(|candidate| candidate.4.clone()),
        );
        footer.push(format!("Created by: {}", report_creator()));
        footer.push(format!("clab version: {}", env!("CARGO_PKG_VERSION")));
        let debug = debug_info(
            args.debug,
            &args.config,
            &std::iter::once(args.baseline.as_path())
                .chain(candidates_for_test.iter().map(|candidate| candidate.5))
                .collect::<Vec<_>>(),
        )?;
        let candidates = candidates_for_test
            .into_iter()
            .map(|(label, curves, branch, mass, _, _)| (label, curves, branch, mass))
            .collect::<Vec<_>>();
        knc::compare_plots(
            &baseline.config,
            test,
            (base, base_branch, base_mass),
            &candidates,
            ("Baseline", &format!("plots/knc/{}/comparison", test.name())),
            knc::PlotOptions {
                footer: &footer.join("\n"),
                debug: debug.as_deref(),
                workers,
            },
            &progress,
        )
    })?;
    for group in rendered {
        plots.extend(group);
    }
    let report = serde_json::json!({
        "schema_version": 1, "command": "compare",
        "domain": baseline.domains.values().next().map_or("", ComparedDomain::domain),
        "domains": baseline.domains.values().map(|domain| serde_json::json!({
            "domain": domain.domain(),
            "test": domain.test().map(KncTest::name),
        })).collect::<Vec<_>>(),
        "baseline": {"input": provenance(&args.baseline)?, "case": {"name": baseline.config.name, "loadcase": baseline.config.loadcase}},
        "candidates": candidates, "status": status,
    });
    if let Some(output) = &args.output {
        let csv = metrics_csv(&args.baseline, &rows)?;
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
        progress.phase("Writing report bundle…");
        write_report_bundle(output, files, args.force, &inputs)?;
        progress.done("Writing report bundle");
        progress.finish();
        println!("report: {}", output.display());
    }
    println!(
        "comparison: baseline {}; {} candidate(s); {}",
        args.baseline.display(),
        args.candidates.len(),
        status
    );
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
                (Some("knc"), Some(test @ ("vertical" | "roll"))) => {
                    clab_core::config::load_config_with_input(config, path, Some(test))
                        .map(|case| (case, false, true))
                }
                (Some(domain), test) => bail!(
                    "unsupported domain '{domain}'{} in '{}'; supported: svc, knc vertical, knc roll",
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
    let knc_tests = if include_knc {
        knc_configs(&case)
    } else {
        Vec::new()
    };
    if include_svc && case.svc_source_kind.is_some() {
        validate_comparison_input(&case.simulation_json)?;
    }
    for (_, knc) in &knc_tests {
        validate_comparison_input(knc_source(knc))?;
        if knc.source_kind == clab_core::config::SourceKind::Simulation
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
        for (_, knc) in &knc_tests {
            paths.push(knc_source(knc));
            if let Some(static_input) = &knc.static_simulation_hdf5 {
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
    for (test, knc) in knc_tests {
        let source = knc_source(knc);
        let report = knc::analyze_input(&case, test, source, svc_corner_mass_kg.as_ref(), None)?;
        let detail: serde_json::Value = serde_json::from_str(&report.detail)?;
        sources.push(source.to_path_buf());
        if let Some(static_input) = &knc.static_simulation_hdf5 {
            sources.push(static_input.clone());
        }
        domains.insert(
            knc_key(test),
            ComparedDomain::Knc {
                test,
                curves: detail["curves"].clone(),
                metrics: report.metrics,
                branch: report.branch,
                corner_mass_kg: report.corner_mass_kg,
            },
        );
    }
    Ok(Input {
        config: case,
        domains,
        sources,
    })
}

/// KnC tests a compare input configures.
fn knc_configs(
    case: &clab_core::config::CaseConfig,
) -> Vec<(KncTest, &clab_core::config::VerticalConfig)> {
    [KncTest::Vertical, KncTest::Roll]
        .into_iter()
        .filter_map(|test| test.config(case).map(|knc| (test, knc)))
        .collect()
}

fn knc_source(knc: &clab_core::config::VerticalConfig) -> &Path {
    match knc.source_kind {
        clab_core::config::SourceKind::Simulation => &knc.simulation_hdf5,
        clab_core::config::SourceKind::Measurement => &knc.measurement_hdf5,
    }
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

fn compare_knc_metrics(
    test: KncTest,
    baseline: &[serde_json::Value],
    candidate: &[serde_json::Value],
    tolerances: Option<&clab_core::config::ValidationTolerances>,
) -> Vec<Metric> {
    let baseline = baseline
        .iter()
        .filter_map(|metric| metric["metric"].as_str().map(|name| (name, metric)))
        .collect::<BTreeMap<_, _>>();
    let candidate = candidate
        .iter()
        .filter_map(|metric| metric["metric"].as_str().map(|name| (name, metric)))
        .collect::<BTreeMap<_, _>>();
    baseline
        .keys()
        .chain(candidate.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
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

fn metrics_csv(baseline: &Path, rows: &[(PathBuf, Metric)]) -> Result<String> {
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
    ])?;
    for (candidate, metric) in rows {
        csv.write_record([
            safe_cell(&baseline.display().to_string()),
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
        ])?;
    }
    Ok(String::from_utf8(csv.into_inner()?)?)
}

fn print_metrics(
    baseline: &Path,
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
        let mut grouped: GroupedMetrics<'_> = Vec::new();
        for (candidate, metric) in rows.iter().filter(|(_, metric)| in_domain(metric)) {
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
        println!("{title}\nbaseline: {}", baseline.display());
        for (index, candidate) in candidates.iter().enumerate() {
            println!("candidate_{}: {}", index + 1, candidate.display());
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
            display::print_warnings(domain, domain_test, warnings);
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
    if !terminal {
        return status.into();
    }
    let color = match status {
        "ok" => "32",
        "FAILED" => "31",
        "missing" | "not compared" => "33",
        "tolerance missing" => "36",
        _ => return status.into(),
    };
    format!("\x1b[{color}m{status}\x1b[0m")
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
    use super::{Metric, comparison_status, render_status};
    use std::path::PathBuf;

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
