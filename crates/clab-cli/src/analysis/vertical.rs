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
    config::{CaseConfig, SignalRef, VerticalBranch},
    conventions::{apply_convention, vertical_conventions},
};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyModule},
};
use serde::{Deserialize, Serialize};

const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
const AXES: [&str; 3] = ["x", "y", "z"];
const DEFAULT_GRADIENT_POINTS_MM: f64 = 10.0;

#[derive(Debug, Deserialize, Serialize)]
struct Curve {
    corner: String,
    signal: String,
    values: Vec<f64>,
    unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    travel: Option<Vec<f64>>,
}

pub(crate) struct Reports {
    pub detail: String,
    pub plots: BTreeMap<String, Vec<u8>>,
    pub curve_count: usize,
    pub metrics: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
    pub normalization: serde_json::Value,
    pub branch: VerticalBranch,
}

pub(crate) fn analyze_input(config: &CaseConfig, path: &Path, plots: bool) -> Result<Reports> {
    analyze_input_with_footer(config, path, plots, None, None)
}

pub(crate) fn analyze_input_with_footer(
    config: &CaseConfig,
    path: &Path,
    plots: bool,
    footer: Option<&str>,
    debug: Option<&str>,
) -> Result<Reports> {
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
    let measurement = file.summary().source_kind == SourceKind::Measurement;
    warnings.extend(default_warnings(config, measurement));
    let relative_travel = normalization["method"] != "none";
    let metrics = gradient_metrics(config, &curves, relative_travel, measurement, &mut warnings)?;
    warnings.sort();
    warnings.dedup();
    let detail = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "domain": "knc",
        "test": "vertical",
        "metrics": metrics,
        "curves": curves,
        "warnings": warnings,
        "normalization": normalization,
    });
    let curve_count = detail["curves"].as_array().unwrap().len();
    Ok(Reports {
        detail: serde_json::to_string_pretty(&detail)? + "\n",
        plots: if plots {
            curve_plots(
                config,
                &detail["curves"],
                source,
                footer.unwrap_or(""),
                measurement,
                debug,
            )?
        } else {
            BTreeMap::new()
        },
        curve_count,
        metrics: detail["metrics"].as_array().unwrap().clone(),
        warnings: detail["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        normalization: detail["normalization"].clone(),
        branch: branch_options(config, measurement).clone(),
    })
}

/// Warn about every optional vertical setting the configuration leaves unset.
fn default_warnings(config: &CaseConfig, measurement: bool) -> Vec<String> {
    let Some(vertical) = &config.vertical else {
        return Vec::new();
    };
    let branch = vertical.branch.for_measurement(measurement);
    let mut warnings = Vec::new();
    let mut warn = |key: &str, table: &str, default: String| {
        warnings.push(format!(
            "KnC vertical config key '{key}' missing in [{table}]; using default {default}"
        ));
    };
    if vertical.window.gradient_points_mm.is_none() {
        let effective = effective_gradient_points(&vertical.window);
        let limited = if effective < DEFAULT_GRADIENT_POINTS_MM {
            format!(" (limited to travel window: {effective} mm)")
        } else {
            String::new()
        };
        warn(
            "gradient_points_mm",
            "setup.knc.metric",
            format!("{DEFAULT_GRADIENT_POINTS_MM} mm{limited}"),
        );
    }
    for key in &branch.defaulted {
        match *key {
            "direction" => warn("direction", "setup.knc.metric", branch.direction.clone()),
            "index" => warn("index", "setup.knc.metric", branch.index.to_string()),
            "plot_branches" => warn(
                "plot_branches",
                "setup.knc.window",
                branch.plot_branches.clone(),
            ),
            _ => {}
        }
    }
    warnings
}

fn branch_options(config: &CaseConfig, measurement: bool) -> &VerticalBranch {
    config
        .vertical
        .as_ref()
        .expect("vertical config")
        .branch
        .for_measurement(measurement)
}

/// Calculate secant metrics at configured symmetric wheel-center travel.
///
/// Gradient points default to 10 mm, limited to the travel window. Wheel-center
/// Z must be relative travel: measurement offsets or a static simulation
/// baseline. Unavailable metrics keep `value: null` and add one warning with
/// reason.
fn gradient_metrics(
    config: &CaseConfig,
    curves: &[Curve],
    relative_travel: bool,
    measurement: bool,
    warnings: &mut Vec<String>,
) -> Result<Vec<serde_json::Value>> {
    let Some(window) = config.vertical.as_ref().map(|vertical| &vertical.window) else {
        return Ok(Vec::new());
    };
    let gradient = effective_gradient_points(window);
    if gradient <= 0.0 {
        warnings.push(format!(
            "KnC vertical travel window {}..{} mm does not contain 0 mm; vertical metrics skipped",
            window.minimum_mm, window.maximum_mm
        ));
        return Ok(Vec::new());
    }
    if !relative_travel {
        warnings.push(
            "KnC vertical metrics require static_simulation normalization; vertical metrics skipped"
                .into(),
        );
        return Ok(Vec::new());
    }
    let curves = serde_json::to_string(curves)?;
    let branch = serde_json::to_string(branch_options(config, measurement))?;
    let response: String = with_kernel(|py, module| {
        let request = PyDict::new(py);
        request.set_item("gradient_mm", gradient)?;
        request.set_item("curves", &curves)?;
        request.set_item("branch", &branch)?;
        module.getattr("metrics")?.call1((request,))?.extract()
    })
    .context("failed to calculate vertical metrics with embedded Python")?;
    let response: serde_json::Value = serde_json::from_str(&response)
        .context("embedded Python returned invalid vertical metrics")?;
    let metrics = response["metrics"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("embedded Python returned incomplete vertical metrics"))?;
    Ok(metrics
        .iter()
        .map(|metric| {
            if let Some(reason) = metric["reason"].as_str() {
                warnings.push(format!(
                    "KnC vertical metric '{}' unavailable: {reason}",
                    metric["metric"].as_str().unwrap_or_default()
                ));
            }
            serde_json::json!({
                "domain": "knc",
                "test": "vertical",
                "metric": metric["metric"],
                "unit": metric["unit"],
                "value": metric["value"],
                "interval_mm": [-gradient, gradient],
                "lower": metric["lower"],
                "upper": metric["upper"],
            })
        })
        .collect())
}

fn effective_gradient_points(window: &clab_core::config::VerticalWindow) -> f64 {
    window.gradient_points_mm.unwrap_or_else(|| {
        DEFAULT_GRADIENT_POINTS_MM
            .min(-window.minimum_mm)
            .min(window.maximum_mm)
    })
}

/// Render same canonical curves together. Rust owns input selection; Python only draws arrays.
pub(crate) fn compare_plots(
    config: &CaseConfig,
    (baseline, baseline_branch): (&serde_json::Value, &VerticalBranch),
    candidates: &[(String, serde_json::Value, VerticalBranch)],
    baseline_label: &str,
    footer: &str,
    prefix: &str,
    debug: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let baseline = serde_json::to_string(baseline)?;
    let candidates_json = serde_json::to_string(
        &candidates
            .iter()
            .map(|(label, curves, _)| serde_json::json!({"label": label, "curves": curves}))
            .collect::<Vec<_>>(),
    )?;
    let branch = serde_json::to_string(
        &std::iter::once(baseline_branch)
            .chain(candidates.iter().map(|(_, _, branch)| branch))
            .collect::<Vec<_>>(),
    )?;
    let reports: BTreeMap<String, Vec<u8>> = with_kernel(|py, module| {
        module
            .getattr("plot_compare")?
            .call1((
                &baseline,
                &candidates_json,
                baseline_label,
                footer,
                logo(py),
                config
                    .vertical
                    .as_ref()
                    .map(|vertical| effective_gradient_points(&vertical.window)),
                &branch,
                debug,
            ))?
            .extract()
    })
    .context("failed to generate vertical comparison PDF reports with embedded Python")?;
    Ok(reports
        .into_iter()
        .map(|(name, pdf)| (format!("{prefix}/{name}"), pdf))
        .collect())
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
    for suffix in ["wheel_load", "toe", "camber", "tire_deflection"] {
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
    for (suffix, channel, unit, relative) in [
        ("wheel_load", format!("{prefix}fz"), "N", false),
        ("toe", format!("{prefix}steer"), "deg", false),
        ("camber", format!("{prefix}cam"), "deg", false),
        ("wheel_pad_z", format!("{prefix}pz"), "mm", true),
        ("tire_deflection", format!("{prefix}tdef"), "mm", true),
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
        let offset = if relative {
            channel.info.offset.unwrap_or_default()
        } else {
            0.0
        };
        curves.push(Curve {
            corner: corner.into(),
            signal: suffix.into(),
            values: channel
                .values
                .into_iter()
                .map(|value| (value - offset) * sign)
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

fn curve_plots(
    config: &CaseConfig,
    curves: &serde_json::Value,
    source_label: &str,
    footer: &str,
    measurement: bool,
    debug: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let curves = serde_json::to_string(curves)?;
    let gradient = config
        .vertical
        .as_ref()
        .map(|vertical| effective_gradient_points(&vertical.window));
    let branch = serde_json::to_string(branch_options(config, measurement))?;
    with_kernel(|py, module| {
        module
            .getattr("plot")?
            .call1((curves, source_label, footer, logo(py), gradient, &branch, debug))?
            .extract()
    })
    .context("failed to generate vertical PDF reports with embedded Python; install Python dependencies with '.venv/bin/python -m pip install -e ./py-chassislab'")
}

/// Load embedded vertical kernel and run `call` with it.
fn with_kernel<T>(call: impl FnOnce(Python<'_>, &Bound<'_, PyModule>) -> PyResult<T>) -> Result<T> {
    let plot_source = CStr::from_bytes_with_nul(
        concat!(
            include_str!("../../../../py-chassislab/src/chassis_lab/knc_plot_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    let source = CStr::from_bytes_with_nul(
        concat!(
            include_str!("../../../../py-chassislab/src/chassis_lab/knc_vertical_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    Ok(Python::attach(|py| -> PyResult<T> {
        add_venv_site_packages(py)?;
        let plot_module = PyModule::from_code(
            py,
            plot_source,
            c"knc_plot_kernel.py",
            c"chassis_lab.knc_plot_kernel",
        )?;
        PyModule::import(py, "sys")?
            .getattr("modules")?
            .set_item("chassis_lab.knc_plot_kernel", plot_module)?;
        let module = PyModule::from_code(
            py,
            source,
            c"knc_vertical_kernel.py",
            c"chassis_lab.knc_vertical_kernel",
        )?;
        call(py, &module)
    })?)
}

fn logo(py: Python<'_>) -> Bound<'_, PyBytes> {
    PyBytes::new(
        py,
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/flb-logo.pdf")),
    )
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
