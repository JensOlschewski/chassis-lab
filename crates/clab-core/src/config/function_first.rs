//! Today's canonical configuration schema: an `[input]` table selects each
//! domain's source, `[setup]`/`[signals]`/`[mappings]`/`[validation]` may be
//! external role files, and KnC tests are built by translating
//! `[input.knc.<test>]` + `[setup.knc.<test>]` into the shape
//! [`legacy::parse_vertical`] already knows how to parse.

use std::{collections::BTreeMap, fs, path::Path};

use toml::{Value, map::Map};

use super::legacy::{VerticalConfig, parse_vertical};
use super::support::{
    keys, mapping_file_path, merge_map, merged_mapping, parse_measurement_signals, parse_signals,
    parse_tolerance_table, positive, resolve, string_value, table, value,
};
use super::{
    CaseConfig, ConfigError, Domain, KNC_TESTS, MeasurementSignalRef, SignalRef, SourceKind,
    ValidationConfig, ValidationTolerances,
};

pub(crate) fn parse_function_first(
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
            super::support::string(case, "name", "[case]")?,
            super::support::string(case, "loadcase", "[case]")?,
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
            std::path::PathBuf::new(),
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

pub(crate) fn load_function_role_files(
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
pub(crate) fn resolve_external_paths(
    value: &mut Value,
    base: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    match value {
        Value::Table(table) => {
            for (key, value) in table {
                if matches!(key.as_str(), "file" | "input_file" | "static_file") {
                    if let Some(path) = value.as_str() {
                        let path = super::support::expand(Path::new(path))?;
                        if !path.is_absolute() {
                            *value = Value::String(
                                super::support::absolute(&base.join(path))?
                                    .display()
                                    .to_string(),
                            );
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
    std::path::PathBuf,
    BTreeMap<String, SignalRef>,
    Option<ValidationConfig>,
    Option<std::path::PathBuf>,
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
) -> Result<Option<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let Some(raw) = container.get(name) else {
        return Ok(None);
    };
    let table = table(raw, &format!("{where_}.{name}"))?;
    keys(table, &["file", "input_file"], &format!("{where_}.{name}"))?;
    Ok(Some(std::path::PathBuf::from(string_value(
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
) -> Result<Option<std::path::PathBuf>, Box<dyn std::error::Error>> {
    let table = table(raw, where_)?;
    keys(table, &["file", "input_file"], where_)?;
    let Some(file) = table.get("file").or_else(|| table.get("input_file")) else {
        return Err(ConfigError(format!("{where_} requires file")).into());
    };
    Ok(Some(std::path::PathBuf::from(string_value(
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

fn function_validation(
    root: &toml::map::Map<String, Value>,
    reference: Option<std::path::PathBuf>,
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
