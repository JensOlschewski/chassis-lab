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
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalWindow {
    pub minimum_mm: f64,
    pub maximum_mm: f64,
    pub step_mm: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalConfig {
    pub measurement_hdf5: PathBuf,
    pub simulation_hdf5: PathBuf,
    pub setup: VerticalSetup,
    pub window: VerticalWindow,
    pub signals: BTreeMap<String, SignalRef>,
    pub signal_map: Option<PathBuf>,
    pub tolerances: ValidationTolerances,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Svc,
    Knc,
}
#[derive(Debug, Clone, PartialEq)]
pub struct CaseConfig {
    pub path: PathBuf,
    pub name: String,
    pub loadcase: String,
    pub simulation_json: PathBuf,
    pub gravity: f64,
    pub signals: BTreeMap<String, SignalRef>,
    pub validation: Option<ValidationConfig>,
    pub vertical: Option<VerticalConfig>,
    pub svc_signal_map: Option<PathBuf>,
}
impl CaseConfig {
    pub fn configured_domains(&self) -> Vec<Domain> {
        let mut domains = Vec::new();
        if !self.simulation_json.as_os_str().is_empty() {
            domains.push(Domain::Svc);
        }
        if self.vertical.is_some() {
            domains.push(Domain::Knc);
        }
        domains
    }
}
type SignalSource = (Option<PathBuf>, BTreeMap<String, SignalRef>);

pub fn load_config(path: impl AsRef<Path>) -> Result<CaseConfig, Box<dyn std::error::Error>> {
    let path = absolute(&expand(path.as_ref())?)?;
    if !path.is_file() {
        return Err(format!("config file does not exist: {}", path.display()).into());
    }
    let text = fs::read_to_string(&path)?;
    let raw: toml::Table = text
        .parse()
        .map_err(|e| ConfigError(format!("invalid TOML in {}: {e}", path.display())))?;
    let root = &raw;
    if root.contains_key("input") {
        parse_function_first(root, &path)
    } else if root.contains_key("svc") || root.contains_key("vertical") {
        parse_unified(root, &path)
    } else {
        parse_legacy(root, &path)
    }
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
    let (case, case_where) = if let Some(meta) = root.get("meta") {
        (table(meta, "[meta]")?, "[meta]")
    } else {
        (
            table(
                root.get("case")
                    .ok_or_else(|| ConfigError("missing required [meta] table".into()))?,
                "[case]",
            )?,
            "[case]",
        )
    };
    let case_keys = if case_where == "[case]" {
        &["name", "domain", "loadcase"][..]
    } else {
        &["name", "loadcase"][..]
    };
    keys(case, case_keys, case_where)?;
    require(case, "name", case_where)?;
    require(case, "loadcase", case_where)?;
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
    let vertical = function_vertical(&root, input, path)?;
    if svc.is_none() && vertical.is_none() {
        return Err(ConfigError("[input] must define at least one domain".into()).into());
    }
    let (simulation_json, signals, validation, svc_signal_map) =
        svc.unwrap_or_else(|| (PathBuf::new(), BTreeMap::new(), None, None));
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", case_where)?,
        loadcase: string(case, "loadcase", case_where)?,
        simulation_json,
        gravity,
        signals,
        validation,
        vertical,
        svc_signal_map,
    })
}

fn load_function_role_files(
    root: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<toml::map::Map<String, Value>, Box<dyn std::error::Error>> {
    let mut root = root.clone();
    for section in ["input", "setup", "signals", "mappings", "validation"] {
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
                if matches!(key.as_str(), "file" | "input_file") {
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
    let Some(simulation) = simulation else {
        return Ok(None);
    };
    let mappings = merged_mapping(root, "svc", None, path)?;
    let signals = parse_signals(Some(&Value::Table(mappings.clone())), "[signals.svc]")?;
    let validation = function_validation(root, measurement, path)?;
    let map_path = mapping_file_path(root, "svc", None, path)?;
    Ok(Some((simulation, signals, validation, map_path)))
}

fn function_vertical(
    root: &toml::map::Map<String, Value>,
    input: &toml::map::Map<String, Value>,
    path: &Path,
) -> Result<Option<VerticalConfig>, Box<dyn std::error::Error>> {
    let Some(raw) = input.get("knc") else {
        return Ok(None);
    };
    let knc = table(raw, "[input.knc]")?;
    let Some(raw) = knc.get("vertical") else {
        return Ok(None);
    };
    let test = table(raw, "[input.knc.vertical]")?;
    let measurement = function_input(test, "measurement", "[input.knc.vertical]")?
        .ok_or_else(|| ConfigError("[input.knc.vertical.measurement] is required".into()))?;
    let simulation = function_input(test, "simulation", "[input.knc.vertical]")?
        .ok_or_else(|| ConfigError("[input.knc.vertical.simulation] is required".into()))?;
    let mappings = merged_mapping(root, "knc", Some("vertical"), path)?;
    let setup_root = table(
        root.get("setup")
            .ok_or_else(|| ConfigError("missing [setup] table".into()))?,
        "[setup]",
    )?;
    let setup = if let Some(knc) = setup_root.get("knc") {
        let mut setup = table(knc, "[setup.knc]")?.clone();
        if let Some(vertical) = setup.remove("vertical") {
            merge_map(&mut setup, table(&vertical, "[setup.knc.vertical]")?);
        }
        setup
    } else {
        table(
            setup_root
                .get("vertical")
                .ok_or_else(|| ConfigError("missing [setup.knc.vertical] table".into()))?,
            "[setup.knc.vertical]",
        )?
        .clone()
    };
    let mut vertical = Map::new();
    let mut inputs = Map::new();
    inputs.insert(
        "measurement_hdf5".into(),
        Value::String(measurement.to_string_lossy().into()),
    );
    inputs.insert(
        "simulation_hdf5".into(),
        Value::String(simulation.to_string_lossy().into()),
    );
    vertical.insert("inputs".into(), Value::Table(inputs));
    vertical.insert("signals".into(), Value::Table(mappings));
    let mut setup_copy = setup.clone();
    let window = setup_copy.remove("window").unwrap_or_else(|| {
        let mut w = Map::new();
        for key in ["minimum_mm", "maximum_mm", "step_mm"] {
            if let Some(value) = setup_copy.remove(key) {
                w.insert(key.into(), value);
            }
        }
        Value::Table(w)
    });
    vertical.insert("setup".into(), Value::Table(setup_copy));
    vertical.insert("window".into(), window);
    if let Some(validation) = knc_validation(root)? {
        vertical.insert("validation".into(), Value::Table(validation));
    }
    Ok(Some(parse_vertical(&Value::Table(vertical), path)?))
}

fn knc_validation(
    root: &toml::map::Map<String, Value>,
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
    if let Some(vertical) = knc.get("vertical") {
        let vertical = table(vertical, "[validation.knc.vertical]")?;
        if let Some(tolerances) = vertical.get("tolerances") {
            let mut override_ = Map::new();
            override_.insert("tolerances".into(), tolerances.clone());
            merge_map(&mut validation, &override_);
        } else if let Some(override_) = vertical.get("validation") {
            merge_map(
                &mut validation,
                table(override_, "[validation.knc.vertical.validation]")?,
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

fn merged_mapping(
    root: &toml::map::Map<String, Value>,
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
        && let Some(base) = all.get(domain)
    {
        let base_table = table(base, &format!("[signals.{domain}]"))?;
        if let Some(shared) = base_table.get("base") {
            merge_mapping_source(
                &mut result,
                shared,
                path,
                &format!("[signals.{domain}.base]"),
            )?;
        } else {
            merge_mapping_source(
                &mut result,
                base_table.get("signals").unwrap_or(base),
                path,
                &format!("[signals.{domain}.signals]"),
            )?;
        }
        if let Some(test) = test
            && let Some(child) = base_table.get(test)
        {
            merge_mapping_source(
                &mut result,
                child
                    .as_table()
                    .and_then(|table| table.get("signals"))
                    .unwrap_or(child),
                path,
                &format!("[signals.{domain}.{test}]"),
            )?;
        }
    }
    if result.is_empty() {
        return Err(ConfigError(format!("[signals.{domain}] must define mappings")).into());
    }
    Ok(result)
}

fn merge_mapping_source(
    target: &mut Map<String, Value>,
    source: &Value,
    path: &Path,
    where_: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let source = table(source, where_)?;
    if let Some(file) = source.get("file") {
        let file = resolve(file, path, &format!("{where_}.file"))?;
        let text = fs::read_to_string(&file)?;
        let external: toml::Table = text
            .parse()
            .map_err(|e| ConfigError(format!("invalid mapping TOML in {}: {e}", file.display())))?;
        let values = external
            .get("mappings")
            .or_else(|| external.get("signal_mappings"))
            .or_else(|| external.get("signals"));
        if let Some(values) = values {
            merge_map(
                target,
                table(values, &format!("mapping file {}", file.display()))?,
            );
        } else {
            merge_map(target, &external);
        }
    }
    for (key, value) in source {
        if key != "file" && !matches!(key.as_str(), "vertical" | "svc" | "knc") {
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
    let Some(reference) = reference else {
        return Ok(None);
    };
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
    let reference = resolve(
        &Value::String(reference.to_string_lossy().into()),
        path,
        "[input.svc.measurement].input_file",
    )?;
    let (absolute, relative_percent) = match tolerances {
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
        gravity: positive(value(physics, "gravity", "[physics]")?, "[physics].gravity")?,
        signals: signal_map.1,
        validation,
        vertical: None,
        svc_signal_map: signal_map.0,
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
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", "[case]")?,
        loadcase: string(case, "loadcase", "[case]")?,
        simulation_json,
        gravity,
        signals,
        validation,
        vertical,
        svc_signal_map,
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
            "signals",
            "signal_mappings",
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
    let w = if condensed {
        if v.contains_key("window") {
            return Err(ConfigError(
                "[vertical] cannot define both condensed setup window and [vertical.window]".into(),
            )
            .into());
        }
        ["window_minimum_mm", "window_maximum_mm", "window_step_mm"]
            .iter()
            .map(|k| value(s, k, "[vertical.setup]"))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let t = table(
            v.get("window")
                .ok_or_else(|| ConfigError("missing required [vertical.window] table".into()))?,
            "[vertical.window]",
        )?;
        keys(
            t,
            &["minimum_mm", "maximum_mm", "step_mm"],
            "[vertical.window]",
        )?;
        [
            value(t, "minimum_mm", "[vertical.window]")?,
            value(t, "maximum_mm", "[vertical.window]")?,
            value(t, "step_mm", "[vertical.window]")?,
        ]
        .to_vec()
    };
    let min = finite(w[0], "minimum_mm")?;
    let max = finite(w[1], "maximum_mm")?;
    let step = positive(w[2], "step_mm")?;
    if min >= max {
        return Err(ConfigError("window minimum_mm must be less than maximum_mm".into()).into());
    };
    let source = signal_source(v, path, "[vertical]", Some(i))?;
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
        },
        signals: source.1,
        signal_map: source.0,
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
    use super::*;
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
        fs::write(&p, "[case]\nname='n'\nloadcase='l'\n[vertical.inputs]\nmeasurement_hdf5='measurement.h5'\nsimulation_hdf5='simulation.h5'\n[vertical.setup]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\n[vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n[vertical.signals.x]\nentity='e'\ncomponent='c'\nunit='mm'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.configured_domains(), vec![Domain::Knc]);
    }

    #[test]
    fn parses_external_function_first_role_files() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.json"),
            r#"{"svc":{"simulation":{"input_file":"simulation.json"},"measurement":{"input_file":"reference.json"}}}"#,
        )
        .unwrap();
        fs::write(
            d.path().join("mappings.toml"),
            "[svc.base.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
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
        assert_eq!(config.validation.unwrap().tolerances.absolute.len(), 0);
        assert_eq!(config.signals["total"].component, "c");
    }

    #[test]
    fn resolves_external_input_paths_relative_to_input_role_file() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("input")).unwrap();
        fs::write(
            d.path().join("input/input.toml"),
            "[svc.simulation]\nfile='data/simulation.json'\n[svc.measurement]\nfile='data/reference.json'\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(
            &p,
            "[meta]\nname='n'\nloadcase='l'\n[input]\nfile='input/input.toml'\n[signals.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n",
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
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input.svc.simulation]\nfile='simulation.json'\n[input.svc.measurement]\nfile='reference.json'\n[signals.svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[validation.svc.tolerances.absolute]\ntotal=1\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
        assert_eq!(config.validation.unwrap().tolerances.absolute["total"], 1.0);
    }

    #[test]
    fn parses_canonical_toml_role_files_and_signal_inheritance() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("input.toml"), "[svc.simulation]\nfile='simulation.json'\n[svc.measurement]\nfile='reference.json'\n[knc.vertical.simulation]\nfile='simulation.h5'\n[knc.vertical.measurement]\nfile='measurement.h5'\n").unwrap();
        fs::write(d.path().join("setup.toml"), "[knc.vertical]\nbrakes_applied=false\nengine_running=false\nfront_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n").unwrap();
        fs::write(d.path().join("signals.toml"), "[svc.signals.total]\nentity='e'\ncomponent='c'\nunit='kg'\n[knc.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n[knc.vertical.signals.front_left_wheel_center_x]\nentity='fl'\ncomponent='x'\nunit='mm'\n").unwrap();
        fs::write(
            d.path().join("validation.toml"),
            "[svc.validation.tolerances.absolute]\ntotal=1\n[knc.validation.tolerances.absolute]\nrear_left_wheel_center_x_mm=0.2\n[knc.vertical.tolerances.absolute]\nfront_left_wheel_center_x_mm=0.1\n",
        )
        .unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p, "[meta]\nname='n'\nloadcase='l'\n[physics]\ngravity=9.8\n[input]\nfile='input.toml'\n[setup]\nfile='setup.toml'\n[signals]\nfile='signals.toml'\n[validation]\nfile='validation.toml'\n").unwrap();
        let config = load_config(&p).unwrap();
        assert_eq!(config.signals["total"].unit, "kg");
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
    fn merges_knc_defaults_with_vertical_setup_and_tolerances() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("input.toml"),
            "[knc.vertical.simulation]\nfile='simulation.h5'\n[knc.vertical.measurement]\nfile='measurement.h5'\n",
        )
        .unwrap();
        fs::write(
            d.path().join("setup.toml"),
            "[knc]\nbrakes_applied=true\nengine_running=false\nfront_arb_connected=true\nrear_arb_connected=true\n[knc.vertical.window]\nminimum_mm=-1\nmaximum_mm=1\nstep_mm=1\n",
        )
        .unwrap();
        fs::write(
            d.path().join("signals.toml"),
            "[knc.vertical.signals.front_left_travel]\nentity='fl'\ncomponent='travel'\nunit='mm'\n",
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
}
