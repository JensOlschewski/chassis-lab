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
    analysis::{svc, vertical},
    analyze::{absolute_path, debug_info, write_bundle},
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
    Vertical {
        curves: serde_json::Value,
        metrics: Vec<serde_json::Value>,
        branch: clab_core::config::VerticalBranch,
        /// This series' own `ride_frequency` mass map (same source as
        /// `vertical::Reports::corner_mass_kg`): baseline and candidate
        /// resolve it from their own SVC run, so it travels per series,
        /// not shared.
        corner_mass_kg: BTreeMap<String, f64>,
    },
}

struct Input {
    config: clab_core::config::CaseConfig,
    domains: BTreeMap<&'static str, ComparedDomain>,
    sources: Vec<PathBuf>,
}

struct ComparisonTolerances {
    svc: clab_core::config::ValidationTolerances,
    vertical: clab_core::config::ValidationTolerances,
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
        vertical: baseline
            .config
            .vertical
            .as_ref()
            .map(|v| v.tolerances.clone())
            .unwrap_or_else(empty_tolerances),
    });
    let mut failed = false;
    let mut incomplete = false;
    let mut rows = Vec::new();
    let mut candidates = Vec::new();
    let mut plots = BTreeMap::new();
    let mut vertical_candidates = Vec::new();
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
                domains.push(serde_json::json!({"domain": name, "test": if *name == "knc" { serde_json::Value::String("vertical".into()) } else { serde_json::Value::Null }, "status": "not compared"}));
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
                    ComparedDomain::Vertical {
                        metrics: base_metrics,
                        ..
                    },
                    ComparedDomain::Vertical {
                        curves: candidate_curves,
                        metrics: candidate_metrics,
                        branch: candidate_branch,
                        corner_mass_kg: candidate_mass,
                    },
                ) => {
                    let compared = compare_vertical_gradient_metrics(
                        base_metrics,
                        candidate_metrics,
                        tolerances.as_ref().map(|value| &value.vertical),
                    );
                    failed |= compared.iter().any(|metric| metric.status == "failed");
                    incomplete |= compared.iter().any(|metric| metric.status == "missing");
                    metrics.extend(compared);
                    vertical_candidates.push((
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
                    domains.push(serde_json::json!({"domain": "knc", "test": "vertical", "status": "compared"}));
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
    print_metrics(
        &args.baseline,
        &args.candidates,
        &baseline.domains,
        &rows,
        &unavailable_domains,
        args.tolerances,
    );
    let status = comparison_status(failed, incomplete, &rows, args.tolerances);
    if args.output.is_some()
        && let Some(ComparedDomain::Vertical {
            curves: base,
            branch: base_branch,
            corner_mass_kg: base_mass,
            ..
        }) = baseline.domains.get("knc")
    {
        let footer = baseline
            .config
            .vertical
            .iter()
            .map(|vertical| format!("KnC setup: {}", vertical.setup.summary()))
            .chain(std::iter::once(format!(
                "Config: {}",
                absolute_path(&args.config)?.display()
            )))
            .chain(std::iter::once(format!(
                "Baseline: {} | {} | {}",
                baseline.config.name,
                baseline.config.loadcase,
                absolute_path(&args.baseline)?.display()
            )))
            .chain(
                vertical_candidates
                    .iter()
                    .map(|(_, _, _, _, metadata, _)| metadata.clone()),
            )
            .chain([
                format!("Created by: {}", report_creator()),
                format!("clab version: {}", env!("CARGO_PKG_VERSION")),
            ])
            .collect::<Vec<_>>()
            .join("\n");
        let debug = debug_info(
            args.debug,
            &args.config,
            &std::iter::once(args.baseline.as_path())
                .chain(vertical_candidates.iter().map(|candidate| candidate.5))
                .collect::<Vec<_>>(),
        )?;
        if debug.is_some()
            && (!base_branch.measurement
                || vertical_candidates
                    .iter()
                    .any(|(_, _, branch, _, _, _)| !branch.measurement))
        {
            eprintln!(
                "warning: knc::vertical: debug rate plots require measurement input; skipped for simulation data"
            );
        }
        let candidates = vertical_candidates
            .into_iter()
            .map(|(label, curves, branch, mass, _, _)| (label, curves, branch, mass))
            .collect::<Vec<_>>();
        plots.extend(vertical::compare_plots(
            &baseline.config,
            (base, base_branch, base_mass),
            &candidates,
            "Baseline",
            &footer,
            "plots/knc/vertical/comparison",
            debug.as_deref(),
        )?);
    }
    let report = serde_json::json!({
        "schema_version": 1, "command": "compare",
        "domain": baseline.domains.keys().next().copied().unwrap_or(""),
        "domains": baseline.domains.keys().map(|name| serde_json::json!({
            "domain": name,
            "test": if *name == "knc" { serde_json::Value::String("vertical".into()) } else { serde_json::Value::Null },
        })).collect::<Vec<_>>(),
        "baseline": {"input": provenance(&args.baseline)?, "case": {"name": baseline.config.name, "loadcase": baseline.config.loadcase}},
        "candidates": candidates, "status": status,
    });
    if let Some(output) = &args.output {
        let csv = metrics_csv(&args.baseline, &rows)?;
        let detail = serde_json::to_string_pretty(&report)? + "\n";
        let mut files = vec![
            ("report.json", detail.clone().into_bytes()),
            ("metrics.csv", csv.into_bytes()),
        ];
        if baseline.domains.contains_key("svc") {
            files.push(("svc.json", detail.into_bytes()));
        }
        if baseline.domains.contains_key("knc") {
            files.push((
                "knc-vertical.json",
                (serde_json::to_string_pretty(&report)? + "\n").into_bytes(),
            ));
        }
        files.extend(plots.iter().map(|(name, pdf)| (name.as_str(), pdf.clone())));
        let mut inputs = vec![args.config.as_path(), args.baseline.as_path()];
        inputs.extend(baseline.sources.iter().map(PathBuf::as_path));
        inputs.extend(args.candidates.iter().map(PathBuf::as_path));
        inputs.extend(candidate_sources.iter().map(PathBuf::as_path));
        write_bundle(output, &files, args.force, &inputs)?;
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
    let (case, include_svc, include_vertical) = match extension.as_deref() {
        Some("toml") => {
            clab_core::config::load_config_and_run(config, path).map(|case| (case, true, true))
        }
        Some("json") => clab_core::config::load_config_with_input(config, path, false)
            .map(|case| (case, true, false)),
        Some("h5" | "hdf5") => {
            let file = CanonicalFile::open(path)?;
            match (
                file.summary().domain.as_deref(),
                file.summary().test.as_deref(),
            ) {
                (Some("svc"), _) => clab_core::config::load_config_with_input(config, path, false)
                    .map(|case| (case, true, false)),
                (Some("knc"), Some("vertical")) => {
                    clab_core::config::load_config_with_input(config, path, true)
                        .map(|case| (case, false, true))
                }
                (Some(domain), test) => bail!(
                    "unsupported domain '{domain}'{} in '{}'; supported: svc, knc vertical",
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
                        clab_core::config::load_config_with_input(config, path, false).ok();
                    match svc_case {
                        Some(svc_case)
                            if file.summary().source_kind == SourceKind::Simulation
                                && svc::supports_hdf5(&svc_case, &file) =>
                        {
                            Ok((svc_case, true, false))
                        }
                        _ => clab_core::config::load_config_with_input(config, path, true)
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
    if include_svc && case.svc_source_kind.is_some() {
        validate_comparison_input(&case.simulation_json)?;
    }
    if include_vertical && let Some(vertical_config) = &case.vertical {
        let source = match vertical_config.source_kind {
            clab_core::config::SourceKind::Simulation => &vertical_config.simulation_hdf5,
            clab_core::config::SourceKind::Measurement => &vertical_config.measurement_hdf5,
        };
        validate_comparison_input(source)?;
        if vertical_config.source_kind == clab_core::config::SourceKind::Simulation
            && let Some(static_input) = &vertical_config.static_simulation_hdf5
        {
            validate_comparison_input(static_input)?;
        }
    }
    if matches!(extension.as_deref(), Some("toml")) {
        let mut paths = Vec::new();
        if include_svc && is_hdf5(&case.simulation_json) {
            paths.push(case.simulation_json.as_path());
        }
        if include_vertical && let Some(vertical) = &case.vertical {
            paths.push(match vertical.source_kind {
                clab_core::config::SourceKind::Simulation => &vertical.simulation_hdf5,
                clab_core::config::SourceKind::Measurement => &vertical.measurement_hdf5,
            });
            if let Some(static_input) = &vertical.static_simulation_hdf5 {
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
                clab_core::svc_reference::load_svc_reference(
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
    if include_vertical && let Some(vertical_config) = &case.vertical {
        let source = match vertical_config.source_kind {
            clab_core::config::SourceKind::Simulation => &vertical_config.simulation_hdf5,
            clab_core::config::SourceKind::Measurement => &vertical_config.measurement_hdf5,
        };
        let report = vertical::analyze_input(&case, source, svc_corner_mass_kg.as_ref(), None)?;
        let detail: serde_json::Value = serde_json::from_str(&report.detail)?;
        sources.push(source.clone());
        if let Some(static_input) = &vertical_config.static_simulation_hdf5 {
            sources.push(static_input.clone());
        }
        domains.insert(
            "knc",
            ComparedDomain::Vertical {
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

fn compare_vertical_gradient_metrics(
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
                test: Some("vertical"),
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
) {
    for (domain, title) in [("svc", "SVC"), ("knc", "KnC Vertical")] {
        if !domains.contains_key(domain) {
            continue;
        }
        println!();
        let mut grouped: GroupedMetrics<'_> = Vec::new();
        for (candidate, metric) in rows.iter().filter(|(_, metric)| metric.domain == domain) {
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
        println!("{title}\nbaseline: {}", baseline.display());
        for (index, candidate) in candidates.iter().enumerate() {
            println!("candidate_{}: {}", index + 1, candidate.display());
        }
        println!("running {} metrics", grouped.len());
        for (test, metric_name, values) in grouped {
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
                        .any(|(unavailable, name)| *unavailable == index && *name == domain)
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
            .filter(|(_, unavailable)| *unavailable == domain)
        {
            println!("  {domain} ... {}", colored_status("not compared"));
        }
        let domain_rows: Vec<_> = rows
            .iter()
            .filter(|(_, metric)| metric.domain == domain)
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
                        .any(|(_, unavailable)| *unavailable == domain)
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
