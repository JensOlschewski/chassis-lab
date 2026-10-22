"""KnC roll metrics called by Rust's ``analysis/knc.rs::gradient_metrics``.

Rust maps source signals, applies conventions and static/measurement offsets,
then calls ``metrics(request)`` through embedded PyO3. Python restores absolute
geometry where needed, derives missing signals, and calculates scalar metrics.
Rust writes reports. Corner plots reuse the vertical plot renderer.

Shared with vertical in ``knc_kernel.py``: branch selection, restoring offsets,
and ``gradient_metrics``. Each gradient is the secant
    [response(+half) - response(-half)] / (2 * half)
after interpolation on the selected sweep, using its metric-specific axis.
This module defines roll-only inputs, derived curves, and metric formulas.
"""

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
    gradient_curve,
    gradient_metrics,
    index_curves,
    metric_row,
    metric_sweep,
    signal_values,
    sweep_gradient,
    with_pad_travel,
)
from chassis_lab.knc_plot_kernel import (
    AxlePanel,
    AxlePlotSpec,
    GradientDisplay,
    PlotSpec,
    SignalStyle,
    render_axle_reports,
    render_comparison_reports,
    render_contact_path_debug,
    render_rate_debug,
    render_reports,
)

# Every metric selects samples from the configured body-roll sweep. Its secant
# axis can differ: wheel travel (mm), suspension roll (deg), or body roll (deg).
ROLL_ANGLE = Signal("roll_angle", "total")
SUSPENSION_ROLL_ANGLE = Signal("suspension_roll_angle", "axle")
WHEEL_TRAVEL = Signal("wheel_center_z")

# Roll-specific metric catalog. knc_kernel.gradient_metrics evaluates every
# entry for every corner on the selected body-roll sweep. At each corner:
#   steer/camber per mm: Δtoe or Δcamber / Δwheel-center Z, at ±gradient_mm.
#   steer/camber per deg: Δtoe or Δcamber / Δaxle suspension roll,
#                         at ±gradient_deg of suspension roll.
#   wheel rate: Δwheel load / Δwheel-center Z, at ±gradient_mm.
# These are roll-test metrics, not rates from the vertical test's bounce sweep.
CORNER_METRICS = (
    GradientMetric(
        "roll_steer", "deg_per_mm", "toe", "deg/mm", axis=WHEEL_TRAVEL
    ),
    GradientMetric(
        "roll_camber", "deg_per_mm", "camber", "deg/mm", axis=WHEEL_TRAVEL
    ),
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
    GradientMetric(
        "roll_wheel_rate", "n_per_mm", "wheel_load", "N/mm", axis=WHEEL_TRAVEL
    ),
)

# Roll stiffness: Δderived moment / Δbody roll at ±gradient_deg. The moment
# is derived below, not read from mapped source moment channels.
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

# Preferred lateral contact position for the roll center, then the fallback.
ROLL_CENTER_LATERAL = ("center_of_pressure_y", "contact_patch_y")

PLOT_SIGNALS = {
    "wheel_center_z": SignalStyle("Wheel center Z", "mm"),
    "wheel_center_x": SignalStyle("Wheel center X", "mm"),
    "wheel_center_y": SignalStyle("Wheel center Y", "mm"),
    "wheel_pad_z": SignalStyle("Wheel pad Z", "mm"),
    "contact_patch_x": SignalStyle("Contact patch X", "mm"),
    "contact_patch_y": SignalStyle("Contact patch Y", "mm"),
    "roll_angle": SignalStyle("Body roll angle", "deg"),
    "suspension_roll_angle": SignalStyle("Suspension roll angle", "deg"),
    "toe": SignalStyle("Toe", "deg"),
    "camber": SignalStyle("Camber", "deg"),
    "wheel_load": SignalStyle("Wheel load", "newton"),
    "suspension_rate": SignalStyle("Roll suspension rate", "newton/mm"),
    "ride_rate": SignalStyle("Roll ride rate", "newton/mm"),
}


def _plot_spec(
    name: str,
    x: str,
    y: str,
    gradient: GradientDisplay | None = None,
    x_at: str = "self",
) -> PlotSpec:
    return PlotSpec(
        f"knc_roll_{name}",
        f"Roll - {PLOT_SIGNALS[x].label} / {PLOT_SIGNALS[y].label}",
        x,
        y,
        optional=True,
        gradient=gradient,
        x_at=x_at,
        sweep=ROLL_ANGLE,
        gradient_unit=(
            "deg" if x in ("roll_angle", "suspension_roll_angle") else "mm"
        ),
    )


PLOT_SPECS = (
    _plot_spec(
        "toe", "wheel_center_z", "toe", GradientDisplay("roll steer", "deg/mm")
    ),
    _plot_spec(
        "toe_vs_roll_angle",
        "roll_angle",
        "toe",
        GradientDisplay("body-roll steer", "deg/deg"),
        "total",
    ),
    _plot_spec(
        "toe_vs_suspension_roll_angle",
        "suspension_roll_angle",
        "toe",
        GradientDisplay("roll steer", "deg/deg"),
        "axle",
    ),
    _plot_spec(
        "camber",
        "wheel_center_z",
        "camber",
        GradientDisplay("roll camber", "deg/mm"),
    ),
    _plot_spec(
        "camber_vs_roll_angle",
        "roll_angle",
        "camber",
        GradientDisplay("body-roll camber", "deg/deg"),
        "total",
    ),
    _plot_spec(
        "camber_vs_suspension_roll_angle",
        "suspension_roll_angle",
        "camber",
        GradientDisplay("roll camber", "deg/deg"),
        "axle",
    ),
    _plot_spec(
        "wheel_load",
        "wheel_center_z",
        "wheel_load",
        GradientDisplay("roll wheel rate", "N/mm"),
    ),
    PlotSpec(
        "knc_roll_suspension_rate",
        "Roll - Wheel center Z / Suspension rate",
        "wheel_center_z",
        "wheel_load",
        "suspension_rate",
        transform=gradient_curve,
        optional=True,
        sweep=ROLL_ANGLE,
    ),
    PlotSpec(
        "knc_roll_ride_rate",
        "Roll - Wheel pad Z / Ride rate",
        "wheel_pad_z",
        "wheel_load",
        "ride_rate",
        transform=gradient_curve,
        optional=True,
        sweep=ROLL_ANGLE,
    ),
    _plot_spec(
        "wheel_load_vs_roll_angle", "roll_angle", "wheel_load", x_at="total"
    ),
    _plot_spec(
        "wheel_load_vs_suspension_roll_angle",
        "suspension_roll_angle",
        "wheel_load",
        x_at="axle",
    ),
    _plot_spec("wcz_vs_wcx", "wheel_center_z", "wheel_center_x"),
    _plot_spec("wcz_vs_wcy", "wheel_center_z", "wheel_center_y"),
    _plot_spec("wpz_vs_cpx", "wheel_pad_z", "contact_patch_x"),
    _plot_spec("wpz_vs_cpy", "wheel_pad_z", "contact_patch_y"),
)


def _axle_plot(
    name: str, signal: str, unit: str, suspension: bool
) -> AxlePlotSpec:
    axis = "suspension_roll_angle" if suspension else "roll_angle"
    panels = []
    for location in ("total", *AXLES):
        x = (
            "mean_suspension_roll_angle"
            if suspension and location == "total"
            else axis
        )
        x_label = (
            "Mean front/rear suspension roll angle [deg]"
            if suspension and location == "total"
            else "Suspension roll angle [deg]"
            if suspension
            else "Body roll angle [deg]"
        )
        panels.append(
            AxlePanel(
                location,
                x,
                signal,
                location.capitalize(),
                x_label,
                f"{name} [{unit}]",
                "total" if not suspension else None,
            )
        )
    return AxlePlotSpec(
        f"knc_roll_{signal}_vs_{axis}",
        f"Roll - {name} / {x_label}",
        tuple(panels),
    )


def _center_plot(axle: str) -> AxlePlotSpec:
    return AxlePlotSpec(
        f"knc_roll_roll_center_{axle}",
        f"Roll - {axle.capitalize()} roll center",
        (
            AxlePanel(
                axle,
                "roll_center_y",
                "roll_center_height",
                "Position",
                "Absolute lateral position [mm]",
                "Height above ground [mm]",
            ),
            AxlePanel(
                axle,
                "roll_angle",
                "roll_center_height",
                "Height",
                "Body roll angle [deg]",
                "Height above ground [mm]",
                "total",
            ),
            AxlePanel(
                axle,
                "roll_angle",
                "roll_center_y",
                "Lateral migration",
                "Body roll angle [deg]",
                "Y relative to vehicle center [mm]",
                "total",
            ),
        ),
    )


AXLE_PLOT_SPECS = (
    _axle_plot("Roll moment", "roll_moment", "Nm", False),
    _axle_plot("Roll moment", "roll_moment", "Nm", True),
    _axle_plot("Roll stiffness", "roll_stiffness", "Nm/deg", False),
    _axle_plot("Roll stiffness", "roll_stiffness", "Nm/deg", True),
    *(_center_plot(axle) for axle in AXLES),
)


def _plot_curves(
    raw: list[dict[str, object]], options: BranchOptions
) -> list[dict[str, object]]:
    curves = index_curves(with_pad_travel(raw))
    _derive_suspension_roll(curves, [])
    _derive_roll_moments(curves, [])
    _plot_axle_curves(curves, options)
    return list(curves.values())


def _put_curve(
    curves: CurveIndex,
    location: str,
    signal: str,
    values: np.ndarray,
    unit: str,
) -> None:
    curves[(location, signal)] = {
        "corner": location,
        "signal": signal,
        "values": values,
        "unit": unit,
    }


def _plot_axle_curves(curves: CurveIndex, options: BranchOptions) -> None:
    """Prepare axle plots without changing the secant scalar metrics.

    Stiffness is the local d(derived moment)/d(body roll), separately on each
    body-roll sweep. The total suspension-angle axis is the front/rear mean;
    it is an x-axis only, not the stiffness denominator. Roll-center geometry
    uses raw contact-path slopes on each plotted sweep, without fitting.
    """
    roll = signal_values(curves, "total", "roll_angle")
    angles = [curves.get((axle, "suspension_roll_angle")) for axle in AXLES]
    if all(angle is not None for angle in angles):
        front, rear = (
            signal_values(curves, axle, "suspension_roll_angle")
            for axle in AXLES
        )
        if len(front) == len(rear) == len(roll):
            _put_curve(
                curves,
                "total",
                "mean_suspension_roll_angle",
                (front + rear) / 2.0,
                "deg",
            )

    for location in (*AXLES, "total"):
        moment = curves.get((location, "roll_moment"))
        if moment is None:
            continue
        values = signal_values(curves, location, "roll_moment")
        if len(values) != len(roll):
            continue
        rate = np.full_like(roll, np.nan)
        for sweep in options.plot_sweeps(roll):
            result = sweep_gradient(roll, values, None, sweep, options)
            if result is None:
                continue
            _, slope, _, _, masked = result
            finite = np.isfinite(roll[sweep]) & np.isfinite(values[sweep])
            segment = np.full(len(finite), np.nan)
            segment[finite] = np.where(masked, np.nan, slope)
            rate[sweep] = segment
            # Do not draw a line across the reversal into another sweep.
            if sweep.start:
                rate[sweep.start] = np.nan
        if np.isfinite(rate).any():
            _put_curve(curves, location, "roll_stiffness", rate, "Nm/deg")

    for axle in AXLES:
        try:
            lateral = _lateral_signal(curves, axle, [])
            samples = _roll_center_inputs(curves, axle, lateral)
        except KncKernelError:
            continue
        height = np.full_like(roll, np.nan)
        position = np.full_like(roll, np.nan)
        for sweep in options.plot_sweeps(roll):
            if not all(
                np.isfinite(values[sweep]).all() for values in samples.values()
            ):
                continue
            try:
                z, y, _ = _branch_roll_center(samples, sweep, options)
            except KncKernelError:
                continue
            height[sweep], position[sweep] = z, y
            if sweep.start:
                height[sweep.start] = position[sweep.start] = np.nan
        if (np.isfinite(height) & np.isfinite(position)).any():
            _put_curve(curves, axle, "roll_center_height", height, "mm")
            _put_curve(curves, axle, "roll_center_y", position, "mm")


def plot(
    curves_json: str,
    source_label: str = "Simulation",
    footer: str = "",
    logo: bytes | None = None,
    gradient_points_mm: float | None = None,
    branch: str | None = None,
    gradient_points_deg: float | None = None,
    debug: str | None = None,
) -> dict[str, bytes]:
    options = BranchOptions.from_json(branch)
    curves = _plot_curves(json.loads(curves_json), options)
    reports = render_reports(
        curves,
        PLOT_SPECS,
        PLOT_SIGNALS,
        source_label,
        "plots/knc/roll",
        footer,
        logo,
        gradient_points_mm,
        options,
        gradient_points_deg,
        warn_missing=True,
    )
    reports.update(
        render_axle_reports(
            curves,
            [],
            AXLE_PLOT_SPECS,
            source_label,
            footer,
            logo,
            "plots/knc/roll",
        )
    )
    if debug:
        info = json.loads(debug)
        reports.update(
            render_rate_debug(
                curves,
                PLOT_SPECS,
                options,
                "plots/knc/roll",
                info,
                info["inputs"][0],
                logo,
            )
        )
        reports.update(
            _roll_center_debug(
                curves,
                options,
                info,
                info["inputs"][0],
                logo,
                "plots/knc/roll",
                "measurement",
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
    gradient_points_deg: float | None = None,
    debug: str | None = None,
) -> dict[str, bytes]:
    candidates = json.loads(candidates_json)
    branches = (
        [BranchOptions.from_json(item) for item in json.loads(branches_json)]
        if branches_json
        else [BranchOptions()] * (len(candidates) + 1)
    )
    baseline = _plot_curves(json.loads(baseline_json), branches[0])
    candidates = [
        {**item, "curves": _plot_curves(item["curves"], options)}
        for item, options in zip(candidates, branches[1:], strict=True)
    ]
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
        gradient_points_deg,
        warn_missing=True,
    )
    reports.update(
        render_axle_reports(
            baseline,
            candidates,
            AXLE_PLOT_SPECS,
            baseline_label,
            footer,
            logo,
        )
    )
    if debug:
        info = json.loads(debug)
        for index, (curves, options) in enumerate(
            zip(
                [baseline, *(item["curves"] for item in candidates)],
                branches,
                strict=True,
            )
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
            reports.update(
                _roll_center_debug(
                    curves,
                    options,
                    info,
                    info["inputs"][index],
                    logo,
                    "",
                    f"series_{index}",
                )
            )
    return reports


def _roll_center_debug(
    curves: list[dict[str, object]],
    options: BranchOptions,
    info: dict[str, Any],
    input_path: str,
    logo: bytes | None,
    prefix: str,
    source: str,
) -> dict[str, bytes]:
    if not options.measurement:
        return {}
    indexed = index_curves(curves)
    reports = {}
    for axle in AXLES:
        try:
            lateral = _lateral_signal(indexed, axle, [])
            samples = _roll_center_inputs(indexed, axle, lateral)
        except KncKernelError:
            continue
        name = f"debug/{source}_knc_roll_roll_center_{axle}.pdf"
        path = f"{prefix}/{name}" if prefix else name
        reports[path] = render_contact_path_debug(
            samples, axle, options, info, input_path, logo
        )
    return reports


def metrics(request: dict[str, object]) -> str:
    """Derive signals, then calculate roll metrics on the selected sweep.

    PyO3 request: curves (JSON string of canonical Curve objects with optional
    offsets), gradient_mm and gradient_deg (positive secant half-widths),
    branch (JSON string of BranchOptions), arb_connected (JSON string with
    front/rear boolean or null). Both tests use this Rust-to-Python entry-point
    shape; roll adds gradient_deg and arb_connected.

    Return JSON string: {"metrics": [...], "warnings": [...]}. Unavailable
    metrics keep value: null and carry a reason. Rust tags and writes rows.
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

    # Fill absent axle angles from global geometry; derive axle and total
    # moments from loads and instantaneous geometric contact-patch track.
    _derive_suspension_roll(curves, warnings)
    _derive_roll_moments(curves, warnings)

    corner_rates = gradient_metrics(
        curves, CORNER_METRICS, CORNERS, points, ROLL_ANGLE, options
    )
    axle_stiffness = gradient_metrics(
        curves, AXLE_METRICS, (*AXLES, "total"), points, ROLL_ANGLE, options
    )
    results = [
        *corner_rates,
        *_wheel_rates_without_arb(corner_rates, arb_connected),
        *axle_stiffness,
        _roll_stiffness_distribution(axle_stiffness, points["deg"]),
        *_roll_center_metrics(curves, points["deg"], options, warnings),
    ]
    results.sort(key=lambda item: str(item["metric"]))
    return json.dumps({"metrics": results, "warnings": warnings})


def _derive_roll_moments(curves: CurveIndex, warnings: list[str]) -> None:
    """Use the same differential-load moment for measurements and simulations.

    M_axle = (Fz_left - Fz_right) * (Y_right - Y_left) / 2000 [Nm].
    Forces are N, absolute contact Y is mm, and 2000 combines the half-track
    lever arm with conversion from N mm to Nm. Total is front plus rear.
    Source/rig moment channels are intentionally not used in this calculation.
    """
    for location in (*AXLES, "total"):
        curves.pop((location, "roll_moment"), None)
    for location in AXLES:
        key = (location, "roll_moment")
        try:
            left, right = f"{location}_left", f"{location}_right"
            load_left = absolute(curves, left, "wheel_load")
            load_right = absolute(curves, right, "wheel_load")
            contact_left = absolute(curves, left, "contact_patch_y")
            contact_right = absolute(curves, right, "contact_patch_y")
            lengths = {
                len(values)
                for values in (
                    load_left,
                    load_right,
                    contact_left,
                    contact_right,
                    signal_values(curves, "total", "roll_angle"),
                )
            }
            if len(lengths) != 1:
                raise KncKernelError(
                    "wheel-load, contact-position and roll-angle lengths differ"
                )
            track = contact_right - contact_left
            if not (np.isfinite(track).all() and (track > 0.0).all()):
                raise KncKernelError(
                    "instantaneous contact track must be positive and finite"
                )
            if not (
                np.isfinite(load_left).all() and np.isfinite(load_right).all()
            ):
                raise KncKernelError("wheel loads must be finite")
        except KncKernelError as error:
            warnings.append(f"{location} roll moment unavailable: {error}")
            continue
        curves[key] = {
            "corner": location,
            "signal": "roll_moment",
            "unit": "Nm",
            "values": (load_left - load_right) * track / 2000.0,
        }
    if all((location, "roll_moment") in curves for location in AXLES):
        curves[("total", "roll_moment")] = {
            "corner": "total",
            "signal": "roll_moment",
            "unit": "Nm",
            "values": sum(
                signal_values(curves, location, "roll_moment")
                for location in AXLES
            ),
        }


def _derive_suspension_roll(curves: CurveIndex, warnings: list[str]) -> None:
    """Fill absent axle angles from absolute global wheel-center geometry.

    In the front-view convention (right Y positive, Z up), axle-line tilt is
    atan2(z_right - z_left, y_right - y_left). Suspension roll is body roll
    minus this tilt, in degrees. Common heave/lateral translation cancels.
    This is a front-view angle, not a full 3-D relative Euler rotation.
    Explicit mapped suspension-roll signals always take precedence.
    """
    for axle in AXLES:
        key = (axle, "suspension_roll_angle")
        if key in curves:
            continue
        try:
            body = signal_values(curves, "total", "roll_angle")
            left_y = absolute(curves, f"{axle}_left", "wheel_center_global_y")
            left_z = absolute(curves, f"{axle}_left", "wheel_center_global_z")
            right_y = absolute(curves, f"{axle}_right", "wheel_center_global_y")
            right_z = absolute(curves, f"{axle}_right", "wheel_center_global_z")
        except KncKernelError:
            continue
        positions = (left_y, left_z, right_y, right_z)
        if len({len(body), *(len(v) for v in positions)}) != 1:
            warnings.append(
                f"{axle} suspension roll: wheel-center signal lengths differ"
            )
            continue
        track = right_y - left_y
        valid = (
            np.isfinite(body)
            & np.logical_and.reduce([np.isfinite(v) for v in positions])
            & (track > 0.0)
        )
        angle = np.full_like(body, np.nan)
        angle[valid] = body[valid] - np.rad2deg(
            np.arctan2(right_z[valid] - left_z[valid], track[valid])
        )
        if not valid.all():
            warnings.append(
                f"{axle} suspension roll: "
                "invalid wheel-center geometry excluded"
            )
        curves[key] = {
            "corner": axle,
            "signal": "suspension_roll_angle",
            "unit": "deg",
            "values": angle,
        }
        warnings.append(
            f"{axle} suspension roll derived from body roll "
            "and global wheel-center Y/Z"
        )


def _wheel_rates_without_arb(
    corner: list[dict[str, object]], arb_connected: dict[str, bool | None]
) -> list[dict[str, object]]:
    """Reuse the computed roll wheel rate under the `_without_arb` name.

    No ARB force is subtracted here. Only a disconnected axle measures wheel
    rate without ARB; a connected or unspecified axle reports the reason.
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
    """Raw height near 0 deg and secants between samples near ±half deg.

    No interpolation, fit, or hysteresis averaging is applied. Measured
    turnaround edges use the shared curvature mask; secants use the actual
    retained sample angles, recorded with every row.
    """
    results = []
    for axle in AXLES:
        height_row = metric_row(
            axle, f"roll_center_height_{axle}_mm", "mm", "deg", half
        )
        height_change_row = metric_row(
            axle,
            f"roll_center_height_change_{axle}_mm_per_deg",
            "mm/deg",
            "deg",
            half,
        )
        migration_row = metric_row(
            axle,
            f"roll_center_lateral_migration_{axle}_mm_per_deg",
            "mm/deg",
            "deg",
            half,
        )
        rows = (height_row, height_change_row, migration_row)
        try:
            lateral = _lateral_signal(curves, axle, warnings)
            height, position, sampled_roll = _roll_center_samples(
                curves, axle, lateral, half, options
            )
        except KncKernelError as error:
            for row in rows:
                row["reason"] = str(error)
        else:
            height_row.update(value=height[1], lower=height[0], upper=height[2])
            for row, line in (
                (height_change_row, height),
                (migration_row, position),
            ):
                row.update(
                    value=(line[2] - line[0])
                    / (sampled_roll[2] - sampled_roll[0]),
                    lower=line[0],
                    upper=line[2],
                )
            for row in rows:
                row["lateral_signal"] = lateral
                row["sampled_roll_angle_deg"] = sampled_roll.tolist()
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


def _roll_center_samples(
    curves: CurveIndex,
    axle: str,
    lateral: str,
    half: float,
    options: BranchOptions,
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Raw geometry at the selected sweep's nearest samples to -half, 0, +half.

    Require finite input throughout the sweep and retained finite geometry
    covering the interval. Do not bridge invalid samples or extrapolate.
    """
    samples = _roll_center_inputs(curves, axle, lateral)
    sweep = metric_sweep(samples["roll"], "total_roll_angle", options)
    if not all(np.isfinite(values[sweep]).all() for values in samples.values()):
        raise KncKernelError(f"{axle} roll-center sweep has nonfinite input")
    height, position, roll = _branch_roll_center(samples, sweep, options)
    retained = np.flatnonzero(np.isfinite(height) & np.isfinite(position))
    if (
        not retained.size
        or roll[retained].min() > -half
        or roll[retained].max() < half
    ):
        raise KncKernelError(
            f"{axle} roll center does not cover roll_angle "
            f"{-half:g}..{half:g} deg"
        )
    indices = np.array(
        [
            retained[np.argmin(np.abs(roll[retained] - target))]
            for target in (-half, 0.0, half)
        ]
    )
    sampled_roll = roll[indices]
    if sampled_roll[2] <= sampled_roll[0]:
        raise KncKernelError(f"{axle} roll-center sampled angle span is zero")
    if not (
        np.isfinite(height[indices]).all()
        and np.isfinite(position[indices]).all()
    ):
        raise KncKernelError(
            f"{axle} roll-center geometry is nonfinite at selected samples"
        )
    return height[indices], position[indices], sampled_roll


def _roll_center_inputs(
    curves: CurveIndex, axle: str, lateral: str
) -> dict[str, np.ndarray]:
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
    return samples


def _branch_roll_center(
    samples: dict[str, np.ndarray], sweep: slice, options: BranchOptions
) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """Differentiate raw paths and mask measured turnaround edges."""
    if len(samples["roll"][sweep]) < 2:
        raise KncKernelError("roll-center sweep has too few samples")
    contact_slopes = {}
    masked = np.zeros(len(samples["roll"][sweep]), dtype=bool)
    for side in ("left", "right"):
        result = sweep_gradient(
            samples[f"z_{side}"], samples[f"y_{side}"], None, sweep, options
        )
        if result is None or len(result[1]) != len(masked):
            raise KncKernelError("roll-center sweep has nonfinite contact path")
        contact_slopes[side] = result[1]
        masked |= result[4] | ~np.isfinite(result[1])
    height, position = roll_center(
        contact_slopes["left"],
        contact_slopes["right"],
        samples["y_left"][sweep],
        samples["z_left"][sweep],
        samples["y_right"][sweep],
        samples["z_right"][sweep],
        np.radians(samples["roll"][sweep]),
    )
    height[masked] = np.nan
    position[masked] = np.nan
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
