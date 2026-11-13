//! Reading solver solution JSON.

use anyhow::{Context, Result};
use serde_json::Value;

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
