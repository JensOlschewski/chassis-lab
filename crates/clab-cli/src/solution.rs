//! Reading solver solution JSON and rendering it as text.

use std::fmt::Write;

use anyhow::{Result, bail};
use clab_core::solution::{bodies, matrix, matrix3, time};
use serde::Serialize;
use serde_json::{Value, json};

const ZERO_THRESHOLD: f64 = 0.5e-12;
/// A body velocity has 6 rows: 3 linear, then 3 angular.
const DOF_PER_BODY: usize = 6;

/// The time vector and the velocity of each body, in model order, as 6 rows over time.
fn velocities(solution: &Value) -> Result<(Vec<f64>, Vec<(&str, Vec<Vec<f64>>)>)> {
    let time = time(solution)?;
    let bodies = bodies(solution)?
        .iter()
        .map(|(name, body)| {
            let [linear, angular] = ["linear", "angular"].map(|part| {
                let what = format!("{name}.velocity.{part}");
                matrix3(&body["velocity"][part], time.len(), &what)
            });
            Ok((name.as_str(), [linear?, angular?].concat()))
        })
        .collect::<Result<_>>()?;
    Ok((time, bodies))
}

/// One block per time step and one line per body, in kinemagic's text style.
pub(crate) fn render_text(solution: &Value) -> Result<String> {
    let (time, bodies) = velocities(solution)?;
    let mut out = String::new();
    for (step, t) in time.iter().enumerate() {
        writeln!(out, "time {}", format_number(*t))?;
        for (index, (_, rows)) in bodies.iter().enumerate() {
            let values: Vec<String> = rows.iter().map(|row| format_number(row[step])).collect();
            writeln!(out, "body {} velocity [{}]", index + 1, values.join(", "))?;
        }
    }
    Ok(out)
}

fn signal(name: String, values: impl Serialize) -> Value {
    json!({ "name": name, "unit": "", "values": values })
}

/// The solution in the shape the app plots HDF5 signals in: the time axis plus one
/// signal per body velocity component and one
/// per suspension characteristic (`toecamber.l_toe`, `wheel_points.l_wc.x`, …).
/// The solver states no units.
pub(crate) fn plot_signals(solution: &Value) -> Result<Value> {
    const COMPONENTS: [&str; DOF_PER_BODY] = ["vx", "vy", "vz", "wx", "wy", "wz"];
    let (time, bodies) = velocities(solution)?;
    let mut signals: Vec<Value> = bodies
        .into_iter()
        .flat_map(|(name, rows)| {
            COMPONENTS
                .iter()
                .zip(rows)
                .map(move |(component, values)| signal(format!("{name}.{component}"), values))
        })
        .collect();
    for (section, entries) in solution["characteristics"]
        .as_object()
        .into_iter()
        .flatten()
    {
        for (key, value) in entries.as_object().into_iter().flatten() {
            let name = format!("{section}.{key}");
            // A vector over time, or x/y/z rows; a step the solver could not compute is null.
            match value.as_array().map(|rows| rows.first()) {
                Some(Some(Value::Array(_))) => {
                    let rows = matrix(value, &name)?;
                    if rows.len() != 3 || rows.iter().any(|row| row.len() != time.len()) {
                        bail!("`{name}` is not 3 × {}", time.len());
                    }
                    signals.extend(
                        rows.iter()
                            .zip(["x", "y", "z"])
                            .map(|(row, axis)| signal(format!("{name}.{axis}"), row)),
                    );
                }
                Some(_) => {
                    let values = value.as_array().unwrap();
                    if values.len() != time.len()
                        || values.iter().any(|n| !n.is_number() && !n.is_null())
                    {
                        bail!("`{name}` does not match `time`");
                    }
                    signals.push(signal(name, values));
                }
                None => {}
            }
        }
    }
    Ok(json!({ "axis": time, "signals": signals }))
}

/// Steps, time span, counts and the peak velocity of each body.
pub(crate) fn render_summary(solution: &Value) -> Result<String> {
    let (time, bodies) = velocities(solution)?;
    let mut out = String::new();
    writeln!(out, "steps: {}", time.len())?;
    if let (Some(first), Some(last)) = (time.first(), time.last()) {
        writeln!(out, "time: {first:.4} to {last:.4}")?;
    }
    writeln!(out, "bodies: {}", bodies.len())?;
    if let Some(joints) = solution["joints"].as_object() {
        let count = |group: &str| joints.values().filter(|j| j["group"] == group).count();
        writeln!(
            out,
            "joints: {} primary, {} secondary",
            count("primary"),
            count("secondary")
        )?;
    }

    let width = bodies
        .iter()
        .map(|(name, _)| name.len())
        .max()
        .unwrap_or(0)
        .max("body".len());
    writeln!(
        out,
        "{:<width$}  {:>12}  {:>12}",
        "body", "peak linear", "peak angular"
    )?;
    for (name, rows) in &bodies {
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

    fn rows(values: [[f64; 2]; 3]) -> Value {
        json!(values)
    }

    fn body(linear: [[f64; 2]; 3], angular: [[f64; 2]; 3]) -> Value {
        json!({ "velocity": { "linear": rows(linear), "angular": rows(angular) } })
    }

    fn solution() -> Value {
        json!({
            "format": 2,
            "time": [0.0, 0.5],
            "bodies": { "Link": body(
                [[1.0, 2.0], [-1e-15, 0.0], [0.0, 0.0]],
                [[0.0, 0.0], [0.0, 0.0], [0.0, 3.0]],
            ) },
        })
    }

    #[test]
    fn renders_one_line_per_body_and_step() {
        assert_eq!(
            render_text(&solution()).unwrap(),
            "time 0.000000000000\n\
             body 1 velocity [1.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000]\n\
             time 0.500000000000\n\
             body 1 velocity [2.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 0.000000000000, 3.000000000000]\n"
        );
    }

    #[test]
    fn plot_signals_are_named_per_body_and_component() {
        let signals = plot_signals(&solution()).unwrap();
        assert_eq!(signals["axis"], json!([0.0, 0.5]));
        assert_eq!(signals["signals"].as_array().unwrap().len(), 6);
        assert_eq!(signals["signals"][0]["name"], "Link.vx");
        assert_eq!(signals["signals"][5]["name"], "Link.wz");
        assert_eq!(signals["signals"][5]["values"], json!([0.0, 3.0]));
    }

    #[test]
    fn plot_signals_include_suspension_characteristics() {
        let mut solution = solution();
        solution["characteristics"] = json!({
            "toecamber": { "l_toe": [0.2, 0.3] },
            "roll": { "roll_rate": [null, 1.0] },
            "wheel_points": { "l_wc": [[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]] },
        });
        let signals = plot_signals(&solution).unwrap()["signals"].clone();
        let named = |name: &str| {
            signals
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["name"] == name)
        };
        assert_eq!(
            named("toecamber.l_toe").unwrap()["values"],
            json!([0.2, 0.3])
        );
        assert_eq!(
            named("roll.roll_rate").unwrap()["values"],
            json!([null, 1.0])
        );
        assert_eq!(
            named("wheel_points.l_wc.y").unwrap()["values"],
            json!([3.0, 4.0])
        );

        let mut short = solution.clone();
        short["characteristics"]["toecamber"]["l_toe"] = json!([0.2]);
        assert!(plot_signals(&short).is_err());
    }

    #[test]
    fn rejects_the_old_format_and_mismatched_velocity() {
        let old = json!({ "time": [0.0], "v_hat": [[1.0]] });
        assert!(
            render_text(&old)
                .unwrap_err()
                .to_string()
                .contains("format 1")
        );
        let mut short = solution();
        short["time"] = json!([0.0]);
        assert!(render_text(&short).is_err());
        assert!(render_summary(&short).is_err());
    }

    #[test]
    fn summary_lists_steps_names_peaks_and_joints() {
        let mut solution = solution();
        solution["bodies"]["Link"] = body(
            [[3.0, 0.0], [4.0, 0.0], [0.0, 0.0]],
            [[0.0, 0.0], [0.0, 0.0], [0.0, 2.0]],
        );
        let text = render_summary(&solution).unwrap();
        assert!(
            text.contains("steps: 2\ntime: 0.0000 to 0.5000\nbodies: 1\n"),
            "{text}"
        );
        assert!(text.contains("Link  ") && text.contains("5.0000"), "{text}");
        assert!(text.contains("2.0000"), "{text}");
        assert!(!text.contains("joints:"), "{text}");

        solution["joints"] = json!({
            "A": { "group": "primary" }, "B": { "group": "primary" }, "C": { "group": "secondary" },
        });
        let text = render_summary(&solution).unwrap();
        assert!(text.contains("joints: 2 primary, 1 secondary"), "{text}");
    }
}
