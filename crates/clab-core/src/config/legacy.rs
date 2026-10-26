//! Older configuration schemas, and the `VerticalConfig` builder they share
//! with today's canonical schema.
//!
//! `parse_legacy` (oldest flat `[case]`/`[inputs]` schema) and `parse_unified`
//! (`[svc]`/`[vertical]` schema) are reachable only through those older root
//! shapes. `parse_vertical` builds a `VerticalConfig` from a normalized
//! `[vertical]`-shaped table; `function_first::function_knc_test` reuses it
//! by synthesizing an equivalent table rather than duplicating this logic.

use std::{collections::BTreeMap, path::Path};

use toml::{Value, map::Map};

use super::support::{
    aliased, boolean, boolean_value, finite, keys, parse_tolerance_table, parse_validation,
    positive, require, resolve, string, table, value,
};
use super::support::{parse_measurement_signals, parse_signals};
use super::{
    CaseConfig, ConfigError, MeasurementSignalRef, SignalRef, SignalSource, SourceKind,
    ValidationTolerances,
};

#[derive(Debug, Clone, PartialEq)]
pub struct VerticalSetup {
    pub brakes_applied: bool,
    pub engine_running: bool,
    pub front_arb_connected: bool,
    pub rear_arb_connected: Option<bool>,
}

impl VerticalSetup {
    /// One-line test setup description for report footers.
    pub fn summary(&self) -> String {
        let yes_no = |value: bool| if value { "yes" } else { "no" };
        let mut parts = vec![
            format!("brakes applied: {}", yes_no(self.brakes_applied)),
            format!("engine running: {}", yes_no(self.engine_running)),
            format!("front ARB connected: {}", yes_no(self.front_arb_connected)),
        ];
        if let Some(rear) = self.rear_arb_connected {
            parts.push(format!("rear ARB connected: {}", yes_no(rear)));
        }
        parts.join(" | ")
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalWindow {
    pub minimum_mm: f64,
    pub maximum_mm: f64,
    pub step_mm: f64,
    /// Symmetric wheel-center travel offset for secant gradient metrics;
    /// `None` when the configuration does not set it.
    pub gradient_points_mm: Option<f64>,
    /// Symmetric roll-angle offset for roll-angle secant metrics (roll only);
    /// `None` when the configuration does not set it.
    pub gradient_points_deg: Option<f64>,
    /// Per-corner fallback mass for `ride_frequency`, keyed by corner name
    /// (`"front_left"` etc.). Only used when SVC's `<corner>_mass_kg` is not
    /// available in the same run; a configured value always overrides it.
    pub sprung_mass_kg: BTreeMap<String, f64>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalConfig {
    pub measurement_hdf5: std::path::PathBuf,
    pub simulation_hdf5: std::path::PathBuf,
    pub source_kind: SourceKind,
    pub static_simulation_hdf5: Option<std::path::PathBuf>,
    pub setup: VerticalSetup,
    pub window: VerticalWindow,
    pub branch: VerticalBranches,
    pub signals: BTreeMap<String, SignalRef>,
    pub signal_map: Option<std::path::PathBuf>,
    /// Measurement channel mapping: canonical signal name ->
    /// `{entity (channel name), unit}`. Required whenever this vertical
    /// config is used to analyze a measurement HDF5 — there is no
    /// hardcoded-channel-name fallback.
    pub measurement_signals: BTreeMap<String, MeasurementSignalRef>,
    pub tolerances: ValidationTolerances,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct VerticalBranch {
    pub direction: String,
    pub index: usize,
    pub plot_branches: String,
    /// Source kind, set by the parser rather than exposed as a config key.
    pub measurement: bool,
    /// Keys the configuration left unset, so their defaults apply.
    #[serde(skip)]
    pub defaulted: Vec<&'static str>,
}

impl VerticalBranch {
    fn defaults(measurement: bool) -> Self {
        Self {
            direction: "rise".into(),
            index: 2,
            plot_branches: "all".into(),
            measurement,
            defaulted: vec!["direction", "index", "plot_branches"],
        }
    }

    /// Apply the branch keys present in `table`.
    fn apply(
        &mut self,
        table: &toml::map::Map<String, Value>,
        scope: &str,
    ) -> Result<(), ConfigError> {
        for (key, target, choices) in [
            (
                "direction",
                &mut self.direction,
                &["rise", "fall", "any"][..],
            ),
            (
                "plot_branches",
                &mut self.plot_branches,
                &["all", "selected"][..],
            ),
        ] {
            if let Some(raw) = table.get(key) {
                let text = raw
                    .as_str()
                    .ok_or_else(|| ConfigError(format!("{scope}.{key} must be a string")))?;
                if !choices.contains(&text) {
                    return Err(ConfigError(format!(
                        "{scope}.{key} must be one of {}",
                        choices.join(", ")
                    )));
                }
                *target = text.to_owned();
                self.defaulted.retain(|name| *name != key);
            }
        }
        let (key, target) = ("index", &mut self.index);
        if let Some(raw) = table.get(key) {
            *target = raw
                .as_integer()
                .and_then(|count| usize::try_from(count).ok())
                .filter(|count| *count > 0)
                .ok_or_else(|| ConfigError(format!("{scope}.{key} must be a positive integer")))?;
            self.defaulted.retain(|name| *name != key);
        }
        Ok(())
    }
}

/// Branch settings; measurement and simulation sweeps differ in sampling
/// density, so each source kind resolves its own options.
///
/// `[metric]` chooses the sweep the metrics use (`direction`, `index`);
/// `[window]` chooses which sweeps the plots draw (`plot_branches`). Either
/// table may override per source kind in `[metric.measurement]`,
/// `[metric.simulation]`, `[window.measurement]` or `[window.simulation]`.
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalBranches {
    pub measurement: VerticalBranch,
    pub simulation: VerticalBranch,
}

impl VerticalBranches {
    pub fn for_measurement(&self, measurement: bool) -> &VerticalBranch {
        if measurement {
            &self.measurement
        } else {
            &self.simulation
        }
    }

    pub(crate) fn parse(
        metric: &toml::map::Map<String, Value>,
        window: &toml::map::Map<String, Value>,
    ) -> Result<Self, ConfigError> {
        let mut branches = Self {
            measurement: VerticalBranch::defaults(true),
            simulation: VerticalBranch::defaults(false),
        };
        for (name, target) in [
            ("measurement", &mut branches.measurement),
            ("simulation", &mut branches.simulation),
        ] {
            for (scope, source, allowed) in [
                ("[vertical.metric]", metric, &["direction", "index"][..]),
                ("[vertical.window]", window, &["plot_branches"][..]),
            ] {
                target.apply(source, scope)?;
                let Some(sub) = source.get(name) else {
                    continue;
                };
                let scope = format!("{scope}.{name}");
                let sub = sub
                    .as_table()
                    .ok_or_else(|| ConfigError(format!("{scope} must be a table")))?;
                if let Some(key) = sub.keys().find(|key| !allowed.contains(&key.as_str())) {
                    return Err(ConfigError(format!("unknown key '{key}' in {scope}")));
                }
                target.apply(sub, &scope)?;
            }
        }
        Ok(branches)
    }
}

/// Fold the deprecated `[vertical.branch]` table into `metric` and `window`.
///
/// `direction` and `index` belong to `[metric]`, `plot_branches` to `[window]`;
/// keys already set there win.
fn fold_legacy_branch(
    branch: &Value,
    metric: &mut Map<String, Value>,
    window: &mut Map<String, Value>,
) -> Result<(), ConfigError> {
    let branch = branch
        .as_table()
        .ok_or_else(|| ConfigError("[vertical.branch] must be a table".into()))?;
    let fold = |scope: &str,
                source: &Map<String, Value>,
                metric: &mut Map<String, Value>,
                window: &mut Map<String, Value>|
     -> Result<(), ConfigError> {
        for (key, value) in source {
            let target = match key.as_str() {
                "direction" | "index" => &mut *metric,
                "plot_branches" => &mut *window,
                _ => return Err(ConfigError(format!("unknown key '{key}' in {scope}"))),
            };
            target.entry(key.clone()).or_insert_with(|| value.clone());
        }
        Ok(())
    };
    let top: Map<String, Value> = branch
        .iter()
        .filter(|(key, _)| !matches!(key.as_str(), "measurement" | "simulation"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    fold("[vertical.branch]", &top, metric, window)?;
    for name in ["measurement", "simulation"] {
        let Some(source) = branch.get(name) else {
            continue;
        };
        let scope = format!("[vertical.branch.{name}]");
        let source = source
            .as_table()
            .ok_or_else(|| ConfigError(format!("{scope} must be a table")))?;
        let mut sub_metric = Map::new();
        let mut sub_window = Map::new();
        fold(&scope, source, &mut sub_metric, &mut sub_window)?;
        for (target, sub) in [(&mut *metric, sub_metric), (&mut *window, sub_window)] {
            if sub.is_empty() {
                continue;
            }
            let entry = target
                .entry(name.to_string())
                .or_insert_with(|| Value::Table(Map::new()));
            if let Value::Table(existing) = entry {
                for (key, value) in sub {
                    existing.entry(key).or_insert(value);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_legacy(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    keys(
        root,
        &[
            "case",
            "inputs",
            "physics",
            "signal_map",
            "signals",
            "signal_mappings",
            "validation",
            "validation",
        ],
        "top level",
    )?;
    let case = table(
        root.get("case")
            .ok_or_else(|| ConfigError("missing required [case] table".into()))?,
        "[case]",
    )?;
    let inputs = table(
        root.get("inputs")
            .ok_or_else(|| ConfigError("missing required [inputs] table".into()))?,
        "[inputs]",
    )?;
    keys(case, &["name", "domain", "loadcase"], "[case]")?;
    for k in ["name", "domain", "loadcase"] {
        require(case, k, "[case]")?
    }
    keys(
        inputs,
        &["simulation_json", "signal_map", "signal_mappings"],
        "[inputs]",
    )?;
    require(inputs, "simulation_json", "[inputs]")?;
    let physics = table(
        root.get("physics")
            .ok_or_else(|| ConfigError("missing required [physics] table".into()))?,
        "[physics]",
    )?;
    keys(physics, &["gravity"], "[physics]")?;
    require(physics, "gravity", "[physics]")?;
    let signal_map = signal_source(root, path, "top level", None)?;
    let validation = parse_validation(root.get("validation"), path, None, "[validation]")?;
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", "[case]")?,
        loadcase: string(case, "loadcase", "[case]")?,
        simulation_json: resolve(
            value(inputs, "simulation_json", "[inputs]")?,
            path,
            "[inputs].simulation_json",
        )?,
        svc_source_kind: Some(SourceKind::Simulation),
        gravity: positive(value(physics, "gravity", "[physics]")?, "[physics].gravity")?,
        signals: signal_map.1,
        validation,
        vertical: None,
        roll: None,
        svc_signal_map: signal_map.0,
        svc_measurement_signals: BTreeMap::new(),
    })
}

pub(crate) fn parse_unified(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    keys(root, &["case", "physics", "svc", "vertical"], "top level")?;
    let case = table(
        root.get("case")
            .ok_or_else(|| ConfigError("missing required [case] table".into()))?,
        "[case]",
    )?;
    let physics = root
        .get("physics")
        .map(|value| table(value, "[physics]"))
        .transpose()?;
    keys(case, &["name", "loadcase"], "[case]")?;
    for k in ["name", "loadcase"] {
        require(case, k, "[case]")?
    }
    let svc = root
        .get("svc")
        .map(|value| table(value, "[svc]"))
        .transpose()?;
    let gravity = if svc.is_some() {
        let physics =
            physics.ok_or_else(|| ConfigError("missing required [physics] table".into()))?;
        keys(physics, &["gravity"], "[physics]")?;
        positive(value(physics, "gravity", "[physics]")?, "[physics].gravity")?
    } else if let Some(physics) = physics {
        keys(physics, &["gravity"], "[physics]")?;
        physics
            .get("gravity")
            .map(|value| positive(value, "[physics].gravity"))
            .transpose()?
            .unwrap_or(9.80665)
    } else {
        9.80665
    };
    let (simulation_json, signals, validation, svc_signal_map) = if let Some(svc) = svc {
        keys(
            svc,
            &[
                "inputs",
                "mappings",
                "signals",
                "signal_mappings",
                "validation",
            ],
            "[svc]",
        )?;
        let inputs = table(
            svc.get("inputs")
                .ok_or_else(|| ConfigError("missing required [inputs] table".into()))?,
            "[svc.inputs]",
        )?;
        keys(
            inputs,
            &[
                "signal_map",
                "signal_mappings",
                "simulation_json",
                "measurement_reference",
            ],
            "[svc.inputs]",
        )?;
        require(inputs, "simulation_json", "[svc.inputs]")?;
        let source = signal_source(svc, path, "[svc]", Some(inputs))?;
        (
            resolve(
                value(inputs, "simulation_json", "[svc.inputs]")?,
                path,
                "[svc.inputs].simulation_json",
            )?,
            source.1,
            parse_validation(
                svc.get("validation"),
                path,
                inputs.get("measurement_reference"),
                "[svc.validation]",
            )?,
            source.0,
        )
    } else {
        (std::path::PathBuf::new(), BTreeMap::new(), None, None)
    };
    let vertical = root
        .get("vertical")
        .map(|v| parse_vertical(v, path))
        .transpose()?;
    let has_svc = !simulation_json.as_os_str().is_empty();
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", "[case]")?,
        loadcase: string(case, "loadcase", "[case]")?,
        simulation_json,
        svc_source_kind: has_svc.then_some(SourceKind::Simulation),
        gravity,
        signals,
        validation,
        vertical,
        roll: None,
        svc_signal_map,
        svc_measurement_signals: BTreeMap::new(),
    })
}

/// Build a `VerticalConfig` from a normalized `[vertical]`-shaped table.
///
/// Reached directly from `parse_unified`'s raw `[vertical]` table, and from
/// `function_first::function_knc_test`, which synthesizes the same shape
/// from `[input.knc.<test>]` + `[setup.knc.<test>]` for both `vertical` and
/// `roll` rather than duplicating this parser.
pub(crate) fn parse_vertical(
    raw: &Value,
    path: &Path,
) -> Result<VerticalConfig, Box<dyn std::error::Error>> {
    let v = table(raw, "[vertical]")?;
    keys(
        v,
        &[
            "inputs",
            "mappings",
            "setup",
            "window",
            "metric",
            "branch",
            "signals",
            "signal_mappings",
            "measurement_signals",
            "validation",
        ],
        "[vertical]",
    )?;
    let i = table(
        v.get("inputs")
            .ok_or_else(|| ConfigError("missing required [vertical.inputs] table".into()))?,
        "[vertical.inputs]",
    )?;
    let s = table(
        v.get("setup")
            .ok_or_else(|| ConfigError("missing required [vertical.setup] table".into()))?,
        "[vertical.setup]",
    )?;
    keys(
        i,
        &[
            "signal_map",
            "signal_mappings",
            "measurement_hdf5",
            "simulation_hdf5",
            "source_kind",
            "static_simulation_hdf5",
        ],
        "[vertical.inputs]",
    )?;
    require(i, "measurement_hdf5", "[vertical.inputs]")?;
    require(i, "simulation_hdf5", "[vertical.inputs]")?;
    keys(
        s,
        &[
            "brakes_applied",
            "engine_running",
            "front_arb_connected",
            "rear_arb_connected",
            "window_minimum_mm",
            "window_maximum_mm",
            "window_step_mm",
        ],
        "[vertical.setup]",
    )?;
    let condensed = s.contains_key("window_minimum_mm")
        || s.contains_key("window_maximum_mm")
        || s.contains_key("window_step_mm");
    let mut metric = v
        .get("metric")
        .map(|raw| table(raw, "[vertical.metric]"))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    keys(
        &metric,
        &[
            "gradient_points_mm",
            "gradient_points_deg",
            "direction",
            "index",
            "measurement",
            "simulation",
            "sprung_mass_front_left_kg",
            "sprung_mass_front_right_kg",
            "sprung_mass_rear_left_kg",
            "sprung_mass_rear_right_kg",
        ],
        "[vertical.metric]",
    )?;
    let mut sprung_mass_kg = BTreeMap::new();
    for corner in ["front_left", "front_right", "rear_left", "rear_right"] {
        if let Some(value) = metric.get(&format!("sprung_mass_{corner}_kg")) {
            sprung_mass_kg.insert(
                corner.to_owned(),
                positive(value, &format!("sprung_mass_{corner}_kg"))?,
            );
        }
    }
    let mut window_extra = Map::new();
    if let Some(branch) = v.get("branch") {
        fold_legacy_branch(branch, &mut metric, &mut window_extra)?;
    }
    let (w, window_table) = if condensed {
        if v.contains_key("window") {
            return Err(ConfigError(
                "[vertical] cannot define both condensed setup window and [vertical.window]".into(),
            )
            .into());
        }
        (
            ["window_minimum_mm", "window_maximum_mm", "window_step_mm"]
                .iter()
                .map(|k| value(s, k, "[vertical.setup]"))
                .collect::<Result<Vec<_>, _>>()?,
            Map::new(),
        )
    } else {
        let t = table(
            v.get("window")
                .ok_or_else(|| ConfigError("missing required [vertical.window] table".into()))?,
            "[vertical.window]",
        )?;
        keys(
            t,
            &[
                "travel_minimum_mm",
                "travel_maximum_mm",
                "minimum_mm",
                "maximum_mm",
                "step_mm",
                "metric_gradient_points_mm",
                "plot_branches",
                "measurement",
                "simulation",
            ],
            "[vertical.window]",
        )?;
        (
            vec![
                aliased(t, "travel_minimum_mm", "minimum_mm", "[vertical.window]")?,
                aliased(t, "travel_maximum_mm", "maximum_mm", "[vertical.window]")?,
                value(t, "step_mm", "[vertical.window]")?,
            ],
            t.clone(),
        )
    };
    let min = finite(w[0], "travel_minimum_mm")?;
    let max = finite(w[1], "travel_maximum_mm")?;
    let step = positive(w[2], "step_mm")?;
    if min >= max {
        return Err(ConfigError(
            "window travel_minimum_mm must be less than travel_maximum_mm".into(),
        )
        .into());
    };
    // Deprecated: `metric_gradient_points_mm` in the window table.
    let gradient = metric
        .get("gradient_points_mm")
        .or_else(|| window_table.get("metric_gradient_points_mm"))
        .map(|value| positive(value, "gradient_points_mm"))
        .transpose()?;
    if let Some(gradient) = gradient
        && (-gradient < min || gradient > max)
    {
        return Err(ConfigError(format!(
            "gradient_points_mm ±{gradient} must lie inside travel window {min}..{max} mm"
        ))
        .into());
    }
    let gradient_deg = metric
        .get("gradient_points_deg")
        .map(|value| positive(value, "gradient_points_deg"))
        .transpose()?;
    let mut window_branch = window_table;
    for (key, value) in window_extra {
        window_branch.entry(key).or_insert(value);
    }
    let source = signal_source(v, path, "[vertical]", Some(i))?;
    let measurement_signals = parse_measurement_signals(
        v.get("measurement_signals"),
        "[vertical.measurement_signals]",
    )?;
    let branch = VerticalBranches::parse(&metric, &window_branch)?;
    Ok(VerticalConfig {
        measurement_hdf5: resolve(
            value(i, "measurement_hdf5", "[vertical.inputs]")?,
            path,
            "measurement_hdf5",
        )?,
        simulation_hdf5: resolve(
            value(i, "simulation_hdf5", "[vertical.inputs]")?,
            path,
            "simulation_hdf5",
        )?,
        source_kind: match i
            .get("source_kind")
            .and_then(Value::as_str)
            .unwrap_or("simulation")
        {
            "simulation" => SourceKind::Simulation,
            "measurement" => SourceKind::Measurement,
            value => {
                return Err(ConfigError(format!(
                    "[vertical.inputs].source_kind must be simulation or measurement, got {value}"
                ))
                .into());
            }
        },
        static_simulation_hdf5: i
            .get("static_simulation_hdf5")
            .map(|value| resolve(value, path, "static_simulation_hdf5"))
            .transpose()?,
        setup: VerticalSetup {
            brakes_applied: boolean(s, "brakes_applied")?,
            engine_running: boolean(s, "engine_running")?,
            front_arb_connected: boolean(s, "front_arb_connected")?,
            rear_arb_connected: s
                .get("rear_arb_connected")
                .map(|v| boolean_value(v, "rear_arb_connected"))
                .transpose()?,
        },
        window: VerticalWindow {
            minimum_mm: min,
            maximum_mm: max,
            step_mm: step,
            gradient_points_mm: gradient,
            gradient_points_deg: gradient_deg,
            sprung_mass_kg,
        },
        branch,
        signals: source.1,
        signal_map: source.0,
        measurement_signals,
        tolerances: vertical_tolerances(v)?,
    })
}

fn vertical_tolerances(
    vertical: &toml::map::Map<String, Value>,
) -> Result<ValidationTolerances, Box<dyn std::error::Error>> {
    let Some(validation) = vertical.get("validation") else {
        return Ok(ValidationTolerances {
            absolute: BTreeMap::new(),
            relative_percent: BTreeMap::new(),
        });
    };
    let validation = table(validation, "[vertical.validation]")?;
    let tolerances = validation
        .get("tolerances")
        .or_else(|| {
            validation
                .get("validation")
                .and_then(Value::as_table)
                .and_then(|v| v.get("tolerances"))
        })
        .map(|value| table(value, "[vertical.validation.tolerances]"))
        .transpose()?;
    let (absolute, relative_percent) = tolerances
        .map(|value| parse_tolerance_table(value, "[vertical.validation.tolerances]"))
        .transpose()?
        .unwrap_or_default();
    Ok(ValidationTolerances {
        absolute,
        relative_percent,
    })
}

fn signal_source(
    container: &toml::map::Map<String, Value>,
    path: &Path,
    where_: &str,
    inputs: Option<&toml::map::Map<String, Value>>,
) -> Result<SignalSource, Box<dyn std::error::Error>> {
    let source = inputs.unwrap_or(container);
    let legacy_map = source
        .get("signal_mappings")
        .or_else(|| source.get("signal_map"));
    let mappings = container.get("mappings");
    let mappings_table = mappings
        .map(|value| table(value, &format!("{where_}.mappings")))
        .transpose()?;
    let mappings_file = mappings_table.and_then(|table| table.get("file"));
    let mappings_inline = mappings_table.and_then(|table| {
        if table.contains_key("file") {
            None
        } else {
            Some(mappings.expect("mappings table exists"))
        }
    });
    let inline = mappings_inline
        .or_else(|| container.get("signal_mappings"))
        .or_else(|| container.get("signals"));
    if source.get("signal_mappings").is_some() && source.get("signal_map").is_some() {
        return Err(ConfigError(format!(
            "{where_}.inputs cannot define both signal_mappings and signal_map"
        ))
        .into());
    }
    if mappings_file.is_some() && mappings_table.unwrap().len() != 1 {
        return Err(ConfigError(format!(
            "{where_}.mappings cannot define both file and inline signal mappings"
        ))
        .into());
    }
    if container.get("signal_mappings").is_some() && container.get("signals").is_some() {
        return Err(ConfigError(format!(
            "{where_} cannot define both signal_mappings and signals"
        ))
        .into());
    };
    if (legacy_map.is_some() || inline.is_some()) && mappings_file.is_some() {
        return Err(ConfigError(format!(
            "{where_} cannot define both modular and inline signal mappings"
        ))
        .into());
    }
    if let Some(m) = legacy_map.or(mappings_file) {
        let key = if mappings_file.is_some() {
            "mappings.file"
        } else if source.get("signal_mappings").is_some() {
            "signal_mappings"
        } else {
            "signal_map"
        };
        let p = resolve(m, path, &format!("{where_}.{key}"))?;
        if !p.is_file() {
            return Err(format!("signal map does not exist: {}", p.display()).into());
        };
        let t: toml::Table = std::fs::read_to_string(&p)?
            .parse()
            .map_err(|e| ConfigError(format!("invalid signal map TOML in {}: {e}", p.display())))?;
        let t = &t;
        keys(
            t,
            &["signals", "signal_mappings", "mappings"],
            &format!("signal map {}", p.display()),
        )?;
        let values = t
            .get("mappings")
            .or_else(|| t.get("signal_mappings"))
            .or_else(|| t.get("signals"));
        return Ok((
            Some(p),
            parse_signals(values, "signal map signal_mappings")?,
        ));
    }
    if let Some(v) = inline {
        let key = if container.get("signal_mappings").is_some() {
            "signal_mappings"
        } else {
            "signals"
        };
        return Ok((None, parse_signals(Some(v), &format!("{where_}.{key}"))?));
    }
    Err(ConfigError(format!("{where_} must define mappings or signal mappings")).into())
}
