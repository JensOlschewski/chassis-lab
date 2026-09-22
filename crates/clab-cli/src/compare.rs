use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
};

use anyhow::{Result, bail};
use clap::Args;
use serde::Serialize;
use sha2::Digest;

use crate::{analysis::svc, analyze::write_bundle};

#[derive(Debug, Args)]
pub(crate) struct CompareArgs {
    /// Shared TOML configuration: mappings, units, conventions, physics, and setup.
    config: PathBuf,
    /// Baseline SVC JSON or run TOML manifest.
    baseline: PathBuf,
    /// Candidate SVC JSON or run TOML manifests.
    #[arg(required = true, num_args = 1..)]
    candidates: Vec<PathBuf>,
    /// Optional TOML tolerance definition. Signals without tolerances are informational.
    #[arg(short, long)]
    tolerances: Option<PathBuf>,
    /// Directory for deterministic report bundle.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Replace report bundle already in output directory.
    #[arg(long)]
    force: bool,
}

#[derive(Debug)]
pub(crate) struct ToleranceFailure;

impl fmt::Display for ToleranceFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SVC comparison failed tolerance checks")
    }
}

impl std::error::Error for ToleranceFailure {}

#[derive(Clone, Serialize)]
struct Metric {
    domain: &'static str,
    test: Option<&'static str>,
    metric: String,
    unit: String,
    baseline_value: f64,
    candidate_value: f64,
    difference: f64,
    absolute_tolerance: Option<f64>,
    relative_percent: Option<f64>,
    allowed_difference: Option<f64>,
    status: &'static str,
}

pub(crate) fn compare(args: CompareArgs) -> Result<()> {
    let baseline_config = load_input(&args.config, &args.baseline)?;
    if baseline_config.svc_source_kind != Some(clab_core::config::SourceKind::Simulation) {
        bail!("compare supports simulation SVC inputs only");
    }
    let baseline = svc::evaluate_case(&baseline_config)?;
    let tolerances = match &args.tolerances {
        Some(path) => load_tolerances(path)?,
        None => baseline_config
            .validation
            .as_ref()
            .map(|v| v.tolerances.clone())
            .unwrap_or(clab_core::config::ValidationTolerances {
                absolute: BTreeMap::new(),
                relative_percent: BTreeMap::new(),
            }),
    };
    let mut failed = false;
    let mut rows = Vec::new();
    let mut candidate_reports = Vec::new();
    for candidate_path in &args.candidates {
        let candidate_config = load_input(&args.config, candidate_path)?;
        if candidate_config.svc_source_kind != Some(clab_core::config::SourceKind::Simulation) {
            bail!("compare supports simulation SVC inputs only");
        }
        let candidate = svc::evaluate_case(&candidate_config)?;
        let metrics = compare_metrics(&baseline, &candidate, &tolerances)?;
        failed |= metrics.iter().any(|metric| metric.status == "failed");
        rows.extend(
            metrics
                .iter()
                .cloned()
                .map(|metric| (candidate_path.clone(), metric)),
        );
        candidate_reports.push(serde_json::json!({
            "input": provenance(candidate_path)?,
            "case": {"name": candidate_config.name, "loadcase": candidate_config.loadcase},
            "metrics": metrics,
        }));
    }
    let status = if rows
        .iter()
        .all(|(_, metric)| metric.status == "informational")
    {
        "informational"
    } else if failed {
        "failed"
    } else {
        "passed"
    };
    let report = serde_json::json!({
        "schema_version": 1,
        "command": "compare",
        "domain": "svc",
        "baseline": {"input": provenance(&args.baseline)?, "case": {"name": baseline_config.name, "loadcase": baseline_config.loadcase}},
        "candidates": candidate_reports,
        "status": status,
    });
    if let Some(output) = &args.output {
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
                safe_cell(&args.baseline.display().to_string()),
                safe_cell(&candidate.display().to_string()),
                safe_cell(metric.domain),
                metric.test.map(safe_cell).unwrap_or_default(),
                safe_cell(&metric.metric),
                safe_cell(&metric.unit),
                metric.baseline_value.to_string(),
                metric.candidate_value.to_string(),
                metric.difference.to_string(),
                metric
                    .absolute_tolerance
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                metric
                    .relative_percent
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                metric
                    .allowed_difference
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                safe_cell(metric.status),
            ])?;
        }
        let detail = serde_json::to_string_pretty(&report)? + "\n";
        let inputs = std::iter::once(args.config.as_path())
            .chain(std::iter::once(args.baseline.as_path()))
            .chain(args.candidates.iter().map(PathBuf::as_path))
            .chain(args.tolerances.iter().map(PathBuf::as_path))
            .collect::<Vec<_>>();
        write_bundle(
            output,
            &[
                ("report.json", detail.clone()),
                ("metrics.csv", String::from_utf8(csv.into_inner()?)?),
                ("svc.json", detail),
            ],
            args.force,
            &inputs,
        )?;
        println!("report: {}", output.display());
    }
    println!(
        "SVC comparison: baseline {}; {} candidate(s); {}",
        args.baseline.display(),
        args.candidates.len(),
        status
    );
    if failed {
        bail!(ToleranceFailure);
    }
    Ok(())
}

fn load_input(config: &Path, input: &Path) -> Result<clab_core::config::CaseConfig> {
    match input
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("json") => clab_core::config::load_config_with_input(config, input, false),
        Some("toml") => clab_core::config::load_config_and_run(config, input),
        _ => bail!("compare accepts SVC JSON or run TOML inputs"),
    }
    .map_err(|error| {
        anyhow::anyhow!(
            "failed to load compare input '{}': {error}",
            input.display()
        )
    })
}

fn compare_metrics(
    baseline: &BTreeMap<String, f64>,
    candidate: &BTreeMap<String, f64>,
    tolerances: &clab_core::config::ValidationTolerances,
) -> Result<Vec<Metric>> {
    let mut metrics = Vec::new();
    for (name, baseline) in baseline {
        let Ok(unit) = svc::metric_unit(name) else {
            continue;
        };
        let candidate = *candidate
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("candidate SVC metric missing: {name}"))?;
        let absolute = tolerances.absolute.get(name).copied();
        let relative = tolerances
            .relative_percent
            .get(name)
            .map(|percent| baseline.abs() * percent / 100.0);
        let allowed = match (absolute, relative) {
            (Some(a), Some(r)) => Some(a.min(r)),
            (Some(v), None) | (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        let difference = (candidate - baseline).abs();
        metrics.push(Metric {
            domain: "svc",
            test: None,
            metric: name.clone(),
            unit,
            baseline_value: *baseline,
            candidate_value: candidate,
            difference,
            absolute_tolerance: absolute,
            relative_percent: tolerances.relative_percent.get(name).copied(),
            allowed_difference: allowed,
            status: match allowed {
                None => "informational",
                Some(allowed) if difference <= allowed => "passed",
                Some(_) => "failed",
            },
        });
    }
    Ok(metrics)
}

fn load_tolerances(path: &Path) -> Result<clab_core::config::ValidationTolerances> {
    let root: toml::Table = fs::read_to_string(path)?.parse()?;
    let svc = root
        .get("svc")
        .and_then(toml::Value::as_table)
        .or_else(|| {
            root.get("validation")
                .and_then(toml::Value::as_table)
                .and_then(|v| v.get("svc"))
                .and_then(toml::Value::as_table)
        })
        .unwrap_or(&root);
    let validation = svc
        .get("validation")
        .and_then(toml::Value::as_table)
        .unwrap_or(svc);
    let tolerance = validation
        .get("tolerances")
        .and_then(toml::Value::as_table)
        .unwrap_or(validation);
    let map = |name: &str| -> Result<BTreeMap<String, f64>> {
        tolerance
            .get(name)
            .and_then(toml::Value::as_table)
            .map(|table| {
                table
                    .iter()
                    .map(|(key, value)| {
                        value
                            .as_float()
                            .or_else(|| value.as_integer().map(|value| value as f64))
                            .filter(|value| value.is_finite() && *value >= 0.0)
                            .map(|value| Ok((key.clone(), value)))
                            .unwrap_or_else(|| {
                                bail!("tolerance '{key}' must be finite and non-negative")
                            })
                    })
                    .collect()
            })
            .unwrap_or_else(|| Ok(BTreeMap::new()))
    };
    Ok(clab_core::config::ValidationTolerances {
        absolute: map("absolute")?,
        relative_percent: map("relative_percent")?,
    })
}

fn safe_cell(value: &str) -> String {
    if matches!(
        value.trim_start().as_bytes().first(),
        Some(b'=' | b'+' | b'-' | b'@')
    ) {
        format!("'{value}")
    } else {
        value.into()
    }
}

fn provenance(path: &Path) -> Result<serde_json::Value> {
    Ok(
        serde_json::json!({"path": path, "sha256": format!("{:x}", sha2::Sha256::digest(fs::read(path)?))}),
    )
}
