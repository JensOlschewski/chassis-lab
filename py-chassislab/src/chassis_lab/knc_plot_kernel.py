"""Reusable, path-free plotting primitives for KnC report kernels."""

import json
from collections.abc import Callable
from dataclasses import dataclass, field, replace
from datetime import datetime
from io import BytesIO
from math import floor, log10
from typing import Any, NamedTuple, cast

import numpy as np
from pypdf import PdfReader, PdfWriter, Transformation

# ---------------------------------------------------------------------------
# Settings
# ---------------------------------------------------------------------------

CORNERS = (
    ("front_left", "Front left"),
    ("front_right", "Front right"),
    ("rear_left", "Rear left"),
    ("rear_right", "Rear right"),
)

# Also used for clab analyze
BASELINE_COLOR = "#000000"

# Used for clab compare reports, in candidate order.
CANDIDATE_COLORS = (
    "#e41a1c",
    "#377eb8",
    "#e7298a",
    "#00a6a6",
    "#ff7f00",
    "#6a3d9a",
)

# Debug plot colors.
RAW_COLOR = "#7f7f7f"
USED_COLOR = "#2ca02c"
CUT_COLOR = "#d62728"
METRIC_COLOR = "#ffbf00"

# Gradient secant line: drawn in the color of its source, dashed.
# Any matplotlib linestyle ("dashed", "dotted", "dashdot", "solid").
GRADIENT_LINESTYLE = "dashed"
GRADIENT_LINEWIDTH = 0.9

# Marker at each of the two points the gradient is defined at: a circle with a
# short vertical line through it. Any matplotlib marker works for either.
GRADIENT_MARKER_COLOR = "#d62728"
GRADIENT_MARKER = "o"
GRADIENT_MARKER_SIZE = 5
GRADIENT_MARKER_WIDTH = 1.2
GRADIENT_TICK_MARKER = "|"
GRADIENT_TICK_SIZE = 12

# Gradient metric table above the footer.
METRIC_FONTSIZE = 8
METRIC_TITLE_COLOR = "#222222"
METRIC_VALUE_COLOR = "#222222"
METRIC_LINE_HEIGHT = 0.185  # [inch] per table row
METRIC_GAP = 0.08  # [inch] between footer and table
XLABEL_CLEARANCE = 0.62  # [inch] reserved below the lowest panels
METRIC_LEFT = 0.02  # same left edge as the footer text
METRIC_CORNER_LABELS = {
    "front_left": "Left front",
    "front_right": "Right front",
    "rear_left": "Left rear",
    "rear_right": "Right rear",
}

# Figure sizes [inch]: report page (A4 landscape) and debug sheet (A3).
REPORT_FIGSIZE = (11.69, 8.27)
DEBUG_FIGSIZE = (16.54, 11.69)

# Page layout as figure fractions (before footer scaling).
PAGE_LEFT = 0.08
PAGE_RIGHT = 0.97
PAGE_TOP = 0.89
PAGE_WSPACE = 0.28
PAGE_HSPACE = 0.42
DEBUG_HSPACE = 0.6

# Footer band is sized in inches of the report page, so it looks the same on
# every figure size.
FOOTER_BASE = 0.12
FOOTER_PER_LINE = 0.025
FOOTER_MAX = 0.35
FOOTER_FONTSIZE = 7
FOOTER_COLOR = "#4a4a4a"
LOGO_PER_LINE = 0.017
LOGO_MARGIN = 0.05

PDF_METADATA = {
    "Creator": "ChassisLab",
    "Producer": "ChassisLab",
    "CreationDate": None,
}


# ---------------------------------------------------------------------------
# Branch detection options
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SignalStyle:
    label: str
    unit: str


@dataclass(frozen=True)
class GradientDisplay:
    """Secant of `PlotSpec.y_signal` over `x_signal` drawn on a plot.

    The metric is the secant between the two travel points; `kind` is
    "gradient" (dy/dx) or "angle" (atan(-dy/dx) in degrees). Its values are
    listed in a table above the footer as "<corner> <label> in <unit>".
    """

    label: str
    unit: str
    kind: str = "gradient"

    def value(self, delta: float, span: float) -> float:
        if self.kind == "angle":
            return float(np.degrees(np.arctan(-delta / span)))
        return delta / span


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


# (x, y, denominator, sweeps, branch options) -> plotted (x, y)
Transform = Callable[
    [np.ndarray, np.ndarray, np.ndarray | None, list[slice], BranchOptions],
    tuple[np.ndarray, np.ndarray],
]
CurveIndex = dict[tuple[str, str], dict[str, object]]


class Source(NamedTuple):
    """One plotted source: curves and line color."""

    label: str
    curves: CurveIndex
    color: str


Series = list[Source]


# ---------------------------------------------------------------------------
# Branch splitting and rate computation
# ---------------------------------------------------------------------------


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
        result = _sweep_gradient(x, y, denominator, sweep, options)
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


def _sweep_gradient(
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
# Plot catalog entry
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class PlotSpec:
    """One plot derived from canonical x/y signals."""

    name: str
    title: str
    x_signal: str
    y_signal: str
    output_signal: str | None = None
    transform: Transform | None = None
    optional: bool = False
    # Draw the secant between the +-gradient travel points and list its metric.
    gradient: GradientDisplay | None = None
    denominator_signal: str | None = None
    # Maps the sweep gradient to the plotted value (rate debug plots only).
    from_gradient: Callable[[np.ndarray], np.ndarray] | None = None
    y_limits: tuple[float, float] | None = None
    # BranchOptions fields overriding the run's branch detection and edge
    # trimming for this plot only. `measurement` is never overridden.
    # Example: {"edge_width": 100, "edge_percentile": 0.9}
    branch: dict[str, Any] = field(default_factory=dict)

    def branch_options(self, options: BranchOptions) -> BranchOptions:
        return replace(options, **self.branch)

    @property
    def is_rate(self) -> bool:
        """Plot is a per-sweep gradient, so the rate debug plot applies."""
        return (
            self.transform is gradient_curve or self.from_gradient is not None
        )

    @property
    def plotted_signal(self) -> str:
        return self.output_signal or self.y_signal

    @property
    def signals(self) -> tuple[str, ...]:
        """Canonical signals the plot reads."""
        return (
            self.x_signal,
            self.y_signal,
            *([self.denominator_signal] if self.denominator_signal else []),
        )


# ---------------------------------------------------------------------------
# Report plots
# ---------------------------------------------------------------------------


def render_reports(
    curves: list[dict[str, object]],
    specs: tuple[PlotSpec, ...],
    signals: dict[str, SignalStyle],
    source_label: str,
    output_prefix: str,
    footer: str = "",
    logo: bytes | None = None,
    gradient_points_mm: float | None = None,
    branches: BranchOptions | None = None,
) -> dict[str, bytes]:
    """Render one source using same catalog as comparison reports."""
    series = [
        Source(
            source_label,
            _index(curves),
            BASELINE_COLOR,
        )
    ]
    return _render_catalog(
        specs,
        signals,
        series,
        output_prefix,
        footer,
        logo,
        gradient_points_mm,
        [branches or BranchOptions()],
    )


def render_comparison_reports(
    baseline: list[dict[str, object]],
    candidates: list[dict[str, object]],
    specs: tuple[PlotSpec, ...],
    signals: dict[str, SignalStyle],
    baseline_label: str,
    footer: str,
    logo: bytes,
    gradient_points_mm: float | None = None,
    branches: list[BranchOptions] | None = None,
) -> dict[str, bytes]:
    """Render baseline and candidates using one declarative plot catalog."""
    series = [
        Source(
            baseline_label,
            _index(baseline),
            BASELINE_COLOR,
        )
    ]
    series.extend(
        Source(
            str(candidate["label"]),
            _index(cast("list[dict[str, object]]", candidate["curves"])),
            CANDIDATE_COLORS[index % len(CANDIDATE_COLORS)],
        )
        for index, candidate in enumerate(candidates)
    )
    return _render_catalog(
        specs,
        signals,
        series,
        "",
        footer,
        logo,
        gradient_points_mm,
        branches or [BranchOptions()] * len(series),
    )


def _render_catalog(
    specs: tuple[PlotSpec, ...],
    signals: dict[str, SignalStyle],
    series: Series,
    output_prefix: str,
    footer: str,
    logo: bytes | None,
    gradient_points_mm: float | None,
    branches: list[BranchOptions],
) -> dict[str, bytes]:
    reports = {}
    for spec in specs:
        if not _available(spec, [source.curves for source in series]):
            if spec.optional:
                continue
            raise ValueError(
                "plot requires canonical signals "
                f"{spec.x_signal} and {spec.y_signal}"
            )
        filename = f"{spec.name}.pdf"
        path = f"{output_prefix}/{filename}" if output_prefix else filename
        reports[path] = _render_plot(
            spec,
            signals,
            series,
            footer,
            logo,
            gradient_points_mm,
            branches,
        )
    return reports


def _render_plot(
    spec: PlotSpec,
    signals: dict[str, SignalStyle],
    series: Series,
    footer: str,
    logo: bytes | None,
    gradient_points_mm: float | None,
    branches: list[BranchOptions],
) -> bytes:
    plt = _pyplot()
    gradient = (
        spec.gradient
        if gradient_points_mm is not None and gradient_points_mm > 0
        else None
    )
    figure, panels = _new_figure(
        plt,
        2,
        2,
        REPORT_FIGSIZE,
        footer,
        len(CORNERS) + 1 if gradient else 0,
    )
    x_style = signals[spec.x_signal]
    y_style = signals[spec.plotted_signal]
    figure.suptitle(spec.title, fontsize=16)
    table: list[tuple[str, list[float | None]]] = []
    for panel, (corner, corner_label) in zip(panels.flat, CORNERS, strict=True):
        values: list[float | None] = []
        for (source_label, curves, color), options in zip(
            series, branches, strict=True
        ):
            x, y = _curve_xy(spec, curves, corner, spec.branch_options(options))
            panel.plot(x, y, color=color, linewidth=1.8, label=source_label)
            if gradient and gradient_points_mm is not None:
                secant = _secant(
                    spec, curves, corner, options, gradient_points_mm
                )
                values.append(
                    None
                    if secant is None
                    else _draw_secant(
                        panel,
                        gradient,
                        secant,
                        gradient_points_mm,
                        color,
                    )
                )
        if gradient:
            table.append((corner, values))
        panel.set_title(corner_label, loc="left", fontweight="bold")
        panel.set_xlabel(f"{x_style.label} [{x_style.unit}]")
        panel.set_ylabel(f"{y_style.label} [{y_style.unit}]")
        if spec.y_limits:
            panel.set_ylim(*spec.y_limits)
        panel.legend(loc="best", frameon=False)
    if footer:
        _add_footer(figure, footer)
    if gradient and gradient_points_mm is not None:
        _add_metric_table(
            figure, gradient, gradient_points_mm, table, series, footer
        )
    return _save_pdf(figure, plt, footer, logo)


def _secant(
    spec: PlotSpec,
    curves: CurveIndex,
    corner: str,
    options: BranchOptions,
    gradient_mm: float,
) -> tuple[float, float] | None:
    """Plotted response at -gradient and +gradient travel, if covered.

    Uses the metric sweep of `wheel_center_z`, like the vertical metrics.
    """
    travel = _values(curves, corner, spec.x_signal)
    response = _values(curves, corner, spec.y_signal)
    sweep = options.metric_sweep(_values(curves, corner, "wheel_center_z"))
    if sweep is None or len(travel) != len(response):
        return None
    travel, response = travel[sweep], response[sweep]
    finite = np.isfinite(travel) & np.isfinite(response)
    travel, response = travel[finite], response[finite]
    if len(travel) < 2:
        return None
    order = np.argsort(travel, kind="stable")
    unique, starts, counts = np.unique(
        travel[order], return_index=True, return_counts=True
    )
    mean = np.add.reduceat(response[order], starts) / counts
    if unique[0] > -gradient_mm or unique[-1] < gradient_mm:
        return None
    lower, upper = np.interp([-gradient_mm, gradient_mm], unique, mean)
    return float(lower), float(upper)


def _draw_secant(
    panel: Any,
    gradient: GradientDisplay,
    secant: tuple[float, float],
    gradient_mm: float,
    color: str,
) -> float:
    """Draw the dashed secant over the panel, markers at its two points."""
    points = ((-gradient_mm, secant[0]), (gradient_mm, secant[1]))
    panel.axline(
        *points,
        color=color,
        linestyle=GRADIENT_LINESTYLE,
        linewidth=GRADIENT_LINEWIDTH,
        zorder=3,
    )
    for marker, size in (
        (GRADIENT_MARKER, GRADIENT_MARKER_SIZE),
        (GRADIENT_TICK_MARKER, GRADIENT_TICK_SIZE),
    ):
        panel.plot(
            *zip(*points, strict=True),
            linestyle="none",
            color=GRADIENT_MARKER_COLOR,
            marker=marker,
            markersize=size,
            markeredgewidth=GRADIENT_MARKER_WIDTH,
            zorder=4,
        )
    return gradient.value(secant[1] - secant[0], 2.0 * gradient_mm)


def _curve_xy(
    spec: PlotSpec, curves: CurveIndex, corner: str, options: BranchOptions
) -> tuple[np.ndarray, np.ndarray]:
    x = _values(curves, corner, spec.x_signal)
    y = _values(curves, corner, spec.y_signal)
    if len(x) != len(y):
        raise ValueError(
            f"{corner} {spec.y_signal} and {spec.x_signal} lengths differ"
        )
    if spec.transform is None:
        return x, y
    denominator = _denominator(spec, curves, corner)
    sweeps = options.plot_sweeps(_values(curves, corner, "wheel_center_z"))
    return spec.transform(x, y, denominator, sweeps, options)


# ---------------------------------------------------------------------------
# Rate debug plots
# ---------------------------------------------------------------------------


def render_rate_debug(
    curves: list[dict[str, object]],
    specs: tuple[PlotSpec, ...],
    options: BranchOptions,
    prefix: str,
    info: dict[str, Any],
    input_path: str,
    logo: bytes | None = None,
    source: str = "measurement",
) -> dict[str, bytes]:
    """Visualize branch detection and rate masks without changing reports.

    `info` holds the config path, creator and clab version; `input_path` is
    the data file of these curves.
    """
    if not options.measurement:
        return {}
    indexed = _index(curves)
    reports = {}
    for spec in specs:
        if not spec.is_rate or not _available(spec, [indexed]):
            continue
        spec_options = spec.branch_options(options)
        footer = _debug_footer(info, input_path, spec_options)
        path = f"debug/{source}_{spec.name}.pdf"
        reports[f"{prefix}/{path}" if prefix else path] = _rate_debug_plot(
            indexed, spec, spec_options, footer, logo
        )
    return reports


def _debug_footer(
    info: dict[str, Any], input_path: str, options: BranchOptions
) -> str:
    return "\n".join(
        (
            f"Config: {info['config']}",
            f"BranchData: {input_path}",
            f"Debug branch: direction={options.direction} "
            f"index={options.index} plot_branches={options.plot_branches}",
            "Debug cut: ends from first |d²y/dx²| > "
            f"P{options.edge_percentile * 100:g}",
            f"Debug edge zone: {options.edge_width} samples per end, "
            f"min branch {options.min_branch_fraction:g} of samples",
            f"Created by: {info['creator']}",
            f"clab version: {info['version']}",
        )
    )


def _rate_debug_plot(
    curves: CurveIndex,
    spec: PlotSpec,
    options: BranchOptions,
    footer: str,
    logo: bytes | None,
) -> bytes:
    """One row per corner: branches, curvature, raw rate, final rate."""
    plt = _pyplot()
    figure, panels = _new_figure(plt, 4, 4, DEBUG_FIGSIZE, footer)
    figure.suptitle(f"{spec.title} - debug", fontsize=16)
    rate = spec.plotted_signal
    for row, (corner, label) in zip(panels, CORNERS, strict=True):
        branch, curvature, raw, final = row
        branch.set_title(f"{label}: branch detection", loc="left")
        curvature.set_title("Second derivative |d²y/dx²|", loc="left")
        raw.set_title(f"Raw {rate}", loc="left")
        final.set_title(f"Final {rate}", loc="left")

        travel = _values(curves, corner, "wheel_center_z")
        plotted = options.plot_sweeps(travel)
        _draw_branches(branch, travel, plotted, options.metric_sweep(travel))
        for sweep in plotted:
            _draw_sweep(
                (curvature, raw, final), spec, curves, corner, sweep, options
            )

        branch.set_xlabel("sample")
        branch.set_ylabel("wheel_center_z [mm]")
        for panel in (curvature, raw, final):
            panel.set_xlabel(f"{spec.x_signal} [mm]")
        curvature.set_ylabel("|d²y/dx²|")
        curvature.set_yscale("symlog")
        raw.set_ylabel(rate)
        final.set_ylabel(rate)
        if spec.y_limits:
            final.set_ylim(*spec.y_limits)
        for panel in row:
            _legend_unique(panel)
    _add_footer(figure, footer)
    return _save_pdf(figure, plt, footer, logo)


def _draw_branches(
    panel: Any, travel: np.ndarray, plotted: list[slice], metric: slice | None
) -> None:
    """Color every sample by role: not plotted, plotted, metric branch."""
    in_plotted = np.zeros(len(travel), dtype=bool)
    for sweep in plotted:
        in_plotted[sweep] = True
    in_metric = np.zeros(len(travel), dtype=bool)
    if metric is not None:
        in_metric[metric] = True
    sample = np.arange(len(travel))
    panel.plot(travel, color=RAW_COLOR, alpha=0.4, linewidth=0.8, label="raw")
    for mask, color, name in (
        (~in_plotted, CUT_COLOR, "not plotted"),
        (in_plotted, USED_COLOR, "plotted branch"),
        (in_metric, METRIC_COLOR, "metric branch"),
    ):
        panel.scatter(sample[mask], travel[mask], s=3, color=color, label=name)


def _draw_sweep(
    panels: tuple[Any, Any, Any],
    spec: PlotSpec,
    curves: CurveIndex,
    corner: str,
    sweep: slice,
    options: BranchOptions,
) -> None:
    """Draw curvature, raw rate and final rate of one plotted sweep."""
    result = _sweep_gradient(
        _values(curves, corner, spec.x_signal),
        _values(curves, corner, spec.y_signal),
        _denominator(spec, curves, corner),
        sweep,
        options,
    )
    if result is None:
        return
    curvature_panel, raw_panel, final_panel = panels
    axis, slope, curvature, _, masked = result
    value = spec.from_gradient(slope) if spec.from_gradient else slope

    curvature_panel.plot(
        axis, curvature, color=RAW_COLOR, alpha=0.5, linewidth=0.8
    )
    curvature_panel.scatter(
        axis[masked], curvature[masked], s=8, color=CUT_COLOR, label="removed"
    )
    raw_panel.plot(axis, value, color=RAW_COLOR, alpha=0.5, linewidth=0.8)
    raw_panel.scatter(
        axis[masked], value[masked], s=8, color=CUT_COLOR, label="removed"
    )
    final_panel.plot(
        axis,
        np.where(masked, np.nan, value),
        color=USED_COLOR,
        linewidth=1.2,
        label="used",
    )


def _legend_unique(panel: Any) -> None:
    handles, labels = panel.get_legend_handles_labels()
    unique = dict(zip(labels, handles, strict=True))
    if unique:
        panel.legend(
            unique.values(),
            unique.keys(),
            fontsize=7,
            frameon=False,
            markerscale=2,
        )


# ---------------------------------------------------------------------------
# Shared figure helpers
# ---------------------------------------------------------------------------


def _values(curves: CurveIndex, corner: str, signal: str) -> np.ndarray:
    return np.asarray(curves[(corner, signal)]["values"], dtype=np.float64)


def _denominator(
    spec: PlotSpec, curves: CurveIndex, corner: str
) -> np.ndarray | None:
    if spec.denominator_signal is None:
        return None
    return _values(curves, corner, spec.denominator_signal)


def _index(curves: list[dict[str, object]]) -> CurveIndex:
    return {(str(item["corner"]), str(item["signal"])): item for item in curves}


def _available(spec: PlotSpec, sources: list[CurveIndex]) -> bool:
    return all(
        (corner, signal) in curves
        for curves in sources
        for corner, _ in CORNERS
        for signal in spec.signals
    )


def _new_figure(
    plt: Any,
    rows: int,
    columns: int,
    size: tuple[float, float],
    footer: str,
    metric_rows: int = 0,
) -> tuple[Any, Any]:
    if not footer and not metric_rows:
        return plt.subplots(rows, columns, figsize=size, layout="constrained")
    figure, panels = plt.subplots(rows, columns, figsize=size)
    figure.subplots_adjust(
        left=PAGE_LEFT,
        right=PAGE_RIGHT,
        top=PAGE_TOP,
        bottom=_metric_layout(footer, size[1], metric_rows)[2],
        wspace=PAGE_WSPACE,
        hspace=PAGE_HSPACE if rows <= 2 else DEBUG_HSPACE,
    )
    return figure, panels


def _metric_layout(
    footer: str, figure_height: float, rows: int
) -> tuple[float, float, float]:
    """Metric table bottom, height and panel bottom as figure fractions.

    The table sits above the footer text and the logo; the panels start above
    it with room left for their x labels.
    """
    band = _footer_height(footer, figure_height) if footer else 0.0
    text = (
        (footer.count("\n") + 2) * FOOTER_FONTSIZE * 1.2 / 72 if footer else 0.0
    )
    logo = (
        min(
            (footer.count("\n") + 2) * LOGO_PER_LINE,
            band - LOGO_MARGIN,
        )
        * figure_height
        if footer
        else 0.0
    )
    base = 0.025 * figure_height
    box_bottom = max(base + text, base + logo) + (METRIC_GAP if rows else 0.0)
    box_height = rows * METRIC_LINE_HEIGHT
    panels = max(
        band * figure_height,
        box_bottom + box_height + (XLABEL_CLEARANCE if rows else 0.0),
    )
    return (
        box_bottom / figure_height,
        box_height / figure_height,
        panels / figure_height,
    )


def _footer_height(footer: str, figure_height: float) -> float:
    """Footer band as figure fraction; sized in inches, like report pages."""
    band = min(
        FOOTER_BASE + (footer.count("\n") + 2) * FOOTER_PER_LINE, FOOTER_MAX
    )
    return band * REPORT_FIGSIZE[1] / figure_height


def _add_footer(figure: Any, footer: str) -> None:
    figure.text(
        0.02,
        0.025,
        f"{footer}\nGenerated: {datetime.now().astimezone():%Y-%m-%d %H:%M %Z}",
        fontsize=FOOTER_FONTSIZE,
        color=FOOTER_COLOR,
        va="bottom",
    )


def _metric_decimals(values: list[float]) -> int:
    """Decimals that give four significant digits to the smallest value."""
    nonzero = [abs(value) for value in values if value]
    if not nonzero:
        return 0
    return min(max(0, 3 - floor(log10(min(nonzero)))), 8)


def _add_metric_table(
    figure: Any,
    gradient: GradientDisplay,
    gradient_mm: float,
    table: list[tuple[str, list[float | None]]],
    series: Series,
    footer: str,
) -> None:
    """Table above the footer: one row per corner, one cell per source.

    Cells show the source name in its plot color followed by the metric. All
    values share one number of decimals.
    """
    height = figure.get_figheight()
    bottom, table_height, _ = _metric_layout(footer, height, len(table) + 1)
    decimals = _metric_decimals(
        [value for _, values in table for value in values if value is not None]
    )
    renderer = figure._get_renderer()
    page_width = figure.bbox.width
    row = METRIC_LINE_HEIGHT / height
    top = bottom + table_height

    def put(x: float, y: float, text: str, **style: Any) -> float:
        """Draw text; return its right edge as figure fraction."""
        artist = figure.text(
            x, y, text, fontsize=METRIC_FONTSIZE, va="top", **style
        )
        return x + artist.get_window_extent(renderer).width / page_width

    def width(text: str, **style: Any) -> float:
        artist = figure.text(0, 0, text, fontsize=METRIC_FONTSIZE, **style)
        extent = artist.get_window_extent(renderer).width / page_width
        artist.remove()
        return extent

    put(
        METRIC_LEFT,
        top,
        f"{gradient.label.capitalize()}, secant between "
        f"{-gradient_mm:+g} mm and {gradient_mm:+g} mm wheel center travel",
        color=METRIC_TITLE_COLOR,
        fontweight="bold",
    )
    labels = [
        f"{METRIC_CORNER_LABELS[corner]} {gradient.label} in {gradient.unit}:"
        for corner, _ in table
    ]
    cells = [
        [
            (name, "n/a" if value is None else f"{value:.{decimals}f}")
            for (name, *_), value in zip(series, values, strict=True)
        ]
        for _, values in table
    ]
    start = METRIC_LEFT + max(width(label) for label in labels) + 0.02
    columns = [
        max(
            width(cells[row][column][0], fontweight="bold")
            + 0.006
            + width(cells[row][column][1], fontweight="bold")
            for row in range(len(cells))
        )
        + 0.025
        for column in range(len(series))
    ]
    for index, (label, line) in enumerate(zip(labels, cells, strict=True)):
        y = top - (index + 1) * row
        put(METRIC_LEFT, y, label, color=METRIC_TITLE_COLOR)
        x = start
        for (name, value), (_, _, color), step in zip(
            line, series, columns, strict=True
        ):
            end = put(x, y, name, color=color, fontweight="bold")
            put(
                end + 0.006,
                y,
                value,
                color=METRIC_VALUE_COLOR,
                fontweight="bold",
            )
            x += step


def _save_pdf(figure: Any, plt: Any, footer: str, logo: bytes | None) -> bytes:
    output = BytesIO()
    figure.savefig(output, format="pdf", metadata=PDF_METADATA)
    figure_height = figure.get_figheight()
    plt.close(figure)
    if not footer or logo is None:
        return output.getvalue()
    slot_height = min(
        (footer.count("\n") + 2) * LOGO_PER_LINE,
        _footer_height(footer, figure_height) - LOGO_MARGIN,
    )
    return _stamp_logo(output.getvalue(), logo, slot_height)


def _stamp_logo(report: bytes, logo: bytes, slot_height: float) -> bytes:
    """Add the vector PDF logo to the same footer slot as the old PNG."""
    writer = PdfWriter(clone_from=BytesIO(report))
    page = writer.pages[0]
    logo_page = PdfReader(BytesIO(logo)).pages[0]
    page_width = float(page.mediabox.width)
    page_height = float(page.mediabox.height)
    logo_width = float(logo_page.mediabox.width)
    logo_height = float(logo_page.mediabox.height)
    slot_width = 0.18 * page_width
    slot_height *= page_height
    scale = min(slot_width / logo_width, slot_height / logo_height)
    x = 0.80 * page_width + (slot_width - logo_width * scale) / 2
    y = 0.025 * page_height + (slot_height - logo_height * scale) / 2
    transform = (
        Transformation()
        .translate(
            -float(logo_page.mediabox.left), -float(logo_page.mediabox.bottom)
        )
        .scale(scale)
        .translate(x, y)
    )
    page.merge_transformed_page(logo_page, transform)
    result = BytesIO()
    writer.write(result)
    return result.getvalue()


def _pyplot() -> Any:
    import matplotlib

    matplotlib.use("pdf")
    matplotlib.rcParams["pdf.fonttype"] = 42
    from matplotlib import pyplot

    return pyplot
