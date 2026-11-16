//! Terminal viewer for solver solutions, adapted from kinemagic's viewer.

use clab_core::view::{Projection, Vec2, ViewFrame};

mod terminal;

pub(crate) use terminal::run_viewer;

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

#[cfg(test)]
mod tests {
    use clab_core::view::frames_from_solution;
    use serde_json::json;

    use super::*;

    fn solution() -> serde_json::Value {
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
    fn bounds_cover_all_frames_and_fit_the_viewport() {
        let frames = frames_from_solution(&solution(), None).unwrap();
        let bounds = ProjectedBounds::for_frames(&frames, Projection::Xz);
        assert_eq!((bounds.min(), bounds.max()), ([0.0, 0.0], [3.0, 1.0]));
        let wide = bounds.fit_aspect(300, 100);
        assert_eq!(wide, bounds);
        let square = bounds.fit_aspect(100, 100);
        assert_eq!((square.min(), square.max()), ([0.0, -1.0], [3.0, 2.0]));
    }
}
