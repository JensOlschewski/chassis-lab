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

    cycle: which full cycle (a fall and the rise after it) the metrics fit.
    plot_branches: which sweeps the plots draw; "selected" draws only the
    branches of the metric cycle.
    branch_min_fraction: measured branches shorter than this fraction of all
    samples are ignored (noise flips at turnarounds).
    reversal_min_fraction: a reversal that travels back less than this
    fraction of the total travel span does not split a branch.
    edge_trim_width: samples searched for the curvature cut at each sweep end.
    edge_trim_percentile: |d²y/dx²| percentile (0-1) above which a point is
    an outlier that starts the cut.
    """

    cycle: int = 1
    plot_branches: str = "all"
    measurement: bool = False
    branch_min_fraction: float = 0.05
    reversal_min_fraction: float = 0.01
    edge_trim_width: int = 200
    edge_trim_percentile: float = 0.95

    @classmethod
    def from_json(cls, raw: str | dict[str, Any] | None) -> "BranchOptions":
        values = json.loads(raw) if isinstance(raw, str) else raw
        return cls(**(values or {}))

    def cycle_branches(self, travel: np.ndarray) -> tuple[slice, slice] | None:
        """Falling and rising branch of the metric cycle, if there is one."""
        branches = branch_ranges(travel, self)
        starts = [
            index
            for index, (_, sign) in enumerate(branches[:-1])
            if sign < 0 and branches[index + 1][1] > 0
        ]
        if self.cycle < 1 or len(starts) < self.cycle:
            return None
        index = starts[self.cycle - 1]
        return branches[index][0], branches[index + 1][0]

    def metric_cycle(self, travel: np.ndarray) -> slice | None:
        """Samples of the metric cycle, if there is one."""
        branches = self.cycle_branches(travel)
        if branches is None:
            return None
        return slice(branches[0].start, branches[1].stop)

    def plot_sweeps(self, travel: np.ndarray) -> list[slice]:
        if self.plot_branches == "selected":
            return list(self.cycle_branches(travel) or ())
        return [segment for segment, _ in branch_ranges(travel, self)]


def _fill_zeros(signs: np.ndarray) -> np.ndarray:
    """Give flat steps the direction of the previous (else next) moving step."""

    def forward(values: np.ndarray) -> np.ndarray:
        last = np.maximum.accumulate(
            np.where(values != 0, np.arange(len(values)), 0)
        )
        return values[last]

    return forward(forward(signs)[::-1])[::-1]


def _merge_small_reversals(
    starts: np.ndarray,
    ends: np.ndarray,
    signs: np.ndarray,
    steps: np.ndarray,
    fraction: float,
) -> tuple[np.ndarray, np.ndarray]:
    """Join a run with its neighbors when it travels back less than the limit.

    The limit is `fraction` of the total travel span. A small dip inside a
    ramp then no longer splits it into two branches.
    """
    path = np.cumsum(steps)
    span = float(np.ptp(path)) if path.size else 0.0
    limit = fraction * span
    runs = [[int(a), int(b)] for a, b in zip(starts, ends, strict=True)]
    merged = True
    while merged and limit > 0:
        merged = False
        for i in range(1, len(runs) - 1):
            first, middle, last = runs[i - 1], runs[i], runs[i + 1]
            small = abs(steps[middle[0] : middle[1]].sum()) < limit
            if small and signs[first[0]] == signs[last[0]] != 0:
                runs[i - 1 : i + 2] = [[first[0], last[1]]]
                merged = True
                break
    return (
        np.array([run[0] for run in runs], dtype=int),
        np.array([run[1] for run in runs], dtype=int),
    )


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
    steps = np.where(np.isfinite(dz), dz, 0.0)
    signs = _fill_zeros(np.sign(steps))
    starts = np.r_[0, np.flatnonzero(signs[1:] != signs[:-1]) + 1]
    ends = np.r_[starts[1:], len(signs)]
    starts, ends = _merge_small_reversals(
        starts, ends, signs, steps, options.reversal_min_fraction
    )
    minimum = (
        max(2, int(options.branch_min_fraction * len(travel)))
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
    return float(
        np.percentile(curvature[valid], options.edge_trim_percentile * 100)
    )


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
    width = options.edge_trim_width
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
# Least-squares metric fit
# ---------------------------------------------------------------------------


class LineFit(NamedTuple):
    slope: float
    intercept: float
    samples: int


def metric_cycle(reference: np.ndarray, options: BranchOptions) -> slice:
    """Samples of the metric cycle of the `reference` sweep.

    A cycle runs from one upper reversal to the next: a falling branch and the
    rising branch after it. The partial branch before the first reversal is
    never part of a cycle.
    """
    cycle = options.metric_cycle(reference)
    if cycle is None:
        raise KncKernelError(f"sweep has no full cycle {options.cycle}")
    return cycle


def fit_slope(x: np.ndarray, y: np.ndarray, window: float) -> LineFit:
    """Least-squares line of `y` on `x` over the samples with |x| <= window.

    `x` must span the whole window, so a sweep that stops short is reported
    instead of fitted over a narrower range.
    """
    finite = np.isfinite(x) & np.isfinite(y)
    if (
        not finite.any()
        or x[finite].min() > -window
        or x[finite].max() < window
    ):
        raise KncKernelError(f"data does not cover {-window:g}..{window:g}")
    inside = finite & (np.abs(x) <= window)
    if inside.sum() < 3 or np.unique(x[inside]).size < 2:
        raise KncKernelError(f"fewer than 3 samples within ±{window:g}")
    slope, intercept = np.polyfit(x[inside], y[inside], 1)
    return LineFit(float(slope), float(intercept), int(inside.sum()))


# ---------------------------------------------------------------------------
# Slope metrics
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
    """Least-squares slope metric of `signal` on `axis`.

    The metric is named `<name>_<location>_<suffix>`. `gradient` is the fitted
    slope, divided by the slope of `denominator` when given. `angle` is
    atan(-slope) in degrees. `points` selects the half-width of the fit
    window ("mm", "deg" or "N"). `scale` converts the result to
    `unit`, e.g. 1000 for mm/kN from an axis in N.
    """

    name: str
    suffix: str
    signal: str
    unit: str
    kind: str = "gradient"
    axis: Signal = Signal("wheel_center_z")
    denominator: str | None = None
    points: str = "mm"
    scale: float = 1.0


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


def _fit(
    curves: CurveIndex,
    location: str,
    signal: str,
    axis: Signal,
    branch: Signal,
    window: float,
    unit: str,
    options: BranchOptions,
) -> LineFit:
    """Least-squares line of `signal` on `axis` over the metric cycle of the
    `branch` travel, within +-`window` of the axis."""
    axis_location = axis.location(location)
    x = signal_values(curves, axis_location, axis.name)
    y = signal_values(curves, location, signal)
    branch_location = branch.location(location)
    reference = signal_values(curves, branch_location, branch.name)
    if not len(x) == len(y) == len(reference):
        raise KncKernelError(
            f"{location}_{signal} and {axis_location}_{axis.name} lengths "
            "differ"
        )
    cycle = metric_cycle(reference, options)
    try:
        return fit_slope(x[cycle], y[cycle], window)
    except KncKernelError as error:
        raise KncKernelError(
            f"{location}_{signal} vs {axis.name} {unit}: {error}"
        ) from None


def gradient_metrics(
    curves: CurveIndex,
    catalog: tuple[GradientMetric, ...],
    locations: tuple[str, ...],
    points: dict[str, float],
    branch: Signal,
    options: BranchOptions,
) -> list[dict[str, object]]:
    """Evaluate every catalog metric at every location.

    `branch` is the travel signal whose metric cycle selects the samples;
    unavailable metrics keep `value: None` and carry a `reason`. `lower` and
    `upper` are the fitted line at -window and +window.
    """
    results = []
    for location in locations:
        for metric in catalog:
            window = points[metric.points]
            result = metric_row(
                location,
                f"{metric.name}_{location}_{metric.suffix}",
                metric.unit,
                metric.points,
                window,
            )
            try:
                fit = _fit(
                    curves,
                    location,
                    metric.signal,
                    metric.axis,
                    branch,
                    window,
                    metric.points,
                    options,
                )
                slope = fit.slope
                if metric.denominator is not None:
                    reference = _fit(
                        curves,
                        location,
                        metric.denominator,
                        metric.axis,
                        branch,
                        window,
                        metric.points,
                        options,
                    )
                    if reference.slope == 0.0:
                        raise KncKernelError(
                            "denominator signal does not change"
                        )
                    slope /= reference.slope
            except KncKernelError as error:
                result["reason"] = str(error)
            else:
                result["lower"] = fit.intercept - fit.slope * window
                result["upper"] = fit.intercept + fit.slope * window
                result["value"] = (
                    degrees(atan(-slope))
                    if metric.kind == "angle"
                    else metric.scale * slope
                )
            results.append(result)
    return results


def fit_ends(
    curves: CurveIndex,
    location: str,
    signal: str,
    axis: Signal,
    branch: Signal,
    window: float,
    unit: str,
    options: BranchOptions,
) -> tuple[float, float]:
    """Fitted `signal` at axis = -window and +window."""
    fit = _fit(curves, location, signal, axis, branch, window, unit, options)
    return (
        fit.intercept - fit.slope * window,
        fit.intercept + fit.slope * window,
    )
