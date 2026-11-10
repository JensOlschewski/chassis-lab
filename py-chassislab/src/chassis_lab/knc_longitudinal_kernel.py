"""Path-free numerical kernel for Rust-owned KnC longitudinal analysis.

Longitudinal tests drive each wheel with a force in X (+X rearward, +Y right)
and measure the response. Metrics are the least-squares slope of the response
over wheel force within +-1 kN, fitted over one full force cycle (rising and
falling branch together). Strapped acceleration (brakes off) reports
longitudinal compliance; braking (brakes on) reports brake steer and windup
compliance.
"""

import json
from dataclasses import replace
from typing import Any, cast

from chassis_lab.knc_kernel import (
    CORNERS,
    BranchOptions,
    CurveIndex,
    GradientMetric,
    KncKernelError,
    Signal,
    fit_slope,
    index_curves,
    metric_cycle,
    metric_row,
    signal_values,
)
from chassis_lab.knc_plot_kernel import (
    GradientDisplay,
    PlotSpec,
    SignalStyle,
    render_comparison_reports,
    render_reports,
    render_secant_debug,
    selected_specs,
)
from chassis_lab.knc_plot_kernel import (
    plot_jobs as catalog_jobs,
)

FORCE = Signal("force_x")

# Half-width of the fit window in N; the metrics are per kN.
FIT_WINDOW_N = 1000.0
PER_KN = 1000.0

ACCELERATION_METRICS = (
    GradientMetric(
        "longitudinal_compliance",
        "mm_per_kn",
        "wheel_center_x",
        "mm/kN",
        axis=FORCE,
        points="N",
        scale=PER_KN,
    ),
)

BRAKING_METRICS = (
    GradientMetric(
        "brake_steer",
        "deg_per_kn",
        "toe",
        "deg/kN",
        axis=FORCE,
        points="N",
        scale=PER_KN,
    ),
    GradientMetric(
        "windup_compliance",
        "deg_per_kn",
        "spin",
        "deg/kN",
        axis=FORCE,
        points="N",
        scale=PER_KN,
    ),
)

PLOT_SIGNALS = {
    "force_x": SignalStyle("Wheel force X", "N"),
    "camber": SignalStyle("Camber", "deg"),
    "toe": SignalStyle("Toe", "deg"),
    "spin": SignalStyle("Castor", "deg"),
    "projected_contact_patch_x": SignalStyle("Projected contact patch X", "mm"),
    "wheel_center_x": SignalStyle("Wheel center X", "mm"),
    "wheel_center_y": SignalStyle("Wheel center Y", "mm"),
}


def _spec(
    name: str,
    title: str,
    y_signal: str,
    display: GradientDisplay | None = None,
    metric: str | None = None,
) -> PlotSpec:
    return PlotSpec(
        f"knc_longitudinal_{name}_vs_force_x",
        f"Longitudinal - Wheel force X / {title}",
        "force_x",
        y_signal,
        optional=True,
        gradient=display,
        gradient_unit="N",
        sweep=FORCE,
        metric=metric,
    )


# The secant of a metric is drawn only on the plot of its signal, and only in
# the test that reports it: `plot_specs` drops the other test's displays.
PLOT_SPECS = (
    _spec("camber", "Camber", "camber"),
    _spec(
        "toe",
        "Toe",
        "toe",
        GradientDisplay("brake steer", "deg/kN", scale=PER_KN),
        "brake_steer_{corner}_deg_per_kn",
    ),
    _spec(
        "castor",
        "Castor",
        "spin",
        GradientDisplay("windup compliance", "deg/kN", scale=PER_KN),
        "windup_compliance_{corner}_deg_per_kn",
    ),
    _spec(
        "proj_cp_x",
        "Projected contact patch X",
        "projected_contact_patch_x",
    ),
    _spec(
        "wc_x",
        "Wheel center X",
        "wheel_center_x",
        GradientDisplay("longitudinal compliance", "mm/kN", scale=PER_KN),
        "longitudinal_compliance_{corner}_mm_per_kn",
    ),
    _spec("wc_y", "Wheel center Y", "wheel_center_y"),
)


B1_ONLY_SIGNALS = ("spin", "projected_contact_patch_x")


def plot_specs(brakes: bool) -> tuple[PlotSpec, ...]:
    """Catalog with only the plots, secants and metrics the setup reports.

    Castor and projected contact patch X are braking plots; B0 omits them.
    """
    return tuple(
        spec
        if spec.gradient is None
        or (spec.y_signal != "wheel_center_x") == brakes
        else replace(spec, gradient=None, metric=None)
        for spec in PLOT_SPECS
        if brakes or spec.y_signal not in B1_ONLY_SIGNALS
    )


def _not_covered(
    catalog: tuple[GradientMetric, ...], needs_setup: str, half_n: float
) -> list[dict[str, object]]:
    """Value-less rows for metrics the stored setup cannot report."""
    rows = []
    for location in CORNERS:
        for metric in catalog:
            row = metric_row(
                location,
                f"{metric.name}_{location}_{metric.suffix}",
                metric.unit,
                metric.points,
                half_n,
            )
            row["status"] = "not_covered"
            row["needs_setup"] = needs_setup
            rows.append(row)
    return rows


def _fit_metrics(
    curves: CurveIndex,
    catalog: tuple[GradientMetric, ...],
    window_n: float,
    cycle: int,
    options: BranchOptions,
) -> list[dict[str, object]]:
    """Least-squares slope per corner; `lower` and `upper` are the fitted
    response at -window and +window."""
    results = []
    for location in CORNERS:
        for metric in catalog:
            row = metric_row(
                location,
                f"{metric.name}_{location}_{metric.suffix}",
                metric.unit,
                metric.points,
                window_n,
            )
            try:
                force = signal_values(curves, location, FORCE.name)
                response = signal_values(curves, location, metric.signal)
                if len(force) != len(response):
                    raise KncKernelError(
                        f"{location}_{metric.signal} and "
                        f"{location}_{FORCE.name} lengths differ"
                    )
                span = metric_cycle(force, cycle, options)
                fit = fit_slope(force[span], response[span], window_n)
            except KncKernelError as error:
                row["reason"] = str(error)
            else:
                row["value"] = metric.scale * fit.slope
                row["lower"] = fit.intercept - fit.slope * window_n
                row["upper"] = fit.intercept + fit.slope * window_n
            results.append(row)
    return results


def metrics(request: dict[str, object]) -> str:
    """Least-squares KnC longitudinal metrics within +-1 kN wheel force.

    Metrics of the other setup are listed as `not_covered`, with the setup
    that reports them, so a single-setup run does not look complete.
    """
    curves = index_curves(json.loads(str(request["curves"])))
    options = BranchOptions.from_json(
        cast("str | dict[str, Any] | None", request.get("branch"))
    )
    window_n = float(cast("float", request.get("gradient_n") or FIT_WINDOW_N))
    cycle = int(cast("int", request.get("cycle") or 1))
    if request["brakes"]:
        catalog, other = BRAKING_METRICS, ACCELERATION_METRICS
        needs_setup = "B0"
    else:
        catalog, other = ACCELERATION_METRICS, BRAKING_METRICS
        needs_setup = "B1"
    results = _fit_metrics(curves, catalog, window_n, cycle, options)
    results += _not_covered(other, needs_setup, window_n)
    results.sort(key=lambda item: str(item["metric"]))
    return json.dumps({"metrics": results})


def plot(
    curves_json: str,
    source_label: str = "Simulation",
    footer: str = "",
    logo: bytes | None = None,
    branch: str | None = None,
    selected: list[str] | None = None,
    series: bool = False,
    brakes: bool = False,
    debug: str | None = None,
    gradient_points_n: float | None = None,
    **_: object,
) -> dict[str, bytes]:
    """Render the longitudinal report catalog through the KnC plot kernel."""
    curves = json.loads(curves_json)
    options = BranchOptions.from_json(branch)
    half_n = gradient_points_n or FIT_WINDOW_N
    reports = render_reports(
        curves,
        selected_specs(plot_specs(brakes), selected, "plot"),
        PLOT_SIGNALS,
        source_label,
        "plots/knc/longitudinal",
        footer,
        logo,
        half_n,
        options,
        series_json=series,
    )
    if debug:
        info = json.loads(debug)
        reports.update(
            render_secant_debug(
                curves,
                selected_specs(plot_specs(brakes), selected, "debug:0"),
                options,
                "plots/knc/longitudinal",
                info,
                info["inputs"][0],
                half_n,
                logo,
            )
        )
    return reports


def plot_jobs(debug: bool, series_count: int = 1) -> list[str]:
    jobs = catalog_jobs(PLOT_SPECS, (), False, series_count)
    if debug:
        jobs += [
            f"debug:{index}:{spec.name}"
            for index in range(series_count)
            for spec in PLOT_SPECS
            if spec.gradient is not None
        ]
    return jobs


def plot_compare(
    baseline_json: str,
    candidates_json: str,
    baseline_label: str,
    footer: str,
    logo: bytes,
    branches_json: str | None = None,
    selected: list[str] | None = None,
    series: bool = False,
    brakes: bool = False,
    gradient_points_n: float | None = None,
    **_: object,
) -> dict[str, bytes]:
    """Render longitudinal overlays; one branch option per series."""
    candidates = json.loads(candidates_json)
    branches = (
        [BranchOptions.from_json(item) for item in json.loads(branches_json)]
        if branches_json
        else [BranchOptions()] * (len(candidates) + 1)
    )
    return render_comparison_reports(
        json.loads(baseline_json),
        candidates,
        selected_specs(plot_specs(brakes), selected, "plot"),
        PLOT_SIGNALS,
        baseline_label,
        footer,
        logo,
        gradient_points_n or FIT_WINDOW_N,
        branches,
        series_json=series,
    )
