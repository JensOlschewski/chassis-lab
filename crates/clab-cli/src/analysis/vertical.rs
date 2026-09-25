use std::{
    collections::BTreeMap,
    env,
    ffi::CStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clab_core::{
    canonical_hdf5::{CanonicalFile, SourceKind},
    config::{CaseConfig, SignalRef},
    conventions::{apply_convention, vertical_conventions},
};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyModule},
};
use serde::{Deserialize, Serialize};

const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
const AXES: [&str; 3] = ["x", "y", "z"];

#[derive(Debug, Deserialize, Serialize)]
struct Curve {
    corner: String,
    signal: String,
    values: Vec<f64>,
    unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    travel: Option<Vec<f64>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Comparison {
    pub corner: String,
    pub axis: String,
    pub maximum_reference_mm: f64,
    pub maximum_candidate_mm: f64,
    pub maximum_error_mm: f64,
}

pub(crate) struct Reports {
    pub detail: String,
    pub plots: BTreeMap<String, String>,
    pub curve_count: usize,
    pub warnings: Vec<String>,
    pub normalization: serde_json::Value,
}

pub(crate) fn analyze_input(config: &CaseConfig, path: &Path, plots: bool) -> Result<Reports> {
    let file = CanonicalFile::open(path)
        .with_context(|| format!("failed to read HDF5 '{}'", path.display()))?;
    let (curves, source, mut warnings, normalization) = match file.summary().source_kind {
        SourceKind::Simulation => {
            let (mut curves, mut warnings) = simulation_curves(config, &file)?;
            if let Some(static_path) = config
                .vertical
                .as_ref()
                .and_then(|v| v.static_simulation_hdf5.as_ref())
            {
                warnings.extend(subtract_static(
                    config,
                    &mut curves,
                    &CanonicalFile::open(static_path)?,
                )?);
                (
                    curves,
                    "Simulation",
                    warnings,
                    serde_json::json!({"static_simulation": static_path, "method": "drop_initial_sample_and_subtract_final_sample"}),
                )
            } else {
                (
                    curves,
                    "Simulation",
                    {
                        warnings.push(
                            "KnC simulation has no static_simulation; curves are not normalized"
                                .into(),
                        );
                        warnings
                    },
                    serde_json::json!({"static_simulation": null, "method": "none"}),
                )
            }
        }
        SourceKind::Measurement => (
            measurement_curves(&file)?,
            "Measurement",
            Vec::new(),
            serde_json::json!({"method": "measurement_native"}),
        ),
    };
    warnings.sort();
    warnings.dedup();
    let detail = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "domain": "knc",
        "test": "vertical",
        "metrics": [],
        "curves": curves,
        "warnings": warnings,
        "normalization": normalization,
    });
    let curve_count = detail["curves"].as_array().unwrap().len();
    Ok(Reports {
        detail: serde_json::to_string_pretty(&detail)? + "\n",
        plots: if plots {
            curve_plots(&detail["curves"], source)?
        } else {
            BTreeMap::new()
        },
        curve_count,
        warnings: detail["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        normalization: detail["normalization"].clone(),
    })
}

/// Render same canonical curves together. Rust owns input selection; Python only draws arrays.
pub(crate) fn compare_plots(
    baseline: &serde_json::Value,
    candidates: &[(String, serde_json::Value)],
    baseline_label: &str,
    footer: &str,
    prefix: &str,
) -> Result<BTreeMap<String, String>> {
    let baseline = serde_json::to_string(baseline)?;
    let candidates = serde_json::to_string(
        &candidates
            .iter()
            .map(|(label, curves)| serde_json::json!({"label": label, "curves": curves}))
            .collect::<Vec<_>>(),
    )?;
    let source = CStr::from_bytes_with_nul(
        concat!(
            include_str!("../../../../py-chassislab/src/chassis_lab/knc_vertical_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    let reports = Python::attach(|py| -> PyResult<String> {
        add_venv_site_packages(py)?;
        let module = PyModule::from_code(
            py,
            source,
            c"knc_vertical_kernel.py",
            c"chassis_lab.knc_vertical_kernel",
        )?;
        module
            .getattr("plot_compare")?
            .call1((
                &baseline,
                &candidates,
                baseline_label,
                footer,
                PyBytes::new(
                    py,
                    include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/flb-logo.png")),
                ),
            ))?
            .extract()
    })
    .context("failed to generate vertical comparison SVG reports with embedded Python")?;
    let reports: BTreeMap<String, String> = serde_json::from_str(&reports)
        .context("embedded Python returned invalid vertical comparison SVG reports")?;
    Ok(reports
        .into_iter()
        .map(|(name, svg)| (format!("{prefix}/{name}"), svg))
        .collect())
}

/// Compare normalized canonical curves on configured travel grid.
pub(crate) fn compare_metrics(
    config: &CaseConfig,
    baseline: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Result<Vec<Comparison>> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("KnC vertical configuration is required"))?;
    let mut grid = Vec::new();
    let mut value = vertical.window.minimum_mm;
    while value <= vertical.window.maximum_mm {
        grid.push(value);
        value += vertical.window.step_mm;
    }
    if grid
        .last()
        .is_some_and(|value| *value < vertical.window.maximum_mm)
    {
        grid.push(vertical.window.maximum_mm);
    }
    let curves = comparison_curves(baseline, candidate)?;
    let curves = serde_json::to_string(&curves)?;
    let source = CStr::from_bytes_with_nul(
        concat!(
            include_str!("../../../../py-chassislab/src/chassis_lab/knc_vertical_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    let response = Python::attach(|py| -> PyResult<String> {
        add_venv_site_packages(py)?;
        let module = PyModule::from_code(
            py,
            source,
            c"knc_vertical_kernel.py",
            c"chassis_lab.knc_vertical_kernel",
        )?;
        let request = PyDict::new(py);
        request.set_item("grid", &grid)?;
        request.set_item("curves", &curves)?;
        module.getattr("evaluate")?.call1((request,))?.extract()
    })
    .context("failed to compare vertical curves with embedded Python")?;
    let response: serde_json::Value = serde_json::from_str(&response)
        .context("embedded Python returned invalid vertical comparison metrics")?;
    serde_json::from_value(response["comparisons"].clone())
        .context("embedded Python returned incomplete vertical comparison metrics")
}

fn comparison_curves(
    baseline: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Result<BTreeMap<String, serde_json::Value>> {
    let baseline = index_curves(baseline)?;
    let candidate = index_curves(candidate)?;
    let mut curves = BTreeMap::new();
    for corner in CORNERS {
        for axis in AXES {
            let coordinate = format!("wheel_center_{axis}");
            let travel = "wheel_center_z".to_string();
            let curve = |source: &BTreeMap<(String, String), serde_json::Value>| -> Result<serde_json::Value> {
                let values = source.get(&(corner.into(), coordinate.clone())).ok_or_else(|| anyhow::anyhow!("canonical vertical curve missing: {corner} {coordinate}"))?;
                let travel_values = match values.get("travel") {
                    Some(travel) => travel,
                    None => &source
                        .get(&(corner.into(), travel.clone()))
                        .ok_or_else(|| anyhow::anyhow!("canonical vertical travel curve missing: {corner}"))?
                        ["values"],
                };
                Ok(serde_json::json!({"travel": travel_values, "values": values["values"]}))
            };
            curves.insert(
                format!("{corner}:{axis}"),
                serde_json::json!({"baseline": curve(&baseline)?, "candidate": curve(&candidate)?}),
            );
        }
    }
    Ok(curves)
}

fn index_curves(
    curves: &serde_json::Value,
) -> Result<BTreeMap<(String, String), serde_json::Value>> {
    curves
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("canonical vertical curves must be an array"))?
        .iter()
        .map(|curve| {
            Ok((
                (
                    curve["corner"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("canonical vertical curve corner missing"))?
                        .into(),
                    curve["signal"]
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("canonical vertical curve signal missing"))?
                        .into(),
                ),
                curve.clone(),
            ))
        })
        .collect()
}

fn subtract_static(
    config: &CaseConfig,
    curves: &mut [Curve],
    static_file: &CanonicalFile,
) -> Result<Vec<String>> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("KnC vertical configuration is required"))?;
    let conventions = vertical_conventions();
    let mut warnings = Vec::new();
    for curve in curves {
        let name = format!("{}_{}", curve.corner, curve.signal);
        let reference = mapping(vertical, &name)?;
        let mut convention = convention(&conventions, &name)?;
        convention.source_unit = reference.unit.clone();
        let signal = static_file.read_signal(
            &reference.entity,
            Some(&reference.component),
            Some(&convention.source_unit),
        )?;
        if signal.info.unit.is_empty() {
            warnings.push(missing_unit_warning(
                &reference.entity,
                &reference.component,
                &convention.source_unit,
            ));
        }
        let static_value = apply_convention(
            &signal.values,
            &name,
            format!("{}/{}", reference.entity, reference.component),
            convention,
            config.gravity,
        )?
        .values
        .last()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("static signal '{name}' contains no values"))?;
        for value in &mut curve.values {
            *value -= static_value;
        }
    }
    Ok(warnings)
}

fn simulation_curves(
    config: &CaseConfig,
    simulation: &CanonicalFile,
) -> Result<(Vec<Curve>, Vec<String>)> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("KnC vertical configuration is required"))?;
    let conventions = vertical_conventions();
    let mut curves = Vec::new();
    let mut warnings = Vec::new();
    for corner in CORNERS {
        let travel_name = format!("{corner}_travel");
        let travel_reference = mapping(vertical, &travel_name)?;
        let mut travel_convention = convention(&conventions, &travel_name)?;
        travel_convention.source_unit = travel_reference.unit.clone();
        let travel_signal = simulation.read_signal(
            &travel_reference.entity,
            Some(&travel_reference.component),
            Some(&travel_convention.source_unit),
        )?;
        let travel = apply_convention(
            &travel_signal.values,
            &travel_name,
            format!("{}/{}", travel_reference.entity, travel_reference.component),
            travel_convention,
            config.gravity,
        )?
        .values;
        let travel = drop_initial_sample(travel);
        for axis in AXES {
            let name = format!("{corner}_wheel_center_{axis}");
            let reference = mapping(vertical, &name)?;
            let mut convention = convention(&conventions, &name)?;
            convention.source_unit = reference.unit.clone();
            let signal = simulation.read_signal(
                &reference.entity,
                Some(&reference.component),
                Some(&convention.source_unit),
            )?;
            if signal.info.unit.is_empty() {
                warnings.push(missing_unit_warning(
                    &reference.entity,
                    &reference.component,
                    &convention.source_unit,
                ));
            }
            let value = apply_convention(
                &signal.values,
                &name,
                format!("{}/{}", reference.entity, reference.component),
                convention,
                config.gravity,
            )?
            .values;
            curves.push(Curve {
                corner: corner.into(),
                signal: format!("wheel_center_{axis}"),
                values: drop_initial_sample(value),
                unit: "mm".into(),
                travel: Some(travel.clone()),
            });
        }
        optional_simulation_curves(
            config,
            simulation,
            vertical,
            &conventions,
            corner,
            &mut curves,
            &mut warnings,
        )?;
    }
    Ok((curves, warnings))
}

fn measurement_curves(measurement: &CanonicalFile) -> Result<Vec<Curve>> {
    let mut curves = Vec::new();
    for (corner, prefix) in CORNERS.into_iter().zip(["lf", "rf", "lr", "rr"]) {
        for axis in AXES {
            let channel =
                measurement.read_signal(&format!("{prefix}wc{axis}"), None, Some("mm"))?;
            curves.push(Curve {
                corner: corner.into(),
                signal: format!("wheel_center_{axis}"),
                values: channel
                    .values
                    .into_iter()
                    .map(|value| value - channel.info.offset.unwrap_or_default())
                    .collect(),
                unit: "mm".into(),
                travel: None,
            });
        }
        optional_measurement_curves(measurement, corner, prefix, &mut curves)?;
    }
    Ok(curves)
}

fn optional_simulation_curves(
    config: &CaseConfig,
    simulation: &CanonicalFile,
    vertical: &clab_core::config::VerticalConfig,
    conventions: &BTreeMap<String, clab_core::conventions::SignalConvention>,
    corner: &str,
    curves: &mut Vec<Curve>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for suffix in ["wheel_load", "toe", "camber"] {
        let name = format!("{corner}_{suffix}");
        let Some(reference) = vertical.signals.get(&name) else {
            continue;
        };
        let mut convention = convention(conventions, &name)?;
        convention.source_unit = reference.unit.clone();
        let unit = convention.unit.as_str().into();
        let signal = simulation.read_signal(
            &reference.entity,
            Some(&reference.component),
            Some(&convention.source_unit),
        )?;
        if signal.info.unit.is_empty() {
            warnings.push(missing_unit_warning(
                &reference.entity,
                &reference.component,
                &convention.source_unit,
            ));
        }
        let values = apply_convention(
            &signal.values,
            &name,
            format!("{}/{}", reference.entity, reference.component),
            convention,
            config.gravity,
        )?
        .values;
        curves.push(Curve {
            corner: corner.into(),
            signal: suffix.into(),
            values: drop_initial_sample(values),
            unit,
            travel: None,
        });
    }
    Ok(())
}

/// Adams legacy evaluation removes column one from every multi-sample signal.
fn drop_initial_sample(values: Vec<f64>) -> Vec<f64> {
    if values.len() > 1 {
        values.into_iter().skip(1).collect()
    } else {
        values
    }
}

fn missing_unit_warning(entity: &str, component: &str, declared_unit: &str) -> String {
    format!(
        "simulation signal '{entity}/{component}' has no HDF5 unit metadata; using mapped unit '{declared_unit}'"
    )
}

fn optional_measurement_curves(
    measurement: &CanonicalFile,
    corner: &str,
    prefix: &str,
    curves: &mut Vec<Curve>,
) -> Result<()> {
    for (suffix, channel, unit) in [
        ("wheel_load", format!("{prefix}fz"), "N"),
        ("toe", format!("{prefix}steer"), "deg"),
        ("camber", format!("{prefix}cam"), "deg"),
    ] {
        if !measurement
            .signals()
            .iter()
            .any(|info| info.safe_name == channel)
        {
            continue;
        }
        let channel = measurement.read_signal(&channel, None, Some(unit))?;
        let sign = if suffix == "toe" && matches!(corner, "front_right" | "rear_right") {
            -1.0
        } else {
            1.0
        };
        curves.push(Curve {
            corner: corner.into(),
            signal: suffix.into(),
            values: channel
                .values
                .into_iter()
                .map(|value| value * sign)
                .collect(),
            unit: unit.into(),
            travel: None,
        });
    }
    Ok(())
}

fn mapping<'a>(
    vertical: &'a clab_core::config::VerticalConfig,
    name: &str,
) -> Result<&'a SignalRef> {
    vertical
        .signals
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("vertical signal mapping missing: {name}"))
}

fn convention(
    conventions: &BTreeMap<String, clab_core::conventions::SignalConvention>,
    name: &str,
) -> Result<clab_core::conventions::SignalConvention> {
    conventions
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("vertical signal convention missing: {name}"))
}

fn curve_plots(curves: &serde_json::Value, source_label: &str) -> Result<BTreeMap<String, String>> {
    let curves = serde_json::to_string(curves)?;
    let source = CStr::from_bytes_with_nul(
        concat!(
            include_str!("../../../../py-chassislab/src/chassis_lab/knc_vertical_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    let reports = Python::attach(|py| -> PyResult<String> {
        add_venv_site_packages(py)?;
        let module = PyModule::from_code(
            py,
            source,
            c"knc_vertical_kernel.py",
            c"chassis_lab.knc_vertical_kernel",
        )?;
        module.getattr("plot")?.call1((curves, source_label))?.extract()
    })
    .context("failed to generate vertical SVG reports with embedded Python; install Python dependencies with '.venv/bin/python -m pip install -e ./py-chassislab'")?;
    serde_json::from_str(&reports).context("embedded Python returned invalid vertical SVG reports")
}

fn add_venv_site_packages(py: Python<'_>) -> PyResult<()> {
    let venv = env::var_os("VIRTUAL_ENV")
        .map(Into::into)
        .or_else(|| env::current_dir().ok().and_then(|path| find_venv(&path)));
    let Some(venv) = venv else {
        return Ok(());
    };
    if let Ok(entries) = fs::read_dir(venv.join("lib")) {
        for entry in entries.flatten() {
            let site_packages = entry.path().join("site-packages");
            if site_packages.is_dir() {
                PyModule::import(py, "site")?.call_method1("addsitedir", (site_packages,))?;
            }
        }
    }
    Ok(())
}

fn find_venv(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .map(|directory| directory.join(".venv"))
        .find(|venv| venv.is_dir())
}
