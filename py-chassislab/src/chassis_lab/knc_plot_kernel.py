"""Reusable, path-free plotting primitives for KnC report kernels."""

from __future__ import annotations

import warnings
from collections.abc import Callable
from dataclasses import dataclass, field, replace
from datetime import datetime
from functools import lru_cache
from io import BytesIO
from math import floor, log10
from typing import Any, NamedTuple, cast

import numpy as np
from pypdf import PdfReader, PdfWriter, Transformation

from chassis_lab.knc_kernel import (
    BranchOptions,
    CurveIndex,
    KncKernelError,
    Signal,
    gradient_curve,
    secant_points,
    sweep_gradient,
)

# ---------------------------------------------------------------------------
# Settings
# ---------------------------------------------------------------------------

CORNERS = (
    ("front_left", "Front left"),
    ("front_right", "Front right"),
    ("rear_left", "Rear left"),
    ("rear_right", "Rear right"),
)


def plot_jobs(
    specs: tuple[PlotSpec, ...],
    axle_specs: tuple[AxlePlotSpec, ...],
    debug: bool,
    series_count: int,
) -> list[str]:
    """Independent PDFs; each spec still contains all four corner panels."""
    jobs = [f"plot:{spec.name}" for spec in specs]
    jobs += [f"axle:{spec.name}" for spec in axle_specs]
    if debug:
        jobs += [
            f"debug:{index}:{spec.name}"
            for index in range(series_count)
            for spec in specs
            if spec.is_rate
        ]
    return jobs


def selected_specs(
    specs: tuple[Any, ...], selected: list[str] | None, kind: str
) -> tuple[Any, ...]:
    if selected is None:
        return specs
    return tuple(spec for spec in specs if f"{kind}:{spec.name}" in selected)


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
# Signal and gradient display
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


# (x, y, denominator, sweeps, branch options) -> plotted (x, y)
Transform = Callable[
    [np.ndarray, np.ndarray, np.ndarray | None, list[slice], BranchOptions],
    tuple[np.ndarray, np.ndarray],
]


class Source(NamedTuple):
    """One plotted source: curves and line color."""

    label: str
    curves: CurveIndex
    color: str


Series = list[Source]


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
    # Roll uses a body-roll sweep even when the displayed x is wheel travel
    # or an axle-level suspension angle. Vertical keeps the travel defaults.
    x_at: str = "self"
    sweep: Signal = Signal("wheel_center_z")
    gradient_unit: str = "mm"

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


@dataclass(frozen=True)
class AxlePanel:
    location: str
    x_signal: str
    y_signal: str
    title: str
    x_label: str
    y_label: str
    x_location: str | None = None


@dataclass(frozen=True)
class AxlePlotSpec:
    name: str
    title: str
    panels: tuple[AxlePanel, AxlePanel, AxlePanel]


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
    gradient_points_deg: float | None = None,
    warn_missing: bool = False,
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
        gradient_points_deg,
        warn_missing,
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
    gradient_points_deg: float | None = None,
    warn_missing: bool = False,
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
        gradient_points_deg,
        warn_missing,
    )


def render_axle_reports(
    baseline: list[dict[str, object]],
    candidates: list[dict[str, object]],
    specs: tuple[AxlePlotSpec, ...],
    baseline_label: str,
    footer: str,
    logo: bytes | None,
    output_prefix: str = "",
    warn_missing: bool = True,
) -> dict[str, bytes]:
    """Three-panel axle reports from already derived canonical curves."""
    series = [Source(baseline_label, _index(baseline), BASELINE_COLOR)]
    series.extend(
        Source(
            str(item["label"]),
            _index(cast("list[dict[str, object]]", item["curves"])),
            CANDIDATE_COLORS[index % len(CANDIDATE_COLORS)],
        )
        for index, item in enumerate(candidates)
    )
    reports = {}
    skipped = []
    for spec in specs:
        if not all(
            (location, signal) in source.curves
            for source in series
            for panel in spec.panels
            for location, signal in (
                (panel.x_location or panel.location, panel.x_signal),
                (panel.location, panel.y_signal),
            )
        ):
            skipped.append(spec.name)
            continue
        figure, panels = _new_axle_figure(footer)
        figure.suptitle(spec.title, fontsize=16)
        for axes, panel in zip(panels, spec.panels, strict=True):
            for source in series:
                x = _values(
                    source.curves,
                    panel.x_location or panel.location,
                    panel.x_signal,
                )
                y = _values(source.curves, panel.location, panel.y_signal)
                if len(x) != len(y):
                    raise ValueError(
                        f"{panel.location} {panel.x_signal} and "
                        f"{panel.y_signal} lengths differ"
                    )
                axes.plot(
                    x, y, color=source.color, linewidth=1.8, label=source.label
                )
            axes.set_title(panel.title, loc="left", fontweight="bold")
            axes.set_xlabel(panel.x_label)
            axes.set_ylabel(panel.y_label)
            axes.legend(loc="best", frameon=False)
        if footer:
            _add_footer(figure, footer)
        filename = f"{spec.name}.pdf"
        path = f"{output_prefix}/{filename}" if output_prefix else filename
        reports[path] = _save_pdf(figure, _pyplot(), footer, logo)
    if skipped and warn_missing:
        warnings.warn(
            "KnC axle plots skipped because signals or geometry are missing: "
            + ", ".join(skipped),
            stacklevel=2,
        )
    return reports


def _new_axle_figure(footer: str) -> tuple[Any, tuple[Any, Any, Any]]:
    """One wide top panel and two equal lower panels, with footer room."""
    plt = _pyplot()
    figure = plt.figure(
        figsize=REPORT_FIGSIZE, layout="constrained" if not footer else None
    )
    if footer:
        grid = figure.add_gridspec(
            2,
            2,
            left=PAGE_LEFT,
            right=PAGE_RIGHT,
            top=PAGE_TOP,
            bottom=_metric_layout(footer, REPORT_FIGSIZE[1], 0)[2],
            wspace=PAGE_WSPACE,
            hspace=PAGE_HSPACE,
        )
    else:
        grid = figure.add_gridspec(2, 2)
    return figure, (
        figure.add_subplot(grid[0, :]),
        figure.add_subplot(grid[1, 0]),
        figure.add_subplot(grid[1, 1]),
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
    gradient_points_deg: float | None,
    warn_missing: bool,
) -> dict[str, bytes]:
    reports = {}
    skipped = []
    for spec in specs:
        if not _available(spec, [source.curves for source in series]):
            if spec.optional:
                skipped.append(spec.name)
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
            gradient_points_deg,
        )
    if skipped and warn_missing:
        warnings.warn(
            "KnC plots skipped because canonical signals are missing: "
            + ", ".join(skipped),
            stacklevel=2,
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
    gradient_points_deg: float | None,
) -> bytes:
    plt = _pyplot()
    half = (
        gradient_points_deg
        if spec.gradient_unit == "deg"
        else gradient_points_mm
    )
    gradient = spec.gradient if half is not None and half > 0 else None
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
            if gradient and half is not None:
                secant = _secant(spec, curves, corner, options, half)
                values.append(
                    None
                    if secant is None
                    else _draw_secant(
                        panel,
                        gradient,
                        secant,
                        half,
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
    if gradient and half is not None:
        _add_metric_table(
            figure,
            gradient,
            half,
            table,
            series,
            footer,
            spec.gradient_unit,
            (
                "wheel center travel"
                if spec.sweep == Signal("wheel_center_z")
                else x_style.label
            ),
        )
    return _save_pdf(figure, plt, footer, logo)


def _secant(
    spec: PlotSpec,
    curves: CurveIndex,
    corner: str,
    options: BranchOptions,
    half: float,
) -> tuple[float, float] | None:
    """Same interpolated secant and sweep as the scalar KnC metric."""
    try:
        return secant_points(
            curves,
            corner,
            spec.y_signal,
            Signal(spec.x_signal, spec.x_at),
            spec.sweep,
            half,
            spec.gradient_unit,
            options,
        )
    except KncKernelError:
        return None


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
    x = _values(
        curves, Signal(spec.x_signal, spec.x_at).location(corner), spec.x_signal
    )
    y = _values(curves, corner, spec.y_signal)
    if len(x) != len(y):
        raise ValueError(
            f"{corner} {spec.y_signal} and {spec.x_signal} lengths differ"
        )
    if spec.transform is None:
        return x, y
    denominator = _denominator(spec, curves, corner)
    sweeps = options.plot_sweeps(
        _values(curves, spec.sweep.location(corner), spec.sweep.name)
    )
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


def render_contact_path_debug(
    samples: dict[str, np.ndarray],
    axle: str,
    options: BranchOptions,
    info: dict[str, Any],
    input_path: str,
    logo: bytes | None,
) -> bytes:
    """Show body-roll branches and both contact slopes used by roll center.

    Each final panel excludes the union of both sides' turnaround cuts, just
    as the geometric roll-center calculation does.
    """
    footer = _debug_footer(info, input_path, options)
    plt = _pyplot()
    figure, panels = _new_figure(plt, 2, 4, (DEBUG_FIGSIZE[0], 8), footer)
    figure.suptitle(
        f"Roll - {axle.capitalize()} roll center - debug", fontsize=16
    )
    roll = samples["roll"]
    plotted = options.plot_sweeps(roll)
    for row, side in zip(panels, ("left", "right"), strict=True):
        branch, curvature_panel, raw_panel, final_panel = row
        branch.set_title(f"{side.capitalize()}: branch detection", loc="left")
        curvature_panel.set_title("Second derivative |d²y/dz²|", loc="left")
        raw_panel.set_title("Raw contact slope", loc="left")
        final_panel.set_title("Final contact slope (both paths)", loc="left")
        _draw_branches(branch, roll, plotted, options.metric_sweep(roll))
        for sweep in plotted:
            if not all(
                np.isfinite(values[sweep]).all() for values in samples.values()
            ):
                continue
            results = {
                name: sweep_gradient(
                    samples[f"z_{name}"],
                    samples[f"y_{name}"],
                    None,
                    sweep,
                    options,
                )
                for name in ("left", "right")
            }
            if any(result is None for result in results.values()):
                continue
            own = results[side]
            assert own is not None
            combined = np.zeros(len(own[1]), dtype=bool)
            for result in results.values():
                assert result is not None
                combined |= result[4] | ~np.isfinite(result[1])
            axis, slope, curvature, _, _ = own
            curvature_panel.plot(axis, curvature, color=RAW_COLOR, alpha=0.5)
            curvature_panel.scatter(
                axis[combined],
                curvature[combined],
                s=8,
                color=CUT_COLOR,
                label="removed",
            )
            raw_panel.plot(axis, slope, color=RAW_COLOR, alpha=0.5)
            raw_panel.scatter(
                axis[combined],
                slope[combined],
                s=8,
                color=CUT_COLOR,
                label="removed",
            )
            final_panel.plot(
                axis,
                np.where(combined, np.nan, slope),
                color=USED_COLOR,
                linewidth=1.2,
                label="used",
            )
        branch.set_xlabel("sample")
        branch.set_ylabel("roll_angle [deg]")
        for panel in (curvature_panel, raw_panel, final_panel):
            panel.set_xlabel("wheel_pad_z [mm]")
        curvature_panel.set_ylabel("|d²y/dz²|")
        curvature_panel.set_yscale("symlog")
        raw_panel.set_ylabel("d(lateral)/d(wheel_pad_z)")
        final_panel.set_ylabel("d(lateral)/d(wheel_pad_z)")
        for panel in row:
            _legend_unique(panel)
    _add_footer(figure, footer)
    return _save_pdf(figure, plt, footer, logo)


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

        sweep_values = _values(
            curves, spec.sweep.location(corner), spec.sweep.name
        )
        plotted = options.plot_sweeps(sweep_values)
        _draw_branches(
            branch, sweep_values, plotted, options.metric_sweep(sweep_values)
        )
        for sweep in plotted:
            _draw_sweep(
                (curvature, raw, final), spec, curves, corner, sweep, options
            )

        branch.set_xlabel("sample")
        sweep_unit = "deg" if spec.sweep.name == "roll_angle" else "mm"
        branch.set_ylabel(f"{spec.sweep.name} [{sweep_unit}]")
        for panel in (curvature, raw, final):
            panel.set_xlabel(f"{spec.x_signal} [{spec.gradient_unit}]")
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
    result = sweep_gradient(
        _values(
            curves,
            Signal(spec.x_signal, spec.x_at).location(corner),
            spec.x_signal,
        ),
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
    for curves in sources:
        for corner, _ in CORNERS:
            if (
                (
                    Signal(spec.x_signal, spec.x_at).location(corner),
                    spec.x_signal,
                )
                not in curves
                or (corner, spec.y_signal) not in curves
                or (
                    spec.denominator_signal is not None
                    and (corner, spec.denominator_signal) not in curves
                )
                or (
                    spec.transform is not None
                    and (spec.sweep.location(corner), spec.sweep.name)
                    not in curves
                )
            ):
                return False
    return True


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
    unit: str = "mm",
    axis_label: str = "wheel center travel",
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
        f"{-gradient_mm:+g} {unit} and {gradient_mm:+g} {unit} "
        f"{axis_label.lower()}",
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


@lru_cache(maxsize=1)
def _parse_logo(logo: bytes) -> Any:
    """Parse the embedded logo PDF once; every report reuses the same page.

    One `plot()` call stamps a logo on every generated figure (14+ for
    vertical, more for roll and debug), always with the same `logo` bytes,
    so re-parsing it with `PdfReader` each time is pure waste.
    """
    return PdfReader(BytesIO(logo)).pages[0]


def _stamp_logo(report: bytes, logo: bytes, slot_height: float) -> bytes:
    """Add the vector PDF logo to the same footer slot as the old PNG."""
    writer = PdfWriter(clone_from=BytesIO(report))
    page = writer.pages[0]
    logo_page = _parse_logo(logo)
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
