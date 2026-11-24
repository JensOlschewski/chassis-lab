//! Reading solver solution JSON.

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value};

/// The solution layout the solver writes: bodies and joints addressed by name.
const FORMAT: u64 = 2;

/// Rejects a solution that is not in the current format.
pub fn check_format(solution: &Value) -> Result<()> {
    match solution["format"].as_u64() {
        Some(FORMAT) => Ok(()),
        found => bail!(
            "solution format {} is not supported (need {FORMAT}): solve the model again with `clab solve`",
            found.map_or("1".to_owned(), |format| format.to_string())
        ),
    }
}

/// The `bodies` of a solution, by name, in model order. Ground is not among them.
pub fn bodies(solution: &Value) -> Result<&Map<String, Value>> {
    check_format(solution)?;
    solution["bodies"]
        .as_object()
        .context("solution has no body data: solve the model again with `clab solve`")
}

/// A scalar history, or the three component histories of a vector.
pub struct Signal<'a> {
    pub rows: usize,
    pub values: &'a Value,
}

/// Discover time-series leaves without cloning their samples or building viewer frames.
pub fn catalog(solution: &Value) -> Result<std::collections::BTreeMap<String, Signal<'_>>> {
    use crate::config::expr::{COMPONENTS, is_identifier};
    use std::collections::BTreeMap;
    fn walk<'a>(
        out: &mut BTreeMap<String, Signal<'a>>,
        path: String,
        v: &'a Value,
        n: usize,
    ) -> Result<()> {
        if let Some(fields) = v.as_object() {
            for (key, child) in fields {
                if is_identifier(key) && !COMPONENTS.contains(&key.as_str()) {
                    walk(out, format!("{path}.{key}"), child, n)?;
                }
            }
        } else if let Some(rows) = v.as_array() {
            if rows.is_empty() {
                return Ok(());
            }
            let history =
                |a: &[Value]| a.len() == n && a.iter().all(|x| x.is_number() || x.is_null());
            if history(rows) {
                out.insert(path, Signal { rows: 1, values: v });
            } else if rows.len() == 3
                && rows
                    .iter()
                    .all(|r| r.as_array().is_some_and(|a| history(a)))
            {
                for (axis, row) in COMPONENTS.iter().zip(rows) {
                    out.insert(
                        format!("{path}.{axis}"),
                        Signal {
                            rows: 1,
                            values: row,
                        },
                    );
                }
                out.insert(path, Signal { rows: 3, values: v });
            } else {
                bail!("solution signal `{path}` does not match the time axis ({n} samples)");
            }
        }
        Ok(())
    }
    bodies(solution)?;
    let n = time(solution)?.len();
    let mut out = BTreeMap::new();
    for key in ["time", "bodies", "joints", "characteristics"] {
        walk(&mut out, key.into(), &solution[key], n)?;
    }
    Ok(out)
}

/// A 3 × `steps` matrix: x, y and z rows over the time steps.
pub fn matrix3(value: &Value, steps: usize, what: &str) -> Result<Vec<Vec<f64>>> {
    let rows = matrix(value, what)?;
    if rows.len() != 3 || rows.iter().any(|row| row.len() != steps) {
        bail!("`{what}` is not 3 × {steps}");
    }
    Ok(rows)
}

/// Reads a JSON array of equally long number arrays (a MATLAB matrix).
pub fn matrix(value: &Value, what: &str) -> Result<Vec<Vec<f64>>> {
    let rows = value
        .as_array()
        .with_context(|| format!("solution has no `{what}` matrix"))?;
    rows.iter()
        .map(|row| {
            row.as_array()
                .with_context(|| format!("`{what}` is not a matrix"))?
                .iter()
                .map(|n| {
                    n.as_f64()
                        .with_context(|| format!("`{what}` is not numeric"))
                })
                .collect()
        })
        .collect()
}

pub fn time(solution: &Value) -> Result<Vec<f64>> {
    solution["time"]
        .as_array()
        .context("solution has no `time` vector")?
        .iter()
        .map(|n| n.as_f64().context("`time` is not numeric"))
        .collect()
}
