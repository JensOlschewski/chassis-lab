//! Typed parser for reviewed SVC reference JSON files.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use thiserror::Error;

use crate::config::MeasurementSignalRef;

#[derive(Debug, Clone, PartialEq, Error)]
#[error("{0}")]
pub struct SvcReferenceError(pub String);

#[derive(Debug, Clone, PartialEq)]
pub struct SvcCoordinateSystem {
    pub cg_x_origin: String,
    pub cg_x_positive: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvcReferenceSource {
    pub file: String,
    pub sha256: String,
    pub result: String,
    pub test_date: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvcReference {
    pub path: PathBuf,
    pub schema_version: i64,
    pub coordinate_system: SvcCoordinateSystem,
    pub sources: Vec<SvcReferenceSource>,
    pub values: BTreeMap<String, f64>,
}

pub fn load_svc_reference(
    path: impl AsRef<Path>,
    measurement_signals: &BTreeMap<String, MeasurementSignalRef>,
) -> Result<SvcReference, Box<dyn std::error::Error>> {
    let path = if path.as_ref().is_absolute() {
        path.as_ref().to_path_buf()
    } else {
        std::env::current_dir()?.join(path.as_ref())
    };
    if !path.is_file() {
        return Err(format!("SVC reference file does not exist: {}", path.display()).into());
    };

    let path = path.canonicalize()?;
    let raw: Value = serde_json::from_str(&fs::read_to_string(&path)?)
        .map_err(|e| SvcReferenceError(format!("invalid JSON in {}: {e}", path.display())))?;
    let root = obj(&raw, "SVC reference")?;

    reject(
        root,
        &[
            "schema_version",
            "source_kind",
            "case",
            "coordinate_system",
            "sources",
            "values",
        ],
        "SVC reference",
    )?;
    for key in [
        "schema_version",
        "source_kind",
        "coordinate_system",
        "sources",
        "values",
    ] {
        if !root.contains_key(key) {
            return Err(SvcReferenceError(format!("SVC reference missing key(s): {key}")).into());
        }
    }

    let version = num(root, "schema_version")?;
    if version != 1 {
        return Err(SvcReferenceError(format!(
            "unsupported SVC reference schema version: {version}"
        ))
        .into());
    };
    if nonempty(root, "source_kind", "SVC reference")? != "measurement" {
        return Err(
            SvcReferenceError("SVC reference source_kind must be measurement".into()).into(),
        );
    }

    if let Some(case) = root.get("case") {
        validate_case(case)?;
    }
    let co = parse_coordinates(root.get("coordinate_system").unwrap())?;
    let sources = parse_sources(root.get("sources").unwrap())?;
    let values = parse_values(root.get("values").unwrap(), measurement_signals)?;

    consistency(&values)?;
    Ok(SvcReference {
        path,
        schema_version: version,
        coordinate_system: co,
        sources,
        values,
    })
}

fn validate_case(v: &Value) -> Result<(), Box<dyn std::error::Error>> {
    let t = obj(v, "case")?;
    exact(t, &["name", "loadcase"], "case")?;
    nonempty(t, "name", "case")?;
    nonempty(t, "loadcase", "case")?;
    Ok(())
}

fn parse_coordinates(v: &Value) -> Result<SvcCoordinateSystem, Box<dyn std::error::Error>> {
    let t = obj(v, "coordinate_system")?;
    exact(t, &["cg_x_origin", "cg_x_positive"], "coordinate_system")?;
    let o = nonempty(t, "cg_x_origin", "coordinate_system")?;
    let p = nonempty(t, "cg_x_positive", "coordinate_system")?;

    if o != "front_axle_wheel_center" || p != "rearward" {
        return Err(SvcReferenceError("SVC CG-X coordinates must use front_axle_wheel_center origin and rearward-positive direction".into()).into());
    };

    Ok(SvcCoordinateSystem {
        cg_x_origin: o,
        cg_x_positive: p,
    })
}

fn parse_sources(v: &Value) -> Result<Vec<SvcReferenceSource>, Box<dyn std::error::Error>> {
    let a = v
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| SvcReferenceError("sources must be a non-empty array".into()))?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();

    for (i, v) in a.iter().enumerate() {
        let t = obj(v, &format!("sources[{i}]"))?;
        reject(
            t,
            &["file", "sha256", "test_date", "result"],
            &format!("sources[{i}]"),
        )?;

        for k in ["file", "sha256", "result"] {
            if !t.contains_key(k) {
                return Err(SvcReferenceError(format!("sources[{i}] missing key(s): {k}")).into());
            }
        }

        let file = nonempty(t, "file", &format!("sources[{i}]"))?;

        if !seen.insert(file.clone()) {
            return Err(SvcReferenceError(format!("duplicate reference source: {file}")).into());
        };

        let sha = nonempty(t, "sha256", &format!("sources[{i}]"))?;

        if sha.len() != 64
            || !sha
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return Err(SvcReferenceError(format!(
                "sources[{i}].sha256 must be lowercase SHA-256 hex"
            ))
            .into());
        };

        let date = t
            .get("test_date")
            .map(|v| nonempty_value(v, &format!("sources[{i}].test_date")))
            .transpose()?;

        if let Some(d) = &date {
            let parts: Vec<_> = d.split('-').collect();
            let valid = parts.len() == 3
                && parts[0].len() == 4
                && parts[1].len() == 2
                && parts[2].len() == 2
                && parts
                    .iter()
                    .all(|part| part.chars().all(|c| c.is_ascii_digit()))
                && valid_calendar_date(&parts);

            if !valid {
                return Err(SvcReferenceError(format!(
                    "sources[{i}].test_date must use YYYY-MM-DD"
                ))
                .into());
            }
        }

        out.push(SvcReferenceSource {
            file,
            sha256: sha,
            result: nonempty(t, "result", &format!("sources[{i}]"))?,
            test_date: date,
        })
    }

    Ok(out)
}

fn valid_calendar_date(parts: &[&str]) -> bool {
    let Ok(year) = parts[0].parse::<u32>() else {
        return false;
    };

    let Ok(month) = parts[1].parse::<u32>() else {
        return false;
    };

    let Ok(day) = parts[2].parse::<u32>() else {
        return false;
    };

    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year % 400 == 0 || (year % 4 == 0 && year % 100 != 0) => 29,
        2 => 28,
        _ => return false,
    };

    (1..=days).contains(&day)
}

/// Translate raw JSON keys to canonical metric names via
/// `[measurement.svc.signals]` (see `clab_core::config`) — the reference
/// file can name its keys however it wants; only the mapping decides what
/// they mean. A JSON key with no mapping entry is an error, not a silent
/// drop: a renamed/misspelled key would otherwise vanish instead of failing.
fn parse_values(
    v: &Value,
    measurement_signals: &BTreeMap<String, MeasurementSignalRef>,
) -> Result<BTreeMap<String, f64>, Box<dyn std::error::Error>> {
    let t = obj(v, "values")?;

    if t.is_empty() {
        return Err(SvcReferenceError("values must define at least one metric".into()).into());
    };

    let raw_to_canonical: BTreeMap<&str, &str> = measurement_signals
        .iter()
        .map(|(canonical, reference)| (reference.entity.as_str(), canonical.as_str()))
        .collect();

    t.iter()
        .map(|(raw_key, v)| {
            let canonical = raw_to_canonical.get(raw_key.as_str()).ok_or_else(|| {
                SvcReferenceError(format!("unknown SVC reference key: {raw_key}"))
            })?;
            Ok((
                (*canonical).to_string(),
                finite(v, &format!("reference metric '{raw_key}'"))?,
            ))
        })
        .collect()
}

fn consistency(v: &BTreeMap<String, f64>) -> Result<(), Box<dyn std::error::Error>> {
    check(
        v,
        &["front_left_mass_kg", "front_right_mass_kg"],
        "front_mass_kg",
        1e-6,
    )?;

    check(
        v,
        &["rear_left_mass_kg", "rear_right_mass_kg"],
        "rear_mass_kg",
        1e-6,
    )?;

    check(v, &["front_mass_kg", "rear_mass_kg"], "total_mass_kg", 1e-6)?;

    if v.contains_key("total_mass_kg") && v["total_mass_kg"] <= 0.0 {
        return Err(SvcReferenceError("total_mass_kg must be positive".into()).into());
    };

    if [
        "front_mass_kg",
        "rear_mass_kg",
        "total_mass_kg",
        "front_distribution_percent",
        "rear_distribution_percent",
    ]
    .iter()
    .all(|k| v.contains_key(*k))
    {
        for a in ["front", "rear"] {
            if (v[&format!("{a}_distribution_percent")]
                - 100.0 * v[&format!("{a}_mass_kg")] / v["total_mass_kg"])
                .abs()
                > 0.051
            {
                return Err(SvcReferenceError(format!(
                    "{a}_distribution_percent is inconsistent with masses"
                ))
                .into());
            }
        }
    }

    if [
        "rear_mass_kg",
        "total_mass_kg",
        "wheelbase_mm",
        "cg_x_from_front_axle_mm",
    ]
    .iter()
    .all(|k| v.contains_key(*k))
        && (v["cg_x_from_front_axle_mm"]
            - v["rear_mass_kg"] / v["total_mass_kg"] * v["wheelbase_mm"])
            .abs()
            > 0.001
    {
        return Err(SvcReferenceError(
            "cg_x_from_front_axle_mm is inconsistent with mass distribution".into(),
        )
        .into());
    }

    Ok(())
}

fn check(
    v: &BTreeMap<String, f64>,
    parts: &[&str],
    result: &str,
    tol: f64,
) -> Result<(), Box<dyn std::error::Error>> {
    if parts.iter().all(|k| v.contains_key(*k))
        && v.contains_key(result)
        && (parts.iter().map(|k| v[*k]).sum::<f64>() - v[result]).abs() > tol
    {
        return Err(SvcReferenceError(format!("{result} is inconsistent with components")).into());
    }
    Ok(())
}

fn obj<'a>(
    v: &'a Value,
    w: &str,
) -> Result<&'a serde_json::Map<String, Value>, Box<dyn std::error::Error>> {
    v.as_object()
        .ok_or_else(|| SvcReferenceError(format!("{w} must be an object")).into())
}
fn exact(
    t: &serde_json::Map<String, Value>,
    keys: &[&str],
    w: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    reject(t, keys, w)?;

    for k in keys {
        if !t.contains_key(*k) {
            return Err(SvcReferenceError(format!("{w} missing key(s): {k}")).into());
        }
    }

    Ok(())
}
fn reject(
    t: &serde_json::Map<String, Value>,
    keys: &[&str],
    w: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(k) = t.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(SvcReferenceError(format!("unknown {w} key(s): {k}")).into());
    }

    Ok(())
}
fn nonempty(
    t: &serde_json::Map<String, Value>,
    k: &str,
    w: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    nonempty_value(t.get(k).unwrap(), &format!("{w}.{k}"))
}

fn nonempty_value(v: &Value, w: &str) -> Result<String, Box<dyn std::error::Error>> {
    v.as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| SvcReferenceError(format!("{w} must be a non-empty string")).into())
}

fn num(t: &serde_json::Map<String, Value>, k: &str) -> Result<i64, Box<dyn std::error::Error>> {
    t[k].as_i64()
        .ok_or_else(|| SvcReferenceError("schema_version must be integer".into()).into())
}

fn finite(v: &Value, w: &str) -> Result<f64, Box<dyn std::error::Error>> {
    let n = v
        .as_f64()
        .ok_or_else(|| SvcReferenceError(format!("{w} must be numeric")))?;

    if !n.is_finite() {
        return Err(SvcReferenceError(format!("{w} must be finite")).into());
    }

    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conventions::SVC_METRICS;

    fn data() -> String {
        r#"{"schema_version":1,"source_kind":"measurement","coordinate_system":{"cg_x_origin":"front_axle_wheel_center","cg_x_positive":"rearward"},"sources":[{"file":"source.txt","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","result":"reviewed","test_date":"2019-09-06"}],"values":{"total_mass_kg":1500}}"#.into()
    }

    /// Every canonical SVC metric mapped to itself, so tests can use the
    /// canonical name as the JSON key directly.
    fn identity_mapping() -> BTreeMap<String, MeasurementSignalRef> {
        SVC_METRICS
            .iter()
            .map(|&(name, unit)| {
                (
                    name.to_string(),
                    MeasurementSignalRef {
                        entity: name.to_string(),
                        unit: unit.to_string(),
                    },
                )
            })
            .collect()
    }

    #[test]
    fn parses_reference_and_rejects_unmapped_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reference.json");
        fs::write(&path, data()).unwrap();
        assert_eq!(
            load_svc_reference(&path, &identity_mapping())
                .unwrap()
                .values["total_mass_kg"],
            1500.0
        );
        fs::write(&path, data().replace("total_mass_kg", "unknown_metric")).unwrap();
        assert!(
            load_svc_reference(&path, &identity_mapping())
                .unwrap_err()
                .to_string()
                .contains("unknown")
        );
    }

    #[test]
    fn resolves_renamed_key_through_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reference.json");
        fs::write(&path, data().replace("total_mass_kg", "totalMass")).unwrap();
        let mut mapping = identity_mapping();
        mapping.insert(
            "total_mass_kg".into(),
            MeasurementSignalRef {
                entity: "totalMass".into(),
                unit: "kg".into(),
            },
        );
        assert_eq!(
            load_svc_reference(&path, &mapping).unwrap().values["total_mass_kg"],
            1500.0
        );
    }

    #[test]
    fn accepts_legacy_case_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reference.json");
        fs::write(
            &path,
            data().replace(
                "\"coordinate_system\"",
                "\"case\":{\"name\":\"n\",\"loadcase\":\"l\"},\"coordinate_system\"",
            ),
        )
        .unwrap();
        assert!(load_svc_reference(&path, &identity_mapping()).is_ok());
    }

    #[test]
    fn rejects_inconsistent_mass_values() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reference.json");
        let json = data().replace(
            "\"total_mass_kg\":1500",
            "\"front_mass_kg\":1,\"rear_mass_kg\":1,\"total_mass_kg\":3",
        );
        fs::write(&path, json).unwrap();
        assert!(
            load_svc_reference(&path, &identity_mapping())
                .unwrap_err()
                .to_string()
                .contains("total_mass")
        );
    }
}
