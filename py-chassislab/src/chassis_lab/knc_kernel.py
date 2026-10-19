"""Path-free numerical core shared by the KnC test kernels.

Curves are `{"corner", "signal", "values", "unit", "offset"?}` dicts. `corner`
is a wheel corner or, for axle signals, an axle location (`front`, `rear`,
`total`). `offset` is the baseline Rust subtracted from relative values.
"""

import json
from dataclasses import dataclass
from math import atan, degrees
from typing import Any, NamedTuple

import numpy as np

CORNERS = ("front_left", "front_right", "rear_left", "rear_right")
AXLES = ("front", "rear")

CurveIndex = dict[tuple[str, str], dict[str, object]]


class KncKernelError(ValueError):
    """Curve data cannot be evaluated safely."""


# ---------------------------------------------------------------------------
# Branch detection
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class BranchOptions:
    """Branch detection and edge trimming; per-plot via `PlotSpec.branch`.

    direction, index, plot_branches: which sweep the metrics use and which
    sweeps the plots draw.
    min_branch_fraction: measured branches shorter than this fraction of all
    samples are ignored (noise flips at turnarounds).
    edge_width: samples searched for the curvature cut at each sweep end.
    edge_percentile: |d²y/dx²| percentile (0-1) above which a point is an
    outlier that starts the cut.
    """

    direction: str = "rise"
    index: int = 2
    plot_branches: str = "all"
    measurement: bool = False
    min_branch_fraction: float = 0.05
    edge_width: int = 200
    edge_percentile: float = 0.95

    @classmethod
    def from_json(cls, raw: str | dict[str, Any] | None) -> "BranchOptions":
        values = json.loads(raw) if isinstance(raw, str) else raw
        return cls(**(values or {}))

    def sweeps(self, travel: np.ndarray) -> list[slice]:
        """Sweeps of the configured direction, in travel order."""
        return [
            segment
            for segment, sign in branch_ranges(travel, self)
            if self.direction == "any"
            or sign == (1 if self.direction == "rise" else -1)
        ]

    def metric_sweep(self, travel: np.ndarray) -> slice | None:
        sweeps = self.sweeps(travel)
        return sweeps[self.index - 1] if len(sweeps) >= self.index else None

    def plot_sweeps(self, travel: np.ndarray) -> list[slice]:
        if self.plot_branches == "selected":
            selected = self.metric_sweep(travel)
            return [] if selected is None else [selected]
        return [segment for segment, _ in branch_ranges(travel, self)]


def _fill_zeros(signs: np.ndarray) -> np.ndarray:
    """Give flat steps the direction of the previous (else next) moving step."""

    def forward(values: np.ndarray) -> np.ndarray:
        last = np.maximum.accumulate(
            np.where(values != 0, np.arange(len(values)), 0)
        )
        return values[last]

    return forward(forward(signs)[::-1])[::-1]


def branch_ranges(
    travel: np.ndarray, options: BranchOptions
) -> list[tuple[slice, int]]:
    """Split raw travel at reversals, ignoring flat steps and short branches.

    Measurements drop the first branch: it runs from the start position to the
    first reversal and is only a partial, noisy sweep.
    """
    dz = np.diff(travel)
    if not np.isfinite(dz).any():
        return []
    signs = _fill_zeros(np.where(np.isfinite(dz), np.sign(dz), 0.0))
    starts = np.r_[0, np.flatnonzero(signs[1:] != signs[:-1]) + 1]
    ends = np.r_[starts[1:], len(signs)]
    minimum = (
        max(2, int(options.min_branch_fraction * len(travel)))
        if options.measurement
        else 2
    )
    branches = [
        (slice(int(start), int(end) + 1), int(signs[start]))
        for start, end in zip(starts, ends, strict=True)
        if signs[start] != 0 and end - start + 1 >= minimum
    ]
    return branches[1:] if options.measurement else branches


def gradient_curve(
    x: np.ndarray,
    y: np.ndarray,
    denominator: np.ndarray | None,
    sweeps: list[slice],
    options: BranchOptions,
) -> tuple[np.ndarray, np.ndarray]:
    """Pointwise MATLAB-style d(y)/d(denominator or x) for each sweep.

    Sweeps stay separate (NaN gap between them) so hysteresis remains visible.
    Measured sweeps trim each end from its first curvature outlier outward.
    """
    xs: list[np.ndarray] = [np.empty(0)]
    ys: list[np.ndarray] = [np.empty(0)]
    for sweep in sweeps:
        result = sweep_gradient(x, y, denominator, sweep, options)
        if result is None:
            continue
        axis, slope, _, _, masked = result
        slope[masked] = np.nan
        xs += [axis, np.array([np.nan])]
        ys += [slope, np.array([np.nan])]
    return np.concatenate(xs), np.concatenate(ys)


def _curvature_limit(
    curvature: np.ndarray, options: BranchOptions
) -> float | None:
    valid = np.isfinite(curvature)
    if not valid.any():
        return None
    return float(np.percentile(curvature[valid], options.edge_percentile * 100))


def sweep_gradient(
    x: np.ndarray,
    y: np.ndarray,
    denominator: np.ndarray | None,
    sweep: slice,
    options: BranchOptions,
) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, np.ndarray] | None:
    """Return x, raw rate, |curvature|, edge zone, and actual mask."""
    axis, values = x[sweep], y[sweep]
    travel = axis if denominator is None else denominator[sweep]
    finite = np.isfinite(axis) & np.isfinite(values) & np.isfinite(travel)
    axis, values, travel = axis[finite], values[finite], travel[finite]
    if len(axis) < 2:
        return None
    with np.errstate(divide="ignore", invalid="ignore"):
        slope = np.gradient(values, travel)
        curvature = np.abs(np.gradient(slope, travel))
    slope[~np.isfinite(slope)] = np.nan
    size = len(slope)
    width = options.edge_width
    position = np.arange(size)
    at_edge = (position < width) | (position >= size - width)
    masked = np.zeros(size, dtype=bool)
    limit = _curvature_limit(curvature, options)
    if options.measurement and limit is not None:
        outlier = ~(curvature <= limit)
        # Search from the middle toward each end. Keep the cutoff point and
        # all points between it and the turnaround masked, with no gaps.
        start = np.flatnonzero(outlier[: min(width, (size + 1) // 2)])
        end_from = max(size - width, size // 2)
        end = np.flatnonzero(outlier[end_from:])
        if start.size:
            masked[: start[-1] + 1] = True
        if end.size:
            masked[end_from + end[0] :] = True
    return axis, slope, curvature, at_edge, masked


# ---------------------------------------------------------------------------
# Curve access
# ---------------------------------------------------------------------------


def index_curves(curves: list[dict[str, object]]) -> CurveIndex:
    return {(str(item["corner"]), str(item["signal"])): item for item in curves}


def signal_values(curves: CurveIndex, location: str, signal: str) -> np.ndarray:
    """Relative samples of `<location>_<signal>`."""
    if (location, signal) not in curves:
        raise KncKernelError(f"missing canonical signal {location}_{signal}")
    return np.asarray(curves[(location, signal)]["values"], dtype=np.float64)


def absolute(curves: CurveIndex, location: str, signal: str) -> np.ndarray:
    """Samples of `<location>_<signal>` with the subtracted offset restored."""
    offset = curves.get((location, signal), {}).get("offset") or 0.0
    return signal_values(curves, location, signal) + float(offset)


def with_pad_travel(
    curves: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Add wheel pad Z as wheel-center Z plus tire deflection when absent."""
    indexed = index_curves(curves)
    derived = []
    for corner in CORNERS:
        center = indexed.get((corner, "wheel_center_z"))
        deflection = indexed.get((corner, "tire_deflection"))
        if (corner, "wheel_pad_z") in indexed or not (center and deflection):
            continue
        pad = np.asarray(center["values"], dtype=np.float64) + np.asarray(
            deflection["values"], dtype=np.float64
        )
        derived.append(
            {
                "corner": corner,
                "signal": "wheel_pad_z",
                "values": pad.tolist(),
                "unit": "mm",
            }
        )
    return [*curves, *derived]


# ---------------------------------------------------------------------------
# Interpolation
# ---------------------------------------------------------------------------


def interpolate(
    x: np.ndarray, y: np.ndarray, grid: np.ndarray, source: str
) -> np.ndarray:
    """Interpolate finite samples after sorting and duplicate averaging."""
    finite = np.isfinite(x) & np.isfinite(y)
    x, y = x[finite], y[finite]
    if len(x) < 2:
        raise KncKernelError(f"{source} has insufficient finite samples")
    order = np.argsort(x, kind="stable")
    x, y = x[order], y[order]
    unique, starts, counts = np.unique(x, return_index=True, return_counts=True)
    means = np.add.reduceat(y, starts) / counts
    if unique[0] > grid[0] or unique[-1] < grid[-1]:
        raise KncKernelError(f"{source} does not cover requested window")
    return np.interp(grid, unique, means)


def mean_monotonic(
    x: np.ndarray, y: np.ndarray, grid: np.ndarray, source: str
) -> np.ndarray:
    """Interpolate every complete monotonic branch, then average branches."""
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
        interpolate(x[index], y[index], grid, source)
        for index in np.split(np.arange(len(x)), boundaries)
        if len(index) >= 2
        and np.min(x[index]) <= grid[0]
        and np.max(x[index]) >= grid[-1]
    ]
    if not branches:
        raise KncKernelError(
            f"{source} has no monotonic branch covering requested window"
        )
    return np.mean(branches, axis=0)


# ---------------------------------------------------------------------------
# Secant metrics
# ---------------------------------------------------------------------------


class Signal(NamedTuple):
    """Canonical signal placed relative to the metric location.

    `at` is "self" (the metric location), "axle" (the corner's axle), or a
    fixed location such as "total".
    """

    name: str
    at: str = "self"

    def location(self, location: str) -> str:
        if self.at == "self":
            return location
        if self.at == "axle":
            return location.split("_")[0]
        return self.at


@dataclass(frozen=True)
class GradientMetric:
    """Secant metric of `signal` between symmetric points on `axis`.

    The metric is named `<name>_<location>_<suffix>`. `gradient` divides by
    the axis span, or by the span of `denominator` when given. `angle` is
    atan(-delta / axis span) in degrees. `points` selects the half-width of
    the secant window ("mm" or "deg").
    """

    name: str
    suffix: str
    signal: str
    unit: str
    kind: str = "gradient"
    axis: Signal = Signal("wheel_center_z")
    denominator: str | None = None
    points: str = "mm"


def metric_row(
    location: str, metric: str, unit: str, points: str, half: float
) -> dict[str, object]:
    """Unavailable metric row; callers fill `value`, `lower`, `upper`."""
    return {
        "corner": location,
        "metric": metric,
        "unit": unit,
        "value": None,
        "lower": None,
        "upper": None,
        "reason": None,
        f"interval_{points}": [-half, half],
    }


def gradient_metrics(
    curves: CurveIndex,
    catalog: tuple[GradientMetric, ...],
    locations: tuple[str, ...],
    points: dict[str, float],
    branch: Signal,
    options: BranchOptions,
) -> list[dict[str, object]]:
    """Evaluate every catalog metric at every location.

    `branch` is the travel signal whose configured sweep selects the samples;
    unavailable metrics keep `value: None` and carry a `reason`.
    """
    results = []
    for location in locations:
        for metric in catalog:
            half = points[metric.points]
            result = metric_row(
                location,
                f"{metric.name}_{location}_{metric.suffix}",
                metric.unit,
                metric.points,
                half,
            )
            try:
                lower, upper = secant_points(
                    curves,
                    location,
                    metric.signal,
                    metric.axis,
                    branch,
                    half,
                    metric.points,
                    options,
                )
                span = 2.0 * half
                if metric.denominator is not None:
                    low, high = secant_points(
                        curves,
                        location,
                        metric.denominator,
                        metric.axis,
                        branch,
                        half,
                        metric.points,
                        options,
                    )
                    span = high - low
                    if span == 0.0:
                        raise KncKernelError(
                            "denominator signal does not change"
                        )
            except KncKernelError as error:
                result["reason"] = str(error)
            else:
                delta = upper - lower
                result["lower"] = lower
                result["upper"] = upper
                result["value"] = (
                    degrees(atan(-delta / (2.0 * half)))
                    if metric.kind == "angle"
                    else delta / span
                )
            results.append(result)
    return results


def metric_sweep(
    travel: np.ndarray, name: str, options: BranchOptions
) -> slice:
    """Configured metric sweep of `travel`, the branch signal `name`."""
    sweep = options.metric_sweep(travel)
    if sweep is None:
        raise KncKernelError(
            f"{name} has no {options.direction} branch {options.index}"
        )
    return sweep


def secant_points(
    curves: CurveIndex,
    location: str,
    signal: str,
    axis: Signal,
    branch: Signal,
    half: float,
    unit: str,
    options: BranchOptions,
) -> tuple[float, float]:
    """`signal` at axis = -half and +half on the configured branch sweep."""
    axis_location = axis.location(location)
    travel = signal_values(curves, axis_location, axis.name)
    response = signal_values(curves, location, signal)
    branch_location = branch.location(location)
    reference = signal_values(curves, branch_location, branch.name)
    if not len(travel) == len(response) == len(reference):
        raise KncKernelError(
            f"{location}_{signal} and {axis_location}_{axis.name} lengths "
            "differ"
        )
    sweep = metric_sweep(reference, f"{branch_location}_{branch.name}", options)
    grid = np.array([-half, half], dtype=np.float64)
    try:
        lower, upper = interpolate(
            travel[sweep], response[sweep], grid, location
        )
    except KncKernelError:
        raise KncKernelError(
            f"{location}_{signal} does not cover {axis.name} travel "
            f"{-half:g}..{half:g} {unit}"
        ) from None
    return float(lower), float(upper)
