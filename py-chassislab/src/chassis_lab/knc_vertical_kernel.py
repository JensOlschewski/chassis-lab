"""Path-free numerical kernel for Rust-owned KnC vertical analysis."""

import json
from datetime import datetime
from io import BytesIO, StringIO
from math import sqrt

import numpy as np


class VerticalKernelError(ValueError):
    """Curve data cannot be compared safely."""


def evaluate(request: dict[str, object]) -> str:
    """Compare canonical KnC vertical curves on requested travel grid.

    ``request["grid"]`` is travel in millimetres. ``request["curves"]`` is a
    JSON object keyed by ``<corner>:<axis>`` for all four corners and ``x``,
    ``y``, and ``z`` axes. Each value contains baseline and candidate travel/
    value arrays in millimetres. Return JSON with normalized curves and scalar
    comparison metrics. Raise ``VerticalKernelError`` when a curve cannot cover
    requested grid through a finite monotonic branch.
    """
    grid = np.asarray(request["grid"], dtype=np.float64)
    curves = json.loads(str(request["curves"]))
    comparisons = []
    for corner in ("front_left", "front_right", "rear_left", "rear_right"):
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
    """Interpolate finite samples after stable sorting and duplicate averaging.

    Reject curves with fewer than two finite samples or incomplete grid
    coverage.
    """
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


def plot(curves_json: str, source_label: str = "Simulation") -> str:
    """Render direct vertical curves as deterministic four-corner SVG reports.

    ``curves_json`` is a JSON array of canonical curve records with ``corner``,
    ``signal``, ``unit``, and signal-unit ``values`` fields. Wheel-center values
    are millimetres; optional load, toe, and camber values use their declared
    units. ``source_label`` identifies each line in SVG legends. Return JSON
    mapping report-relative paths to SVG document strings.
    """
    plt = _pyplot()
    curves = json.loads(curves_json)
    reports = {}
    corners = (
        ("front_left", "Front left"),
        ("front_right", "Front right"),
        ("rear_left", "Rear left"),
        ("rear_right", "Rear right"),
    )
    indexed = {(item["corner"], item["signal"]): item for item in curves}

    for x_signal, y_signal in (
        ("wheel_center_y", "wheel_center_z"),
        ("wheel_center_x", "wheel_center_y"),
        ("wheel_center_x", "wheel_center_z"),
    ):
        reports[
            f"plots/knc/vertical/{y_signal.replace('_', '-')}-vs-"
            f"{x_signal.replace('_', '-')}.svg"
        ] = _plot_group(
            corners,
            indexed,
            x_signal,
            y_signal,
            f"{_label(y_signal)} vs {_label(x_signal)}",
            "mm",
            source_label,
            plt,
        )

    for signal, title, unit in (
        ("wheel_load", "Wheel load vs Wheel center Z", "newton"),
        ("toe", "Toe vs Wheel center Z", "deg"),
        ("camber", "Camber vs Wheel center Z", "deg"),
    ):
        if all((corner, signal) in indexed for corner, _ in corners):
            reports[
                f"plots/knc/vertical/{signal.replace('_', '-')}-vs-"
                "wheel-center-z.svg"
            ] = _plot_group(
                corners,
                indexed,
                "wheel_center_z",
                signal,
                title,
                unit,
                source_label,
                plt,
            )
            if signal == "wheel_load":
                reports[
                    "plots/knc/vertical/wheel-rate-vs-wheel-center-z.svg"
                ] = _plot_group(
                    corners,
                    indexed,
                    "wheel_center_z",
                    signal,
                    "Wheel rate vs Wheel center Z",
                    "newton/mm",
                    source_label,
                    plt,
                    transform=_wheel_rate,
                )
    return json.dumps(reports)


def plot_compare(
    baseline_json: str,
    candidates_json: str,
    baseline_label: str,
    footer: str,
    logo: bytes,
) -> str:
    """Render baseline/candidate vertical SVG overlays.

    Baseline JSON is canonical curve array. Candidate JSON is an array of
    ``{"label": str, "curves": [...]}`` records with same curve coverage.
    Footer and PNG logo are rendered in every SVG. Return JSON mapping
    report-relative filenames to SVG document strings.
    """
    plt = _pyplot()
    baseline = json.loads(baseline_json)
    candidates = json.loads(candidates_json)
    logo_image = plt.imread(BytesIO(logo), format="png")
    corners = (
        ("front_left", "Front left"),
        ("front_right", "Front right"),
        ("rear_left", "Rear left"),
        ("rear_right", "Rear right"),
    )
    base = {(item["corner"], item["signal"]): item for item in baseline}
    candidates = [
        (
            item["label"],
            {
                (curve["corner"], curve["signal"]): curve
                for curve in item["curves"]
            },
        )
        for item in candidates
    ]
    reports = {}
    for x_signal, y_signal in (
        ("wheel_center_y", "wheel_center_z"),
        ("wheel_center_x", "wheel_center_y"),
        ("wheel_center_x", "wheel_center_z"),
    ):
        reports[
            f"{y_signal.replace('_', '-')}-vs-{x_signal.replace('_', '-')}.svg"
        ] = _plot_overlay_group(
            corners,
            base,
            x_signal,
            y_signal,
            f"{_label(y_signal)} vs {_label(x_signal)}",
            "mm",
            baseline_label,
            candidates,
            footer,
            logo_image,
            plt,
        )
    return json.dumps(reports)


def _plot_overlay_group(
    corners: tuple[tuple[str, str], ...],
    baseline: dict[tuple[str, str], dict[str, object]],
    x_signal: str,
    y_signal: str,
    title: str,
    unit: str,
    baseline_label: str,
    candidates: list[tuple[str, dict[tuple[str, str], dict[str, object]]]],
    footer: str,
    logo_image: np.ndarray,
    plt: object,
) -> str:
    footer_lines = footer.count("\n") + 2
    footer_height = min(0.12 + footer_lines * 0.025, 0.35)
    figure, panels = plt.subplots(2, 2, figsize=(11.69, 8.27))
    figure.subplots_adjust(
        left=0.08,
        right=0.97,
        top=0.89,
        bottom=footer_height,
        wspace=0.28,
        hspace=0.42,
    )
    figure.suptitle(title, fontsize=16, fontweight="bold")
    for panel, (corner, label) in zip(panels.flat, corners, strict=True):
        for curves, source_label, color in [
            (baseline, baseline_label, "#000000"),
            *[
                (curves, label, _candidate_color(index))
                for index, (label, curves) in enumerate(candidates)
            ],
        ]:
            x = np.asarray(
                curves[(corner, x_signal)]["values"], dtype=np.float64
            )
            y = np.asarray(
                curves[(corner, y_signal)]["values"], dtype=np.float64
            )
            if len(x) != len(y):
                raise VerticalKernelError(
                    f"{corner} {y_signal} and {x_signal} lengths differ"
                )
            panel.plot(x, y, color=color, linewidth=1.8, label=source_label)
        panel.set_title(label, loc="left", fontweight="bold")
        panel.set_xlabel(f"{_label(x_signal)} [mm]")
        panel.set_ylabel(f"{_label(y_signal)} [{unit}]")
        panel.grid(True, color="#d9e2f3", linewidth=0.8)
        panel.legend(loc="best", frameon=False)
    figure.text(
        0.02,
        0.025,
        f"{footer}\nGenerated: {datetime.now().astimezone():%Y-%m-%d %H:%M %Z}",
        fontsize=7,
        color="#4a4a4a",
        va="bottom",
    )
    logo_height = min(footer_lines * 0.017, footer_height - 0.05)
    logo_panel = figure.add_axes((0.80, 0.025, 0.18, logo_height))
    logo_panel.imshow(logo_image)
    logo_panel.axis("off")
    output = StringIO()
    figure.savefig(
        output, format="svg", metadata={"Date": None, "Creator": "ChassisLab"}
    )
    plt.close(figure)
    return output.getvalue()


def _candidate_color(index: int) -> str:
    colors = ("#e41a1c", "#377eb8", "#e7298a", "#00a6a6", "#ff7f00", "#6a3d9a")
    return colors[index % len(colors)]


def _plot_group(
    corners: tuple[tuple[str, str], ...],
    indexed: dict[tuple[str, str], dict[str, object]],
    x_signal: str,
    y_signal: str,
    title: str,
    unit: str,
    source_label: str,
    plt: object,
    transform: object = None,
) -> str:
    figure, panels = plt.subplots(
        2, 2, figsize=(11.69, 8.27), layout="constrained"
    )
    figure.suptitle(title, fontsize=16)
    for panel, (corner, label) in zip(panels.flat, corners, strict=True):
        x = np.asarray(indexed[(corner, x_signal)]["values"], dtype=np.float64)
        y = np.asarray(indexed[(corner, y_signal)]["values"], dtype=np.float64)
        if len(x) != len(y):
            raise VerticalKernelError(
                f"{corner} {y_signal} and {x_signal} lengths differ"
            )
        if transform is not None:
            x, y = transform(x, y)
        panel.plot(x, y, color="#1f4e79", linewidth=1.8, label=source_label)
        panel.set_title(label, loc="left", fontweight="bold")
        panel.set_xlabel(f"{_label(x_signal)} [mm]")
        panel.set_ylabel(f"{_label(y_signal)} [{unit}]")
        panel.grid(True, color="#d9e2f3", linewidth=0.8)
        panel.legend(loc="best", frameon=False)
    output = StringIO()
    figure.savefig(
        output, format="svg", metadata={"Date": None, "Creator": "ChassisLab"}
    )
    plt.close(figure)
    return output.getvalue()


def _pyplot() -> object:
    import matplotlib

    matplotlib.use("svg")
    matplotlib.rcParams["svg.fonttype"] = "none"
    matplotlib.rcParams["svg.hashsalt"] = "chassis-lab"
    from matplotlib import pyplot

    return pyplot


def _wheel_rate(x: np.ndarray, y: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """Return load gradient on original finite travel samples."""
    finite = np.isfinite(x) & np.isfinite(y)
    if not np.any(finite):
        return np.array([], dtype=np.float64), np.array([], dtype=np.float64)
    x_finite, y_finite = x[finite], y[finite]
    order = np.argsort(x_finite, kind="stable")
    x_sorted, y_sorted = x_finite[order], y_finite[order]
    unique, starts, counts = np.unique(
        x_sorted, return_index=True, return_counts=True
    )
    averaged = np.add.reduceat(y_sorted, starts) / counts
    gradient = (
        np.zeros_like(averaged)
        if len(unique) < 2
        else np.gradient(averaged, unique)
    )
    return x_finite, np.interp(x_finite, unique, gradient)


def _label(signal: str) -> str:
    labels = {
        "wheel_center_x": "Wheel center X",
        "wheel_center_y": "Wheel center Y",
        "wheel_center_z": "Wheel center Z",
        "wheel_load": "Wheel load",
        "toe": "Toe",
        "camber": "Camber",
    }
    return labels[signal]
