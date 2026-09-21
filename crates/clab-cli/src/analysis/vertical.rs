use std::{
    collections::BTreeMap,
    env,
    ffi::CStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use clab_core::{
    config::{CaseConfig, SignalRef},
    conventions::{apply_convention, vertical_conventions},
    input_hdf5::{Hdf5Input, open_hdf5_input},
    measurement_hdf5::MeasurementFile,
    simulation_hdf5::SimulationFile,
};
use pyo3::{prelude::*, types::PyModule};
use serde::Serialize;

const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
const AXES: [&str; 3] = ["x", "y", "z"];

#[derive(Debug, Serialize)]
struct Curve {
    corner: String,
    signal: String,
    values: Vec<f64>,
    unit: String,
}

pub(crate) struct Reports {
    pub detail: String,
    pub plots: BTreeMap<String, String>,
    pub curve_count: usize,
    pub warnings: Vec<String>,
    pub normalization: serde_json::Value,
}

pub(crate) fn analyze_input(config: &CaseConfig, path: &Path) -> Result<Reports> {
    let (curves, source, warnings, normalization) = match open_hdf5_input(path)
        .with_context(|| format!("failed to read HDF5 '{}'", path.display()))?
    {
        Hdf5Input::Simulation(file) => {
            let mut curves = simulation_curves(config, &file)?;
            if let Some(static_path) = config
                .vertical
                .as_ref()
                .and_then(|v| v.static_simulation_hdf5.as_ref())
            {
                subtract_static(config, &mut curves, &SimulationFile::open(static_path)?)?;
                (
                    curves,
                    "Simulation",
                    Vec::<String>::new(),
                    serde_json::json!({"static_simulation": static_path, "method": "subtract_final_sample"}),
                )
            } else {
                (
                    curves,
                    "Simulation",
                    vec![
                        "KnC simulation has no static_simulation; curves are not normalized".into(),
                    ],
                    serde_json::json!({"static_simulation": null, "method": "none"}),
                )
            }
        }
        Hdf5Input::Measurement(file) => (
            measurement_curves(&file)?,
            "Measurement",
            Vec::new(),
            serde_json::json!({"method": "measurement_native"}),
        ),
    };
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
        plots: curve_plots(&detail["curves"], source)?,
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

fn subtract_static(
    config: &CaseConfig,
    curves: &mut [Curve],
    static_file: &SimulationFile,
) -> Result<()> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("KnC vertical configuration is required"))?;
    let conventions = vertical_conventions();
    for curve in curves {
        let name = format!("{}_{}", curve.corner, curve.signal);
        let reference = mapping(vertical, &name)?;
        let convention = convention(&conventions, &name)?;
        let expected_unit = (curve.signal.starts_with("wheel_center_")
            || curve.signal == "wheel_load")
            .then_some(convention.source_unit.as_str());
        let static_value = apply_convention(
            &static_file
                .read_signal(&reference.entity, &reference.component, expected_unit)?
                .values,
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
    Ok(())
}

fn simulation_curves(config: &CaseConfig, simulation: &SimulationFile) -> Result<Vec<Curve>> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("KnC vertical configuration is required"))?;
    let conventions = vertical_conventions();
    let mut curves = Vec::new();
    for corner in CORNERS {
        for axis in AXES {
            let name = format!("{corner}_wheel_center_{axis}");
            let reference = mapping(vertical, &name)?;
            let convention = convention(&conventions, &name)?;
            let value = apply_convention(
                &simulation
                    .read_signal(
                        &reference.entity,
                        &reference.component,
                        Some(&convention.source_unit),
                    )?
                    .values,
                &name,
                format!("{}/{}", reference.entity, reference.component),
                convention,
                config.gravity,
            )?
            .values;
            curves.push(Curve {
                corner: corner.into(),
                signal: format!("wheel_center_{axis}"),
                values: value,
                unit: "mm".into(),
            });
        }
        optional_simulation_curves(
            config,
            simulation,
            vertical,
            &conventions,
            corner,
            &mut curves,
        )?;
    }
    Ok(curves)
}

fn measurement_curves(measurement: &MeasurementFile) -> Result<Vec<Curve>> {
    let mut curves = Vec::new();
    for (corner, prefix) in CORNERS.into_iter().zip(["lf", "rf", "lr", "rr"]) {
        for axis in AXES {
            let channel = measurement.read_channel(&format!("{prefix}wc{axis}"), Some("mm"))?;
            curves.push(Curve {
                corner: corner.into(),
                signal: format!("wheel_center_{axis}"),
                values: channel
                    .values
                    .into_iter()
                    .map(|value| value - channel.info.offset)
                    .collect(),
                unit: "mm".into(),
            });
        }
        optional_measurement_curves(measurement, corner, prefix, &mut curves)?;
    }
    Ok(curves)
}

fn optional_simulation_curves(
    config: &CaseConfig,
    simulation: &SimulationFile,
    vertical: &clab_core::config::VerticalConfig,
    conventions: &BTreeMap<String, clab_core::conventions::SignalConvention>,
    corner: &str,
    curves: &mut Vec<Curve>,
) -> Result<()> {
    for suffix in ["wheel_load", "toe", "camber"] {
        let name = format!("{corner}_{suffix}");
        let Some(reference) = vertical.signals.get(&name) else {
            continue;
        };
        let convention = convention(conventions, &name)?;
        let unit = convention.unit.as_str().into();
        // ADAMS writes toe/camber values without HDF5 unit metadata. Their
        // configured mapping still declares the canonical degree convention.
        let expected_unit = (suffix == "wheel_load").then_some(convention.source_unit.as_str());
        let values = apply_convention(
            &simulation
                .read_signal(&reference.entity, &reference.component, expected_unit)?
                .values,
            &name,
            format!("{}/{}", reference.entity, reference.component),
            convention,
            config.gravity,
        )?
        .values;
        curves.push(Curve {
            corner: corner.into(),
            signal: suffix.into(),
            values,
            unit,
        });
    }
    Ok(())
}

fn optional_measurement_curves(
    measurement: &MeasurementFile,
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
            .channels()
            .iter()
            .any(|info| info.group_name == channel)
        {
            continue;
        }
        let channel = measurement.read_channel(&channel, Some(unit))?;
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
            include_str!("../../../../py-chassislab/src/chassis_lab/vertical_kernel.py"),
            "\0"
        )
        .as_bytes(),
    )?;
    let reports = Python::attach(|py| -> PyResult<String> {
        add_venv_site_packages(py)?;
        let module = PyModule::from_code(
            py,
            source,
            c"vertical_kernel.py",
            c"chassis_lab.vertical_kernel",
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
