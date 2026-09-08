//! Typed, validated case configuration.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use toml::Value;

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
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationConfig {
    pub reference: PathBuf,
    pub tolerances: ValidationTolerances,
}
#[derive(Debug, Clone, PartialEq)]
pub struct VerticalSetup {
    pub code: String,
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
}
#[derive(Debug, Clone, PartialEq)]
pub struct CaseConfig {
    pub path: PathBuf,
    pub name: String,
    pub domain: String,
    pub variant: String,
    pub loadcase: String,
    pub simulation_hdf5: PathBuf,
    pub gravity: f64,
    pub signals: BTreeMap<String, SignalRef>,
    pub validation: Option<ValidationConfig>,
    pub vertical: Option<VerticalConfig>,
    pub svc_signal_map: Option<PathBuf>,
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
    if root.contains_key("svc") || root.contains_key("vertical") {
        parse_unified(root, &path)
    } else {
        parse_legacy(root, &path)
    }
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
    let physics = table(
        root.get("physics")
            .ok_or_else(|| ConfigError("missing required [physics] table".into()))?,
        "[physics]",
    )?;
    keys(case, &["name", "domain", "variant", "loadcase"], "[case]")?;
    for k in ["name", "domain", "variant", "loadcase"] {
        require(case, k, "[case]")?
    }
    keys(inputs, &["simulation_hdf5"], "[inputs]")?;
    require(inputs, "simulation_hdf5", "[inputs]")?;
    keys(physics, &["gravity"], "[physics]")?;
    require(physics, "gravity", "[physics]")?;
    let signal_map = signal_source(root, path, "top level", None)?;
    let validation = parse_validation(root.get("validation"), path, None, "[validation]")?;
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", "[case]")?,
        domain: string(case, "domain", "[case]")?,
        variant: string(case, "variant", "[case]")?,
        loadcase: string(case, "loadcase", "[case]")?,
        simulation_hdf5: resolve(
            value(inputs, "simulation_hdf5", "[inputs]")?,
            path,
            "[inputs].simulation_hdf5",
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
    let physics = table(
        root.get("physics")
            .ok_or_else(|| ConfigError("missing required [physics] table".into()))?,
        "[physics]",
    )?;
    let svc = table(
        root.get("svc")
            .ok_or_else(|| ConfigError("missing required [svc] table".into()))?,
        "[svc]",
    )?;
    keys(case, &["name", "variant", "loadcase"], "[case]")?;
    for k in ["name", "variant", "loadcase"] {
        require(case, k, "[case]")?
    }
    keys(physics, &["gravity"], "[physics]")?;
    require(physics, "gravity", "[physics]")?;
    keys(svc, &["inputs", "signals", "validation"], "[svc]")?;
    let inputs = table(
        svc.get("inputs")
            .ok_or_else(|| ConfigError("missing required [inputs] table".into()))?,
        "[svc.inputs]",
    )?;
    keys(
        inputs,
        &["signal_map", "simulation_hdf5", "measurement_reference"],
        "[svc.inputs]",
    )?;
    require(inputs, "simulation_hdf5", "[svc.inputs]")?;
    let source = signal_source(svc, path, "[svc]", Some(inputs))?;
    let validation = parse_validation(
        svc.get("validation"),
        path,
        inputs.get("measurement_reference"),
        "[svc.validation]",
    )?;
    let vertical = root
        .get("vertical")
        .map(|v| parse_vertical(v, path))
        .transpose()?;
    Ok(CaseConfig {
        path: path.to_path_buf(),
        name: string(case, "name", "[case]")?,
        domain: if vertical.is_some() {
            "multi".into()
        } else {
            "svc".into()
        },
        variant: string(case, "variant", "[case]")?,
        loadcase: string(case, "loadcase", "[case]")?,
        simulation_hdf5: resolve(
            value(inputs, "simulation_hdf5", "[svc.inputs]")?,
            path,
            "[svc.inputs].simulation_hdf5",
        )?,
        gravity: positive(value(physics, "gravity", "[physics]")?, "[physics].gravity")?,
        signals: source.1,
        validation,
        vertical,
        svc_signal_map: source.0,
    })
}

fn parse_vertical(raw: &Value, path: &Path) -> Result<VerticalConfig, Box<dyn std::error::Error>> {
    let v = table(raw, "[vertical]")?;
    keys(v, &["inputs", "setup", "window", "signals"], "[vertical]")?;
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
        &["signal_map", "measurement_hdf5", "simulation_hdf5"],
        "[vertical.inputs]",
    )?;
    require(i, "measurement_hdf5", "[vertical.inputs]")?;
    require(i, "simulation_hdf5", "[vertical.inputs]")?;
    keys(
        s,
        &[
            "code",
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
            code: string(s, "code", "[vertical.setup]")?,
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
    })
}

fn signal_source(
    container: &toml::map::Map<String, Value>,
    path: &Path,
    where_: &str,
    inputs: Option<&toml::map::Map<String, Value>>,
) -> Result<SignalSource, Box<dyn std::error::Error>> {
    let source = inputs.unwrap_or(container);
    let map = source.get("signal_map");
    let inline = container.get("signals");
    if map.is_some() && inline.is_some() {
        return Err(ConfigError(format!(
            "{where_} cannot define both signal_map and inline signals"
        ))
        .into());
    };
    if let Some(m) = map {
        let p = resolve(m, path, &format!("{where_}.signal_map"))?;
        if !p.is_file() {
            return Err(format!("signal map does not exist: {}", p.display()).into());
        };
        let t: toml::Table = fs::read_to_string(&p)?
            .parse()
            .map_err(|e| ConfigError(format!("invalid signal map TOML in {}: {e}", p.display())))?;
        let t = &t;
        keys(t, &["signals"], &format!("signal map {}", p.display()))?;
        return Ok((
            Some(p),
            parse_signals(t.get("signals"), "signal map signals")?,
        ));
    }
    if let Some(v) = inline {
        return Ok((None, parse_signals(Some(v), &format!("{where_}.signals"))?));
    }
    Err(ConfigError(format!("{where_} must define signal_map or inline signals")).into())
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
    let tol = table(
        value(t, "tolerances", where_)?,
        &format!("{where_}.tolerances"),
    )?;
    keys(
        tol,
        &["absolute", "relative_percent"],
        &format!("{where_}.tolerances"),
    )?;
    let absolute = parse_tolerances(tol.get("absolute"), "absolute")?;
    let relative_percent = parse_tolerances(tol.get("relative_percent"), "relative_percent")?;
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
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nvariant='v'\nloadcase='l'\n[inputs]\nsimulation_hdf5='x.h5'\n[physics]\ngravity=9.8\n[signals.x]\nentity='e'\ncomponent='c'\nunit='u'\n").unwrap();
        let c = load_config(&p).unwrap();
        assert_eq!(c.signals["x"].entity, "e");
        assert_eq!(c.simulation_hdf5, d.path().join("x.h5"));
    }
    #[test]
    fn rejects_unknown_and_bad_tolerance() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("case.toml");
        fs::write(&p,"[case]\nname='n'\ndomain='svc'\nvariant='v'\nloadcase='l'\nextra=1\n[inputs]\nsimulation_hdf5='x'\n[physics]\ngravity=9.8\n").unwrap();
        assert!(load_config(&p).unwrap_err().to_string().contains("unknown"));
    }
}
