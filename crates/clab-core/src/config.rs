//! Typed, validated case configuration.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use toml::{Value, map::Map};

#[derive(Debug, Clone, PartialEq, Error)]
#[error("{0}")]
pub struct ConfigError(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct SignalRef {
    pub entity: String,
    pub component: String,
    pub unit: String,
}
/// Raw-signal mapping for a measurement source: a flat channel or JSON key
/// name, unlike simulation's entity+component pair. Shared by every
/// measurement-mapping table (KnC vertical HDF5 channels, SVC reference
/// JSON keys).
#[derive(Debug, Clone, PartialEq)]
pub struct MeasurementSignalRef {
    pub entity: String,
    pub unit: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationTolerances {
    pub absolute: BTreeMap<String, f64>,
    pub relative_percent: BTreeMap<String, f64>,
}
type ToleranceMaps = (BTreeMap<String, f64>, BTreeMap<String, f64>);
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationConfig {
    pub reference: PathBuf,
    pub tolerances: ValidationTolerances,
}
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
    pub measurement_hdf5: PathBuf,
    pub simulation_hdf5: PathBuf,
    pub source_kind: SourceKind,
    pub static_simulation_hdf5: Option<PathBuf>,
    pub setup: VerticalSetup,
    pub window: VerticalWindow,
    pub branch: VerticalBranches,
    pub signals: BTreeMap<String, SignalRef>,
    pub signal_map: Option<PathBuf>,
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

    fn parse(
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

/// KnC tests analyzed from one configured input each. `static` is not listed:
/// it is only a baseline for these tests.
pub const KNC_TESTS: [&str; 2] = ["vertical", "roll"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Svc,
    Knc,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Simulation,
    Measurement,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CaseConfig {
    pub path: PathBuf,
    pub name: String,
    pub loadcase: String,
    pub simulation_json: PathBuf,
    pub svc_source_kind: Option<SourceKind>,
    pub gravity: f64,
    pub signals: BTreeMap<String, SignalRef>,
    pub validation: Option<ValidationConfig>,
    pub vertical: Option<VerticalConfig>,
    /// KnC roll test; shares the vertical test's configuration shape.
    pub roll: Option<VerticalConfig>,
    pub svc_signal_map: Option<PathBuf>,
    /// SVC measurement reference JSON key mapping: canonical metric name ->
    /// `{entity (JSON key), unit}`. Required whenever this config's SVC
    /// input is a measurement reference — there is no hardcoded canonical
    /// name fallback.
    pub svc_measurement_signals: BTreeMap<String, MeasurementSignalRef>,
}
impl CaseConfig {
    pub fn configured_domains(&self) -> Vec<Domain> {
        let mut domains = Vec::new();
        if !self.simulation_json.as_os_str().is_empty() {
            domains.push(Domain::Svc);
        }
        if self.vertical.is_some() || self.roll.is_some() {
            domains.push(Domain::Knc);
        }
        domains
    }
}
type SignalSource = (Option<PathBuf>, BTreeMap<String, SignalRef>);

/// Load and validate one case configuration.
///
/// The root schema selects parsing mode: function-first configurations have an
/// `[input]` table, unified configurations have `[svc]` or `[vertical]`, and
/// older flat configurations use the legacy schema. All referenced paths are
/// resolved relative to the configuration file.
pub fn load_config(path: impl AsRef<Path>) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (path, raw) = load_toml(path.as_ref())?;
    parse_root(&raw, &path)
}

fn parse_root(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    if root.contains_key("input") {
        parse_function_first(root, path)
    } else if root.contains_key("svc") || root.contains_key("vertical") {
        parse_unified(root, path)
    } else {
        parse_legacy(root, path)
    }
}

/// Load shared analysis configuration and one run manifest.
///
/// External role files are expanded before both documents are merged, so each
/// role keeps paths relative to its own TOML file. Run metadata identifies each
/// run; absent metadata is reported as `undefined`.
pub fn load_config_and_run(
    config: impl AsRef<Path>,
    run: impl AsRef<Path>,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (config_path, config_root) = load_toml(config.as_ref())?;
    let (run_path, run_root) = load_toml(run.as_ref())?;
    let mut config_root = load_function_role_files(&config_root, &config_path)?;
    // Run metadata never inherits from reusable shared configuration.
    config_root.remove("meta");
    config_root.remove("case");
    let mut run_root = Value::Table(run_root);
    resolve_external_paths(
        &mut run_root,
        run_path.parent().unwrap_or_else(|| Path::new(".")),
    )?;
    let run_root = load_function_role_files(table(&run_root, "run manifest")?, &run_path)?;
    let mut root = config_root;
    merge_map(&mut root, &run_root);
    parse_root(&root, &config_path)
}

pub fn validate_run_input_files(run: impl AsRef<Path>) -> Result<(), Box<dyn std::error::Error>> {
    let (run, root) = load_toml(run.as_ref())?;
    validate_run_input_tables(&root, &run)
}

fn validate_run_input_tables(
    root: &toml::Table,
    run: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(input) = root.get("input").and_then(Value::as_table) else {
        return Ok(());
    };
    if let Some(svc) = input.get("svc").and_then(Value::as_table) {
        validate_run_input_table(svc, run)?;
    }
    if let Some(knc) = input.get("knc").and_then(Value::as_table) {
        for test in KNC_TESTS {
            if let Some(input) = knc.get(test).and_then(Value::as_table) {
                validate_run_input_table(input, run)?;
            }
        }
        if let Some(static_input) = knc.get("static").and_then(Value::as_table) {
            validate_run_input_table(static_input, run)?;
        }
    }
    Ok(())
}

fn validate_run_input_table(
    input: &toml::map::Map<String, Value>,
    run: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for key in ["file", "input_file", "static_file"] {
        let Some(file) = input.get(key).and_then(Value::as_str) else {
            continue;
        };
        let path = expand(Path::new(file))?;
        let path = if path.is_absolute() {
            path
        } else {
            run.parent().unwrap_or_else(|| Path::new(".")).join(path)
        };
        if !path.is_file() {
            return Err(ConfigError(format!("{file} cannot be found")).into());
        }
    }
    for key in ["simulation", "measurement", "static_simulation"] {
        if let Some(input) = input.get(key).and_then(Value::as_table) {
            validate_run_input_table(input, run)?;
        }
    }
    Ok(())
}

/// Load shared analysis configuration for a direct normalized input.
///
/// `knc_test` names the KnC test the input belongs to, `None` for SVC. A KnC
/// input supplies the same file in both roles solely to build its typed
/// mapping/setup configuration. The caller must only read its actual source.
/// Direct input has no run manifest, so absent `[meta]` name and loadcase are
/// reported as `undefined`.
pub fn load_config_with_input(
    config: impl AsRef<Path>,
    input: impl AsRef<Path>,
    knc_test: Option<&str>,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let (path, raw) = load_toml(config.as_ref())?;
    let mut root = load_function_role_files(&raw, &path)?;
    let input = absolute(&expand(input.as_ref())?)?.display().to_string();
    let mut role = Map::new();
    if let Some(test) = knc_test {
        let mut test_role = Map::new();
        test_role.insert("file".into(), Value::String(input.clone()));
        let mut knc = Map::new();
        knc.insert(test.into(), Value::Table(test_role));
        role.insert("knc".into(), Value::Table(knc));
    } else {
        let mut svc = Map::new();
        svc.insert("file".into(), Value::String(input));
        role.insert("svc".into(), Value::Table(svc));
    }
    if let Some(test) = knc_test
        && let Some(existing) = root
            .get_mut("input")
            .and_then(Value::as_table_mut)
            .and_then(|input| input.get_mut("knc"))
            .and_then(Value::as_table_mut)
            .and_then(|knc| knc.get_mut(test))
            .and_then(Value::as_table_mut)
    {
        // Direct input replaces active source, but keeps configured static input.
        existing.remove("file");
        existing.remove("simulation");
        existing.remove("measurement");
    }
    match root.get_mut("input") {
        Some(Value::Table(existing)) => merge_map(existing, &role),
        Some(_) => return Err(ConfigError("[input] must be a table".into()).into()),
        None => {
            root.insert("input".into(), Value::Table(role));
        }
    }
    parse_function_first(&root, &path)
}

fn load_toml(path: &Path) -> Result<(PathBuf, toml::Table), Box<dyn std::error::Error>> {
    let path = absolute(&expand(path)?)?;
    if !path.is_file() {
        return Err(format!("config file does not exist: {}", path.display()).into());
    }
    let text = fs::read_to_string(&path)?;
    let raw = text
        .parse()
        .map_err(|e| ConfigError(format!("invalid TOML in {}: {e}", path.display())))?;
    Ok((path, raw))
}

fn parse_function_first(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let root = load_function_role_files(root, path)?;
    keys(
        &root,
        &[
            "meta",
            "case",
            "physics",
            "input",
            "signals",
            "mappings",
            "setup",
            "validation",
        ],
        "top level",
    )?;
    let (name, loadcase) = if let Some(meta) = root.get("meta") {
        let meta = table(meta, "[meta]")?;
        keys(meta, &["name", "loadcase"], "[meta]")?;
        (
            meta.get("name")
                .map(|value| string_value(value, "[meta].name"))
                .transpose()?
                .unwrap_or_else(|| "undefined".into()),
            meta.get("loadcase")
                .map(|value| string_value(value, "[meta].loadcase"))
                .transpose()?
                .unwrap_or_else(|| "undefined".into()),
        )
    } else if let Some(case) = root.get("case") {
        let case = table(case, "[case]")?;
        keys(case, &["name", "domain", "loadcase"], "[case]")?;
        (
            string(case, "name", "[case]")?,
            string(case, "loadcase", "[case]")?,
        )
    } else {
        ("undefined".into(), "undefined".into())
    };
    let input = table(value(&root, "input", "top level")?, "[input]")?;
    keys(input, &["svc", "knc"], "[input]")?;
    let physics = root
        .get("physics")
        .map(|v| table(v, "[physics]"))
        .transpose()?;
    let gravity = physics
        .and_then(|p| p.get("gravity"))
        .map(|v| positive(v, "[physics].gravity"))
        .transpose()?
        .unwrap_or(9.80665);

    let svc = function_svc(&root, input, path)?;
    if let Some(knc) = input.get("knc") {
        let knc = table(knc, "[input.knc]")?;
        keys(knc, &["vertical", "roll", "static"], "[input.knc]")?;
        if knc.contains_key("static") && !KNC_TESTS.iter().any(|test| knc.contains_key(*test)) {
            return Err(ConfigError(
                "[input.knc.static] requires [input.knc.vertical] or [input.knc.roll]".into(),
            )
            .into());
        }
    }
    let vertical = function_knc_test(&root, input, path, "vertical")?;
    let roll = function_knc_test(&root, input, path, "roll")?;
    if svc.is_none() && vertical.is_none() && roll.is_none() {
        return Err(ConfigError("[input] must define at least one domain".into()).into());
    }
    let mut source_kinds = svc
        .iter()
        .map(|(_, _, _, _, kind, _)| *kind)
        .chain(vertical.iter().chain(&roll).map(|test| test.source_kind));
    if let Some(first) = source_kinds.next()
        && source_kinds.any(|kind| kind != first)
    {
        return Err(
            ConfigError("run manifest mixes simulation and measurement inputs".into()).into(),
        );
    }
    let (
        simulation_json,
        signals,
        validation,
        svc_signal_map,
        svc_source_kind,
        svc_measurement_signals,
    ) = svc.unwrap_or_else(|| {
        (
            PathBuf::new(),
            BTreeMap::new(),
            None,
            None,
            SourceKind::Simulation,
            BTreeMap::new(),
        )
    });
    let has_svc = !simulation_json.as_os_str().is_empty();
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name,
        loadcase,
        simulation_json,
        svc_source_kind: has_svc.then_some(svc_source_kind),
        gravity,
        signals,
        validation,
        vertical,
        roll,
        svc_signal_map,
        svc_measurement_signals,
    })
}

fn load_function_role_files(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<toml::map::Map<String, Value>, Box<dyn std::error::Error>> {
    let mut root = root.clone();
    if root.contains_key("compare") && root.contains_key("validation") {
        return Err(ConfigError("[compare] cannot be combined with [validation]".into()).into());
    }
    for section in [
        "input",
        "setup",
        "signals",
        "mappings",
        "validation",
        "compare",
    ] {
        let Some(raw) = root.get(section) else {
            continue;
        };
        let section_table = table(raw, &format!("[{section}]"))?;
        let Some(file) = section_table
            .get("file")
            .or_else(|| section_table.get("input_file"))
        else {
            continue;
        };
        if section_table.len() != 1 {
            return Err(ConfigError(format!(
                "[{section}] input_file cannot be combined with inline values"
            ))
            .into());
        }
        let legacy_json =
            section_table.contains_key("input_file") && matches!(section, "input" | "setup");
        let file = resolve(file, path, &format!("[{section}].file"))?;
        let text = fs::read_to_string(&file).map_err(|error| {
            ConfigError(format!(
                "failed to read {section} file '{}': {error}",
                file.display()
            ))
        })?;
        let mut external = if legacy_json {
            let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
                ConfigError(format!("invalid JSON in {}: {error}", file.display()))
            })?;
            json_to_toml(&value, &format!("{section} file {}", file.display()))?
        } else {
            Value::Table(text.parse::<toml::Table>().map_err(|error| {
                ConfigError(format!("invalid TOML in {}: {error}", file.display()))
            })?)
        };
        resolve_external_paths(
            &mut external,
            file.parent().unwrap_or_else(|| Path::new(".")),
        )?;
        root.insert(section.into(), external);
    }
    if let Some(compare) = root.remove("compare") {
        root.insert("validation".into(), compare);
    }
    Ok(root)
}

/// Resolve paths declared inside an external role against that role's directory.
///
/// Role contents lose their original location when merged into case config, so
/// normalize references before merging.
fn resolve_external_paths(
    value: &mut Value,
    base: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match value {
        Value::Table(table) => {
            for (key, value) in table {
                if matches!(key.as_str(), "file" | "input_file" | "static_file") {
                    if let Some(path) = value.as_str() {
                        let path = expand(Path::new(path))?;
                        if !path.is_absolute() {
                            *value =
                                Value::String(absolute(&base.join(path))?.display().to_string());
                        }
                    }
                } else {
                    resolve_external_paths(value, base)?;
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                resolve_external_paths(value, base)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn json_to_toml(
    value: &serde_json::Value,
    where_: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(match value {
        serde_json::Value::Null => {
            return Err(ConfigError(format!("{where_} cannot contain null")).into());
        }
        serde_json::Value::Bool(value) => Value::Boolean(*value),
        serde_json::Value::Number(value) => Value::Float(
            value
                .as_f64()
                .ok_or_else(|| ConfigError(format!("{where_} contains invalid number")))?,
        ),
        serde_json::Value::String(value) => Value::String(value.clone()),
        serde_json::Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| json_to_toml(value, where_))
                .collect::<Result<_, _>>()?,
        ),
        serde_json::Value::Object(values) => Value::Table(
            values
                .iter()
                .map(|(key, value)| Ok((key.clone(), json_to_toml(value, where_)?)))
                .collect::<Result<_, Box<dyn std::error::Error>>>()?,
        ),
    })
}

type FunctionSvc = (
    PathBuf,
    BTreeMap<String, SignalRef>,
    Option<ValidationConfig>,
    Option<PathBuf>,
    SourceKind,
    BTreeMap<String, MeasurementSignalRef>,
);

fn function_svc(
    root: &toml::map::Map<String, Value>,
    input: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<Option<FunctionSvc>, Box<dyn std::error::Error>> {
    let Some(raw) = input.get("svc") else {
        return Ok(None);
    };
    let svc = table(raw, "[input.svc]")?;
    keys(
        svc,
        &["file", "input_file", "simulation", "measurement"],
        "[input.svc]",
    )?;
    let direct = svc.get("file").or_else(|| svc.get("input_file"));
    let (simulation, source_kind) = if let Some(file) = direct {
        if svc.len() != 1 {
            return Err(
                ConfigError("[input.svc].file cannot be combined with role tables".into()).into(),
            );
        }
        let source = resolve(file, path, "[input.svc].file")?;
        let kind = source_kind(&source, Domain::Svc, None)?;
        (source, kind)
    } else {
        let simulation = function_input(svc, "simulation", "[input.svc]")?
            .map(|p| {
                resolve(
                    &Value::String(p.to_string_lossy().into()),
                    path,
                    "[input.svc.simulation].file",
                )
            })
            .transpose()?;
        let measurement = function_input(svc, "measurement", "[input.svc]")?;
        match (simulation, measurement) {
            (Some(_), Some(_)) => {
                return Err(ConfigError(
                    "[input.svc] must define exactly one of simulation or measurement".into(),
                )
                .into());
            }
            (Some(path), None) => (path, SourceKind::Simulation),
            (None, Some(path)) => (path, SourceKind::Measurement),
            (None, None) => return Ok(None),
        }
    };
    let mappings = merged_mapping(root, "simulation", "svc", None, path)?;
    let signals = parse_signals(Some(&Value::Table(mappings.clone())), "[signals.svc]")?;
    let measurement_mappings = merged_mapping(root, "measurement", "svc", None, path)?;
    let measurement_signals = parse_measurement_signals(
        Some(&Value::Table(measurement_mappings)),
        "[signals.svc.measurement]",
    )?;
    let validation = function_validation(
        root,
        (source_kind == SourceKind::Measurement).then_some(simulation.clone()),
        path,
    )?;
    let map_path = mapping_file_path(root, "svc", None, path)?;
    Ok(Some((
        simulation,
        signals,
        validation,
        map_path,
        source_kind,
        measurement_signals,
    )))
}

/// Parse one KnC test (`vertical` or `roll`) from `[input.knc.<test>]`,
/// `[setup.knc]` merged with `[setup.knc.<test>]`, and `<test>` mappings.
/// `[input.knc.static]` is the shared static simulation baseline.
fn function_knc_test(
    root: &toml::map::Map<String, Value>,
    input: &toml::map::Map<String, Value>,
    path: &Path,
    name: &str,
) -> Result<Option<VerticalConfig>, Box<dyn std::error::Error>> {
    let Some(raw) = input.get("knc") else {
        return Ok(None);
    };
    let knc = table(raw, "[input.knc]")?;
    let Some(raw) = knc.get(name) else {
        return Ok(None);
    };
    let scope = format!("[input.knc.{name}]");
    let test = table(raw, &scope)?;
    keys(
        test,
        &[
            "file",
            "input_file",
            "static_file",
            "simulation",
            "measurement",
            "static_simulation",
        ],
        &scope,
    )?;
    let direct = test.get("file").or_else(|| test.get("input_file"));
    let static_baseline = knc
        .get("static")
        .map(|raw| function_input_table(raw, "[input.knc.static]"))
        .transpose()?
        .flatten()
        .map(|file| {
            resolve(
                &Value::String(file.to_string_lossy().into()),
                path,
                "[input.knc.static].file",
            )
        })
        .transpose()?;
    if let Some(static_baseline) = &static_baseline
        && source_kind(static_baseline, Domain::Knc, Some("static"))? != SourceKind::Simulation
    {
        return Err(ConfigError(format!(
            "[input.knc.static] '{}' source_kind must be simulation",
            static_baseline.display()
        ))
        .into());
    }
    let has_static_baseline = static_baseline.is_some();
    let (source_kind, source, static_simulation) = if let Some(file) = direct {
        if test.keys().any(|key| {
            !matches!(
                key.as_str(),
                "file" | "input_file" | "static_file" | "static_simulation"
            )
        }) {
            return Err(
                ConfigError(format!("{scope}.file cannot be combined with role tables")).into(),
            );
        }
        let source = resolve(file, path, &format!("{scope}.file"))?;
        let input_kind = source_kind(&source, Domain::Knc, Some(name))?;
        let static_file = test
            .get("static_file")
            .map(|file| resolve(file, path, &format!("{scope}.static_file")))
            .transpose()?;
        let static_simulation = function_input(test, "static_simulation", &scope)?;
        if static_file.is_some() && static_simulation.is_some() {
            return Err(ConfigError(format!(
                "{scope}.static_file cannot be combined with static_simulation"
            ))
            .into());
        }
        if (static_file.is_some() || static_simulation.is_some()) && static_baseline.is_some() {
            return Err(ConfigError(format!(
                "{scope} static input cannot be combined with [input.knc.static]"
            ))
            .into());
        }
        let static_file = static_baseline.or(static_file).or(static_simulation);
        if let Some(static_file) = &static_file
            && source_kind(static_file, Domain::Knc, Some("static"))? != SourceKind::Simulation
        {
            return Err(ConfigError(format!(
                "{name} static_file '{}' source_kind must be simulation",
                static_file.display()
            ))
            .into());
        }
        (input_kind, source, static_file)
    } else {
        let measurement = function_input(test, "measurement", &scope)?;
        let simulation = function_input(test, "simulation", &scope)?;
        let static_simulation = function_input(test, "static_simulation", &scope)?;
        let (source, source_kind) = match (simulation, measurement) {
            (Some(_), Some(_)) => {
                return Err(ConfigError(format!(
                    "{scope} must define exactly one of simulation or measurement"
                ))
                .into());
            }
            (Some(path), None) => (path, SourceKind::Simulation),
            (None, Some(path)) => {
                if static_simulation.is_some() {
                    return Err(ConfigError(format!(
                        "[input.knc.{name}.static_simulation] requires simulation input"
                    ))
                    .into());
                }
                (path, SourceKind::Measurement)
            }
            (None, None) => {
                return Err(
                    ConfigError(format!("{scope} must define simulation or measurement")).into(),
                );
            }
        };
        validate_vertical_hdf5_source(&source, source_kind, &format!("{name} input"), Some(name))?;
        if static_simulation.is_some() && static_baseline.is_some() {
            return Err(ConfigError(format!(
                "[input.knc.{name}.static_simulation] cannot be combined with [input.knc.static]"
            ))
            .into());
        }
        (source_kind, source, static_baseline.or(static_simulation))
    };
    if has_static_baseline && source_kind != SourceKind::Simulation {
        return Err(ConfigError(format!("[input.knc.static] requires simulation {scope}")).into());
    }
    if let Some(static_simulation) = &static_simulation {
        validate_vertical_hdf5_source(
            static_simulation,
            SourceKind::Simulation,
            &format!("{name} static_simulation input"),
            Some("static"),
        )?;
    }
    let mappings = merged_mapping(root, "simulation", "knc", Some(name), path)?;
    let measurement_mappings = merged_mapping(root, "measurement", "knc", Some(name), path)?;
    let setup_root = table(
        root.get("setup")
            .ok_or_else(|| ConfigError("missing [setup] table".into()))?,
        "[setup]",
    )?;
    let setup = if let Some(knc) = setup_root.get("knc") {
        let mut setup = table(knc, "[setup.knc]")?.clone();
        let mut overrides = None;
        for test in KNC_TESTS {
            if let Some(raw) = setup.remove(test)
                && test == name
            {
                overrides = Some(raw);
            }
        }
        if let Some(overrides) = overrides {
            merge_map(
                &mut setup,
                table(&overrides, &format!("[setup.knc.{name}]"))?,
            );
        }
        setup
    } else {
        table(
            setup_root
                .get(name)
                .ok_or_else(|| ConfigError(format!("missing [setup.knc.{name}] table")))?,
            &format!("[setup.knc.{name}]"),
        )?
        .clone()
    };
    let mut vertical = Map::new();
    let mut inputs = Map::new();
    inputs.insert(
        "measurement_hdf5".into(),
        Value::String(source.to_string_lossy().into()),
    );
    inputs.insert(
        "simulation_hdf5".into(),
        Value::String(source.to_string_lossy().into()),
    );
    inputs.insert(
        "source_kind".into(),
        Value::String(
            match source_kind {
                SourceKind::Simulation => "simulation",
                SourceKind::Measurement => "measurement",
            }
            .into(),
        ),
    );
    if let Some(static_simulation) = static_simulation {
        inputs.insert(
            "static_simulation_hdf5".into(),
            Value::String(static_simulation.to_string_lossy().into()),
        );
    }
    vertical.insert("inputs".into(), Value::Table(inputs));
    vertical.insert("signals".into(), Value::Table(mappings));
    vertical.insert(
        "measurement_signals".into(),
        Value::Table(measurement_mappings),
    );
    let mut setup_copy = setup.clone();
    // `[meta]` holds the test setup flags; flags directly in `[setup.knc]`
    // remain accepted.
    if let Some(meta) = setup_copy.remove("meta") {
        for (key, value) in table(&meta, "[setup.knc.meta]")? {
            setup_copy.insert(key.clone(), value.clone());
        }
    }
    let window = setup_copy.remove("window").unwrap_or_else(|| {
        let mut w = Map::new();
        for key in [
            "travel_minimum_mm",
            "travel_maximum_mm",
            "minimum_mm",
            "maximum_mm",
            "step_mm",
            "metric_gradient_points_mm",
        ] {
            if let Some(value) = setup_copy.remove(key) {
                w.insert(key.into(), value);
            }
        }
        Value::Table(w)
    });
    if let Some(metric) = setup_copy.remove("metric") {
        vertical.insert("metric".into(), metric);
    }
    if let Some(branch) = setup_copy.remove("branch") {
        vertical.insert("branch".into(), branch);
    }
    vertical.insert("setup".into(), Value::Table(setup_copy));
    vertical.insert("window".into(), window);
    if let Some(validation) = knc_validation(root, name)? {
        vertical.insert("validation".into(), Value::Table(validation));
    }
    Ok(Some(parse_vertical(&Value::Table(vertical), path)?))
}

fn knc_validation(
    root: &toml::map::Map<String, Value>,
    name: &str,
) -> Result<Option<Map<String, Value>>, Box<dyn std::error::Error>> {
    let Some(knc) = root
        .get("validation")
        .and_then(Value::as_table)
        .and_then(|validation| validation.get("knc"))
    else {
        return Ok(None);
    };
    let knc = table(knc, "[validation.knc]")?;
    let mut validation = knc
        .get("validation")
        .map(|value| table(value, "[validation.knc.validation]"))
        .transpose()?
        .cloned()
        .unwrap_or_default();
    if let Some(test) = knc.get(name) {
        let test = table(test, &format!("[validation.knc.{name}]"))?;
        if let Some(tolerances) = test.get("tolerances") {
            let mut override_ = Map::new();
            override_.insert("tolerances".into(), tolerances.clone());
            merge_map(&mut validation, &override_);
        } else if let Some(override_) = test.get("validation") {
            merge_map(
                &mut validation,
                table(override_, &format!("[validation.knc.{name}.validation]"))?,
            );
        }
    }
    Ok((!validation.is_empty()).then_some(validation))
}

fn function_input(
    container: &toml::map::Map<String, Value>,
    name: &str,
    where_: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let Some(raw) = container.get(name) else {
        return Ok(None);
    };
    let table = table(raw, &format!("{where_}.{name}"))?;
    keys(table, &["file", "input_file"], &format!("{where_}.{name}"))?;
    Ok(Some(PathBuf::from(string_value(
        table
            .get("file")
            .or_else(|| table.get("input_file"))
            .unwrap(),
        "file",
    )?)))
}

fn function_input_table(
    raw: &Value,
    where_: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let table = table(raw, where_)?;
    keys(table, &["file", "input_file"], where_)?;
    let Some(file) = table.get("file").or_else(|| table.get("input_file")) else {
        return Err(ConfigError(format!("{where_} requires file")).into());
    };
    Ok(Some(PathBuf::from(string_value(
        file,
        &format!("{where_}.file"),
    )?)))
}

fn validate_vertical_hdf5_source(
    path: &Path,
    expected: SourceKind,
    role: &str,
    expected_test: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Config templates may name future conversion outputs. Validate metadata once present.
    if !path.exists() {
        return Ok(());
    }
    let file = hdf5::File::open(path).map_err(|error| {
        ConfigError(format!(
            "failed to open {role} '{}': {error}",
            path.display()
        ))
    })?;
    let source_kind = if file.attr_names()?.iter().any(|name| name == "source_kind") {
        use hdf5::types::VarLenUnicode;
        Some(
            file.attr("source_kind")?
                .read_scalar::<VarLenUnicode>()
                .map_err(|_| {
                    ConfigError(format!(
                        "{role} '{}' has invalid source_kind",
                        path.display()
                    ))
                })?
                .as_str()
                .to_owned(),
        )
    } else {
        None
    };
    let actual = match source_kind.as_deref() {
        Some("simulation") => SourceKind::Simulation,
        Some("measurement") => SourceKind::Measurement,
        None => {
            return Err(ConfigError(format!(
                "{role} '{}' is missing required source_kind",
                path.display()
            ))
            .into());
        }
        Some(value) => {
            return Err(ConfigError(format!(
                "{role} '{}' has unsupported source_kind '{value}'",
                path.display()
            ))
            .into());
        }
    };
    if actual != expected {
        let kind = match expected {
            SourceKind::Simulation => "simulation",
            SourceKind::Measurement => "measurement",
        };
        return Err(ConfigError(format!(
            "{role} '{}' source_kind must be {kind}",
            path.display()
        ))
        .into());
    }
    check_hdf5_identity(path, Domain::Knc, expected_test)
}

fn source_kind(
    path: &Path,
    domain: Domain,
    expected_test: Option<&str>,
) -> Result<SourceKind, Box<dyn std::error::Error>> {
    match path.extension().and_then(|extension| extension.to_str()) {
        // Config templates may name a normalized simulation output before conversion.
        Some("h5" | "hdf5") if !path.exists() => Ok(SourceKind::Simulation),
        Some("h5" | "hdf5") => {
            let kind = hdf5_source_kind(path, "input")?;
            check_hdf5_identity(path, domain, expected_test)?;
            Ok(kind)
        }
        Some("json") if domain == Domain::Svc => {
            let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)
                .map_err(|error| {
                    ConfigError(format!("invalid SVC JSON '{}': {error}", path.display()))
                })?;
            match value.get("source_kind").and_then(serde_json::Value::as_str) {
                Some("simulation") => Ok(SourceKind::Simulation),
                Some("measurement") => Ok(SourceKind::Measurement),
                Some(value) => Err(ConfigError(format!(
                    "SVC JSON '{}' has unsupported source_kind '{value}'",
                    path.display()
                ))
                .into()),
                None => Err(ConfigError(format!(
                    "SVC JSON '{}' is missing required source_kind",
                    path.display()
                ))
                .into()),
            }
        }
        _ => Err(ConfigError(format!(
            "input '{}' must be {}",
            path.display(),
            if domain == Domain::Svc {
                "SVC JSON or HDF5"
            } else {
                "HDF5"
            }
        ))
        .into()),
    }
}

/// Reject an HDF5 file whose declared domain or test differs from the one the
/// config expects. Files converted before `domain` and `test` were stored carry
/// neither and are accepted.
fn check_hdf5_identity(
    path: &Path,
    domain: Domain,
    expected_test: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = hdf5::File::open(path).map_err(|error| {
        ConfigError(format!(
            "failed to open input '{}': {error}",
            path.display()
        ))
    })?;
    let optional = |name: &str| -> Result<Option<String>, Box<dyn std::error::Error>> {
        use hdf5::types::VarLenUnicode;
        if !file.attr_names()?.iter().any(|value| value == name) {
            return Ok(None);
        }
        let value = file
            .attr(name)?
            .read_scalar::<VarLenUnicode>()
            .map_err(|_| ConfigError(format!("input '{}' has invalid {name}", path.display())))?;
        Ok(Some(value.as_str().to_owned()))
    };
    let (found_domain, found_test) = (optional("domain")?, optional("test")?);
    let expected_domain = match domain {
        Domain::Svc => "svc",
        Domain::Knc => "knc",
    };
    if let Some(found) = found_domain.as_deref()
        && found != expected_domain
    {
        return Err(ConfigError(format!(
            "input '{}' has domain '{found}', expected '{expected_domain}'",
            path.display()
        ))
        .into());
    }
    if let (Some(found), Some(expected)) = (found_test.as_deref(), expected_test)
        && found != expected
    {
        return Err(ConfigError(format!(
            "input '{}' has test '{found}', expected '{expected}'",
            path.display()
        ))
        .into());
    }
    Ok(())
}

fn hdf5_source_kind(path: &Path, role: &str) -> Result<SourceKind, Box<dyn std::error::Error>> {
    let file = hdf5::File::open(path).map_err(|error| {
        ConfigError(format!(
            "failed to open {role} '{}': {error}",
            path.display()
        ))
    })?;
    let source_kind = if file.attr_names()?.iter().any(|name| name == "source_kind") {
        use hdf5::types::VarLenUnicode;
        Some(
            file.attr("source_kind")?
                .read_scalar::<VarLenUnicode>()
                .map_err(|_| {
                    ConfigError(format!(
                        "{role} '{}' has invalid source_kind",
                        path.display()
                    ))
                })?
                .as_str()
                .to_owned(),
        )
    } else {
        None
    };
    match source_kind.as_deref() {
        Some("simulation") => Ok(SourceKind::Simulation),
        Some("measurement") => Ok(SourceKind::Measurement),
        None => Err(ConfigError(format!(
            "{role} '{}' is missing required source_kind",
            path.display()
        ))
        .into()),
        Some(value) => Err(ConfigError(format!(
            "{role} '{}' has unsupported source_kind '{value}'",
            path.display()
        ))
        .into()),
    }
}

/// Resolve one domain's (and optional test's) signal mapping for one
/// `source_kind` ("simulation" or "measurement"). Every mapping table ends
/// in the same shape: `domain[.test].source_kind[.signals]`, so simulation
/// and measurement are resolved identically, just with a different
/// `source_kind` — the only asymmetry left is which one a given fixture
/// happens to define entries under.
fn merged_mapping(
    root: &toml::map::Map<String, Value>,
    source_kind: &str,
    domain: &str,
    test: Option<&str>,
    path: &Path,
) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    let all = root
        .get("signals")
        .map(|v| table(v, "[signals]"))
        .or_else(|| root.get("mappings").map(|v| table(v, "[mappings]")))
        .transpose()?;
    let mut result = Map::new();
    if let Some(all) = all
        && let Some(by_kind) = all.get(source_kind)
    {
        // `[mappings.simulation]`/`[mappings.measurement]` may itself be a
        // `file = "..."` pointer covering every domain, not just an inline
        // table — resolve that one level the same way `merge_mapping_source`
        // resolves a file pointer at the domain/test level.
        let by_kind_table =
            resolve_mapping_file(by_kind, path, &format!("[signals.{source_kind}]"))?;
        let by_kind_table = &by_kind_table;
        if let Some(shared) = by_kind_table.get(domain) {
            let shared_table = table(shared, &format!("[signals.{source_kind}.{domain}]"))?;
            merge_mapping_source(
                &mut result,
                shared_table.get("signals").unwrap_or(shared),
                path,
                &format!("[signals.{source_kind}.{domain}.signals]"),
                domain,
            )?;
        }
        if let Some(test) = test
            && let Some(domain_value) = by_kind_table.get(domain)
            && let Some(specific) =
                table(domain_value, &format!("[signals.{source_kind}.{domain}]"))?.get(test)
        {
            let specific_table = table(
                specific,
                &format!("[signals.{source_kind}.{domain}.{test}]"),
            )?;
            merge_mapping_source(
                &mut result,
                specific_table.get("signals").unwrap_or(specific),
                path,
                &format!("[signals.{source_kind}.{domain}.{test}.signals]"),
                domain,
            )?;
        }
    }
    if result.is_empty() {
        let where_ = match test {
            Some(test) => format!("{source_kind}.{domain}.{test}"),
            None => format!("{source_kind}.{domain}"),
        };
        return Err(ConfigError(format!("[signals.{where_}] must define mappings")).into());
    }
    Ok(result)
}

/// Resolve a mapping node that may be a `file = "..."` pointer into its
/// actual table — the loaded file's `mappings`/`signal_mappings`/`signals`
/// wrapper if present, else the file's content as-is; an inline table (no
/// `file` key) is returned unchanged. Same resolution `merge_mapping_source`
/// does for a domain/test-level file pointer, lifted one level so a
/// `source_kind`-level pointer (covering every domain in one file) works too.
fn resolve_mapping_file(
    value: &Value,
    path: &Path,
    where_: &str,
) -> Result<Map<String, Value>, Box<dyn std::error::Error>> {
    let t = table(value, where_)?;
    let Some(file) = t.get("file") else {
        return Ok(t.clone());
    };
    let file = resolve(file, path, &format!("{where_}.file"))?;
    let text = fs::read_to_string(&file).map_err(|error| {
        ConfigError(format!(
            "failed to read mapping file '{}': {error}",
            file.display()
        ))
    })?;
    let external: toml::Table = text
        .parse()
        .map_err(|e| ConfigError(format!("invalid mapping TOML in {}: {e}", file.display())))?;
    let values = external
        .get("mappings")
        .or_else(|| external.get("signal_mappings"))
        .or_else(|| external.get("signals"));
    match values {
        Some(values) => Ok(table(values, &format!("mapping file {}", file.display()))?.clone()),
        None => Ok(external),
    }
}

fn merge_mapping_source(
    target: &mut Map<String, Value>,
    source: &Value,
    path: &Path,
    where_: &str,
    domain: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = table(source, where_)?;
    if let Some(file) = source.get("file") {
        let file = resolve(file, path, &format!("{where_}.file"))?;
        let text = fs::read_to_string(&file).map_err(|error| {
            ConfigError(format!(
                "failed to read mapping file '{}': {error}",
                file.display()
            ))
        })?;
        let external: toml::Table = text
            .parse()
            .map_err(|e| ConfigError(format!("invalid mapping TOML in {}: {e}", file.display())))?;
        let values = external
            .get("mappings")
            .or_else(|| external.get("signal_mappings"))
            .or_else(|| external.get("signals"));
        let values = values
            .map(|value| table(value, &format!("mapping file {}", file.display())))
            .transpose()?
            .unwrap_or(&external);
        // A domain-level file may retain its original [svc.*] or [knc.*]
        // headers after splitting a source-kind mapping file.
        let values = values
            .get(domain)
            .map(|value| {
                table(
                    value,
                    &format!("mapping file {} [{domain}]", file.display()),
                )
            })
            .transpose()?
            .unwrap_or(values);
        merge_map(target, values);
    }
    for (key, value) in source {
        if key != "file" && !matches!(key.as_str(), "vertical" | "roll" | "svc" | "knc") {
            merge_value(target, key, value);
        }
    }
    Ok(())
}

fn merge_map(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        merge_value(target, key, value);
    }
}

fn merge_value(target: &mut Map<String, Value>, key: &str, value: &Value) {
    if let (Some(Value::Table(existing)), Some(incoming)) = (target.get_mut(key), value.as_table())
    {
        merge_map(existing, incoming);
    } else {
        target.insert(key.to_owned(), value.clone());
    }
}

fn mapping_file_path(
    root: &toml::map::Map<String, Value>,
    domain: &str,
    test: Option<&str>,
    path: &Path,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let Some(mappings) = root.get("mappings").and_then(Value::as_table) else {
        return Ok(None);
    };
    let Some(base) = mappings.get(domain).and_then(Value::as_table) else {
        return Ok(None);
    };
    let source = test
        .and_then(|t| base.get(t).and_then(Value::as_table))
        .or_else(|| base.get("base").and_then(Value::as_table))
        .unwrap_or(base);
    source
        .get("file")
        .map(|v| resolve(v, path, "mapping file"))
        .transpose()
}

fn function_validation(
    root: &toml::map::Map<String, Value>,
    reference: Option<PathBuf>,
    path: &Path,
) -> Result<Option<ValidationConfig>, Box<dyn std::error::Error>> {
    let validation = root
        .get("validation")
        .and_then(Value::as_table)
        .and_then(|v| v.get("svc"));
    let tolerances = validation
        .and_then(Value::as_table)
        .and_then(|v| v.get("validation"))
        .and_then(Value::as_table)
        .and_then(|v| v.get("tolerances"))
        .or_else(|| {
            validation
                .and_then(Value::as_table)
                .and_then(|v| v.get("tolerances"))
        })
        .or_else(|| {
            root.get("validation")
                .and_then(Value::as_table)
                .and_then(|v| v.get("svc"))
                .and_then(Value::as_table)
                .and_then(|v| v.get("tolerances"))
        });
    let (absolute, relative_percent) = match tolerances {
        None if reference.is_none() => return Ok(None),
        None => (BTreeMap::new(), BTreeMap::new()),
        Some(value) if value.as_str().is_some() => {
            let file = resolve(value, path, "[validation.svc].tolerances")?;
            let parsed: toml::Table = fs::read_to_string(&file)?.parse()?;
            parse_tolerance_table(&parsed, "SVC tolerance file")?
        }
        Some(value) => parse_tolerance_table(
            table(value, "[validation.svc].tolerances")?,
            "[validation.svc].tolerances",
        )?,
    };
    let reference = reference
        .map(|reference| {
            resolve(
                &Value::String(reference.to_string_lossy().into()),
                path,
                "[input.svc.measurement].input_file",
            )
        })
        .transpose()?
        .unwrap_or_default();
    Ok(Some(ValidationConfig {
        reference,
        tolerances: ValidationTolerances {
            absolute,
            relative_percent,
        },
    }))
}

fn parse_legacy(
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

fn parse_unified(
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
        (PathBuf::new(), BTreeMap::new(), None, None)
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

fn parse_vertical(raw: &Value, path: &Path) -> Result<VerticalConfig, Box<dyn std::error::Error>> {
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
        let t: toml::Table = fs::read_to_string(&p)?
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
fn parse_signals(
    v: Option<&Value>,
    where_: &str,
) -> Result<BTreeMap<String, SignalRef>, Box<dyn std::error::Error>> {
    let t = table(
        v.ok_or_else(|| ConfigError(format!("{where_} must be a table")))?,
        where_,
    )?;
    if t.is_empty() {
        return Err(ConfigError(format!("{where_} must define at least one signal")).into());
    };
    t.iter()
        .map(|(n, v)| Ok((n.clone(), parse_signal(n, v)?)))
        .collect()
}
fn parse_signal(n: &str, v: &Value) -> Result<SignalRef, Box<dyn std::error::Error>> {
    if n.is_empty() {
        return Err(ConfigError("signal names must be non-empty strings".into()).into());
    };
    let t = table(v, &format!("signal '{n}'"))?;
    keys(
        t,
        &["entity", "component", "unit"],
        &format!("signal '{n}'"),
    )?;
    Ok(SignalRef {
        entity: string(t, "entity", &format!("signal '{n}'"))?,
        component: string(t, "component", &format!("signal '{n}'"))?,
        unit: string_allow_empty(t, "unit", &format!("signal '{n}'"))?,
    })
}
/// Same as `parse_signals`, for measurement mapping tables (flat
/// entity/unit, no component).
fn parse_measurement_signals(
    v: Option<&Value>,
    where_: &str,
) -> Result<BTreeMap<String, MeasurementSignalRef>, Box<dyn std::error::Error>> {
    let t = table(
        v.ok_or_else(|| ConfigError(format!("{where_} must be a table")))?,
        where_,
    )?;
    if t.is_empty() {
        return Err(ConfigError(format!("{where_} must define at least one signal")).into());
    };
    t.iter()
        .map(|(n, v)| Ok((n.clone(), parse_measurement_signal(n, v)?)))
        .collect()
}
fn parse_measurement_signal(
    n: &str,
    v: &Value,
) -> Result<MeasurementSignalRef, Box<dyn std::error::Error>> {
    if n.is_empty() {
        return Err(ConfigError("signal names must be non-empty strings".into()).into());
    };
    let t = table(v, &format!("signal '{n}'"))?;
    keys(t, &["entity", "unit"], &format!("signal '{n}'"))?;
    Ok(MeasurementSignalRef {
        entity: string(t, "entity", &format!("signal '{n}'"))?,
        unit: string_allow_empty(t, "unit", &format!("signal '{n}'"))?,
    })
}

fn parse_validation(
    raw: Option<&Value>,
    path: &Path,
    reference: Option<&Value>,
    where_: &str,
) -> Result<Option<ValidationConfig>, Box<dyn std::error::Error>> {
    if raw.is_none() {
        if reference.is_some() {
            return Err(ConfigError(
                "[svc.validation] is required when [svc.inputs].measurement_reference is defined"
                    .into(),
            )
            .into());
        }
        return Ok(None);
    };
    let t = table(raw.unwrap(), where_)?;
    let unified = where_ == "[svc.validation]";
    keys(
        t,
        if unified {
            &["tolerances"][..]
        } else {
            &["reference", "tolerances"][..]
        },
        where_,
    )?;
    let reference = if unified {
        reference.ok_or_else(|| {
            ConfigError("[svc.inputs].measurement_reference must be a non-empty string".into())
        })?
    } else {
        value(t, "reference", where_)?
    };
    let reference = resolve(reference, path, "validation reference")?;
    let tolerance_value = value(t, "tolerances", where_)?;
    let tolerance_label = format!("{where_}.tolerances");
    let (absolute, relative_percent) = if tolerance_value.as_str().is_some() {
        let tolerance_path = resolve(tolerance_value, path, &tolerance_label)?;
        let text = fs::read_to_string(&tolerance_path).map_err(|error| {
            ConfigError(format!(
                "failed to read tolerance file '{}': {error}",
                tolerance_path.display()
            ))
        })?;
        let file: toml::Table = text.parse().map_err(|error| {
            ConfigError(format!(
                "invalid tolerance TOML in {}: {error}",
                tolerance_path.display()
            ))
        })?;
        parse_tolerance_table(
            &file,
            &format!("tolerance file {}", tolerance_path.display()),
        )?
    } else {
        let tol = table(tolerance_value, &tolerance_label)?;
        parse_tolerance_table(tol, &tolerance_label)?
    };
    if absolute.is_empty() && relative_percent.is_empty() {
        return Err(ConfigError(format!(
            "{where_}.tolerances must define at least one tolerance"
        ))
        .into());
    };
    Ok(Some(ValidationConfig {
        reference,
        tolerances: ValidationTolerances {
            absolute,
            relative_percent,
        },
    }))
}

fn parse_tolerance_table(
    tol: &toml::map::Map<String, Value>,
    where_: &str,
) -> Result<ToleranceMaps, Box<dyn std::error::Error>> {
    keys(tol, &["absolute", "relative_percent"], where_)?;
    let absolute = parse_tolerances(tol.get("absolute"), "absolute")?;
    let relative_percent = parse_tolerances(tol.get("relative_percent"), "relative_percent")?;
    Ok((absolute, relative_percent))
}
fn parse_tolerances(
    raw: Option<&Value>,
    name: &str,
) -> Result<BTreeMap<String, f64>, Box<dyn std::error::Error>> {
    let Some(v) = raw else {
        return Ok(BTreeMap::new());
    };
    let t = table(v, &format!("tolerances.{name}"))?;
    t.iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                positive(v, &format!("tolerance '{k}' in {name}"))?,
            ))
        })
        .collect()
}
fn table<'a>(
    v: &'a Value,
    where_: &str,
) -> Result<&'a toml::map::Map<String, Value>, Box<dyn std::error::Error>> {
    v.as_table()
        .ok_or_else(|| ConfigError(format!("{where_} must be a table")).into())
}
fn keys(
    t: &toml::map::Map<String, Value>,
    allowed: &[&str],
    where_: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(k) = t.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(ConfigError(format!("unknown {where_} key: {k}")).into());
    };
    Ok(())
}
fn require(
    t: &toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if !t.contains_key(key) {
        Err(ConfigError(format!("{where_} is missing key(s): {key}")).into())
    } else {
        Ok(())
    }
}
/// Read `key`, accepting legacy `alias`; reject both together.
fn aliased<'a>(
    t: &'a toml::map::Map<String, Value>,
    key: &str,
    alias: &str,
    where_: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    match (t.get(key), t.get(alias)) {
        (Some(_), Some(_)) => {
            Err(ConfigError(format!("{where_} cannot define both {key} and {alias}")).into())
        }
        (Some(value), None) | (None, Some(value)) => Ok(value),
        (None, None) => Err(ConfigError(format!("{where_} is missing key(s): {key}")).into()),
    }
}
fn value<'a>(
    t: &'a toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    t.get(key)
        .ok_or_else(|| ConfigError(format!("{where_} is missing key(s): {key}")).into())
}
fn string(
    t: &toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let v = value(t, key, where_)?;
    let s = v
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ConfigError(format!("{where_}.{key} must be a non-empty string")))?;
    Ok(s.to_owned())
}
fn string_allow_empty(
    t: &toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(value(t, key, where_)?
        .as_str()
        .ok_or_else(|| ConfigError(format!("{where_}.{key} must be a string")))?
        .to_owned())
}
fn boolean(
    t: &toml::map::Map<String, Value>,
    key: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    boolean_value(value(t, key, "[vertical.setup]")?, key)
}
fn boolean_value(v: &Value, key: &str) -> Result<bool, Box<dyn std::error::Error>> {
    v.as_bool()
        .ok_or_else(|| ConfigError(format!("[vertical.setup].{key} must be boolean")).into())
}
fn finite(v: &Value, where_: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let n = v
        .as_float()
        .or_else(|| v.as_integer().map(|i| i as f64))
        .ok_or_else(|| ConfigError(format!("{where_} must be a number")))?;
    if !n.is_finite() {
        return Err(ConfigError(format!("{where_} must be finite")).into());
    }
    Ok(n)
}
fn positive(v: &Value, where_: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let n = finite(v, where_)?;
    if n <= 0.0 {
        return Err(ConfigError(format!("{where_} must be positive")).into());
    }
    Ok(n)
}
fn expand(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let text = path.to_string_lossy();
    let home = || std::env::var_os("HOME").ok_or_else(|| ConfigError("HOME is not set".into()));
    let text = if text == "~" {
        home()?.to_string_lossy().into_owned()
    } else if let Some(rest) = text.strip_prefix("~/") {
        format!("{}/{rest}", home()?.to_string_lossy())
    } else {
        text.into_owned()
    };
    Ok(PathBuf::from(text))
}
fn resolve(v: &Value, config: &Path, where_: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let p = expand(Path::new(string_value(v, where_)?.as_str()))?;
    if p.is_absolute() {
        Ok(p)
    } else {
        absolute(&config.parent().unwrap_or_else(|| Path::new(".")).join(p))
    }
}
fn string_value(v: &Value, where_: &str) -> Result<String, Box<dyn std::error::Error>> {
    v.as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ConfigError(format!("{where_} must be a non-empty string")).into())
}
fn absolute(p: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()?.join(p)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use hdf5::File;
    use hdf5::types::VarLenUnicode;

    use super::*;

    fn source_kind(path: &Path, value: &str) {
        let file = File::create(path).unwrap();
        match value {
            "simulation" => {
                scalar(&file, "format_version", 3_u32);
                scalar(&file, "sample_count", 0_u64);
                scalar(&file, "signal_count", 0_u64);
                string(&file, "source_kind", "simulation");
                string(&file, "source_file", "result.res");
                file.create_group("results").unwrap();
            }
            "measurement" => {
                scalar(&file, "format_version", 1_u32);
                scalar(&file, "sample_count", 0_u64);
                scalar(&file, "channel_count", 0_u64);
                string(&file, "source_kind", "measurement");
                string(&file, "source_file", "measurement.acd");
                let measurements = file.create_group("measurements").unwrap();
                measurements
                    .new_dataset_builder()
                    .with_data(&[] as &[f64])
                    .create("sample_axis")
                    .unwrap();
                measurements.create_group("channels").unwrap();
            }
            _ => panic!("unsupported source kind {value}"),
        }
        file.flush().unwrap();
    }

    fn scalar<T: hdf5::H5Type>(file: &File, name: &str, value: T) {
        file.new_attr::<T>()
            .create(name)
            .unwrap()
            .write_scalar(&value)
            .unwrap();
    }

    fn string(file: &File, name: &str, value: &str) {
        file.new_attr::<VarLenUnicode>()
            .shape(())
            .create(name)
            .unwrap()
            .write_scalar(&VarLenUnicode::from_str(value).unwrap())
            .unwrap();
    }
    #[test]
    fn parses_legacy_inline() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nloadcase='l'\n[inputs]\nsimulation_json='x.json'\n[physics]\ngravity=9.8\n[signals.x]\nentity='e'\ncomponent='c'\nunit='u'\n").unwrap();
        let c = load_config(&p).unwrap();
        assert_eq!(c.signals["x"].entity, "e");
        assert_eq!(c.simulation_json, d.path().join("x.json"));
    }

    #[test]
    fn parses_legacy_runtime_smoke_fixture() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
        let directory = tempfile::tempdir().unwrap();
        let fixture = directory.path().join("runtime-smoke.toml");
        fs::copy(fixtures.join("runtime-smoke.toml"), &fixture).unwrap();
        fs::copy(
            fixtures.join("runtime-smoke-signals.toml"),
            directory.path().join("example-signals.toml"),
        )
        .unwrap();

        let config = load_config(fixture).unwrap();

        assert_eq!(config.name, "example-config");
        assert!(config.svc_signal_map.is_some());
        assert!(config.vertical.is_some());
    }

    #[test]
    fn rejects_unknown_and_bad_tolerance() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nloadcase='l'\nextra=1\n[inputs]\nsimulation_json='x.json'\n[physics]\ngravity=9.8\n").unwrap();
        assert!(load_config(&p).unwrap_err().to_string().contains("unknown"));
    }

    #[test]
    fn parses_modular_signal_mappings_and_tolerances() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("tolerances.toml"),
            "[absolute]\ntotal_mass_kg=1\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[svc.inputs]\nsimulation_json='simulation.json'\nmeasurement_reference='reference.json'\n[svc.mappings]\nfile='signals.toml'\n[svc.validation]\ntolerances='tolerances.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(
            config.validation.unwrap().tolerances.absolute["total_mass_kg"],
            1.0
        );
    }

    #[test]
    fn rejects_modular_and_inline_signal_mappings_together() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[svc.inputs]\nsimulation_json='simulation.json'\n[svc.mappings]\nfile='signals.toml'\n[svc.mappings.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();

        assert!(load_config(&p).unwrap_err().to_string().contains("both"));
    }
    #[test]
    fn parses_vertical_only_unified_config() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.configured_domains(), vec![Domain::Knc]);
    }

    #[test]
    fn validates_vertical_branch_options() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let base = "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n";
        fs::write(&p, base).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!((branch.measurement.index, branch.simulation.index), (2, 2));
        fs::write(
            &p,
            format!("{base}[vertical.branch]\nindex=1\n[vertical.branch.simulation]\nplot_branches='selected'\n"),
        )
        .unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!((branch.measurement.index, branch.simulation.index), (1, 1));
        assert_eq!(branch.measurement.plot_branches, "all");
        assert_eq!(branch.simulation.plot_branches, "selected");
        for invalid in [
            "index=0",
            "smooth_window=5",
            "min_samples=5",
            "plot_branches='other'",
        ] {
            fs::write(&p, format!("{base}[vertical.branch]\n{invalid}\n")).unwrap();
            assert!(load_config(&p).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn parses_vertical_travel_window_and_metric_gradient_points() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let write = |window: &str| {
            fs::write(&p, format!("[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\n{window}\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n")).unwrap();
        };
        write(
            "travel_minimum_mm=-10\ntravel_maximum_mm=10\nstep_mm=1\nmetric_gradient_points_mm=5",
        );
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(
            (window.minimum_mm, window.maximum_mm, window.step_mm),
            (-10.0, 10.0, 1.0)
        );
        assert_eq!(window.gradient_points_mm, Some(5.0));
        write("minimum_mm=-1\nmaximum_mm=1\nstep_mm=1");
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(window.gradient_points_mm, None);
        for (window, error) in [
            (
                "travel_minimum_mm=-1\nminimum_mm=-1\ntravel_maximum_mm=1\nstep_mm=1",
                "cannot define both travel_minimum_mm and minimum_mm",
            ),
            (
                "travel_minimum_mm=-10\ntravel_maximum_mm=10\nstep_mm=1\nmetric_gradient_points_mm=0",
                "gradient_points_mm must be positive",
            ),
            (
                "travel_minimum_mm=-10\ntravel_maximum_mm=4\nstep_mm=1\nmetric_gradient_points_mm=5",
                "must lie inside travel window",
            ),
        ] {
            write(window);
            let message = load_config(&p).unwrap_err().to_string();
            assert!(message.contains(error), "{message}");
        }
    }

    #[test]
    fn parses_sprung_mass_fallback_per_corner() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.metric]\nsprung_mass_front_left_kg=449.0\nsprung_mass_rear_right_kg=383.0\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n",
        )
        .unwrap();
        let window = load_config(&p).unwrap().vertical.unwrap().window;
        assert_eq!(window.sprung_mass_kg.get("front_left"), Some(&449.0));
        assert_eq!(window.sprung_mass_kg.get("rear_right"), Some(&383.0));
        assert_eq!(window.sprung_mass_kg.get("front_right"), None);
        assert_eq!(window.sprung_mass_kg.len(), 2);
    }

    #[test]
    fn parses_external_function_first_role_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.json"),
            r#"{"svc":{"simulation":{"input_file":"simulation.json"}}}"#,
        )
        .unwrap();
        fs::write(
            d.path().join("mappings.toml"),
            "[simulation.svc.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[measurement.svc.total]\nentity='e'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(d.path().join("validation.toml"), "[svc]\n").unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\ninput_file='input.json'\n[mappings]\ninput_file='mappings.toml'\n[validation]\ninput_file='validation.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.simulation_json, d.path().join("simulation.json"));
        assert!(config.validation.is_none());
        assert_eq!(config.signals["total"].component, "c");
    }

    #[test]
    fn splits_simulation_and_measurement_mappings_into_separate_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("svc-simulation-mappings.toml"),
            "[svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("svc-measurement-mappings.toml"),
            "[front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc]\nfile='svc-simulation-mappings.toml'\n\
             [mappings.measurement.svc]\nfile='svc-measurement-mappings.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn splits_knc_domain_shared_and_vertical_test_mappings_into_separate_files() {
        let d = tempfile::tempdir().unwrap();
        // Domain-shared (every knc test, not just vertical).
        fs::write(
            d.path().join("knc-simulation-mappings.toml"),
            "[knc.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n",
        )
        .unwrap();
        // Vertical-test-specific.
        fs::write(
            d.path().join("knc-vertical-simulation-mappings.toml"),
            "[front_left_wheel_center_x]\nentity='fl'\ncomponent='x'\nunit='mm'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("knc-vertical-measurement-mappings.toml"),
            "[front_left_wheel_center_x]\nentity='lfwcx'\nunit='mm'\n",
        )
        .unwrap();
        // Function-first `[input]`/`[mappings]` path, not the raw `[vertical]`
        // literal path (whose own inline `signals`/`measurement_signals`
        // keys bypass `merged_mapping` entirely and are unrelated here).
        let input = d.path().join("input.toml");
        fs::write(&input, "[knc.vertical.simulation]\nfile='simulation.h5'\n").unwrap();
        let setup = d.path().join("setup.toml");
        fs::write(
            &setup,
            "[knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n\
             [knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n",
        )
        .unwrap();
        let case = d.path().join("function-case.toml");
        fs::write(
            &case,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n\
             [mappings.simulation.knc]\nfile='knc-simulation-mappings.toml'\n\
             [mappings.simulation.knc.vertical]\nfile='knc-vertical-simulation-mappings.toml'\n\
             [mappings.measurement.knc.vertical]\nfile='knc-vertical-measurement-mappings.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&case).unwrap().vertical.unwrap();
        assert_eq!(vertical.signals["front_left_travel"].entity, "fl");
        assert_eq!(vertical.signals["front_left_wheel_center_x"].component, "x");
        assert_eq!(
            vertical.measurement_signals["front_left_wheel_center_x"].entity,
            "lfwcx"
        );
    }

    #[test]
    fn missing_domain_mapping_file_names_the_reference() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc]\nfile='mappings/missing.toml'\n",
        )
        .unwrap();
        let error = load_config(&p).err().unwrap().to_string();
        assert!(
            error.contains(&format!(
                "failed to read mapping file '{}'",
                d.path().join("mappings/missing.toml").display()
            )),
            "{error}"
        );
    }

    #[test]
    fn resolves_fully_flat_inline_mapping_headers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings.simulation.svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n\
             [mappings.measurement.svc.front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn resolves_single_combined_mapping_file() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("combined-mappings.toml"),
            "[simulation.svc.front_left_wheel_load]\nentity='e'\ncomponent='c'\nunit='kg'\n\
             [measurement.svc.front_left_mass_kg]\nentity='e2'\nunit='kg'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n\
             [mappings]\nfile='combined-mappings.toml'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["front_left_wheel_load"].entity, "e");
        assert_eq!(
            config.svc_measurement_signals["front_left_mass_kg"].entity,
            "e2"
        );
    }

    #[test]
    fn resolves_external_input_paths_relative_to_input_role_file() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("input")).unwrap();
        fs::write(
            d.path().join("input/input.toml"),
            "[svc.simulation]\nfile='data/simulation.json'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input/input.toml'\n[signals.simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='e'\nunit='kg'\n",
        )
        .unwrap();

        let config = load_config(&p).unwrap();
        assert_eq!(
            config.simulation_json,
            d.path().join("input/data/simulation.json")
        );
    }

    #[test]
    fn meta_rejects_redundant_domain() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\ndomain='svc'\nloadcase='l'\n[input.svc.simulation]\ninput_file='simulation.json'\n[mappings.svc.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
        )
        .unwrap();
        assert!(load_config(&p).unwrap_err().to_string().contains("unknown"));
    }

    #[test]
    fn parses_canonical_inline_roles() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n[signals.simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='e'\nunit='kg'\n[validation.svc.tolerances.absolute]\ntotal=1\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(config.validation.unwrap().tolerances.absolute["total"], 1.0);
    }

    #[test]
    fn parses_canonical_toml_role_files_and_signal_inheritance() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("input.toml"), "[svc.simulation]\nfile='simulation.json'\n[knc.vertical.simulation]\nfile='simulation.h5'\n").unwrap();
        fs::write(d.path().join("setup.toml"), "[knc.vertical]\nbrakes_applied=false\nengine_running=false\nfront_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n").unwrap();
        fs::write(d.path().join("signals.toml"), "[simulation.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[measurement.svc.signals.total]\nentity='e'\nunit='kg'\n[simulation.knc.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[simulation.knc.vertical.signals.front_left_wheel_center_x]\nentity='fl'\ncomponent='x'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_wheel_center_x]\nentity='fl'\nunit='mm'\n").unwrap();
        fs::write(
            d.path().join("validation.toml"),
            "[svc.validation.tolerances.absolute]\ntotal=1\n[knc.validation.tolerances.absolute]\nrear_left_wheel_center_x_mm=0.2\n[knc.vertical.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.1\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n[compare]\nfile='validation.toml'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(
            config.validation.as_ref().unwrap().tolerances.absolute["total"],
            1.0
        );
        let vertical = config.vertical.unwrap();
        assert_eq!(vertical.signals["front_left_travel"].component, "travel");
        assert_eq!(vertical.signals["front_left_wheel_center_x"].component, "x");
        assert_eq!(
            vertical.tolerances.absolute["front_left_wheel_center_x_mm"],
            0.1
        );
        assert_eq!(
            vertical.tolerances.absolute["rear_left_wheel_center_x_mm"],
            0.2
        );
    }

    #[test]
    fn rejects_ambiguous_compare_and_validation_roles() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[compare]\nfile='tolerances.toml'\n[validation]\nfile='other.toml'\n",
        )
        .unwrap();
        assert!(
            load_config_with_input(&p, d.path().join("input.h5"), Some("vertical"))
                .unwrap_err()
                .to_string()
                .contains("[compare] cannot be combined with [validation]")
        );
    }

    #[test]
    fn merges_knc_defaults_with_vertical_setup_and_tolerances() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.toml"),
            "[knc.vertical.simulation]\nfile='simulation.h5'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("setup.toml"),
            "[knc]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\nrear_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n",
        )
        .unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[simulation.knc.vertical.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_travel]\nentity='fl'\nunit='mm'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("validation.toml"),
            "[knc.validation.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.1\nrear_left_wheel_center_x_mm=0.1\n[knc.vertical.validation.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.05\n[knc.vertical.validation.tolerances.relative_percent]\nrear_left_wheel_center_x_mm=2\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n[validation]\nfile='validation.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&p).unwrap().vertical.unwrap();

        assert!(vertical.setup.brakes_applied);
        assert_eq!(
            vertical.tolerances.absolute["front_left_wheel_center_x_mm"],
            0.05
        );
        assert_eq!(
            vertical.tolerances.absolute["rear_left_wheel_center_x_mm"],
            0.1
        );
        assert_eq!(
            vertical.tolerances.relative_percent["rear_left_wheel_center_x_mm"],
            2.0
        );
    }

    #[test]
    fn vertical_meta_window_and_metric_override_knc_level() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.toml"),
            "[knc.vertical.simulation]\nfile='simulation.h5'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("setup.toml"),
            "[knc.meta]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\nrear_arb_connected=true\n\
             [knc.window]\nminimum_mm=-80\nmaximum_mm=80\nstep_mm=1\nplot_branches='selected'\n\
             [knc.metric]\ngradient_points_mm=10\ndirection='fall'\nindex=3\n\
             [knc.vertical.window]\nminimum_mm=-60\nmaximum_mm=60\n\
             [knc.vertical.metric]\ngradient_points_mm=5\n\
             [knc.vertical.metric.measurement]\nindex=1\n",
        )
        .unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[simulation.knc.vertical.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[measurement.knc.vertical.signals.front_left_travel]\nentity='fl'\nunit='mm'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n",
        )
        .unwrap();

        let vertical = load_config(&p).unwrap().vertical.unwrap();

        assert!(vertical.setup.brakes_applied);
        assert_eq!(vertical.setup.rear_arb_connected, Some(true));
        assert_eq!(
            vertical.setup.summary(),
            "brakes applied: yes | engine running: no | front ARB connected: yes | rear ARB connected: yes"
        );
        // Vertical overrides per key; unset keys inherit from [knc].
        assert_eq!(
            (
                vertical.window.minimum_mm,
                vertical.window.maximum_mm,
                vertical.window.step_mm
            ),
            (-60.0, 60.0, 1.0)
        );
        assert_eq!(vertical.window.gradient_points_mm, Some(5.0));
        let (measurement, simulation) = (&vertical.branch.measurement, &vertical.branch.simulation);
        assert_eq!((measurement.index, simulation.index), (1, 3));
        assert_eq!(measurement.direction, "fall");
        assert_eq!(simulation.plot_branches, "selected");
        assert!(measurement.defaulted.is_empty() && simulation.defaulted.is_empty());
    }

    #[test]
    fn vertical_reports_defaulted_branch_keys() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        let base = "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-10\nmaximum_mm=10\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[vertical.measurement_signals.x]\nentity='e'\nunit='mm'\n";
        fs::write(&p, base).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!(
            branch.simulation.defaulted,
            ["direction", "index", "plot_branches"]
        );
        fs::write(&p, format!("{base}[vertical.metric]\nindex=1\n")).unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert_eq!(branch.simulation.defaulted, ["direction", "plot_branches"]);
        // A key set only for one source kind stays defaulted for the other.
        fs::write(
            &p,
            format!("{base}[vertical.metric.measurement]\nindex=1\n"),
        )
        .unwrap();
        let branch = load_config(&p).unwrap().vertical.unwrap().branch;
        assert!(!branch.measurement.defaulted.contains(&"index"));
        assert!(branch.simulation.defaulted.contains(&"index"));
        for invalid in [
            "[vertical.metric]\nplot_branches='all'\n",
            "[vertical.metric]\nindex=0\n",
            "[vertical.metric.simulation]\ngradient_points_mm=1\n",
        ] {
            fs::write(&p, format!("{base}{invalid}")).unwrap();
            assert!(load_config(&p).is_err(), "accepted {invalid}");
        }
    }

    #[test]
    fn rejects_vertical_role_with_mismatched_hdf5_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("input.h5");
        source_kind(&input, "measurement");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                input.display()
            ),
        )
        .unwrap();

        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("source_kind must be simulation")
        );
    }

    #[test]
    fn rejects_input_whose_declared_domain_or_test_differs() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("case.toml");
        for (attrs, expected) in [
            (&[("domain", "svc")][..], "has domain 'svc', expected 'knc'"),
            (
                &[("domain", "knc"), ("test", "static")][..],
                "has test 'static', expected 'vertical'",
            ),
        ] {
            let input = d.path().join("input.h5");
            source_kind(&input, "simulation");
            let file = File::open_rw(&input).unwrap();
            for (name, value) in attrs {
                string(&file, name, value);
            }
            file.close().unwrap();
            fs::write(
                &config,
                format!(
                    "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                    input.display()
                ),
            )
            .unwrap();

            let message = load_config(&config).unwrap_err().to_string();
            assert!(message.contains(expected), "{message}");
            fs::remove_file(&input).unwrap();
        }
        // Files without domain and test (converted earlier) are accepted.
        let input = d.path().join("input.h5");
        source_kind(&input, "simulation");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.vertical.simulation]\nfile='{}'\n",
                input.display()
            ),
        )
        .unwrap();
        assert!(
            !load_config(&config)
                .err()
                .is_some_and(|error| error.to_string().contains("expected"))
        );
    }

    #[test]
    fn derives_direct_input_kinds_and_rejects_mixed_domains() {
        let d = tempfile::tempdir().unwrap();
        let svc = d.path().join("svc.h5");
        let vertical = d.path().join("vertical.h5");
        source_kind(&svc, "measurement");
        source_kind(&vertical, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.svc]\nfile={:?}\n[input.knc.vertical]\nfile={:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.svc.signals.x]\nentity='e'\ncomponent='c'\nunit='kg'\n[signals.measurement.svc.signals.x]\nentity='e'\nunit='kg'\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n",
                svc, vertical
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("mixes simulation and measurement")
        );
    }

    #[test]
    fn resolves_static_baseline_relative_to_run() {
        let d = tempfile::tempdir().unwrap();
        let run_dir = d.path().join("run");
        fs::create_dir(&run_dir).unwrap();
        let input = run_dir.join("vertical.h5");
        let static_input = run_dir.join("static.h5");
        source_kind(&input, "simulation");
        source_kind(&static_input, "simulation");
        let config = d.path().join("case.toml");
        fs::write(&config, "[meta]\nname='n'\nloadcase='l'\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n[signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n").unwrap();
        let run = run_dir.join("run.toml");
        fs::write(
            &run,
            "[input.knc.static]\nfile='static.h5'\n[input.knc.vertical]\nfile='vertical.h5'\n",
        )
        .unwrap();
        let vertical = load_config_and_run(&config, &run)
            .unwrap()
            .vertical
            .unwrap();
        assert_eq!(vertical.simulation_hdf5, input);
        assert_eq!(vertical.static_simulation_hdf5, Some(static_input));
    }

    #[test]
    fn run_metadata_overrides_shared_configuration_for_each_run() {
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        let first_input = directory.path().join("first.h5");
        let second_input = directory.path().join("second.h5");
        source_kind(&first_input, "simulation");
        source_kind(&second_input, "simulation");
        fs::write(
            &config,
            "[meta]\nname='shared'\nloadcase='shared'\n[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();
        let first_run = directory.path().join("first.toml");
        fs::write(
            &first_run,
            format!(
                "[meta]\nname='first run'\nloadcase='curb'\n[input.svc]\nfile={:?}\n",
                first_input
            ),
        )
        .unwrap();
        let second_run = directory.path().join("second.toml");
        fs::write(
            &second_run,
            format!(
                "[meta]\nname='second run'\nloadcase='two-up'\n[input.svc]\nfile={:?}\n",
                second_input
            ),
        )
        .unwrap();
        let undefined_run = directory.path().join("undefined.toml");
        fs::write(
            &undefined_run,
            format!("[input.svc]\nfile={first_input:?}\n"),
        )
        .unwrap();

        let first = load_config_and_run(&config, &first_run).unwrap();
        let second = load_config_and_run(&config, &second_run).unwrap();
        let undefined = load_config_and_run(&config, &undefined_run).unwrap();

        assert_eq!(
            (first.name.as_str(), first.loadcase.as_str()),
            ("first run", "curb")
        );
        assert_eq!(
            (second.name.as_str(), second.loadcase.as_str()),
            ("second run", "two-up")
        );
        assert_eq!(first.simulation_json, absolute(&first_input).unwrap());
        assert_eq!(second.simulation_json, absolute(&second_input).unwrap());
        assert_eq!(undefined.name, "undefined");
        assert_eq!(undefined.loadcase, "undefined");
    }

    #[test]
    fn direct_input_reports_undefined_metadata_when_absent() {
        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input.h5");
        source_kind(&input, "simulation");
        let config = directory.path().join("config.toml");
        fs::write(
            &config,
            "[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();

        let loaded = load_config_with_input(&config, &input, None).unwrap();

        assert_eq!(loaded.name, "undefined");
        assert_eq!(loaded.loadcase, "undefined");

        fs::write(
            &config,
            "[meta]\nname='named'\n[physics]\ngravity=9.80665\n[signals.simulation.svc.signals.total]\nentity='vehicle'\ncomponent='mass'\nunit='kg'\n[signals.measurement.svc.signals.total]\nentity='vehicle'\nunit='kg'\n",
        )
        .unwrap();
        let loaded = load_config_with_input(&config, &input, None).unwrap();
        assert_eq!(loaded.name, "named");
        assert_eq!(loaded.loadcase, "undefined");
    }

    #[test]
    fn rejects_static_baseline_with_measurement_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("vertical.h5");
        let static_input = d.path().join("static.h5");
        source_kind(&input, "simulation");
        source_kind(&static_input, "measurement");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.static]\nfile={static_input:?}\n[input.knc.vertical]\nfile={input:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("source_kind must be simulation")
        );
    }

    #[test]
    fn rejects_static_baseline_with_measurement_vertical_input() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("vertical.h5");
        let static_input = d.path().join("static.h5");
        source_kind(&input, "measurement");
        source_kind(&static_input, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.knc.static]\nfile={static_input:?}\n[input.knc.vertical]\nfile={input:?}\n[setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("requires simulation [input.knc.vertical]")
        );
    }

    #[test]
    fn rejects_svc_json_without_source_kind() {
        let d = tempfile::tempdir().unwrap();
        let input = d.path().join("svc.json");
        fs::write(&input, "{}").unwrap();
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n[input.svc]\nfile={input:?}\n[signals.simulation.svc.signals.x]\nentity='e'\ncomponent='c'\nunit='kg'\n"
            ),
        )
        .unwrap();
        assert!(
            load_config(&config)
                .unwrap_err()
                .to_string()
                .contains("missing required source_kind")
        );
    }

    #[test]
    fn direct_vertical_input_keeps_configured_static_simulation() {
        let d = tempfile::tempdir().unwrap();
        let simulation = d.path().join("simulation.h5");
        source_kind(&simulation, "simulation");
        let config = d.path().join("case.toml");
        fs::write(
            &config,
            format!(
                "[meta]\nname='n'\nloadcase='l'\n\
                 [setup.knc.vertical]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n\
                 [setup.knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n\
                 [signals.simulation.knc.vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n\
                 [signals.measurement.knc.vertical.signals.x]\nentity='e'\nunit='mm'\n\
                 [input.knc.vertical.static_simulation]\nfile='{}'\n",
                simulation.display(),
            ),
        )
        .unwrap();

        let loaded = load_config_with_input(&config, &simulation, Some("vertical")).unwrap();
        assert_eq!(
            loaded
                .vertical
                .unwrap()
                .static_simulation_hdf5
                .unwrap()
                .canonicalize()
                .unwrap(),
            simulation.canonicalize().unwrap()
        );
    }

    #[test]
    fn example_templates_load_before_generated_outputs_exist() {
        let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");

        assert!(
            load_config_and_run(
                examples.join("config.toml"),
                examples.join("input/run-simulation.toml")
            )
            .is_ok()
        );
    }
}
