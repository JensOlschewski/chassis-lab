//! Reading solver solution JSON and rendering it as text.

use std::fmt::Write;

use anyhow::{Result, bail};
use clab_core::solution::{matrix, time};
use serde_json::Value;

const ZERO_THRESHOLD: f64 = 0.5e-12;
/// `v_hat` holds 6 rows (3 linear, 3 angular velocity) per body.
const DOF_PER_BODY: usize = 6;

/// The time vector and the `v_hat` matrix, checked to have 6 rows per body.
fn velocities(solution: &Value) -> Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let time = time(solution)?;
    let v_hat = matrix(&solution["v_hat"], "v_hat")?;
    if v_hat.len() % DOF_PER_BODY != 0 || v_hat.iter().any(|row| row.len() != time.len()) {
        bail!("`v_hat` does not match `time` and 6 rows per body");
    }
    Ok((time, v_hat))
}

/// One block per time step and one line per body, in kinemagic's text style.
pub(crate) fn render_text(solution: &Value) -> Result<String> {
    let (time, v_hat) = velocities(solution)?;
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

/// What `--include` writes into a solution, and the keys it adds.
const SECTIONS: [(&str, &[&str]); 3] = [
    ("bodies", &["bodies"]),
    ("connectors", &["joints", "bushings"]),
    ("suspension", &["characteristics"]),
];

/// Steps, time span, counts, the sections present and the peak velocity of each body.
pub(crate) fn render_summary(solution: &Value) -> Result<String> {
    let (time, v_hat) = velocities(solution)?;
    let mut out = String::new();
    writeln!(out, "steps: {}", time.len())?;
    if let (Some(first), Some(last)) = (time.first(), time.last()) {
        writeln!(out, "time: {first:.4} to {last:.4}")?;
    }
    let bodies = v_hat.len() / DOF_PER_BODY;
    writeln!(out, "bodies: {bodies}")?;
    if let Some(joints) = solution["joints"].as_object() {
        let count = |kind: &str| {
            joints
                .get(kind)
                .and_then(Value::as_array)
                .map_or(0, Vec::len)
        };
        writeln!(
            out,
            "joints: {} primary, {} secondary",
            count("primary"),
            count("secondary")
        )?;
    }
    let included: Vec<_> = SECTIONS
        .iter()
        .filter(|(_, keys)| keys.iter().any(|key| !solution[key].is_null()))
        .map(|(name, _)| *name)
        .collect();
    writeln!(
        out,
        "included: {}",
        if included.is_empty() {
            "none".to_owned()
        } else {
            included.join(", ")
        }
    )?;
    if !included.contains(&"bodies") {
        writeln!(
            out,
            "note: no body positions; solve with --include bodies to view the motion"
        )?;
    }

    // `v_hat` lists the bodies after ground, so body `n` has bodyID `n`.
    let name = |id: usize| {
        solution["bodies"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|body| body["bodyID"].as_u64() == Some(id as u64))
            .and_then(|body| body["name"].as_str())
            .map_or_else(|| format!("body {id}"), str::to_owned)
    };
    let names: Vec<String> = (1..=bodies).map(name).collect();
    let width = names
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max("body".len());
    writeln!(
        out,
        "{:<width$}  {:>12}  {:>12}",
        "body", "peak linear", "peak angular"
    )?;
    for (name, rows) in names.iter().zip(v_hat.chunks(DOF_PER_BODY)) {
        let [linear, angular] = [&rows[..3], &rows[3..]].map(|axes| {
            (0..time.len())
                .map(|step| axes.iter().map(|row| row[step].powi(2)).sum::<f64>().sqrt())
                .fold(0.0, f64::max)
        });
        writeln!(out, "{name:<width$}  {linear:>12.4}  {angular:>12.4}")?;
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

    fn summarized() -> Value {
        json!({
            "time": [0.0, 0.5],
            "v_hat": [
                [3.0, 0.0], [4.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 0.0], [0.0, 2.0],
            ],
        })
    }

    #[test]
    fn summary_without_bodies_asks_for_them() {
        let text = render_summary(&summarized()).unwrap();
        assert!(
            text.contains("steps: 2\ntime: 0.0000 to 0.5000\nbodies: 1\n"),
            "{text}"
        );
        assert!(text.contains("included: none"), "{text}");
        assert!(text.contains("--include bodies"), "{text}");
        assert!(
            text.contains("body 1  ") && text.contains("5.0000"),
            "{text}"
        );
        assert!(!text.contains("joints:"), "{text}");
    }

    #[test]
    fn summary_lists_sections_names_and_joints() {
        let mut solution = summarized();
        solution["bodies"] = json!([{ "name": "Link", "bodyID": 1 }]);
        solution["joints"] = json!({ "primary": [{}, {}], "secondary": [{}] });
        solution["characteristics"] = json!({});
        let text = render_summary(&solution).unwrap();
        assert!(text.contains("joints: 2 primary, 1 secondary"), "{text}");
        assert!(
            text.contains("included: bodies, connectors, suspension"),
            "{text}"
        );
        assert!(!text.contains("note:"), "{text}");
        assert!(text.contains("Link  "), "{text}");
        assert!(text.contains("2.0000"), "{text}");
    }

    #[test]
    fn summary_rejects_mismatched_v_hat() {
        assert!(render_summary(&json!({ "time": [0.0], "v_hat": [[1.0]] })).is_err());
    }
}
