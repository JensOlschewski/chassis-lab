use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clab_core::{
    config::CaseConfig,
    conventions::{apply_convention_for_final_sample, svc_conventions},
    simulation_hdf5::SimulationFile,
    svc_simulation_json::SvcSimulationFile,
};
use sha2::{Digest, Sha256};

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

pub(crate) struct Reports {
    pub report: serde_json::Value,
    pub detail: String,
    pub csv: String,
    pub metric_count: usize,
    pub warnings: Vec<String>,
}

pub(crate) fn analyze(config: &CaseConfig) -> Result<Reports> {
    reports(config, evaluate_case(config)?, Vec::new())
}

pub(crate) fn evaluate_case(config: &CaseConfig) -> Result<BTreeMap<String, f64>> {
    let simulation = SvcSimulationFile::open(&config.simulation_json).with_context(|| {
        format!(
            "failed to read SVC simulation JSON '{}'",
            config.simulation_json.display()
        )
    })?;
    evaluate(config, |entity, component, unit| {
        Ok(simulation.read_signal(entity, component, unit)?.values)
    })
}

pub(crate) fn analyze_hdf5(config: &CaseConfig) -> Result<Reports> {
    let (values, warnings) = evaluate_hdf5(config)?;
    reports(config, values, warnings)
}

pub(crate) fn supports_hdf5(config: &CaseConfig, simulation: &SimulationFile) -> bool {
    WHEEL_LOADS.into_iter().chain(GEOMETRY).all(|name| {
        config.signals.get(name).is_some_and(|reference| {
            simulation
                .read_signal(
                    &reference.entity,
                    &reference.component,
                    Some(&reference.unit),
                )
                .is_ok()
        })
    })
}

fn evaluate_hdf5(config: &CaseConfig) -> Result<(BTreeMap<String, f64>, Vec<String>)> {
    let simulation = SimulationFile::open(&config.simulation_json).with_context(|| {
        format!(
            "failed to read SVC simulation HDF5 '{}'",
            config.simulation_json.display()
        )
    })?;
    let mut warnings = Vec::new();
    let values = evaluate(config, |entity, component, unit| {
        let signal = simulation.read_signal(entity, component, unit)?;
        if signal.info.unit.is_empty() {
            warnings.push(missing_unit_warning(
                entity,
                component,
                unit.unwrap_or_default(),
            ));
        }
        Ok(signal.values)
    })?;
    warnings.sort();
    warnings.dedup();
    Ok((values, warnings))
}

fn evaluate(
    config: &CaseConfig,
    mut read_signal: impl FnMut(&str, &str, Option<&str>) -> Result<Vec<f64>>,
) -> Result<BTreeMap<String, f64>> {
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
        let mut convention = convention.clone();
        convention.source_unit = reference.unit.clone();
        let signal = read_signal(
            &reference.entity,
            &reference.component,
            Some(&reference.unit),
        )?;
        let transformed = apply_convention_for_final_sample(
            &signal,
            name,
            format!(
                "{}:{}/{}",
                config.simulation_json.display(),
                reference.entity,
                reference.component
            ),
            convention,
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

fn reports(
    config: &CaseConfig,
    values: BTreeMap<String, f64>,
    warnings: Vec<String>,
) -> Result<Reports> {
    let metrics = values
        .iter()
        .filter_map(|(name, value)| {
            metric_unit(name).ok().map(|unit| {
                serde_json::json!({
                    "domain": "svc",
                    "test": null,
                    "metric": name,
                    "unit": unit,
                    "value": value,
                })
            })
        })
        .collect::<Vec<_>>();
    let inputs = serde_json::json!({
        "config": provenance(&config.path)?,
        "simulation_json": provenance(&config.simulation_json)?,
    });
    let metric_count = metrics.len();
    let detail = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "case": {"name": config.name, "loadcase": config.loadcase},
        "inputs": inputs,
        "software": {"chassis_lab": env!("CARGO_PKG_VERSION")},
        "metrics": metrics,
        "warnings": warnings,
    });
    let mut csv = csv::WriterBuilder::new().from_writer(Vec::new());
    csv.write_record(["run", "domain", "test", "metric", "unit", "value"])?;
    for metric in detail["metrics"].as_array().unwrap() {
        csv.write_record([
            safe_cell(&config.name),
            "svc".into(),
            String::new(),
            safe_cell(metric["metric"].as_str().unwrap()),
            metric["unit"].as_str().unwrap().into(),
            metric["value"].as_f64().unwrap().to_string(),
        ])?;
    }
    Ok(Reports {
        report: detail.clone(),
        detail: serde_json::to_string_pretty(&detail)? + "\n",
        csv: String::from_utf8(csv.into_inner()?)?,
        metric_count,
        warnings: detail["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str().map(str::to_owned))
            .collect(),
    })
}

fn missing_unit_warning(entity: &str, component: &str, declared_unit: &str) -> String {
    format!(
        "simulation signal '{entity}/{component}' has no HDF5 unit metadata; using mapped unit '{declared_unit}'"
    )
}

pub(crate) fn metric_unit(name: &str) -> Result<String> {
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
    Ok(serde_json::json!({
        "path": PathBuf::from(path),
        "sha256": format!("{:x}", Sha256::digest(fs::read(path)?)),
    }))
}
