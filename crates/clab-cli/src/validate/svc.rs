use std::{
    collections::BTreeMap,
    fs,
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clab_core::{
    config::{CaseConfig, SignalRef},
    conventions::{SignalConvention, apply_convention_for_final_sample, svc_conventions},
    simulation_hdf5::SimulationFile,
    svc_reference::{SvcReference, load_svc_reference},
};
use clap::Args;
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

const WHEEL_LOADS: [&str; 4] = [
    "front_left_wheel_load",
    "front_right_wheel_load",
    "rear_left_wheel_load",
    "rear_right_wheel_load",
];
const GEOMETRY: [&str; 20] = [
    "front_left_ride_height",
    "front_right_ride_height",
    "rear_left_ride_height",
    "rear_right_ride_height",
    "wheelbase",
    "front_track",
    "rear_track",
    "cg_height",
    "inertia_xx",
    "inertia_yy",
    "inertia_zz",
    "front_left_wheel_center_x",
    "front_left_toe",
    "front_right_toe",
    "rear_left_toe",
    "rear_right_toe",
    "front_left_camber",
    "front_right_camber",
    "rear_left_camber",
    "rear_right_camber",
];

#[derive(Debug, Args)]
pub(crate) struct SvcArgs {
    /// TOML case configuration.
    config: PathBuf,
    #[arg(long)]
    json_output: Option<PathBuf>,
    #[arg(long)]
    csv_output: Option<PathBuf>,
    #[arg(short, long)]
    force: bool,
    #[arg(long, value_enum, default_value_t = Color::Auto)]
    color: Color,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Error)]
#[error("SVC tolerance validation failed")]
pub(crate) struct ToleranceFailure;

#[derive(Debug, Serialize)]
struct Metric {
    name: String,
    unit: String,
    actual: f64,
    reference: f64,
    difference: f64,
    absolute_tolerance: Option<f64>,
    relative_percent: Option<f64>,
    allowed_difference: f64,
    passed: bool,
}

#[derive(Debug, Serialize)]
struct Provenance {
    path: PathBuf,
    sha256: String,
}

pub(crate) fn validate(args: SvcArgs) -> Result<()> {
    let config = clab_core::config::load_config(&args.config).map_err(|e| {
        anyhow::anyhow!(
            "failed to load configuration '{}': {e}",
            args.config.display()
        )
    })?;
    if !matches!(config.domain.as_str(), "svc" | "multi") {
        bail!(
            "SVC evaluator requires domain 'svc', found '{}'",
            config.domain
        );
    }
    let validation = config.validation.as_ref().ok_or_else(|| {
        anyhow::anyhow!("case '{}' has no [validation] configuration", config.name)
    })?;
    let mut protected = vec![
        config.path.as_path(),
        config.simulation_hdf5.as_path(),
        validation.reference.as_path(),
    ];
    if let Some(path) = &config.svc_signal_map {
        protected.push(path);
    }
    validate_outputs(
        args.json_output.as_deref(),
        args.csv_output.as_deref(),
        args.force,
        &protected,
    )?;

    let simulation = SimulationFile::open(&config.simulation_hdf5).with_context(|| {
        format!(
            "failed to read simulation HDF5 '{}'",
            config.simulation_hdf5.display()
        )
    })?;
    let actual = evaluate(&config, &simulation)?;
    let reference = load_svc_reference(&validation.reference).map_err(|e| {
        anyhow::anyhow!(
            "failed to load SVC reference '{}': {e}",
            validation.reference.display()
        )
    })?;
    if (
        reference.case.name.as_str(),
        reference.case.variant.as_str(),
        reference.case.loadcase.as_str(),
    ) != (
        config.name.as_str(),
        config.variant.as_str(),
        config.loadcase.as_str(),
    ) {
        bail!(
            "reference case identity does not match configuration: expected ({:?}, {:?}, {:?}), found ({:?}, {:?}, {:?})",
            config.name,
            config.variant,
            config.loadcase,
            reference.case.name,
            reference.case.variant,
            reference.case.loadcase
        );
    }
    let metrics = compare(&actual, &reference, &validation.tolerances)?;
    let json = report_json(&config, &reference, &metrics)?;
    let mut reports = Vec::new();
    if let Some(path) = args.json_output.as_deref() {
        reports.push((path, json));
    }
    if let Some(path) = args.csv_output.as_deref() {
        reports.push((path, report_csv(&config, &reference, &metrics)?));
    }
    let staged = stage_reports(&reports)?;
    publish_staged(staged, args.force)?;
    print_console(&config, &reference, &metrics, color_enabled(args.color));
    if metrics.iter().any(|metric| !metric.passed) {
        return Err(ToleranceFailure.into());
    }
    Ok(())
}

fn evaluate(config: &CaseConfig, simulation: &SimulationFile) -> Result<BTreeMap<String, f64>> {
    let conventions = svc_conventions();
    let mut values = BTreeMap::new();
    for name in WHEEL_LOADS.into_iter().chain(GEOMETRY) {
        let reference = config
            .signals
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("missing SVC signal mapping: {name}"))?;
        let convention = conventions
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("missing SVC signal convention: {name}"))?;
        validate_mapping_unit(reference, name, convention)?;
        let signal = simulation.read_signal(
            &reference.entity,
            &reference.component,
            Some(&convention.source_unit),
        )?;
        let transformed = apply_convention_for_final_sample(
            &signal.values,
            name,
            format!(
                "{}:{}/{}",
                config.simulation_hdf5.display(),
                reference.entity,
                reference.component
            ),
            convention.clone(),
            config.gravity,
        )?;
        let value = *transformed
            .values
            .last()
            .ok_or_else(|| anyhow::anyhow!("SVC signal '{name}' contains no values"))?;
        if !value.is_finite() {
            bail!("SVC signal '{name}' at final sample must be finite, found {value}");
        }
        values.insert(name.to_owned(), value);
    }
    let front = values["front_left_wheel_load"] + values["front_right_wheel_load"];
    let rear = values["rear_left_wheel_load"] + values["rear_right_wheel_load"];
    let total = front + rear;
    if !total.is_finite() || total <= 0.0 {
        bail!("total SVC mass must be finite and positive, found {total}");
    }
    values.insert("front_mass_kg".into(), front);
    values.insert("rear_mass_kg".into(), rear);
    values.insert("total_mass_kg".into(), total);
    values.insert("front_distribution_percent".into(), 100.0 * front / total);
    values.insert("rear_distribution_percent".into(), 100.0 * rear / total);
    let wheelbase = values["wheelbase"];
    if wheelbase <= 0.0 {
        bail!("wheelbase must be positive, found {wheelbase}");
    }
    values.insert(
        "cg_x_from_front_axle_mm".into(),
        values["rear_distribution_percent"] / 100.0 * wheelbase,
    );
    for (canonical, source) in [
        ("wheelbase_mm", "wheelbase"),
        ("front_track_mm", "front_track"),
        ("rear_track_mm", "rear_track"),
        ("cg_height_mm", "cg_height"),
        ("inertia_xx_kg_mm2", "inertia_xx"),
        ("inertia_yy_kg_mm2", "inertia_yy"),
        ("inertia_zz_kg_mm2", "inertia_zz"),
        ("ride_height_front_left_mm", "front_left_ride_height"),
        ("ride_height_front_right_mm", "front_right_ride_height"),
        ("ride_height_rear_left_mm", "rear_left_ride_height"),
        ("ride_height_rear_right_mm", "rear_right_ride_height"),
        ("toe_front_left_deg", "front_left_toe"),
        ("toe_front_right_deg", "front_right_toe"),
        ("toe_rear_left_deg", "rear_left_toe"),
        ("toe_rear_right_deg", "rear_right_toe"),
        ("camber_front_left_deg", "front_left_camber"),
        ("camber_front_right_deg", "front_right_camber"),
        ("camber_rear_left_deg", "rear_left_camber"),
        ("camber_rear_right_deg", "rear_right_camber"),
    ] {
        values.insert(canonical.into(), values[source]);
    }
    for (name, value) in [
        ("front_left_mass_kg", values["front_left_wheel_load"]),
        ("front_right_mass_kg", values["front_right_wheel_load"]),
        ("rear_left_mass_kg", values["rear_left_wheel_load"]),
        ("rear_right_mass_kg", values["rear_right_wheel_load"]),
    ] {
        values.insert(name.into(), value);
    }
    Ok(values)
}

fn validate_mapping_unit(
    reference: &SignalRef,
    name: &str,
    convention: &SignalConvention,
) -> Result<()> {
    if reference.unit != convention.source_unit {
        bail!(
            "SVC signal '{name}' must use configured unit '{}'",
            convention.source_unit
        );
    }
    Ok(())
}

fn compare(
    actual: &BTreeMap<String, f64>,
    reference: &SvcReference,
    tolerances: &clab_core::config::ValidationTolerances,
) -> Result<Vec<Metric>> {
    let tolerance_names: std::collections::BTreeSet<_> = tolerances
        .absolute
        .keys()
        .chain(tolerances.relative_percent.keys())
        .collect();
    if let Some(name) = tolerance_names
        .into_iter()
        .find(|name| !reference.values.contains_key(*name))
    {
        bail!("tolerance configured for unknown reference metric(s): {name}");
    }
    reference
        .values
        .iter()
        .map(|(name, expected)| {
            let measured = *actual
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("calculated SVC metric missing: {name}"))?;
            if !measured.is_finite() {
                bail!("calculated SVC metric '{name}' must be finite");
            }
            let absolute = tolerances.absolute.get(name).copied();
            let relative = tolerances.relative_percent.get(name).copied();
            if absolute.is_none() && relative.is_none() {
                bail!("no acceptance tolerance configured for metric: {name}");
            }
            let allowed = [
                absolute,
                relative.map(|value| expected.abs() * value / 100.0),
            ]
            .into_iter()
            .flatten()
            .fold(0.0, f64::max);
            Ok(Metric {
                name: name.clone(),
                unit: metric_unit(name)?,
                actual: measured,
                reference: *expected,
                difference: (measured - expected).abs(),
                absolute_tolerance: absolute,
                relative_percent: relative,
                allowed_difference: allowed,
                passed: (measured - expected).abs() <= allowed,
            })
        })
        .collect()
}

fn metric_unit(name: &str) -> Result<String> {
    for (suffix, unit) in [
        ("_kg_mm2", "kg*mm^2"),
        ("_percent", "%"),
        ("_kg", "kg"),
        ("_mm", "mm"),
        ("_deg", "deg"),
    ] {
        if name.ends_with(suffix) {
            return Ok(unit.into());
        }
    }
    bail!("no report unit defined for SVC metric: {name}")
}

fn provenance(path: &Path) -> Result<Provenance> {
    Ok(Provenance {
        path: path.to_path_buf(),
        sha256: format!("{:x}", Sha256::digest(fs::read(path)?)),
    })
}

fn report_json(
    config: &CaseConfig,
    reference: &SvcReference,
    metrics: &[Metric],
) -> Result<String> {
    let mut inputs = serde_json::json!({"config": provenance(&config.path)?, "simulation_hdf5": provenance(&config.simulation_hdf5)?, "reference": provenance(&reference.path)?});
    if let Some(path) = &config.svc_signal_map {
        inputs["signal_map"] = serde_json::to_value(provenance(path)?)?;
    }
    let sources: Vec<_> = reference.sources.iter().map(|source| serde_json::json!({"file": source.file, "sha256": source.sha256, "test_date": source.test_date, "result": source.result})).collect();
    Ok(serde_json::to_string_pretty(
        &serde_json::json!({"schema_version": 2, "case": {"name": config.name, "variant": config.variant, "loadcase": config.loadcase}, "sample_index": -1, "passed": metrics.iter().all(|m| m.passed), "failed_count": metrics.iter().filter(|m| !m.passed).count(), "inputs": inputs, "reference_sources": sources, "software": {"chassis_lab": env!("CARGO_PKG_VERSION")}, "metrics": metrics}),
    )? + "\n")
}

fn report_csv(config: &CaseConfig, reference: &SvcReference, metrics: &[Metric]) -> Result<String> {
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record([
        "case",
        "variant",
        "loadcase",
        "metric",
        "unit",
        "actual",
        "reference",
        "difference",
        "absolute_tolerance",
        "relative_percent",
        "allowed_difference",
        "passed",
        "config_sha256",
        "signal_map_sha256",
        "simulation_sha256",
        "reference_sha256",
    ])?;
    let config_hash = provenance(&config.path)?.sha256;
    let simulation_hash = provenance(&config.simulation_hdf5)?.sha256;
    let reference_hash = provenance(&reference.path)?.sha256;
    let signal_hash = config
        .svc_signal_map
        .as_ref()
        .map(|path| provenance(path))
        .transpose()?
        .map(|p| p.sha256)
        .unwrap_or_default();
    for metric in metrics {
        let row = [
            config.name.clone(),
            config.variant.clone(),
            config.loadcase.clone(),
            metric.name.clone(),
            metric.unit.clone(),
            metric.actual.to_string(),
            metric.reference.to_string(),
            metric.difference.to_string(),
            metric
                .absolute_tolerance
                .map(|v| v.to_string())
                .unwrap_or_default(),
            metric
                .relative_percent
                .map(|v| v.to_string())
                .unwrap_or_default(),
            metric.allowed_difference.to_string(),
            if metric.passed {
                "true".into()
            } else {
                "false".into()
            },
            config_hash.clone(),
            signal_hash.clone(),
            simulation_hash.clone(),
            reference_hash.clone(),
        ];
        writer.write_record(row)?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?)
}

fn print_console(config: &CaseConfig, reference: &SvcReference, metrics: &[Metric], color: bool) {
    println!(
        "SVC validation\ncase: {} ({}, {})\nsimulation: {}\nreference: {}\n\nrunning {} metrics",
        config.name,
        config.variant,
        config.loadcase,
        config.simulation_hdf5.display(),
        reference.path.display(),
        metrics.len()
    );
    for metric in metrics {
        let status = if metric.passed { "ok" } else { "FAILED" };
        let status = if color {
            format!(
                "{}{}\x1b[0m",
                if metric.passed {
                    "\x1b[32m"
                } else {
                    "\x1b[31m"
                },
                status
            )
        } else {
            status.into()
        };
        println!(
            "  {} ... {} (actual {:.8}; reference {:.8}; diff {:.8} {} {:.8} {})",
            metric.name,
            status,
            metric.actual,
            metric.reference,
            metric.difference,
            if metric.passed { "<=" } else { ">" },
            metric.allowed_difference,
            metric.unit
        );
    }
    let failed = metrics.iter().filter(|m| !m.passed).count();
    println!(
        "\nvalidation result: {}. {} passed; {} failed; {} total",
        if failed == 0 { "ok" } else { "FAILED" },
        metrics.len() - failed,
        failed,
        metrics.len()
    );
}

fn validate_outputs(
    json: Option<&Path>,
    csv: Option<&Path>,
    force: bool,
    inputs: &[&Path],
) -> Result<()> {
    let outputs: Vec<_> = [json, csv].into_iter().flatten().collect();
    let mut unique = std::collections::BTreeSet::new();
    for path in outputs {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create output parent directory: {}",
                parent.display()
            )
        })?;
        if path.is_dir() {
            bail!("report output path is a directory: {}", path.display());
        }
        let parent = parent.canonicalize()?;
        let canonical = if path.exists() {
            path.canonicalize()?
        } else {
            parent.join(path.file_name().unwrap())
        };
        if !unique.insert(canonical.clone()) {
            bail!("JSON and CSV output paths must be different");
        }
        for input in inputs {
            if input.exists() && input.canonicalize()? == canonical {
                bail!(
                    "report output must not replace an input: {}",
                    path.display()
                );
            }
        }
        if path.exists() && !force {
            bail!(
                "report output already exists: {}; pass --force to replace it",
                path.display()
            );
        }
    }
    Ok(())
}

fn color_enabled(color: Color) -> bool {
    color_enabled_for(
        color,
        std::io::stdout().is_terminal(),
        std::env::var_os("NO_COLOR").is_some(),
    )
}

fn color_enabled_for(color: Color, terminal: bool, no_color: bool) -> bool {
    match color {
        Color::Always => true,
        Color::Never => false,
        Color::Auto => terminal && !no_color,
    }
}

fn stage_reports<'a>(
    reports: &[(&'a Path, String)],
) -> Result<Vec<(&'a Path, tempfile::NamedTempFile)>> {
    reports
        .iter()
        .map(|(path, content)| {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(content.as_bytes())?;
            temporary.flush()?;
            Ok((*path, temporary))
        })
        .collect()
}

fn publish_staged(staged: Vec<(&Path, tempfile::NamedTempFile)>, force: bool) -> Result<()> {
    for (path, temporary) in staged {
        let result = if force {
            temporary.persist(path).map(|_| ())
        } else {
            temporary.persist_noclobber(path).map(|_| ())
        };
        result.map_err(|error| error.error)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::Color;
    use super::{color_enabled_for, stage_reports};
    use std::fs;

    #[test]
    fn no_force_staging_does_not_replace_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("report.json");
        fs::write(&path, "old").unwrap();

        let staged = stage_reports(&[(&path, "new".into())]).unwrap();
        assert!(staged[0].0.exists());
        assert_eq!(fs::read_to_string(path).unwrap(), "old");
    }

    #[test]
    fn color_modes_respect_terminal_and_no_color() {
        assert!(color_enabled_for(Color::Always, false, true));
        assert!(!color_enabled_for(Color::Never, true, false));
        assert!(color_enabled_for(Color::Auto, true, false));
        assert!(!color_enabled_for(Color::Auto, true, true));
        assert!(!color_enabled_for(Color::Auto, false, false));
    }

    #[test]
    fn staging_failure_leaves_outputs_untouched() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.json");
        let bad_parent = directory.path().join("not-a-directory");
        fs::write(&bad_parent, "file").unwrap();
        let second = bad_parent.join("second.csv");

        assert!(stage_reports(&[(&first, "new".into()), (&second, "new".into())]).is_err());
        assert!(!first.exists());
    }
}
