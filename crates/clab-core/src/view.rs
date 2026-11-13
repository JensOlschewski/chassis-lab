//! Scene frames built from a solver solution; shared by the terminal viewer and the app.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use crate::solution;

pub type Vec3 = [f64; 3];
pub type Vec2 = [f64; 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Projection {
    Xy,
    Xz,
    Yz,
    Isometric,
}

impl Projection {
    pub fn project(self, [x, y, z]: Vec3) -> Vec2 {
        match self {
            Self::Xy => [x, y],
            Self::Xz => [x, z],
            Self::Yz => [y, z],
            Self::Isometric => [(x - y) / 2.0_f64.sqrt(), (x + y + 2.0 * z) / 6.0_f64.sqrt()],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewFrame {
    pub time: f64,
    pub bodies: Vec<SceneBody>,
    pub joints: Vec<SceneJoint>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneBody {
    pub label: String,
    pub origin: Vec3,
    pub segments: Vec<[Vec3; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneJoint {
    pub label: String,
    pub i: Vec3,
    pub j: Vec3,
}

impl ViewFrame {
    pub fn visit_positions(&self, mut visit: impl FnMut(Vec3)) {
        for segment in self.bodies.iter().flat_map(|body| &body.segments) {
            segment.iter().for_each(|position| visit(*position));
        }
        for joint in &self.joints {
            visit(joint.i);
            visit(joint.j);
        }
    }
}

/// What to draw: the bodies (by name) and the points on them.
/// Without a layout everything is drawn.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewLayout {
    bodies: BTreeMap<String, BodyLayout>,
    #[serde(default)]
    joints: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyLayout {
    points: Vec<String>,
    #[serde(default)]
    shape: Shape,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Shape {
    /// Joins the points in the listed order.
    #[default]
    Chain,
    /// Joins every point to the body's centre of mass.
    Star,
    /// One segment between exactly two points.
    Line,
}

impl ViewLayout {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read view file {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("invalid view file {}", path.display()))
    }

    pub fn shows_joints(&self) -> bool {
        self.joints
    }
}

fn body_segments(origin: Vec3, points: &[Vec3], shape: Shape) -> Vec<[Vec3; 2]> {
    match (shape, points) {
        (Shape::Star, _) | (Shape::Chain, [_]) => {
            points.iter().map(|point| [origin, *point]).collect()
        }
        _ => points.windows(2).map(|pair| [pair[0], pair[1]]).collect(),
    }
}

/// A named point track: 3 rows (x, y, z) over all time steps.
struct Track(Vec<Vec<f64>>);

impl Track {
    fn parse(value: &Value, steps: usize, what: &str) -> Result<Self> {
        let rows = solution::matrix(value, what)?;
        if rows.len() != 3 || rows.iter().any(|row| row.len() != steps) {
            bail!("`{what}` is not 3 × {steps}");
        }
        Ok(Self(rows))
    }

    fn at(&self, step: usize) -> Vec3 {
        [self.0[0][step], self.0[1][step], self.0[2][step]]
    }
}

struct BodyTracks {
    label: String,
    shape: Shape,
    origin: Track,
    points: Vec<Track>,
}

struct JointTracks {
    label: String,
    i: Track,
    j: Track,
}

/// Builds one frame per time step from the `bodies` and `joints` of a solution
/// that was solved with `--include bodies,connectors`.
pub fn frames_from_solution(
    solution: &Value,
    layout: Option<&ViewLayout>,
) -> Result<Vec<ViewFrame>> {
    let time = solution::time(solution)?;
    let steps = time.len();

    let bodies = solution["bodies"]
        .as_array()
        .context("solution has no body data: solve with `--include bodies,connectors`")?;
    let mut tracks = Vec::new();
    // Ground never moves; its joint ends are drawn with the joints.
    for body in bodies
        .iter()
        .filter(|body| body["bodyID"].as_i64() != Some(0))
    {
        let label = body["name"].as_str().unwrap_or("body").to_owned();
        let spec = match layout {
            None => None,
            Some(layout) => match layout.bodies.get(&label) {
                Some(spec) => Some(spec),
                None => continue,
            },
        };
        let available = body["points"].as_object();
        let names: Vec<&str> = match spec {
            Some(spec) => spec.points.iter().map(String::as_str).collect(),
            None => available
                .into_iter()
                .flatten()
                .map(|(name, _)| name.as_str())
                .collect(),
        };
        let shape = spec.map_or(Shape::Chain, |spec| spec.shape);
        if shape == Shape::Line && names.len() != 2 {
            bail!("body `{label}`: shape `line` needs exactly 2 points");
        }
        let points = names
            .iter()
            .map(|name| {
                let point = available
                    .and_then(|points| points.get(*name))
                    .with_context(|| {
                        let known: Vec<_> = available.into_iter().flat_map(|p| p.keys()).collect();
                        format!("body `{label}` has no point `{name}`; available: {known:?}")
                    })?;
                Track::parse(&point["position"], steps, &format!("{label}.{name}"))
            })
            .collect::<Result<_>>()?;
        let origin = Track::parse(&body["cm"]["position"], steps, &format!("{label}.cm"))?;
        tracks.push(BodyTracks {
            label,
            shape,
            origin,
            points,
        });
    }
    if let Some(layout) = layout
        && let Some(unknown) = layout
            .bodies
            .keys()
            .find(|name| !tracks.iter().any(|body| &body.label == *name))
    {
        let known: Vec<_> = bodies
            .iter()
            .filter_map(|body| body["name"].as_str())
            .collect();
        bail!("view file: no body `{unknown}`; available: {known:?}");
    }
    let bodies = tracks;

    let mut joints = Vec::new();
    let show_joints = layout.is_none_or(ViewLayout::shows_joints);
    for kind in ["primary", "secondary"] {
        for joint in solution["joints"][kind]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|_| show_joints)
        {
            let label = joint["name"].as_str().unwrap_or("joint").to_owned();
            let (i, j) = (&joint["i"]["position"], &joint["j"]["position"]);
            if i.is_null() || j.is_null() {
                continue;
            }
            joints.push(JointTracks {
                i: Track::parse(i, steps, &format!("{label}.i"))?,
                j: Track::parse(j, steps, &format!("{label}.j"))?,
                label,
            });
        }
    }

    Ok(time
        .into_iter()
        .enumerate()
        .map(|(step, time)| ViewFrame {
            time,
            bodies: bodies
                .iter()
                .map(|body| {
                    let origin = body.origin.at(step);
                    let points: Vec<Vec3> = body.points.iter().map(|p| p.at(step)).collect();
                    SceneBody {
                        label: body.label.clone(),
                        origin,
                        segments: body_segments(origin, &points, body.shape),
                    }
                })
                .collect(),
            joints: joints
                .iter()
                .map(|joint| SceneJoint {
                    label: joint.label.clone(),
                    i: joint.i.at(step),
                    j: joint.j.at(step),
                })
                .collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn isometric_looks_from_minus_x_minus_y_plus_z() {
        // Camera at (-1, -1, 1): x to the upper right, y to the upper left, z up.
        let [x, y, z] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
            .map(|axis| Projection::Isometric.project(axis));
        assert!(x[0] > 0.0 && y[0] < 0.0 && z[0].abs() < 1e-12);
        assert!(x[1] > 0.0 && y[1] > 0.0 && z[1] > 0.0);
    }

    fn solution() -> Value {
        json!({
            "time": [0.0, 1.0],
            "bodies": [{
                "name": "Link", "bodyID": 1,
                "cm": { "position": [[0.0, 1.0], [0.0, 0.0], [0.0, 0.0]] },
                "points": {
                    "A": { "position": [[0.0, 1.0], [0.0, 0.0], [0.0, 0.0]] },
                    "B": { "position": [[2.0, 3.0], [0.0, 0.0], [1.0, 1.0]] },
                },
            }],
            "joints": { "primary": [{
                "name": "J1",
                "i": { "position": [[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]] },
                "j": { "position": [[1.0, 1.0], [0.0, 0.0], [0.0, 0.0]] },
            }], "secondary": [] },
        })
    }

    #[test]
    fn builds_one_frame_per_step() {
        let frames = frames_from_solution(&solution(), None).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].time, 1.0);
        assert_eq!(
            frames[1].bodies[0].segments,
            [[[1.0, 0.0, 0.0], [3.0, 0.0, 1.0]]]
        );
        assert_eq!(frames[1].joints[0].label, "J1");
    }

    fn parse_layout(text: &str) -> ViewLayout {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn layout_picks_bodies_points_and_shape() {
        let layout = parse_layout(r#"{"bodies": {"Link": {"points": ["B", "A"]}}}"#);
        let frames = frames_from_solution(&solution(), Some(&layout)).unwrap();
        assert_eq!(
            frames[0].bodies[0].segments,
            [[[2.0, 0.0, 1.0], [0.0, 0.0, 0.0]]]
        );
        assert!(frames[0].joints.is_empty());

        let star = parse_layout(
            r#"{"bodies": {"Link": {"points": ["A", "B"], "shape": "star"}}, "joints": true}"#,
        );
        let frames = frames_from_solution(&solution(), Some(&star)).unwrap();
        assert_eq!(frames[0].bodies[0].segments.len(), 2);
        assert_eq!(frames[0].joints.len(), 1);
    }

    #[test]
    fn layout_errors_name_the_problem() {
        let check = |text: &str, expected: &str| {
            let error = frames_from_solution(&solution(), Some(&parse_layout(text))).unwrap_err();
            assert!(error.to_string().contains(expected), "{error}");
        };
        check(r#"{"bodies": {"Nope": {"points": []}}}"#, "no body `Nope`");
        check(r#"{"bodies": {"Link": {"points": ["Z"]}}}"#, "no point `Z`");
        check(
            r#"{"bodies": {"Link": {"points": ["A"], "shape": "line"}}}"#,
            "exactly 2",
        );
        assert!(
            serde_json::from_str::<ViewLayout>(r#"{"bodies": {}, "projection": "xz"}"#).is_err()
        );
    }

    #[test]
    fn asks_for_body_data() {
        let error = frames_from_solution(&json!({ "time": [0.0] }), None).unwrap_err();
        assert!(error.to_string().contains("--include bodies,connectors"));
    }
}
