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
    config::{CaseConfig, SignalRef, VerticalBranch, VerticalConfig},
    conventions::{
        BaselinePolicy, ROLL_AXLE_SIGNALS, SignalConvention, apply_convention, roll_conventions,
        vertical_conventions,
    },
};
use pyo3::{
    prelude::*,
    types::{PyBytes, PyDict, PyModule},
};
use serde::{Deserialize, Serialize};

use super::display::{mapping_missing, missing_unit_warning, tag_warnings};

const CORNERS: [&str; 4] = ["front_left", "front_right", "rear_left", "rear_right"];
const AXES: [&str; 3] = ["x", "y", "z"];
const DEFAULT_GRADIENT_POINTS_MM: f64 = 10.0;

/// KnC test analyzed through the shared four-corner pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KncTest {
    Vertical,
    Roll,
}

impl KncTest {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "vertical" => Some(Self::Vertical),
            "roll" => Some(Self::Roll),
            _ => None,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Vertical => "vertical",
            Self::Roll => "roll",
        }
    }

    /// Console section heading.
    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Vertical => "KnC Vertical",
            Self::Roll => "KnC Roll",
        }
    }

    pub(crate) fn config(self, config: &CaseConfig) -> Option<&VerticalConfig> {
        match self {
            Self::Vertical => config.vertical.as_ref(),
            Self::Roll => config.roll.as_ref(),
        }
    }

    fn required_config(self, config: &CaseConfig) -> Result<&VerticalConfig> {
        self.config(config)
            .ok_or_else(|| anyhow::anyhow!("KnC {} configuration is required", self.name()))
    }

    fn conventions(self) -> BTreeMap<String, SignalConvention> {
        match self {
            Self::Vertical => vertical_conventions(),
            Self::Roll => roll_conventions(),
        }
    }

    /// Optional per-corner signals beyond wheel-center x/y/z.
    fn corner_signals(self) -> &'static [&'static str] {
        match self {
            Self::Vertical => &[
                "wheel_load",
                "toe",
                "camber",
                "spin",
                "wheel_pad_z",
                "tire_deflection",
                "contact_patch_x",
                "contact_patch_y",
            ],
            Self::Roll => &[
                "wheel_load",
                "toe",
                "camber",
                "spin",
                "wheel_pad_z",
                "tire_deflection",
                "contact_patch_x",
                "contact_patch_y",
                "center_of_pressure_y",
            ],
        }
    }

    /// Axle-level signals as `(location, signal, required)`.
    fn axle_signals(self) -> Vec<(&'static str, &'static str, bool)> {
        match self {
            Self::Vertical => Vec::new(),
            Self::Roll => ROLL_AXLE_SIGNALS
                .iter()
                .map(|(location, signal, _)| (*location, *signal, *signal == "roll_angle"))
                .collect(),
        }
    }

    /// Roll metrics and plots arrive with #142-#144; until then roll reports
    /// curves only.
    pub(crate) const fn has_metrics(self) -> bool {
        matches!(self, Self::Vertical)
    }
}

/// One sampled signal. `corner` is a corner (`front_left`, ...) or, for roll
/// axle signals, an axle location (`front`, `rear`, `total`). `offset` is the
/// baseline subtracted from relative values (measurement channel offset or
/// static simulation value), so absolute positions stay recoverable.
#[derive(Debug, Deserialize, Serialize)]
struct Curve {
    corner: String,
    signal: String,
    values: Vec<f64>,
    unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    travel: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offset: Option<f64>,
}

pub(crate) struct Reports {
    pub detail: String,
    pub curve_count: usize,
    pub metrics: Vec<serde_json::Value>,
    pub warnings: Vec<String>,
    pub normalization: serde_json::Value,
    pub branch: VerticalBranch,
    pub measurement: bool,
    /// Per-corner mass backing `ride_frequency`: config override, else this
    /// run's SVC `<corner>_mass_kg`, else absent. Computed once here and
    /// reused by both the metric calculation and the `ride_frequency` plot
    /// (`curve_plots`/`compare_plots`), so callers never recompute it.
    pub corner_mass_kg: BTreeMap<String, f64>,
}

/// Calculate KnC curves, metrics, and warnings for one test. Does not render
/// plots: call `curve_plots` separately once the caller is ready for them
/// (e.g. after printing the console metrics table), so slow PDF rendering
/// never delays display.
pub(crate) fn analyze_input(
    config: &CaseConfig,
    test: KncTest,
    path: &Path,
    svc_corner_mass_kg: Option<&BTreeMap<String, f64>>,
    debug: Option<&str>,
) -> Result<Reports> {
    let file = CanonicalFile::open(path)
        .with_context(|| format!("failed to read HDF5 '{}'", path.display()))?;
    let (curves, mut warnings, normalization) = match file.summary().source_kind {
        SourceKind::Simulation => {
            let (mut curves, mut warnings) = simulation_curves(config, test, &file)?;
            if let Some(static_path) = test
                .config(config)
                .and_then(|v| v.static_simulation_hdf5.as_ref())
            {
                warnings.extend(subtract_static(
                    config,
                    test,
                    &mut curves,
                    &CanonicalFile::open(static_path)?,
                )?);
                (
                    curves,
                    warnings,
                    serde_json::json!({"static_simulation": static_path, "method": "drop_initial_sample_and_subtract_final_sample"}),
                )
            } else {
                (
                    curves,
                    {
                        warnings.push(
                            "simulation has no static_simulation; curves are not normalized".into(),
                        );
                        warnings
                    },
                    serde_json::json!({"static_simulation": null, "method": "none"}),
                )
            }
        }
        SourceKind::Measurement => {
            let knc = test.required_config(config)?;
            (
                measurement_curves(test, knc, &file)?,
                Vec::new(),
                serde_json::json!({"method": "measurement_native"}),
            )
        }
    };
    let measurement = file.summary().source_kind == SourceKind::Measurement;
    if debug.is_some() && !measurement {
        warnings
            .push("debug rate plots require measurement input; skipped for simulation data".into());
    }
    let relative_travel = normalization["method"] != "none";
    let (corner_mass_kg, corner_mass_source) = test
        .config(config)
        .map(|knc| corner_mass(&knc.window, svc_corner_mass_kg))
        .unwrap_or_default();
    let metrics = if test.has_metrics() {
        warnings.extend(default_warnings(config, measurement));
        gradient_metrics(
            config,
            &curves,
            relative_travel,
            measurement,
            &corner_mass_kg,
            &corner_mass_source,
            &mut warnings,
        )?
    } else {
        Vec::new()
    };
    warnings.sort();
    warnings.dedup();
    let warnings = tag_warnings("knc", Some(test.name()), warnings);
    let detail = serde_json::json!({
        "schema_version": 1,
        "command": "analyze",
        "domain": "knc",
        "test": test.name(),
        "metrics": metrics,
        "curves": curves,
        "warnings": warnings,
        "normalization": normalization,
    });
    let curve_count = detail["curves"].as_array().unwrap().len();
    Ok(Reports {
        detail: serde_json::to_string_pretty(&detail)? + "\n",
        curve_count,
        metrics: detail["metrics"].as_array().unwrap().clone(),
        warnings: detail["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        normalization: detail["normalization"].clone(),
        branch: test
            .required_config(config)?
            .branch
            .for_measurement(measurement)
            .clone(),
        measurement,
        corner_mass_kg,
    })
}

/// Display source label for plot titles: the counterpart to `Reports::measurement`.
pub(crate) fn source_label(measurement: bool) -> &'static str {
    if measurement {
        "Measurement"
    } else {
        "Simulation"
    }
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
            "config key '{key}' missing in [{table}]; using default {default}"
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
    corner_mass_kg: &BTreeMap<String, f64>,
    corner_mass_source: &BTreeMap<String, &'static str>,
    warnings: &mut Vec<String>,
) -> Result<Vec<serde_json::Value>> {
    let Some(window) = config.vertical.as_ref().map(|vertical| &vertical.window) else {
        return Ok(Vec::new());
    };
    let gradient = effective_gradient_points(window);
    if gradient <= 0.0 {
        warnings.push(format!(
            "travel window {}..{} mm does not contain 0 mm; metrics skipped",
            window.minimum_mm, window.maximum_mm
        ));
        return Ok(Vec::new());
    }
    if !relative_travel {
        warnings.push("metrics require static_simulation normalization; metrics skipped".into());
        return Ok(Vec::new());
    }
    let corner_mass_kg = serde_json::to_string(corner_mass_kg)?;
    let corner_mass_source = serde_json::to_string(corner_mass_source)?;
    let curves = serde_json::to_string(curves)?;
    let branch = serde_json::to_string(branch_options(config, measurement))?;
    let response: String = with_kernel(|py, module| {
        let request = PyDict::new(py);
        request.set_item("gradient_mm", gradient)?;
        request.set_item("curves", &curves)?;
        request.set_item("branch", &branch)?;
        request.set_item("corner_mass_kg", &corner_mass_kg)?;
        request.set_item("corner_mass_source", &corner_mass_source)?;
        module.getattr("metrics")?.call1((request,))?.extract()
    })
    .context("failed to calculate vertical metrics with embedded Python")?;
    let response: serde_json::Value = serde_json::from_str(&response)
        .context("embedded Python returned invalid vertical metrics")?;
    let metrics = response["metrics"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("embedded Python returned incomplete vertical metrics"))?;
    const KNOWN_FIELDS: [&str; 7] = [
        "corner", "metric", "unit", "value", "lower", "upper", "reason",
    ];
    Ok(metrics
        .iter()
        .map(|metric| {
            if let Some(reason) = metric["reason"].as_str() {
                warnings.push(format!(
                    "metric '{}' unavailable: {reason}",
                    metric["metric"].as_str().unwrap_or_default()
                ));
            }
            let mut row = serde_json::json!({
                "domain": "knc",
                "test": "vertical",
                "metric": metric["metric"],
                "unit": metric["unit"],
                "value": metric["value"],
                "interval_mm": [-gradient, gradient],
                "lower": metric["lower"],
                "upper": metric["upper"],
            });
            // Forward derived metrics' extra calc-input fields (e.g. ride_frequency's
            // suspension_rate_n_per_mm/corner_mass_kg/corner_mass_source) generically,
            // without the base row shape knowing every metric's own provenance fields.
            if let Some(object) = metric.as_object() {
                for (key, value) in object {
                    if !KNOWN_FIELDS.contains(&key.as_str()) {
                        row[key] = value.clone();
                    }
                }
            }
            row
        })
        .collect())
}

/// Per-corner mass for `ride_frequency`: a configured `sprung_mass_<corner>_kg`
/// overrides the same run's SVC `<corner>_mass_kg`; otherwise SVC fills the gap.
/// Missing both leaves that corner absent from the maps.
fn corner_mass(
    window: &clab_core::config::VerticalWindow,
    svc_corner_mass_kg: Option<&BTreeMap<String, f64>>,
) -> (BTreeMap<String, f64>, BTreeMap<String, &'static str>) {
    let mut mass = BTreeMap::new();
    let mut source = BTreeMap::new();
    for corner in CORNERS {
        if let Some(value) = window.sprung_mass_kg.get(corner) {
            mass.insert(corner.to_string(), *value);
            source.insert(corner.to_string(), "config");
        } else if let Some(value) = svc_corner_mass_kg.and_then(|map| map.get(corner)) {
            mass.insert(corner.to_string(), *value);
            source.insert(corner.to_string(), "svc");
        }
    }
    (mass, source)
}

fn effective_gradient_points(window: &clab_core::config::VerticalWindow) -> f64 {
    window.gradient_points_mm.unwrap_or_else(|| {
        DEFAULT_GRADIENT_POINTS_MM
            .min(-window.minimum_mm)
            .min(window.maximum_mm)
    })
}

/// Render same canonical curves together. Rust owns input selection; Python only draws arrays.
///
/// `corner_mass_kg` carries one map per series, baseline first then each
/// candidate in order (same ordering as `branch`), each series' own mass
/// since baseline and candidate can each resolve mass from their own SVC
/// run. It renders the `ride_frequency` curve plot for series that have it,
/// same as `curve_plots` does for a single-source `analyze` report.
pub(crate) fn compare_plots(
    config: &CaseConfig,
    (baseline, baseline_branch, baseline_mass): (
        &serde_json::Value,
        &VerticalBranch,
        &BTreeMap<String, f64>,
    ),
    candidates: &[(
        String,
        serde_json::Value,
        VerticalBranch,
        BTreeMap<String, f64>,
    )],
    baseline_label: &str,
    footer: &str,
    prefix: &str,
    debug: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let baseline_json = serde_json::to_string(baseline)?;
    let candidates_json = serde_json::to_string(
        &candidates
            .iter()
            .map(|(label, curves, ..)| serde_json::json!({"label": label, "curves": curves}))
            .collect::<Vec<_>>(),
    )?;
    let branch = serde_json::to_string(
        &std::iter::once(baseline_branch)
            .chain(candidates.iter().map(|(_, _, branch, _)| branch))
            .collect::<Vec<_>>(),
    )?;
    let corner_mass_kg = serde_json::to_string(
        &std::iter::once(baseline_mass)
            .chain(candidates.iter().map(|(_, _, _, mass)| mass))
            .collect::<Vec<_>>(),
    )?;
    let reports: BTreeMap<String, Vec<u8>> = with_kernel(|py, module| {
        module
            .getattr("plot_compare")?
            .call1((
                &baseline_json,
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
                &corner_mass_kg,
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
    test: KncTest,
    curves: &mut [Curve],
    static_file: &CanonicalFile,
) -> Result<Vec<String>> {
    let knc = test.required_config(config)?;
    let conventions = test.conventions();
    let mut warnings = Vec::new();
    for curve in curves {
        let name = format!("{}_{}", curve.corner, curve.signal);
        let mut convention = convention(&conventions, &name)?;
        if convention.baseline != BaselinePolicy::StaticSimulation {
            continue;
        }
        let reference = mapping(test, knc, &name)?;
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
        curve.offset = Some(static_value);
    }
    Ok(warnings)
}

fn simulation_curves(
    config: &CaseConfig,
    test: KncTest,
    simulation: &CanonicalFile,
) -> Result<(Vec<Curve>, Vec<String>)> {
    let knc = test.required_config(config)?;
    let conventions = test.conventions();
    let mut curves = Vec::new();
    let mut warnings = Vec::new();
    for corner in CORNERS {
        let travel_name = format!("{corner}_travel");
        let travel_reference = mapping(test, knc, &travel_name)?;
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
            let reference = mapping(test, knc, &name)?;
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
                offset: None,
            });
        }
        optional_simulation_curves(
            config,
            simulation,
            (test, knc),
            &conventions,
            corner,
            &mut curves,
            &mut warnings,
        )?;
    }
    for (location, signal, required) in test.axle_signals() {
        let name = format!("{location}_{signal}");
        let Some(reference) = knc.signals.get(&name) else {
            if required {
                anyhow::bail!(mapping_missing(test.name(), &name));
            }
            continue;
        };
        let (values, unit) = simulation_signal(
            config,
            simulation,
            &conventions,
            &name,
            reference,
            &mut warnings,
        )?;
        curves.push(Curve {
            corner: location.into(),
            signal: signal.into(),
            values: drop_initial_sample(values),
            unit,
            travel: None,
            offset: None,
        });
    }
    Ok((curves, warnings))
}

fn measurement_curves(
    test: KncTest,
    knc: &VerticalConfig,
    measurement: &CanonicalFile,
) -> Result<Vec<Curve>> {
    let mut curves = Vec::new();
    for corner in CORNERS {
        for axis in AXES {
            let name = format!("{corner}_wheel_center_{axis}");
            let reference = measurement_mapping(test, knc, &name)?;
            let channel =
                measurement.read_signal(&reference.entity, None, Some(&reference.unit))?;
            let offset = channel.info.offset.unwrap_or_default();
            curves.push(Curve {
                corner: corner.into(),
                signal: format!("wheel_center_{axis}"),
                values: channel
                    .values
                    .into_iter()
                    .map(|value| value - offset)
                    .collect(),
                unit: reference.unit.clone(),
                travel: None,
                offset: Some(offset),
            });
        }
        optional_measurement_curves(test, knc, measurement, corner, &mut curves)?;
    }
    for (location, signal, required) in test.axle_signals() {
        let name = format!("{location}_{signal}");
        let channel = knc
            .measurement_signals
            .get(&name)
            .map(|reference| measurement_channel(measurement, reference))
            .transpose()?
            .flatten();
        let Some((values, unit)) = channel else {
            if required {
                anyhow::bail!(mapping_missing(
                    &format!("{} measurement", test.name()),
                    &name
                ));
            }
            continue;
        };
        curves.push(Curve {
            corner: location.into(),
            signal: signal.into(),
            values,
            unit,
            travel: None,
            offset: None,
        });
    }
    Ok(curves)
}

/// Raw values and mapped unit of a measurement channel, `None` when the file
/// lacks it.
fn measurement_channel(
    measurement: &CanonicalFile,
    reference: &clab_core::config::MeasurementSignalRef,
) -> Result<Option<(Vec<f64>, String)>> {
    if !measurement
        .signals()
        .iter()
        .any(|info| info.safe_name == reference.entity)
    {
        return Ok(None);
    }
    let channel = measurement.read_signal(&reference.entity, None, Some(&reference.unit))?;
    Ok(Some((channel.values, reference.unit.clone())))
}

fn optional_simulation_curves(
    config: &CaseConfig,
    simulation: &CanonicalFile,
    (test, knc): (KncTest, &VerticalConfig),
    conventions: &BTreeMap<String, SignalConvention>,
    corner: &str,
    curves: &mut Vec<Curve>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    for suffix in test.corner_signals() {
        // Simulation pad travel is derived in the kernel from wheel-center
        // travel and tire deflection.
        if *suffix == "wheel_pad_z" {
            continue;
        }
        let name = format!("{corner}_{suffix}");
        let Some(reference) = knc.signals.get(&name) else {
            continue;
        };
        let (values, unit) =
            simulation_signal(config, simulation, conventions, &name, reference, warnings)?;
        curves.push(Curve {
            corner: corner.into(),
            signal: (*suffix).into(),
            values: drop_initial_sample(values),
            unit,
            travel: None,
            offset: None,
        });
    }
    Ok(())
}

/// Read one mapped simulation signal and apply its canonical convention.
fn simulation_signal(
    config: &CaseConfig,
    simulation: &CanonicalFile,
    conventions: &BTreeMap<String, SignalConvention>,
    name: &str,
    reference: &SignalRef,
    warnings: &mut Vec<String>,
) -> Result<(Vec<f64>, String)> {
    let mut convention = convention(conventions, name)?;
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
        name,
        format!("{}/{}", reference.entity, reference.component),
        convention,
        config.gravity,
    )?
    .values;
    Ok((values, unit))
}

/// Adams legacy evaluation removes column one from every multi-sample signal.
fn drop_initial_sample(values: Vec<f64>) -> Vec<f64> {
    if values.len() > 1 {
        values.into_iter().skip(1).collect()
    } else {
        values
    }
}

/// Optional per-corner measurement curves: a canonical name absent from
/// `measurement_signals`, or present but not found in the file, just skips
/// that curve (same leniency `optional_simulation_curves` gives a canonical
/// name absent from `vertical.signals`).
fn optional_measurement_curves(
    test: KncTest,
    knc: &VerticalConfig,
    measurement: &CanonicalFile,
    corner: &str,
    curves: &mut Vec<Curve>,
) -> Result<()> {
    for suffix in test.corner_signals() {
        let suffix = *suffix;
        let name = format!("{corner}_{suffix}");
        let Some(reference) = knc.measurement_signals.get(&name) else {
            continue;
        };
        if !measurement
            .signals()
            .iter()
            .any(|info| info.safe_name == reference.entity)
        {
            continue;
        }
        let channel = measurement.read_signal(&reference.entity, None, Some(&reference.unit))?;
        let relative = matches!(
            suffix,
            "wheel_pad_z"
                | "tire_deflection"
                | "contact_patch_x"
                | "contact_patch_y"
                | "center_of_pressure_y"
        );
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
            unit: reference.unit.clone(),
            travel: None,
            offset: relative.then_some(offset),
        });
    }
    Ok(())
}

fn mapping<'a>(test: KncTest, knc: &'a VerticalConfig, name: &str) -> Result<&'a SignalRef> {
    knc.signals
        .get(name)
        .ok_or_else(|| anyhow::anyhow!(mapping_missing(test.name(), name)))
}

fn measurement_mapping<'a>(
    test: KncTest,
    knc: &'a VerticalConfig,
    name: &str,
) -> Result<&'a clab_core::config::MeasurementSignalRef> {
    knc.measurement_signals.get(name).ok_or_else(|| {
        anyhow::anyhow!(mapping_missing(
            &format!("{} measurement", test.name()),
            name
        ))
    })
}

fn convention(
    conventions: &BTreeMap<String, SignalConvention>,
    name: &str,
) -> Result<SignalConvention> {
    conventions
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("KnC signal convention missing: {name}"))
}

/// Render KnC vertical PDF plots. Separate from `analyze_input` so slow
/// rendering happens only after the caller has already displayed metrics.
///
/// `corner_mass_kg` renders the `ride_frequency` curve plot (same per-corner
/// mass the metric uses: config override, else this run's SVC
/// `<corner>_mass_kg` -- see `corner_mass`). Empty when neither is available;
/// the plot is then skipped like any other optional signal.
pub(crate) fn curve_plots(
    config: &CaseConfig,
    curves: &serde_json::Value,
    footer: &str,
    measurement: bool,
    corner_mass_kg: &BTreeMap<String, f64>,
    debug: Option<&str>,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let source_label = source_label(measurement);
    let curves = serde_json::to_string(curves)?;
    let gradient = config
        .vertical
        .as_ref()
        .map(|vertical| effective_gradient_points(&vertical.window));
    let branch = serde_json::to_string(branch_options(config, measurement))?;
    let corner_mass_kg = serde_json::to_string(corner_mass_kg)?;
    with_kernel(|py, module| {
        module
            .getattr("plot")?
            .call1((
                curves,
                source_label,
                footer,
                logo(py),
                gradient,
                &branch,
                debug,
                &corner_mass_kg,
            ))?
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
