//! Reading solver solution JSON and rendering it as text.

use std::fmt::Write;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const ZERO_THRESHOLD: f64 = 0.5e-12;
/// `v_hat` holds 6 rows (3 linear, 3 angular velocity) per body.
const DOF_PER_BODY: usize = 6;

/// Reads a JSON array of equally long number arrays (a MATLAB matrix).
pub(crate) fn matrix(value: &Value, what: &str) -> Result<Vec<Vec<f64>>> {
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

pub(crate) fn time(solution: &Value) -> Result<Vec<f64>> {
    solution["time"]
        .as_array()
        .context("solution has no `time` vector")?
        .iter()
        .map(|n| n.as_f64().context("`time` is not numeric"))
        .collect()
}

/// One block per time step and one line per body, in kinemagic's text style.
pub(crate) fn render_text(solution: &Value) -> Result<String> {
    let time = time(solution)?;
    let v_hat = matrix(&solution["v_hat"], "v_hat")?;
    if v_hat.len() % DOF_PER_BODY != 0 || v_hat.iter().any(|row| row.len() != time.len()) {
        bail!("`v_hat` does not match `time` and 6 rows per body");
    }

    let mut out = String::new();
    for (step, t) in time.iter().enumerate() {
        writeln!(out, "time {}", format_number(*t))?;
        for (index, body) in v_hat.chunks(DOF_PER_BODY).enumerate() {
            let values: Vec<String> = body.iter().map(|row| format_number(row[step])).collect();
            writeln!(out, "body {} velocity [{}]", index + 1, values.join(", "))?;
        }
    }
    Ok(out)
}

fn format_number(value: f64) -> String {
    let value = if value.abs() < ZERO_THRESHOLD {
        0.0
    } else {
        value
    };
    format!("{value:.12}")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn renders_one_line_per_body_and_step() {
        let solution = json!({
            "time": [0.0, 0.5],
            "v_hat": [[1.0, 2.0], [-1e-15, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 3.0]],
        });
        assert_eq!(
            render_text(&solution).unwrap(),
            "time 0.000000000000\n\
             body 1 velocity [1.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000]\n\
             time 0.500000000000\n\
             body 1 velocity [2.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 3.000000000000]\n"
        );
    }

    #[test]
    fn rejects_mismatched_v_hat() {
        let solution = json!({ "time": [0.0], "v_hat": [[1.0]] });
        assert!(render_text(&solution).is_err());
    }
}
