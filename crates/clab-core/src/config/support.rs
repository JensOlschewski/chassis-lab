//! Shared plumbing used by both schema generations: generic TOML/JSON value
//! helpers, and resolving signal-mapping tables/files into [`SignalRef`] or
//! [`MeasurementSignalRef`] maps.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use toml::{Value, map::Map};

use super::{ConfigError, MeasurementSignalRef, SignalRef};

pub(crate) type ToleranceMaps = (BTreeMap<String, f64>, BTreeMap<String, f64>);

/// Resolve one domain's (and optional test's) signal mapping for one
/// `source_kind` ("simulation" or "measurement"). Every mapping table ends
/// in the same shape: `domain[.test].source_kind[.signals]`, so simulation
/// and measurement are resolved identically, just with a different
/// `source_kind` — the only asymmetry left is which one a given fixture
/// happens to define entries under.
pub(crate) fn merged_mapping(
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

pub(crate) fn merge_map(target: &mut Map<String, Value>, source: &Map<String, Value>) {
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

pub(crate) fn mapping_file_path(
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

pub(crate) fn parse_signals(
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
pub(crate) fn parse_measurement_signals(
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

pub(crate) fn parse_validation(
    raw: Option<&Value>,
    path: &Path,
    reference: Option<&Value>,
    where_: &str,
) -> Result<Option<super::ValidationConfig>, Box<dyn std::error::Error>> {
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
    Ok(Some(super::ValidationConfig {
        reference,
        tolerances: super::ValidationTolerances {
            absolute,
            relative_percent,
        },
    }))
}

pub(crate) fn parse_tolerance_table(
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
pub(crate) fn table<'a>(
    v: &'a Value,
    where_: &str,
) -> Result<&'a toml::map::Map<String, Value>, Box<dyn std::error::Error>> {
    v.as_table()
        .ok_or_else(|| ConfigError(format!("{where_} must be a table")).into())
}
pub(crate) fn keys(
    t: &toml::map::Map<String, Value>,
    allowed: &[&str],
    where_: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(k) = t.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(ConfigError(format!("unknown {where_} key: {k}")).into());
    };
    Ok(())
}
pub(crate) fn require(
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
pub(crate) fn aliased<'a>(
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
pub(crate) fn value<'a>(
    t: &'a toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<&'a Value, Box<dyn std::error::Error>> {
    t.get(key)
        .ok_or_else(|| ConfigError(format!("{where_} is missing key(s): {key}")).into())
}
pub(crate) fn string(
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
pub(crate) fn string_allow_empty(
    t: &toml::map::Map<String, Value>,
    key: &str,
    where_: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(value(t, key, where_)?
        .as_str()
        .ok_or_else(|| ConfigError(format!("{where_}.{key} must be a string")))?
        .to_owned())
}
pub(crate) fn boolean(
    t: &toml::map::Map<String, Value>,
    key: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    boolean_value(value(t, key, "[vertical.setup]")?, key)
}
pub(crate) fn boolean_value(v: &Value, key: &str) -> Result<bool, Box<dyn std::error::Error>> {
    v.as_bool()
        .ok_or_else(|| ConfigError(format!("[vertical.setup].{key} must be boolean")).into())
}
pub(crate) fn finite(v: &Value, where_: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let n = v
        .as_float()
        .or_else(|| v.as_integer().map(|i| i as f64))
        .ok_or_else(|| ConfigError(format!("{where_} must be a number")))?;
    if !n.is_finite() {
        return Err(ConfigError(format!("{where_} must be finite")).into());
    }
    Ok(n)
}
pub(crate) fn positive(v: &Value, where_: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let n = finite(v, where_)?;
    if n <= 0.0 {
        return Err(ConfigError(format!("{where_} must be positive")).into());
    }
    Ok(n)
}
pub(crate) fn expand(path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
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
pub(crate) fn resolve(
    v: &Value,
    config: &Path,
    where_: &str,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let p = expand(Path::new(string_value(v, where_)?.as_str()))?;
    if p.is_absolute() {
        Ok(p)
    } else {
        absolute(&config.parent().unwrap_or_else(|| Path::new(".")).join(p))
    }
}
pub(crate) fn string_value(v: &Value, where_: &str) -> Result<String, Box<dyn std::error::Error>> {
    v.as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ConfigError(format!("{where_} must be a non-empty string")).into())
}
pub(crate) fn absolute(p: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
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
