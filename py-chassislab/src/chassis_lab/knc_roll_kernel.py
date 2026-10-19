"""Path-free numerical kernel for Rust-owned KnC roll analysis."""

import json
from typing import Any, cast

import numpy as np

from chassis_lab.knc_kernel import (
    AXLES,
    CORNERS,
    BranchOptions,
    CurveIndex,
    GradientMetric,
    KncKernelError,
    Signal,
    absolute,
    branch_ranges,
    gradient_metrics,
    index_curves,
    metric_row,
    metric_sweep,
    signal_values,
    sweep_gradient,
    with_pad_travel,
)

# Every roll metric uses the configured sweep of the body roll angle.
ROLL_ANGLE = Signal("roll_angle", "total")
SUSPENSION_ROLL_ANGLE = Signal("suspension_roll_angle", "axle")

CORNER_METRICS = (
    GradientMetric("roll_steer", "deg_per_mm", "toe", "deg/mm"),
    GradientMetric("roll_camber", "deg_per_mm", "camber", "deg/mm"),
    GradientMetric(
        "roll_steer",
        "deg_per_deg",
        "toe",
        "deg/deg",
        axis=SUSPENSION_ROLL_ANGLE,
        points="deg",
    ),
    GradientMetric(
        "roll_camber",
        "deg_per_deg",
        "camber",
        "deg/deg",
        axis=SUSPENSION_ROLL_ANGLE,
        points="deg",
    ),
    GradientMetric("roll_wheel_rate", "n_per_mm", "wheel_load", "N/mm"),
)

AXLE_METRICS = (
    GradientMetric(
        "roll_stiffness",
        "nm_per_deg",
        "roll_moment",
        "Nm/deg",
        axis=ROLL_ANGLE,
        points="deg",
    ),
)

# Each hysteresis branch of the roll center is smoothed by a least-squares
# polynomial over roll angle before both branches are averaged; stands in for
# the legacy MATLAB smoothing spline.
ROLL_CENTER_FIT_DEGREE = 3

# Preferred lateral contact position for the roll center, then the fallback.
ROLL_CENTER_LATERAL = ("center_of_pressure_y", "contact_patch_y")


def metrics(request: dict[str, object]) -> str:
    """Calculate KnC roll metrics at symmetric travel and roll-angle points.

    Response: `{"metrics": [...], "warnings": [...]}`; unavailable metrics
    keep `value: None` and carry a `reason`.
    """
    points = {
        "mm": float(cast("float", request["gradient_mm"])),
        "deg": float(cast("float", request["gradient_deg"])),
    }
    curves = index_curves(with_pad_travel(json.loads(str(request["curves"]))))
    options = BranchOptions.from_json(
        cast("str | dict[str, Any] | None", request.get("branch"))
    )
    arb_connected = json.loads(str(request.get("arb_connected") or "{}"))
    warnings: list[str] = []
    corner = gradient_metrics(
        curves, CORNER_METRICS, CORNERS, points, ROLL_ANGLE, options
    )
    axle = gradient_metrics(
        curves, AXLE_METRICS, (*AXLES, "total"), points, ROLL_ANGLE, options
    )
    results = [
        *corner,
        *_wheel_rates_without_arb(corner, arb_connected),
        *axle,
        _roll_stiffness_distribution(axle, points["deg"]),
        *_roll_center_metrics(curves, points["deg"], options, warnings),
    ]
    results.sort(key=lambda item: str(item["metric"]))
    return json.dumps({"metrics": results, "warnings": warnings})


def _wheel_rates_without_arb(
    corner: list[dict[str, object]], arb_connected: dict[str, bool | None]
) -> list[dict[str, object]]:
    """Roll wheel rate again under the `_without_arb` name.

    Only an axle configured with its anti-roll bar disconnected measures the
    wheel rate without ARB; every other axle reports the reason.
    """
    results = []
    for row in corner:
        if not str(row["metric"]).startswith("roll_wheel_rate_"):
            continue
        location = str(row["corner"])
        axle = location.split("_")[0]
        result = {
            **row,
            "metric": f"roll_wheel_rate_without_arb_{location}_n_per_mm",
        }
        connected = arb_connected.get(axle)
        if connected is None:
            reason = f"{axle}_arb_connected is not configured"
        elif connected:
            reason = f"{axle} anti-roll bar is connected"
        else:
            reason = None
        if reason is not None:
            result.update(value=None, lower=None, upper=None, reason=reason)
        results.append(result)
    return results


def _roll_stiffness_distribution(
    axle: list[dict[str, object]], half: float
) -> dict[str, object]:
    """100 * K_front / K_total; K_total falls back to K_front + K_rear."""
    stiffness = {row["corner"]: row["value"] for row in axle}
    result = metric_row(
        "front",
        "roll_stiffness_distribution_front_pct",
        "%",
        "deg",
        half,
    )
    front, rear, total = (
        cast("float | None", stiffness.get(location))
        for location in ("front", "rear", "total")
    )
    if total is None and front is not None and rear is not None:
        total = front + rear
    if front is None or total is None:
        result["reason"] = "front and total roll stiffness unavailable"
    elif total == 0.0:
        result["reason"] = "total roll stiffness is zero"
    else:
        result["value"] = 100.0 * front / total
    return result


def _roll_center_metrics(
    curves: CurveIndex,
    half: float,
    options: BranchOptions,
    warnings: list[str],
) -> list[dict[str, object]]:
    """Roll-center height at 0 deg and its secant changes over roll angle."""
    results = []
    for axle in AXLES:
        rows = [
            metric_row(
                axle, f"roll_center_height_{axle}_mm", "mm", "deg", half
            ),
            metric_row(
                axle,
                f"roll_center_height_change_{axle}_mm_per_deg",
                "mm/deg",
                "deg",
                half,
            ),
            metric_row(
                axle,
                f"roll_center_lateral_migration_{axle}_mm_per_deg",
                "mm/deg",
                "deg",
                half,
            ),
        ]
        try:
            lateral = _lateral_signal(curves, axle, warnings)
            height, position = _roll_center_mid_line(
                curves, axle, lateral, half, options
            )
        except KncKernelError as error:
            for row in rows:
                row["reason"] = str(error)
        else:
            rows[0].update(value=height[1], lower=height[0], upper=height[2])
            for row, line in ((rows[1], height), (rows[2], position)):
                row.update(
                    value=(line[2] - line[0]) / (2.0 * half),
                    lower=line[0],
                    upper=line[2],
                )
            for row in rows:
                row["lateral_signal"] = lateral
        results.extend(rows)
    return results


def _lateral_signal(curves: CurveIndex, axle: str, warnings: list[str]) -> str:
    sides = (f"{axle}_left", f"{axle}_right")
    available = [
        signal
        for signal in ROLL_CENTER_LATERAL
        if all((side, signal) in curves for side in sides)
    ]
    if not available:
        raise KncKernelError(
            f"missing canonical signal {sides[0]}_center_of_pressure_y or "
            f"{sides[0]}_contact_patch_y"
        )
    if available[0] != ROLL_CENTER_LATERAL[0]:
        warnings.append(
            f"{axle} roll center uses contact_patch_y; "
            "center_of_pressure_y is not mapped"
        )
    return available[0]


def _roll_center_mid_line(
    curves: CurveIndex,
    axle: str,
    lateral: str,
    half: float,
    options: BranchOptions,
) -> tuple[np.ndarray, np.ndarray]:
    """Roll-center height and lateral position at roll -half, 0, +half deg.

    The configured sweep and its adjacent opposite sweep (one hysteresis
    loop) are each smoothed over roll angle, then averaged into the mid line.
    """
    left, right = f"{axle}_left", f"{axle}_right"
    samples = {
        "roll": signal_values(curves, "total", "roll_angle"),
        "y_left": absolute(curves, left, lateral),
        "z_left": absolute(curves, left, "wheel_pad_z"),
        "y_right": absolute(curves, right, lateral),
        "z_right": absolute(curves, right, "wheel_pad_z"),
    }
    if len({len(values) for values in samples.values()}) != 1:
        raise KncKernelError(f"{axle} roll-center signal lengths differ")
    finite = np.logical_and.reduce(
        [np.isfinite(values) for values in samples.values()]
    )
    samples = {name: values[finite] for name, values in samples.items()}
    grid = np.array([-half, 0.0, half])
    branches = []
    for sweep in _hysteresis_loop(samples["roll"], options):
        height, position, roll = _branch_roll_center(samples, sweep, options)
        branches.append(
            [
                _smooth(roll, height, grid, f"{axle} roll center"),
                _smooth(roll, position, grid, f"{axle} roll center"),
            ]
        )
    height, position = np.mean(branches, axis=0)
    return height, position


def _hysteresis_loop(
    roll: np.ndarray, options: BranchOptions
) -> tuple[slice, slice]:
    """Configured sweep and the opposite sweep before it (else after it)."""
    selected = metric_sweep(roll, "total_roll_angle", options)
    sweeps = [sweep for sweep, _ in branch_ranges(roll, options)]
    position = sweeps.index(selected)
    neighbours = sweeps[position - 1 : position] or sweeps[position + 1 :][:1]
    if not neighbours:
        raise KncKernelError("total_roll_angle has no opposite sweep")
    return selected, neighbours[0]


def _branch_roll_center(
    samples: dict[str, np.ndarray], sweep: slice, options: BranchOptions
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Roll center of one sweep; turnaround edges trimmed like the rates."""
    slopes = []
    for side in ("left", "right"):
        result = sweep_gradient(
            samples["roll"],
            samples[f"y_{side}"],
            samples[f"z_{side}"],
            sweep,
            options,
        )
        if result is None:
            raise KncKernelError("roll-center sweep has too few samples")
        _, slope, _, _, masked = result
        slope[masked] = np.nan
        slopes.append(slope)
    height, position = roll_center(
        *slopes,
        samples["y_left"][sweep],
        samples["z_left"][sweep],
        samples["y_right"][sweep],
        samples["z_right"][sweep],
        np.radians(samples["roll"][sweep]),
    )
    return height, position, samples["roll"][sweep]


def roll_center(
    slope_left: np.ndarray,
    slope_right: np.ndarray,
    y_left: np.ndarray,
    z_left: np.ndarray,
    y_right: np.ndarray,
    z_right: np.ndarray,
    roll: np.ndarray,
) -> tuple[np.ndarray, np.ndarray]:
    """Geometric roll center from both contact paths (legacy MATLAB method).

    `slope_*` is d(lateral contact position)/d(wheel pad z) of each side, y/z
    the absolute contact positions, `roll` the body roll angle in rad. The
    contact-path angles alpha (left) and beta (right) close a triangle over
    the contact-point base c; by the law of sines the left side is
    a = c / sin(gamma) * sin(alpha). Height is a * sin(beta); the lateral
    position lies rc_z / tan(alpha) from the left contact point, towards the
    right one.
    """
    with np.errstate(divide="ignore", invalid="ignore"):
        alpha = np.abs(np.arctan(slope_left)) + roll
        beta = np.abs(np.arctan(slope_right)) - roll
        gamma = np.pi - alpha - beta
        base = np.hypot(y_right - y_left, z_left - z_right)
        height = base / np.sin(gamma) * np.sin(alpha) * np.sin(beta)
        position = y_left + np.sign(y_right - y_left) * height / np.tan(alpha)
    return height, position


def _smooth(
    roll: np.ndarray, values: np.ndarray, grid: np.ndarray, source: str
) -> np.ndarray:
    """Polynomial least-squares fit over roll angle, evaluated on `grid`."""
    finite = np.isfinite(roll) & np.isfinite(values)
    roll, values = roll[finite], values[finite]
    if len(roll) <= ROLL_CENTER_FIT_DEGREE:
        raise KncKernelError(f"{source} has insufficient finite samples")
    if roll.min() > grid[0] or roll.max() < grid[-1]:
        raise KncKernelError(
            f"{source} does not cover roll_angle {grid[0]:g}..{grid[-1]:g} deg"
        )
    fit = np.polynomial.Polynomial.fit(roll, values, ROLL_CENTER_FIT_DEGREE)
    return fit(grid)
