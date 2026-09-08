"""Path-free numerical kernel for Rust-owned vertical validation."""

import json
from math import sqrt

import numpy as np


class VerticalKernelError(ValueError):
    """Curve data cannot be compared safely."""


def evaluate(request: dict[str, object]) -> str:
    """Return deterministic JSON metrics and SVG for Rust-provided arrays."""
    grid = np.asarray(request["grid"], dtype=np.float64)
    curves = json.loads(str(request["curves"]))
    comparisons = []
    for corner in ("front_left", "front_right", "rear_left", "rear_right"):
        for axis in ("x", "y", "z"):
            curve = curves[f"{corner}:{axis}"]
            measured = _measurement(curve, grid, f"measurement {corner}")
            simulated = _simulation(curve, grid, f"simulation {corner}")
            measured_base = _at_zero(grid, measured)
            simulated_base = _at_zero(grid, simulated)
            measured -= measured_base
            simulated -= simulated_base
            error = simulated - measured
            comparisons.append(
                {
                    "corner": corner,
                    "axis": axis,
                    "travel_mm": grid.tolist(),
                    "measurement_mm": measured.tolist(),
                    "simulation_mm": simulated.tolist(),
                    "measurement_baseline_mm": float(measured_base),
                    "simulation_baseline_mm": float(simulated_base),
                    "baseline_error_mm": abs(
                        float(simulated_base - measured_base)
                    ),
                    "rmse_mm": sqrt(float(np.mean(np.square(error)))),
                    "maximum_error_mm": float(np.max(np.abs(error))),
                }
            )
    return json.dumps({"comparisons": comparisons, "svg": _svg(comparisons)})


def _measurement(
    curve: dict[str, list[float]], grid: np.ndarray, source: str
) -> np.ndarray:
    travel = np.asarray(curve["measurement_travel"], dtype=np.float64)
    cycles = np.asarray(curve["measurement_cycle"], dtype=np.float64)
    values = np.asarray(curve["measurement"], dtype=np.float64)
    return np.mean(
        [
            _interpolate(
                travel[cycles == cycle], values[cycles == cycle], grid, source
            )
            for cycle in (2.0, 3.0)
        ],
        axis=0,
    )


def _simulation(
    curve: dict[str, list[float]], grid: np.ndarray, source: str
) -> np.ndarray:
    travel = np.asarray(curve["simulation_travel"], dtype=np.float64)
    values = np.asarray(curve["simulation"], dtype=np.float64)
    return _mean_monotonic(travel, values, grid, source)


def _mean_monotonic(
    x: np.ndarray, y: np.ndarray, grid: np.ndarray, source: str
) -> np.ndarray:
    finite = np.isfinite(x) & np.isfinite(y)
    x, y = x[finite], y[finite]
    direction = np.sign(np.diff(x))
    direction[direction == 0] = np.nan
    boundaries = (
        np.flatnonzero(
            np.isfinite(direction[:-1])
            & np.isfinite(direction[1:])
            & (direction[:-1] != direction[1:])
        )
        + 1
    )
    branches = [
        _interpolate(x[index], y[index], grid, source)
        for index in np.split(np.arange(len(x)), boundaries)
        if len(index) >= 2
        and np.min(x[index]) <= grid[0]
        and np.max(x[index]) >= grid[-1]
    ]
    if not branches:
        raise VerticalKernelError(
            f"{source} has no monotonic branch covering requested window"
        )
    return np.mean(branches, axis=0)


def _interpolate(
    x: np.ndarray, y: np.ndarray, grid: np.ndarray, source: str
) -> np.ndarray:
    finite = np.isfinite(x) & np.isfinite(y)
    x, y = x[finite], y[finite]
    if len(x) < 2:
        raise VerticalKernelError(f"{source} has insufficient finite samples")
    order = np.argsort(x, kind="stable")
    x, y = x[order], y[order]
    unique, starts, counts = np.unique(x, return_index=True, return_counts=True)
    values = np.add.reduceat(y, starts) / counts
    if unique[0] > grid[0] or unique[-1] < grid[-1]:
        raise VerticalKernelError(f"{source} does not cover requested window")
    return np.interp(grid, unique, values)


def _at_zero(grid: np.ndarray, values: np.ndarray) -> float:
    return float(np.interp(0.0, grid, values))


def _svg(comparisons: list[dict[str, object]]) -> str:
    lines = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="900" height="650">'
        "<title>Vertical trajectories</title>"
    ]
    for index, item in enumerate(comparisons):
        x0, y0 = (index % 3) * 300, (index // 3) * 160
        lines.append(
            f'<text x="{x0 + 8}" y="{y0 + 16}">'
            f"{item['corner']} {item['axis']}</text>"
        )
        x_values = np.asarray(item["travel_mm"], dtype=np.float64)
        y_values = np.concatenate(
            [
                np.asarray(item["measurement_mm"], dtype=np.float64),
                np.asarray(item["simulation_mm"], dtype=np.float64),
            ]
        )
        x_min, x_max = _bounds(x_values)
        y_min, y_max = _bounds(y_values)
        for name, color in (
            ("measurement_mm", "#167d91"),
            ("simulation_mm", "#d2603a"),
        ):
            values = item[name]
            points = " ".join(
                _point(x0, y0, x, value, x_min, x_max, y_min, y_max)
                for x, value in zip(x_values, values, strict=True)
            )
            lines.append(
                f'<polyline fill="none" stroke="{color}" points="{points}"/>'
            )
    return "".join(lines) + "</svg>"


def _bounds(values: np.ndarray) -> tuple[float, float]:
    minimum = float(np.min(values))
    maximum = float(np.max(values))
    if minimum == maximum:
        return minimum - 1.0, maximum + 1.0
    margin = (maximum - minimum) * 0.05
    return minimum - margin, maximum + margin


def _point(
    x0: int,
    y0: int,
    x: float,
    y: float,
    x_min: float,
    x_max: float,
    y_min: float,
    y_max: float,
) -> str:
    scaled_x = x0 + 20 + (float(x) - x_min) / (x_max - x_min) * 260
    scaled_y = y0 + 140 - (float(y) - y_min) / (y_max - y_min) * 110
    return f"{scaled_x:.3f},{scaled_y:.3f}"
