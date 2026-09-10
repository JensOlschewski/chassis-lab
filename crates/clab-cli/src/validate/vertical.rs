use std::{
    collections::BTreeMap,
    env,
    ffi::{CStr, CString},
    fs,
    mem::MaybeUninit,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result, bail};
use clab_core::{
    config::{CaseConfig, SignalRef},
    conventions::{apply_convention, vertical_conventions},
    measurement_hdf5::MeasurementFile,
    simulation_hdf5::SimulationFile,
};
use pyo3::{
    ffi,
    prelude::*,
    types::{PyDict, PyModule},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
const AXES: [&str; 3] = ["x", "y", "z"];

#[derive(Debug, Serialize)]
struct KernelRequest {
    grid: Vec<f64>,
    curves: BTreeMap<String, CurveInput>,
}
#[derive(Debug, Serialize)]
struct CurveInput {
    measurement_travel: Vec<f64>,
    measurement_cycle: Vec<f64>,
    measurement: Vec<f64>,
    simulation_travel: Vec<f64>,
    simulation: Vec<f64>,
}
#[derive(Debug, Deserialize, Serialize)]
struct Comparison {
    corner: String,
    axis: String,
    travel_mm: Vec<f64>,
    measurement_mm: Vec<f64>,
    simulation_mm: Vec<f64>,
    measurement_baseline_mm: f64,
    simulation_baseline_mm: f64,
    baseline_error_mm: f64,
    rmse_mm: f64,
    maximum_error_mm: f64,
}
#[derive(Debug, Deserialize)]
struct KernelResponse {
    comparisons: Vec<Comparison>,
    svg: String,
}

pub(crate) struct Reports {
    pub json: String,
    pub csv: String,
    pub svg: String,
}

pub(crate) fn validate(config: &CaseConfig) -> Result<Reports> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("case '{}' has no [vertical] configuration", config.name))?;
    let measurement = MeasurementFile::open(&vertical.measurement_hdf5).with_context(|| {
        format!(
            "failed to read measurement HDF5 '{}'",
            vertical.measurement_hdf5.display()
        )
    })?;
    let simulation = SimulationFile::open(&vertical.simulation_hdf5).with_context(|| {
        format!(
            "failed to read simulation HDF5 '{}'",
            vertical.simulation_hdf5.display()
        )
    })?;
    validate_setup(&measurement, config)?;
    let request = request(config, &measurement, &simulation)?;
    let response = kernel(&request)?;
    validate_response(&response, &request.grid)?;
    let report = report(config, &response.comparisons)?;
    println!("Vertical trajectory comparison");
    println!(
        "case: {} ({}, {})",
        config.name, config.loadcase, vertical.setup.code
    );
    for comparison in &response.comparisons {
        println!(
            "{} {}: measurement baseline {:.8} mm; simulation baseline {:.8} mm; RMSE {:.8} mm; max {:.8} mm",
            comparison.corner,
            comparison.axis,
            comparison.measurement_baseline_mm,
            comparison.simulation_baseline_mm,
            comparison.rmse_mm,
            comparison.maximum_error_mm
        );
    }
    println!("validation result: informational (no vertical tolerances configured)");
    Ok(Reports {
        json: serde_json::to_string_pretty(&report)? + "\n",
        csv: csv_text(config, &response.comparisons)?,
        svg: response.svg,
    })
}

pub(crate) fn preflight(config: &CaseConfig) -> Result<()> {
    let vertical = config
        .vertical
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("case '{}' has no [vertical] configuration", config.name))?;
    let measurement = MeasurementFile::open(&vertical.measurement_hdf5).with_context(|| {
        format!(
            "failed to read measurement HDF5 '{}'",
            vertical.measurement_hdf5.display()
        )
    })?;
    let simulation = SimulationFile::open(&vertical.simulation_hdf5).with_context(|| {
        format!(
            "failed to read simulation HDF5 '{}'",
            vertical.simulation_hdf5.display()
        )
    })?;
    validate_setup(&measurement, config)?;
    request(config, &measurement, &simulation)?;
    Ok(())
}

fn request(
    config: &CaseConfig,
    measurement: &MeasurementFile,
    simulation: &SimulationFile,
) -> Result<KernelRequest> {
    let vertical = config.vertical.as_ref().unwrap();
    let mut grid = Vec::new();
    let mut value = vertical.window.minimum_mm;
    while value <= vertical.window.maximum_mm {
        grid.push(value);
        value += vertical.window.step_mm;
    }
    if *grid.last().unwrap() < vertical.window.maximum_mm {
        grid.push(vertical.window.maximum_mm);
    }
    if !grid.iter().any(|v| v.abs() < 1e-9) {
        bail!("vertical validation window must include zero");
    }
    let conventions = vertical_conventions();
    let mut curves = BTreeMap::new();
    for (corner, prefix) in CORNERS.into_iter().zip(["lf", "rf", "lr", "rr"]) {
        let cycle = measurement
            .read_channel(&format!("{prefix}slope"), None)?
            .values;
        let travel_channel = measurement.read_channel(&format!("{prefix}wcz"), Some("mm"))?;
        let measurement_travel: Vec<_> = travel_channel
            .values
            .iter()
            .map(|value| value - travel_channel.info.offset)
            .collect();
        let travel_name = format!("{corner}_travel");
        let travel_ref = mapping(vertical, &travel_name)?;
        let travel_convention = convention(&conventions, &travel_name)?;
        validate_mapping_unit(travel_ref, &travel_name, &travel_convention.source_unit)?;
        let travel_signal = simulation.read_signal(
            &travel_ref.entity,
            &travel_ref.component,
            Some(&travel_convention.source_unit),
        )?;
        let mut simulation_travel = apply_convention(
            &travel_signal.values,
            &travel_name,
            format!("{}/{}", travel_ref.entity, travel_ref.component),
            travel_convention,
            config.gravity,
        )?
        .values;
        let initial = *simulation_travel
            .first()
            .ok_or_else(|| anyhow::anyhow!("simulation {corner} travel has no samples"))?;
        if !initial.is_finite() {
            bail!("simulation {corner} travel has invalid static sample");
        }
        for value in &mut simulation_travel {
            *value -= initial;
        }
        for axis in AXES {
            let measured = measurement.read_channel(&format!("{prefix}wc{axis}"), Some("mm"))?;
            if cycle.len() != measurement_travel.len() || cycle.len() != measured.values.len() {
                bail!("measurement {corner} {axis} channel lengths differ");
            }
            let coordinate_name = format!("{corner}_wheel_center_{axis}");
            let coordinate_ref = mapping(vertical, &coordinate_name)?;
            let coordinate_convention = convention(&conventions, &coordinate_name)?;
            validate_mapping_unit(
                coordinate_ref,
                &coordinate_name,
                &coordinate_convention.source_unit,
            )?;
            let coordinate_signal = simulation.read_signal(
                &coordinate_ref.entity,
                &coordinate_ref.component,
                Some(&coordinate_convention.source_unit),
            )?;
            let simulated = apply_convention(
                &coordinate_signal.values,
                &coordinate_name,
                format!("{}/{}", coordinate_ref.entity, coordinate_ref.component),
                coordinate_convention,
                config.gravity,
            )?
            .values;
            if simulation_travel.len() != simulated.len() {
                bail!("simulation {corner} signal lengths differ");
            }
            curves.insert(
                format!("{corner}:{axis}"),
                CurveInput {
                    measurement_travel: measurement_travel.clone(),
                    measurement_cycle: cycle.clone(),
                    measurement: measured.values,
                    simulation_travel: simulation_travel.clone(),
                    simulation: simulated,
                },
            );
        }
    }
    Ok(KernelRequest { grid, curves })
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

fn validate_mapping_unit(ref_: &SignalRef, name: &str, expected: &str) -> Result<()> {
    if ref_.unit != expected {
        bail!("vertical signal '{name}' must use configured unit '{expected}'");
    }
    Ok(())
}

fn validate_setup(measurement: &MeasurementFile, config: &CaseConfig) -> Result<()> {
    let setup = &config.vertical.as_ref().unwrap().setup;
    if measurement.named_test_info("testtype")?.value != "vert" {
        bail!("measurement testtype must be 'vert'");
    }
    for (name, expected) in [
        ("brakeson", Some(setup.brakes_applied)),
        ("engineon", Some(setup.engine_running)),
        ("farbon", Some(setup.front_arb_connected)),
        ("rarbon", setup.rear_arb_connected),
    ] {
        if expected.is_none() {
            continue;
        }
        let value = &measurement.named_test_info(name)?.value;
        let actual = match value.as_str() {
            "1" => Some(true),
            "0" => Some(false),
            "" | "NaN" | "nan" => None,
            _ => bail!("measurement setup metadata {name} has invalid value {value:?}"),
        };
        if expected != actual {
            bail!("measurement setup {name}={actual:?}; expected {expected:?}");
        }
    }
    Ok(())
}

fn kernel(request: &KernelRequest) -> Result<KernelResponse> {
    initialize_python()?;
    Python::attach(|py| -> Result<_> {
        let source = CString::new(include_str!(
            "../../../../py-chassislab/src/chassis_lab/vertical_kernel.py"
        ))?;
        let filename = CString::new("chassis_lab.vertical_kernel")?;
        let module_name = CString::new("chassis_lab.vertical_kernel")?;
        let module = PyModule::from_code(py, &source, &filename, &module_name)
            .context("failed to initialize embedded Python vertical kernel")?;
        let input = PyDict::new(py);
        input.set_item("grid", &request.grid)?;
        input.set_item("curves", serde_json::to_string(&request.curves)?)?;
        let value = module.getattr("evaluate")?.call1((input,))?;
        let text: String = value.extract()?;
        serde_json::from_str(&text).context("Python vertical kernel returned invalid JSON")
    })
}

fn initialize_python() -> Result<()> {
    static INITIALIZED: OnceLock<Result<(), String>> = OnceLock::new();
    match INITIALIZED
        .get_or_init(|| initialize_python_once().map_err(|error| format!("{error:#}")))
        .as_ref()
        .map_err(|error| anyhow::anyhow!("failed to initialize Python runtime: {error}"))
    {
        Ok(()) => Ok(()),
        Err(error) => Err(error),
    }
}

fn initialize_python_once() -> Result<()> {
    let executable = env::current_exe().context("failed to locate clab executable")?;
    let runtime = executable
        .parent()
        .and_then(Path::parent)
        .filter(|root| root.join("lib/python3.12").is_dir())
        .map(|root| {
            if cfg!(windows) {
                root.join("lib/python3.12")
            } else {
                PathBuf::from(root)
            }
        });
    let Some(runtime) = runtime else {
        Python::initialize();
        return Ok(());
    };
    let runtime = path_c_string(&runtime)?;
    let executable = path_c_string(&executable)?;
    let mut config = MaybeUninit::<ffi::PyConfig>::uninit();
    unsafe {
        ffi::PyConfig_InitIsolatedConfig(config.as_mut_ptr());
        let config = config.assume_init_mut();
        config.site_import = 1;
        config.write_bytecode = 0;
        let result = (|| {
            let pointer = config as *mut ffi::PyConfig;
            python_status(
                ffi::PyConfig_SetBytesString(pointer, &raw mut (*pointer).home, runtime.as_ptr()),
                "set Python home",
            )?;
            python_status(
                ffi::PyConfig_SetBytesString(
                    pointer,
                    &raw mut (*pointer).program_name,
                    executable.as_ptr(),
                ),
                "set Python program name",
            )?;
            python_status(
                ffi::PyConfig_SetBytesString(
                    pointer,
                    &raw mut (*pointer).executable,
                    executable.as_ptr(),
                ),
                "set Python executable",
            )?;
            python_status(
                ffi::Py_InitializeFromConfig(pointer),
                "initialize isolated Python",
            )
        })();
        ffi::PyConfig_Clear(config);
        result
    }
}

fn path_c_string(path: &Path) -> Result<CString> {
    CString::new(path.to_string_lossy().as_bytes())
        .with_context(|| format!("path contains a null byte: {}", path.display()))
}

fn python_status(status: ffi::PyStatus, operation: &str) -> Result<()> {
    if unsafe { ffi::PyStatus_Exception(status) } == 0 {
        return Ok(());
    }
    let detail = if status.err_msg.is_null() {
        "Python initialization failed".into()
    } else {
        unsafe { CStr::from_ptr(status.err_msg) }
            .to_string_lossy()
            .into_owned()
    };
    bail!("{operation}: {detail}")
}

fn validate_response(response: &KernelResponse, grid: &[f64]) -> Result<()> {
    if response.comparisons.len() != 12 || response.svg.is_empty() {
        bail!("Python vertical kernel returned incomplete response");
    }
    for (index, item) in response.comparisons.iter().enumerate() {
        let expected = format!("{}:{}", CORNERS[index / 3], AXES[index % 3]);
        if format!("{}:{}", item.corner, item.axis) != expected
            || item.travel_mm != grid
            || item.measurement_mm.len() != grid.len()
            || item.simulation_mm.len() != grid.len()
            || ![
                item.measurement_baseline_mm,
                item.simulation_baseline_mm,
                item.baseline_error_mm,
                item.rmse_mm,
                item.maximum_error_mm,
            ]
            .iter()
            .all(|v| v.is_finite())
        {
            bail!("Python vertical kernel returned invalid comparison {expected}");
        }
    }
    Ok(())
}

fn report(config: &CaseConfig, comparisons: &[Comparison]) -> Result<serde_json::Value> {
    let v = config.vertical.as_ref().unwrap();
    let mut inputs = serde_json::json!({
        "config": record(&config.path)?,
        "measurement_hdf5": record(&v.measurement_hdf5)?,
        "simulation_hdf5": record(&v.simulation_hdf5)?,
    });
    if let Some(path) = &v.signal_map {
        inputs["signal_map"] = record(path)?;
    }
    Ok(
        serde_json::json!({"schema_version": 1, "case": config.name, "loadcase": config.loadcase, "setup_code": v.setup.code, "metrics": comparisons, "inputs": inputs, "software": {"chassis_lab": env!("CARGO_PKG_VERSION")}}),
    )
}
fn record(path: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::json!({"path": path, "sha256": hash(path)?}))
}
fn hash(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}
fn csv_text(config: &CaseConfig, comparisons: &[Comparison]) -> Result<String> {
    let v = config.vertical.as_ref().unwrap();
    let mut writer = csv::WriterBuilder::new().from_writer(Vec::new());
    writer.write_record([
        "case",
        "loadcase",
        "setup_code",
        "corner",
        "axis",
        "measurement_baseline_mm",
        "simulation_baseline_mm",
        "baseline_error_mm",
        "rmse_mm",
        "maximum_error_mm",
        "config_sha256",
        "signal_map_sha256",
        "signal_map_path",
        "measurement_sha256",
        "simulation_sha256",
    ])?;
    let signal_map = v
        .signal_map
        .as_ref()
        .map(|path| -> Result<_> { Ok((path.display().to_string(), hash(path)?)) })
        .transpose()?;
    for c in comparisons {
        writer.write_record([
            safe_cell(&config.name),
            safe_cell(&config.loadcase),
            safe_cell(&v.setup.code),
            safe_cell(&c.corner),
            safe_cell(&c.axis),
            c.measurement_baseline_mm.to_string(),
            c.simulation_baseline_mm.to_string(),
            c.baseline_error_mm.to_string(),
            c.rmse_mm.to_string(),
            c.maximum_error_mm.to_string(),
            hash(&config.path)?,
            signal_map
                .as_ref()
                .map(|value| value.1.clone())
                .unwrap_or_default(),
            signal_map
                .as_ref()
                .map(|value| safe_cell(&value.0))
                .unwrap_or_default(),
            hash(&v.measurement_hdf5)?,
            hash(&v.simulation_hdf5)?,
        ])?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?)
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

#[cfg(test)]
mod tests {
    use super::safe_cell;

    #[test]
    fn protects_formula_cells_after_leading_whitespace() {
        assert_eq!(safe_cell(" =SUM(A1:A2)"), "' =SUM(A1:A2)");
        assert_eq!(safe_cell("\t@cmd"), "'\t@cmd");
        assert_eq!(safe_cell("case"), "case");
    }
}
