//! Terminal viewer for solver solutions, adapted from kinemagic's viewer.

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::solution;

mod terminal;

pub(crate) use terminal::run_viewer;

type Vec3 = [f64; 3];
type Vec2 = [f64; 2];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Projection {
    Xy,
    Xz,
    Yz,
    Isometric,
}

impl Projection {
    pub(crate) fn project(self, [x, y, z]: Vec3) -> Vec2 {
        match self {
            Self::Xy => [x, y],
            Self::Xz => [x, z],
            Self::Yz => [y, z],
            Self::Isometric => [
                (x - y) / 2.0_f64.sqrt(),
                (-x - y + 2.0 * z) / 6.0_f64.sqrt(),
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ViewFrame {
    time: f64,
    bodies: Vec<SceneBody>,
    joints: Vec<SceneJoint>,
}

#[derive(Clone, Debug, PartialEq)]
struct SceneBody {
    label: String,
    origin: Vec3,
    points: Vec<Vec3>,
    segments: Vec<[Vec3; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
struct SceneJoint {
    label: String,
    i: Vec3,
    j: Vec3,
}

impl ViewFrame {
    fn visit_positions(&self, mut visit: impl FnMut(Vec3)) {
        for body in &self.bodies {
            visit(body.origin);
            body.points.iter().for_each(|point| visit(*point));
        }
        for joint in &self.joints {
            visit(joint.i);
            visit(joint.j);
        }
    }
}

/// Segments of a body: one point connects to the origin, several chain in order.
fn body_segments(origin: Vec3, points: &[Vec3]) -> Vec<[Vec3; 2]> {
    match points {
        [point] => vec![[origin, *point]],
        _ => points.windows(2).map(|pair| [pair[0], pair[1]]).collect(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ProjectedBounds {
    min: Vec2,
    max: Vec2,
}

impl ProjectedBounds {
    fn for_frames(frames: &[ViewFrame], projection: Projection) -> Self {
        let mut min = [f64::INFINITY; 2];
        let mut max = [f64::NEG_INFINITY; 2];
        for frame in frames {
            frame.visit_positions(|position| {
                let [x, y] = projection.project(position);
                min = [min[0].min(x), min[1].min(y)];
                max = [max[0].max(x), max[1].max(y)];
            });
        }
        let mut bounds = Self { min, max };
        bounds.widen_degenerate();
        bounds
    }

    /// Gives a flat or empty extent a usable size.
    fn widen_degenerate(&mut self) {
        let (width, height) = (self.width(), self.height());
        for (axis, extent, other) in [(0, width, height), (1, height, width)] {
            if extent <= f64::EPSILON {
                let half = if other > f64::EPSILON {
                    other * 0.05
                } else {
                    0.5
                };
                let center = if extent.is_finite() {
                    self.min[axis]
                } else {
                    0.0
                };
                self.min[axis] = center - half;
                self.max[axis] = center + half;
            }
        }
    }

    pub(crate) fn min(&self) -> Vec2 {
        self.min
    }

    pub(crate) fn max(&self) -> Vec2 {
        self.max
    }

    pub(crate) fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }

    pub(crate) fn height(&self) -> f64 {
        self.max[1] - self.min[1]
    }

    pub(crate) fn padded(self, fraction: f64) -> Self {
        let (dx, dy) = (self.width() * fraction, self.height() * fraction);
        Self {
            min: [self.min[0] - dx, self.min[1] - dy],
            max: [self.max[0] + dx, self.max[1] + dy],
        }
    }

    pub(crate) fn fit_aspect(self, viewport_width: u16, viewport_height: u16) -> Self {
        if viewport_width == 0 || viewport_height == 0 {
            return self;
        }
        let viewport_aspect = f64::from(viewport_width) / f64::from(viewport_height);
        let center = [
            (self.min[0] + self.max[0]) / 2.0,
            (self.min[1] + self.max[1]) / 2.0,
        ];
        if self.width() / self.height() > viewport_aspect {
            let half = self.width() / viewport_aspect / 2.0;
            Self {
                min: [self.min[0], center[1] - half],
                max: [self.max[0], center[1] + half],
            }
        } else {
            let half = self.height() * viewport_aspect / 2.0;
            Self {
                min: [center[0] - half, self.min[1]],
                max: [center[0] + half, self.max[1]],
            }
        }
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
pub(crate) fn frames_from_solution(solution: &Value) -> Result<Vec<ViewFrame>> {
    let time = solution::time(solution)?;
    let steps = time.len();

    let bodies = solution["bodies"]
        .as_array()
        .context("solution has no body data: solve with `--include bodies,connectors`")?;
    // Ground never moves; its joint ends are drawn with the joints.
    let bodies = bodies
        .iter()
        .filter(|body| body["bodyID"].as_i64() != Some(0))
        .map(|body| {
            let label = body["name"].as_str().unwrap_or("body").to_owned();
            let origin = Track::parse(&body["cm"]["position"], steps, &format!("{label}.cm"))?;
            let points = body["points"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(name, point)| {
                    Track::parse(&point["position"], steps, &format!("{label}.{name}"))
                })
                .collect::<Result<_>>()?;
            Ok(BodyTracks {
                label,
                origin,
                points,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut joints = Vec::new();
    for kind in ["primary", "secondary"] {
        for joint in solution["joints"][kind].as_array().into_iter().flatten() {
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
                        segments: body_segments(origin, &points),
                        points,
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
        let frames = frames_from_solution(&solution()).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[1].time, 1.0);
        assert_eq!(
            frames[1].bodies[0].segments,
            [[[1.0, 0.0, 0.0], [3.0, 0.0, 1.0]]]
        );
        assert_eq!(frames[1].joints[0].label, "J1");
    }

    #[test]
    fn asks_for_body_data() {
        let error = frames_from_solution(&json!({ "time": [0.0] })).unwrap_err();
        assert!(error.to_string().contains("--include bodies,connectors"));
    }

    #[test]
    fn bounds_cover_all_frames_and_fit_the_viewport() {
        let frames = frames_from_solution(&solution()).unwrap();
        let bounds = ProjectedBounds::for_frames(&frames, Projection::Xz);
        assert_eq!((bounds.min(), bounds.max()), ([0.0, 0.0], [3.0, 1.0]));
        let wide = bounds.fit_aspect(300, 100);
        assert_eq!(wide, bounds);
        let square = bounds.fit_aspect(100, 100);
        assert_eq!((square.min(), square.max()), ([0.0, -1.0], [3.0, 2.0]));
    }
}
