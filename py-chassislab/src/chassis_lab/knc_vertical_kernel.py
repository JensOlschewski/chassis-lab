"""Path-free numerical kernel for Rust-owned KnC vertical analysis."""

import json
from math import pi, sqrt
from typing import Any, cast

import numpy as np

from chassis_lab.knc_kernel import (
    CORNERS,
    BranchOptions,
    GradientMetric,
    Signal,
    gradient_curve,
    gradient_metrics,
    index_curves,
    mean_monotonic,
    metric_row,
    with_pad_travel,
)
from chassis_lab.knc_plot_kernel import (
    GradientDisplay,
    PlotSpec,
    SignalStyle,
    render_comparison_reports,
    render_rate_debug,
    render_reports,
    selected_specs,
)
from chassis_lab.knc_plot_kernel import (
    plot_jobs as catalog_jobs,
)

# Every vertical metric uses the configured sweep of wheel-center travel.
TRAVEL = Signal("wheel_center_z")

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
        axis=Signal("wheel_pad_z"),
    ),
    GradientMetric(
        "tire_rate",
        "n_per_mm",
        "wheel_load",
        "N/mm",
        "gradient",
        axis=Signal("wheel_pad_z"),
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
        axis=Signal("wheel_pad_z"),
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


def metrics(request: dict[str, object]) -> str:
    """Calculate secant KnC vertical metrics at symmetric travel points."""
    gradient = float(cast("float", request["gradient_mm"]))
    curves = index_curves(with_pad_travel(json.loads(str(request["curves"]))))
    options = BranchOptions.from_json(
        cast("str | dict[str, Any] | None", request.get("branch"))
    )
    results = gradient_metrics(
        curves, GRADIENT_METRICS, CORNERS, {"mm": gradient}, TRAVEL, options
    )
    results.extend(_ride_frequency_metrics(results, request, gradient))
    results.sort(key=lambda item: str(item["metric"]))
    return json.dumps({"metrics": results})


def _ride_frequency_metrics(
    results: list[dict[str, object]],
    request: dict[str, object],
    gradient: float,
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
            **metric_row(
                corner, f"ride_frequency_{corner}_hz", "Hz", "mm", gradient
            ),
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


def _with_ride_frequency(
    curves: list[dict[str, object]],
    corner_mass_kg: dict[str, float],
    options: BranchOptions,
) -> list[dict[str, object]]:
    """Add a per-sample ride_frequency curve where wheel_load and mass allow.

    f_ride(z) = 1/(2*pi) * sqrt(1000 * local wheel rate / mass), using the
    *local* d(wheel_load)/d(wheel_center_z) at every sample -- unlike the
    `ride_frequency` metric, which uses the configured secant
    `suspension_rate`. Same split as `suspension_rate` itself: the metric is
    a secant scalar, the plot is the full local-gradient curve.

    The gradient is taken per sweep -- same branch detection the
    `suspension_rate` plot uses (`options.plot_sweeps`) -- not across the
    full hysteretic travel array, otherwise the derivative blows up at
    sweep reversals where travel briefly reverses direction.
    """
    indexed = index_curves(curves)
    derived = []
    for corner in CORNERS:
        mass = corner_mass_kg.get(corner)
        travel = indexed.get((corner, "wheel_center_z"))
        load = indexed.get((corner, "wheel_load"))
        if mass is None or not (travel and load):
            continue
        travel_values = np.asarray(travel["values"], dtype=np.float64)
        load_values = np.asarray(load["values"], dtype=np.float64)
        rate = np.full_like(travel_values, np.nan)
        for sweep in options.plot_sweeps(travel_values):
            axis, values = travel_values[sweep], load_values[sweep]
            finite = np.isfinite(axis) & np.isfinite(values)
            if finite.sum() < 2:
                continue
            with np.errstate(divide="ignore", invalid="ignore"):
                sweep_rate = np.gradient(values[finite], axis[finite])
            local = np.full(finite.shape, np.nan)
            local[finite] = sweep_rate
            rate[sweep] = local
        with np.errstate(divide="ignore", invalid="ignore"):
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


def _derive_curves(
    curves: list[dict[str, object]],
    corner_mass_kg: dict[str, float],
    options: BranchOptions,
) -> list[dict[str, object]]:
    """All curves synthesized before the generic plot catalog runs.

    One series' worth of derivation -- `plot()` (one source) and
    `plot_compare()` (baseline plus each candidate, one call per series)
    both route through this instead of repeating the sequence.
    """
    curves = with_pad_travel(curves)
    return _with_ride_frequency(curves, corner_mass_kg, options)


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
    return mean_monotonic(
        np.asarray(curve["travel"], dtype=np.float64),
        np.asarray(curve["values"], dtype=np.float64),
        grid,
        source,
    )


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
    selected: list[str] | None = None,
) -> dict[str, bytes]:
    """Render vertical report catalog through generic KnC plot kernel."""
    options = BranchOptions.from_json(branch)
    curves = _derive_curves(
        json.loads(curves_json),
        json.loads(corner_mass_kg) if corner_mass_kg else {},
        options,
    )
    reports = render_reports(
        curves,
        selected_specs(PLOT_SPECS, selected, "plot"),
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
                selected_specs(PLOT_SPECS, selected, "debug:0"),
                options,
                "plots/knc/vertical",
                info,
                info["inputs"][0],
                logo,
            )
        )
    return reports


def plot_jobs(debug: bool, series_count: int = 1) -> list[str]:
    return catalog_jobs(PLOT_SPECS, (), debug, series_count)


def plot_compare(
    baseline_json: str,
    candidates_json: str,
    baseline_label: str,
    footer: str,
    logo: bytes,
    gradient_points_mm: float | None = None,
    branches_json: str | None = None,
    debug: str | None = None,
    corner_mass_kg: str | None = None,
    selected: list[str] | None = None,
) -> dict[str, bytes]:
    """Render vertical overlays.

    `branches_json` and `corner_mass_kg` each have one entry per series,
    baseline first then every candidate in order -- the same ordering
    `vertical::compare_plots` (Rust) builds both lists in.
    """
    candidates_raw = json.loads(candidates_json)
    branches = (
        [BranchOptions.from_json(item) for item in json.loads(branches_json)]
        if branches_json
        else [BranchOptions()] * (len(candidates_raw) + 1)
    )
    masses = (
        json.loads(corner_mass_kg) if corner_mass_kg else [{}] * len(branches)
    )
    baseline = _derive_curves(json.loads(baseline_json), masses[0], branches[0])
    candidates = [
        {**item, "curves": _derive_curves(item["curves"], mass, options)}
        for item, mass, options in zip(
            candidates_raw, masses[1:], branches[1:], strict=True
        )
    ]
    reports = render_comparison_reports(
        baseline,
        candidates,
        selected_specs(PLOT_SPECS, selected, "plot"),
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
            debug_specs = selected_specs(PLOT_SPECS, selected, f"debug:{index}")
            reports.update(
                render_rate_debug(
                    curves,
                    debug_specs,
                    options,
                    "",
                    info,
                    info["inputs"][index],
                    logo,
                    f"series_{index}",
                )
            )
    return reports
