"""Path-free numerical kernel for Rust-owned KnC vertical analysis."""

import json
from math import atan, degrees, pi, sqrt
from typing import Any, NamedTuple, cast

import numpy as np

from chassis_lab.knc_plot_kernel import (
    BranchOptions,
    GradientDisplay,
    PlotSpec,
    SignalStyle,
    gradient_curve,
    render_comparison_reports,
    render_rate_debug,
    render_reports,
)

CORNERS = ("front_left", "front_right", "rear_left", "rear_right")


class GradientMetric(NamedTuple):
    """Secant metric of `signal` between travel points on `axis`.

    The metric is named `<name>_<corner>_<suffix>`. `gradient` divides by the
    travel span, or by the span of `denominator` when given. `angle` uses the
    signal span only.
    """

    name: str
    suffix: str
    signal: str
    unit: str
    kind: str
    axis: str = "wheel_center_z"
    denominator: str | None = None


GRADIENT_METRICS = (
    GradientMetric("bump_steer", "deg_per_mm", "toe", "deg/mm", "gradient"),
    GradientMetric("bump_camber", "deg_per_mm", "camber", "deg/mm", "gradient"),
    GradientMetric("bump_spin", "deg_per_mm", "spin", "deg/mm", "gradient"),
    GradientMetric(
        "wheel_recession_angle", "deg", "wheel_center_x", "deg", "angle"
    ),
    GradientMetric(
        "suspension_rate", "n_per_mm", "wheel_load", "N/mm", "gradient"
    ),
    GradientMetric(
        "ride_rate",
        "n_per_mm",
        "wheel_load",
        "N/mm",
        "gradient",
        axis="wheel_pad_z",
    ),
    GradientMetric(
        "tire_rate",
        "n_per_mm",
        "wheel_load",
        "N/mm",
        "gradient",
        axis="wheel_pad_z",
        denominator="tire_deflection",
    ),
    # Stuetzwinkel: atan(-d(contact_patch_x)/d(wheel_pad_z)), same "angle" math
    # as wheel_recession_angle but measured at the contact patch instead of
    # the wheel center.
    GradientMetric(
        "brake_support_angle",
        "deg",
        "contact_patch_x",
        "deg",
        "angle",
        axis="wheel_pad_z",
    ),
)

PLOT_SIGNALS = {
    "wheel_center_x": SignalStyle("Wheel center X", "mm"),
    "wheel_center_y": SignalStyle("Wheel center Y", "mm"),
    "wheel_center_z": SignalStyle("Wheel center Z", "mm"),
    "wheel_pad_z": SignalStyle("Wheel pad Z", "mm"),
    "tire_deflection": SignalStyle("Tire deflection", "mm"),
    "wheel_load": SignalStyle("Wheel load", "newton"),
    "toe": SignalStyle("Toe", "deg"),
    "camber": SignalStyle("Camber", "deg"),
    "spin": SignalStyle("Spin", "deg"),
    "suspension_rate": SignalStyle("Suspension rate", "newton/mm"),
    "ride_rate": SignalStyle("Ride rate", "newton/mm"),
    "tire_rate": SignalStyle("Tire rate", "newton/mm"),
    "wheel_recession_angle": SignalStyle("Wheel recession angle", "deg"),
    "contact_patch_x": SignalStyle("Contact patch X", "mm"),
    "contact_patch_y": SignalStyle("Contact patch Y", "mm"),
    "brake_support_angle": SignalStyle("Brake support angle", "deg"),
    "ride_frequency": SignalStyle("Ride frequency", "Hz"),
}


# ---------------------------------------------------------------------------
# Plot catalog
# ---------------------------------------------------------------------------


def _recession_angle(gradient: np.ndarray) -> np.ndarray:
    return np.degrees(np.arctan(-gradient))


def _wheel_recession_angle(
    x: np.ndarray,
    y: np.ndarray,
    denominator: np.ndarray | None,
    sweeps: list[slice],
    options: BranchOptions,
) -> tuple[np.ndarray, np.ndarray]:
    """Local wheel recession angle per sweep, edge-trimmed like the rates."""
    x_sweeps, gradient = gradient_curve(x, y, denominator, sweeps, options)
    return x_sweeps, _recession_angle(gradient)


# Add or modify plots here.
#  `name` is the output filename
#  `title` is the figure heading.
#  `branch` optionally overrides BranchOptions for that plot only, e.g.
#  branch={"direction": "fall", "index": 1, "plot_branches": "selected"} or
#  branch={"edge_width": 100, "edge_percentile": 0.9}.
#  Fields: see BranchOptions.
PLOT_SPECS = (
    PlotSpec(
        "knc_vertical_wcz_vs_wcx",
        "Bounce - Vertical Wheel Center Travel / "
        "Longitudinal Disp. of Wheel Center",
        "wheel_center_z",
        "wheel_center_x",
        gradient=GradientDisplay("wheel recession angle", "deg", "angle"),
    ),
    PlotSpec(
        "knc_vertical_wcz_vs_wcy",
        "Bounce - Vertical Wheel Center Travel / Lateral Disp. of Wheel Center",
        "wheel_center_z",
        "wheel_center_y",
    ),
    PlotSpec(
        "knc_vertical_recession_angle",
        "Bounce - Vertical Wheel center Travel / Wheel recession angle",
        "wheel_center_z",
        "wheel_center_x",
        "wheel_recession_angle",
        branch={"edge_width": 100, "edge_percentile": 0.95},
        transform=_wheel_recession_angle,
        from_gradient=_recession_angle,
    ),
    PlotSpec(
        "knc_vertical_wheel_load",
        "Bounce - Vertical Wheel center Travel / Wheel load",
        "wheel_center_z",
        "wheel_load",
        optional=True,
        gradient=GradientDisplay("suspension rate", "N/mm"),
    ),
    PlotSpec(
        "knc_vertical_suspension_rate",
        "Bounce - Vertical Wheel center Travel / Suspension rate",
        "wheel_center_z",
        "wheel_load",
        "suspension_rate",
        transform=gradient_curve,
        optional=True,
        y_limits=(0, 100),
    ),
    PlotSpec(
        "knc_vertical_ride_rate",
        "Bounce - Vertical Wheel pad Travel / Ride rate",
        "wheel_pad_z",
        "wheel_load",
        "ride_rate",
        branch={"edge_width": 100, "edge_percentile": 0.99},
        transform=gradient_curve,
        optional=True,
        y_limits=(0, 100),
    ),
    PlotSpec(
        "knc_vertical_tire_rate",
        "Bounce - Vertical Wheel pad Travel / Tire rate",
        "wheel_pad_z",
        "wheel_load",
        "tire_rate",
        transform=gradient_curve,
        optional=True,
        denominator_signal="tire_deflection",
    ),
    PlotSpec(
        "knc_vertical_toe",
        "Bounce - Vertical Wheel center Travel / Toe",
        "wheel_center_z",
        "toe",
        optional=True,
        gradient=GradientDisplay("bump steer", "deg/mm"),
    ),
    PlotSpec(
        "knc_vertical_camber",
        "Bounce - Vertical Wheel center Travel / Camber",
        "wheel_center_z",
        "camber",
        optional=True,
        gradient=GradientDisplay("bump camber", "deg/mm"),
    ),
    PlotSpec(
        "knc_vertical_spin",
        "Bounce - Vertical Wheel center Travel / Spin",
        "wheel_center_z",
        "spin",
        optional=True,
        gradient=GradientDisplay("bump spin", "deg/mm"),
    ),
    PlotSpec(
        "knc_vertical_wpz_vs_cpx",
        "Bounce - Vertical Wheel pad Travel / Longitudinal Disp. of Patch",
        "wheel_pad_z",
        "contact_patch_x",
        optional=True,
    ),
    PlotSpec(
        "knc_vertical_wpz_vs_cpy",
        "Bounce - Vertical Wheel pad Travel / Lateral Disp. of Contact Patch",
        "wheel_pad_z",
        "contact_patch_y",
        optional=True,
    ),
    PlotSpec(
        "knc_vertical_brake_support_angle",
        "Bounce - Vertical Wheel pad Travel / Brake support angle",
        "wheel_pad_z",
        "contact_patch_x",
        "brake_support_angle",
        branch={"edge_width": 100, "edge_percentile": 0.95},
        transform=_wheel_recession_angle,
        from_gradient=_recession_angle,
        optional=True,
    ),
    PlotSpec(
        "knc_vertical_ride_frequency",
        "Bounce - Vertical Wheel center Travel / Ride frequency",
        "wheel_center_z",
        "ride_frequency",
        optional=True,
        y_limits=(0, 2),
    ),
)


class VerticalKernelError(ValueError):
    """Curve data cannot be compared safely."""


def metrics(request: dict[str, object]) -> str:
    """Calculate secant KnC vertical metrics at symmetric travel points."""
    gradient = float(cast("float", request["gradient_mm"]))
    curves = _with_pad_travel(json.loads(str(request["curves"])))
    indexed = {(item["corner"], item["signal"]): item for item in curves}
    grid = np.array([-gradient, gradient], dtype=np.float64)
    options = BranchOptions.from_json(
        cast("str | dict[str, Any] | None", request.get("branch"))
    )
    results = []
    for corner in CORNERS:
        for metric in GRADIENT_METRICS:
            result = {
                "corner": corner,
                "metric": f"{metric.name}_{corner}_{metric.suffix}",
                "unit": metric.unit,
                "value": None,
                "lower": None,
                "upper": None,
                "reason": None,
            }
            try:
                lower, upper = _secant_points(
                    indexed, corner, metric.signal, grid, metric.axis, options
                )
                span = 2.0 * gradient
                if metric.denominator is not None:
                    span = _span(
                        _secant_points(
                            indexed,
                            corner,
                            metric.denominator,
                            grid,
                            metric.axis,
                            options,
                        )
                    )
            except VerticalKernelError as error:
                result["reason"] = str(error)
            else:
                delta = upper - lower
                result["lower"] = lower
                result["upper"] = upper
                result["value"] = {
                    "gradient": delta / span,
                    "angle": degrees(atan(-delta / (2.0 * gradient))),
                }[metric.kind]
            results.append(result)
    results.extend(_ride_frequency_metrics(results, request))
    results.sort(key=lambda item: str(item["metric"]))
    return json.dumps({"metrics": results})


def _ride_frequency_metrics(
    results: list[dict[str, object]], request: dict[str, object]
) -> list[dict[str, object]]:
    """f_ride = 1/(2*pi) * sqrt(1000 * K_w / m), K_w = suspension_rate, m per
    corner.

    Reverse-engineered from a real Lommel bounce-rig ACD measurement: its
    `*rfreq` channel matches this formula exactly, against the wheel rate
    (`suspension_rate`, not the tire-combined `ride_rate`) and a per-corner
    mass identical to SVC's `<corner>_mass_kg` for the same vehicle. `m` is
    resolved in Rust (`gradient_metrics` in `vertical.rs`): the run's own SVC
    output if present, else the `sprung_mass_<corner>_kg` config fallback.
    """
    suspension_rate = {
        result["corner"]: result["value"]
        for result in results
        if str(result["metric"]).startswith("suspension_rate_")
        and str(result["metric"]).endswith("_n_per_mm")
    }
    corner_mass_kg = json.loads(str(request.get("corner_mass_kg") or "{}"))
    corner_mass_source = json.loads(
        str(request.get("corner_mass_source") or "{}")
    )
    ride_frequency_results = []
    for corner in CORNERS:
        rate = suspension_rate.get(corner)
        mass = corner_mass_kg.get(corner)
        result = {
            "corner": corner,
            "metric": f"ride_frequency_{corner}_hz",
            "unit": "Hz",
            "value": None,
            "lower": None,
            "upper": None,
            "reason": None,
            "suspension_rate_n_per_mm": rate,
            "corner_mass_kg": mass,
            "corner_mass_source": corner_mass_source.get(corner),
        }
        if rate is None:
            result["reason"] = "suspension_rate unavailable"
        elif mass is None:
            result["reason"] = (
                f"corner mass unavailable (no SVC {corner}_mass_kg in this "
                f"run, no sprung_mass_{corner}_kg configured)"
            )
        elif rate <= 0.0 or mass <= 0.0:
            result["reason"] = (
                "suspension_rate and corner mass must be positive"
            )
        else:
            result["value"] = (1.0 / (2.0 * pi)) * sqrt(1000.0 * rate / mass)
        ride_frequency_results.append(result)
    return ride_frequency_results


def _span(points: tuple[float, float]) -> float:
    span = points[1] - points[0]
    if span == 0.0:
        raise VerticalKernelError("denominator signal does not change")
    return span


def _with_pad_travel(
    curves: list[dict[str, object]],
) -> list[dict[str, object]]:
    """Add wheel pad Z as wheel-center Z plus tire deflection when absent."""
    indexed = {(item["corner"], item["signal"]): item for item in curves}
    derived = []
    for corner in CORNERS:
        center = indexed.get((corner, "wheel_center_z"))
        deflection = indexed.get((corner, "tire_deflection"))
        if (corner, "wheel_pad_z") in indexed or not (center and deflection):
            continue
        values = np.asarray(center["values"], dtype=np.float64) + np.asarray(
            deflection["values"], dtype=np.float64
        )
        derived.append(
            {
                "corner": corner,
                "signal": "wheel_pad_z",
                "values": values.tolist(),
                "unit": "mm",
            }
        )
    return [*curves, *derived]


def _with_ride_frequency(
    curves: list[dict[str, object]], corner_mass_kg: dict[str, float]
) -> list[dict[str, object]]:
    """Add a per-sample ride_frequency curve where wheel_load and mass allow.

    f_ride(z) = 1/(2*pi) * sqrt(1000 * local wheel rate / mass), using the
    *local* d(wheel_load)/d(wheel_center_z) at every sample -- unlike the
    `ride_frequency` metric, which uses the configured secant
    `suspension_rate`. Same split as `suspension_rate` itself: the metric is
    a secant scalar, the plot is the full local-gradient curve.
    """
    indexed = {(item["corner"], item["signal"]): item for item in curves}
    derived = []
    for corner in CORNERS:
        mass = corner_mass_kg.get(corner)
        travel = indexed.get((corner, "wheel_center_z"))
        load = indexed.get((corner, "wheel_load"))
        if mass is None or not (travel and load):
            continue
        travel_values = np.asarray(travel["values"], dtype=np.float64)
        load_values = np.asarray(load["values"], dtype=np.float64)
        with np.errstate(divide="ignore", invalid="ignore"):
            rate = np.gradient(load_values, travel_values)
            frequency = np.where(
                rate > 0.0,
                (1.0 / (2.0 * pi)) * np.sqrt(1000.0 * rate / mass),
                np.nan,
            )
        derived.append(
            {
                "corner": corner,
                "signal": "ride_frequency",
                "values": frequency.tolist(),
                "unit": "Hz",
            }
        )
    return [*curves, *derived]


def _secant_points(
    indexed: dict[tuple[str, str], dict[str, object]],
    corner: str,
    signal: str,
    grid: np.ndarray,
    axis: str,
    options: BranchOptions,
) -> tuple[float, float]:
    for required in (axis, signal):
        if (corner, required) not in indexed:
            raise VerticalKernelError(
                f"missing canonical signal {corner}_{required}"
            )
    travel = np.asarray(indexed[(corner, axis)]["values"], dtype=np.float64)
    response = np.asarray(indexed[(corner, signal)]["values"], dtype=np.float64)
    if len(travel) != len(response):
        raise VerticalKernelError(
            f"{corner}_{signal} and {corner}_{axis} lengths differ"
        )
    center = np.asarray(
        indexed[(corner, "wheel_center_z")]["values"], dtype=np.float64
    )
    if len(center) != len(travel):
        raise VerticalKernelError(
            f"{corner}_{axis} and {corner}_wheel_center_z lengths differ"
        )
    sweep = options.metric_sweep(center)
    if sweep is None:
        raise VerticalKernelError(
            f"{corner} has no {options.direction} branch {options.index}"
        )
    travel, response = travel[sweep], response[sweep]
    try:
        lower, upper = _interpolate(travel, response, grid, corner)
    except VerticalKernelError:
        raise VerticalKernelError(
            f"{corner}_{signal} does not cover {axis} travel "
            f"{grid[0]:g}..{grid[-1]:g} mm"
        ) from None
    return float(lower), float(upper)


def evaluate(request: dict[str, object]) -> str:
    """Compare canonical KnC vertical curves on requested travel grid."""
    grid = np.asarray(request["grid"], dtype=np.float64)
    curves = json.loads(str(request["curves"]))
    comparisons = []
    for corner in CORNERS:
        for axis in ("x", "y", "z"):
            curve = curves[f"{corner}:{axis}"]
            baseline = _curve(curve["baseline"], grid, f"baseline {corner}")
            candidate = _curve(curve["candidate"], grid, f"candidate {corner}")
            baseline_base = _at_zero(grid, baseline)
            candidate_base = _at_zero(grid, candidate)
            baseline -= baseline_base
            candidate -= candidate_base
            error = candidate - baseline
            comparisons.append(
                {
                    "corner": corner,
                    "axis": axis,
                    "travel_mm": grid.tolist(),
                    "baseline_mm": baseline.tolist(),
                    "candidate_mm": candidate.tolist(),
                    "baseline_baseline_mm": float(baseline_base),
                    "candidate_baseline_mm": float(candidate_base),
                    "baseline_error_mm": abs(
                        float(candidate_base - baseline_base)
                    ),
                    "maximum_reference_mm": float(np.max(np.abs(baseline))),
                    "maximum_candidate_mm": float(np.max(np.abs(candidate))),
                    "rmse_mm": sqrt(float(np.mean(np.square(error)))),
                    "maximum_error_mm": float(np.max(np.abs(error))),
                }
            )
    return json.dumps({"comparisons": comparisons})


def _curve(
    curve: dict[str, list[float]], grid: np.ndarray, source: str
) -> np.ndarray:
    return _mean_monotonic(
        np.asarray(curve["travel"], dtype=np.float64),
        np.asarray(curve["values"], dtype=np.float64),
        grid,
        source,
    )


def _mean_monotonic(
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
    """Interpolate finite samples after sorting and duplicate averaging."""
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


def plot(
    curves_json: str,
    source_label: str = "Simulation",
    footer: str = "",
    logo: bytes | None = None,
    gradient_points_mm: float | None = None,
    branch: str | None = None,
    debug: str | None = None,
    corner_mass_kg: str | None = None,
) -> dict[str, bytes]:
    """Render vertical report catalog through generic KnC plot kernel."""
    curves = _with_pad_travel(json.loads(curves_json))
    curves = _with_ride_frequency(
        curves, json.loads(corner_mass_kg) if corner_mass_kg else {}
    )
    options = BranchOptions.from_json(branch)
    reports = render_reports(
        curves,
        PLOT_SPECS,
        PLOT_SIGNALS,
        source_label,
        "plots/knc/vertical",
        footer,
        logo,
        gradient_points_mm,
        options,
    )
    if debug:
        info = json.loads(debug)
        reports.update(
            render_rate_debug(
                curves,
                PLOT_SPECS,
                options,
                "plots/knc/vertical",
                info,
                info["inputs"][0],
                logo,
            )
        )
    return reports


def plot_compare(
    baseline_json: str,
    candidates_json: str,
    baseline_label: str,
    footer: str,
    logo: bytes,
    gradient_points_mm: float | None = None,
    branches_json: str | None = None,
    debug: str | None = None,
) -> dict[str, bytes]:
    """Render vertical overlays; `branches_json` has one entry per series."""
    baseline = _with_pad_travel(json.loads(baseline_json))
    candidates = [
        {**item, "curves": _with_pad_travel(item["curves"])}
        for item in json.loads(candidates_json)
    ]
    branches = (
        [BranchOptions.from_json(item) for item in json.loads(branches_json)]
        if branches_json
        else [BranchOptions()] * (len(candidates) + 1)
    )
    reports = render_comparison_reports(
        baseline,
        candidates,
        PLOT_SPECS,
        PLOT_SIGNALS,
        baseline_label,
        footer,
        logo,
        gradient_points_mm,
        branches,
    )
    if debug:
        info = json.loads(debug)
        series = [baseline, *(item["curves"] for item in candidates)]
        for index, (curves, options) in enumerate(
            zip(series, branches, strict=True)
        ):
            reports.update(
                render_rate_debug(
                    curves,
                    PLOT_SPECS,
                    options,
                    "",
                    info,
                    info["inputs"][index],
                    logo,
                    f"series_{index}",
                )
            )
    return reports
