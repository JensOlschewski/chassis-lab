"""Evaluate vertical wheel-center trajectories against Lommel measurements."""

from dataclasses import dataclass
from math import sqrt

import numpy as np
from numpy.typing import NDArray

from chassis_lab.acd import AcdFile, AcdTestInfo
from chassis_lab.config import CaseConfig, VerticalConfig
from chassis_lab.conventions import (
    BaselinePolicy,
    Corner,
    SignalConvention,
    apply_convention,
)
from chassis_lab.hdf5 import AdamsHdf5

AXES = ("x", "y", "z")
CORNER_PREFIXES = {
    Corner.FRONT_LEFT: "lf",
    Corner.FRONT_RIGHT: "rf",
    Corner.REAR_LEFT: "lr",
    Corner.REAR_RIGHT: "rr",
}
VERTICAL_SIGNAL_CONVENTIONS = {
    f"{corner.value}_{suffix}": SignalConvention(
        unit="mm",
        source_unit="mm",
        corner=corner,
        baseline=BaselinePolicy.STATIC_SIMULATION,
    )
    for corner in Corner
    for suffix in (
        "travel",
        "wheel_center_x",
        "wheel_center_y",
        "wheel_center_z",
    )
}
SETUP_FIELDS = {
    "brakeson": "brakes_applied",
    "engineon": "engine_running",
    "farbon": "front_arb_connected",
    "rarbon": "rear_arb_connected",
}


class VerticalEvaluationError(ValueError):
    """Raised when vertical trajectories cannot be compared safely."""


@dataclass(frozen=True)
class TrajectoryComparison:
    corner: Corner
    axis: str
    travel_mm: NDArray[np.float64]
    measurement_mm: NDArray[np.float64]
    simulation_mm: NDArray[np.float64]
    measurement_baseline_mm: float
    simulation_baseline_mm: float
    baseline_error_mm: float
    rmse_mm: float
    maximum_error_mm: float

    def as_dict(self) -> dict[str, object]:
        return {
            "corner": self.corner.value,
            "axis": self.axis,
            "measurement_baseline_mm": self.measurement_baseline_mm,
            "simulation_baseline_mm": self.simulation_baseline_mm,
            "baseline_error_mm": self.baseline_error_mm,
            "rmse_mm": self.rmse_mm,
            "maximum_error_mm": self.maximum_error_mm,
        }


@dataclass(frozen=True)
class VerticalResult:
    case_name: str
    variant: str
    loadcase: str
    setup_code: str
    comparisons: tuple[TrajectoryComparison, ...]

    def as_dict(self) -> dict[str, object]:
        return {
            "case": self.case_name,
            "variant": self.variant,
            "loadcase": self.loadcase,
            "setup_code": self.setup_code,
            "metrics": [
                comparison.as_dict() for comparison in self.comparisons
            ],
        }


def evaluate_vertical(config: CaseConfig) -> VerticalResult:
    """Compare measured and simulated trajectories on a fixed travel grid."""
    vertical = config.vertical
    if vertical is None:
        raise VerticalEvaluationError(
            f"case '{config.name}' has no [vertical] configuration"
        )
    grid = _travel_grid(vertical)
    measurement = _measurement_curves(vertical, grid)
    simulation = _simulation_curves(vertical, grid)
    comparisons = []
    for corner in Corner:
        for axis in AXES:
            measured, measured_baseline = measurement[(corner, axis)]
            simulated, simulated_baseline = simulation[(corner, axis)]
            error = simulated - measured
            comparisons.append(
                TrajectoryComparison(
                    corner=corner,
                    axis=axis,
                    travel_mm=grid.copy(),
                    measurement_mm=measured,
                    simulation_mm=simulated,
                    measurement_baseline_mm=measured_baseline,
                    simulation_baseline_mm=simulated_baseline,
                    baseline_error_mm=abs(
                        simulated_baseline - measured_baseline
                    ),
                    rmse_mm=sqrt(float(np.mean(np.square(error)))),
                    maximum_error_mm=float(np.max(np.abs(error))),
                )
            )
    return VerticalResult(
        case_name=config.name,
        variant=config.variant,
        loadcase=config.loadcase,
        setup_code=vertical.setup.code,
        comparisons=tuple(comparisons),
    )


def validate_vertical_measurement(config: VerticalConfig) -> None:
    """Validate vertical ACD metadata and required channel catalog."""
    acd = AcdFile(config.measurement_acd)
    _validate_measurement_setup(acd, config)
    catalog = {signal.name: signal for signal in acd.signals()}
    required = {
        name
        for prefix in CORNER_PREFIXES.values()
        for name in (
            f"{prefix}slope",
            *(f"{prefix}wc{axis}" for axis in AXES),
        )
    }
    missing = sorted(required - catalog.keys())
    if missing:
        raise VerticalEvaluationError(
            "measurement channel(s) missing: " + ", ".join(missing)
        )
    wrong_units = sorted(
        name
        for name in required
        if not name.endswith("slope") and catalog[name].unit != "mm"
    )
    if wrong_units:
        raise VerticalEvaluationError(
            "measurement channel(s) must use mm: " + ", ".join(wrong_units)
        )


def _travel_grid(config: VerticalConfig) -> NDArray[np.float64]:
    window = config.window
    count = int(
        np.floor((window.maximum_mm - window.minimum_mm) / window.step_mm)
    )
    grid = window.minimum_mm + np.arange(count + 1) * window.step_mm
    if grid[-1] < window.maximum_mm:
        grid = np.append(grid, window.maximum_mm)
    if not np.any(np.isclose(grid, 0.0)):
        raise VerticalEvaluationError(
            "vertical validation window must include zero"
        )
    return np.asarray(grid, dtype=np.float64)


def _measurement_curves(
    config: VerticalConfig, grid: NDArray[np.float64]
) -> dict[tuple[Corner, str], tuple[NDArray[np.float64], float]]:
    validate_vertical_measurement(config)
    acd = AcdFile(config.measurement_acd)
    names = []
    for prefix in CORNER_PREFIXES.values():
        names.extend(
            (
                f"{prefix}slope",
                *(f"{prefix}wc{axis}" for axis in AXES),
            )
        )
    expected = {name: "mm" for name in names if not name.endswith("slope")}
    signals = acd.read_signals(names, expected_units=expected)
    curves = {}
    for corner, prefix in CORNER_PREFIXES.items():
        travel_signal = signals[f"{prefix}wcz"]
        travel = travel_signal.values - travel_signal.offset
        cycle = signals[f"{prefix}slope"].values
        for axis in AXES:
            coordinate = signals[f"{prefix}wc{axis}"].values
            branches = [
                _interpolate_branch(
                    travel[cycle == cycle_number],
                    coordinate[cycle == cycle_number],
                    grid,
                    f"measurement {corner.value} cycle {cycle_number}",
                )
                for cycle_number in (2.0, 3.0)
            ]
            absolute = np.mean(branches, axis=0)
            baseline = _at_zero(grid, absolute)
            curves[(corner, axis)] = (absolute - baseline, baseline)
    return curves


def _simulation_curves(
    config: VerticalConfig, grid: NDArray[np.float64]
) -> dict[tuple[Corner, str], tuple[NDArray[np.float64], float]]:
    curves = {}
    with AdamsHdf5(config.simulation_hdf5) as result:
        for corner in Corner:
            travel_name = f"{corner.value}_travel"
            travel_ref = _signal(config, travel_name)
            travel_signal = result.read_signal(
                travel_ref.entity,
                travel_ref.component,
                expected_unit=travel_ref.unit,
            )
            travel = apply_convention(
                travel_signal.values,
                name=travel_name,
                source=(
                    f"{config.simulation_hdf5}:"
                    f"{travel_signal.entity}/{travel_signal.component}"
                ),
                convention=VERTICAL_SIGNAL_CONVENTIONS[travel_name],
            ).values
            if not len(travel) or not np.isfinite(travel[0]):
                raise VerticalEvaluationError(
                    f"simulation {corner.value} travel has invalid "
                    "static sample"
                )
            travel = travel - travel[0]
            for axis in AXES:
                coordinate_name = f"{corner.value}_wheel_center_{axis}"
                coordinate_ref = _signal(config, coordinate_name)
                coordinate_signal = result.read_signal(
                    coordinate_ref.entity,
                    coordinate_ref.component,
                    expected_unit=coordinate_ref.unit,
                )
                coordinate = apply_convention(
                    coordinate_signal.values,
                    name=coordinate_name,
                    source=(
                        f"{config.simulation_hdf5}:"
                        f"{coordinate_signal.entity}/"
                        f"{coordinate_signal.component}"
                    ),
                    convention=VERTICAL_SIGNAL_CONVENTIONS[coordinate_name],
                ).values
                absolute = _mean_monotonic_branches(
                    travel,
                    coordinate,
                    grid,
                    f"simulation {corner.value}",
                )
                baseline = _at_zero(grid, absolute)
                curves[(corner, axis)] = (absolute - baseline, baseline)
    return curves


def _signal(config: VerticalConfig, name: str):
    try:
        convention = VERTICAL_SIGNAL_CONVENTIONS[name]
    except KeyError as error:
        raise VerticalEvaluationError(
            f"vertical signal convention missing: {name}"
        ) from error
    try:
        signal = config.signal(name)
    except KeyError as error:
        raise VerticalEvaluationError(
            f"vertical signal mapping missing: {name}"
        ) from error
    if signal.unit != convention.source_unit:
        raise VerticalEvaluationError(
            f"vertical signal '{name}' must use configured unit "
            f"'{convention.source_unit}'"
        )
    return signal


def _validate_measurement_setup(acd: AcdFile, config: VerticalConfig) -> None:
    test_type = _metadata_value(acd, "testtype")
    if test_type != "vert":
        raise VerticalEvaluationError(
            f"measurement testtype is {test_type!r}; expected 'vert'"
        )
    for metadata_name, attribute in SETUP_FIELDS.items():
        expected = getattr(config.setup, attribute)
        actual = _metadata_flag(acd, metadata_name)
        if expected is not None and actual != expected:
            raise VerticalEvaluationError(
                f"measurement setup {metadata_name}={actual}; "
                f"expected {expected}"
            )


def _metadata_value(acd: AcdFile, name: str) -> str:
    try:
        info = acd.test_info(name)
    except KeyError as error:
        raise VerticalEvaluationError(
            f"measurement test metadata missing: {name}"
        ) from error
    assert isinstance(info, AcdTestInfo)
    return info.value


def _metadata_flag(acd: AcdFile, name: str) -> bool | None:
    value = _metadata_value(acd, name)
    if value == "1":
        return True
    if value == "0":
        return False
    if not value or value.casefold() == "nan":
        return None
    raise VerticalEvaluationError(
        f"measurement setup metadata {name} has invalid value {value!r}"
    )


def _mean_monotonic_branches(
    x: NDArray[np.float64],
    y: NDArray[np.float64],
    grid: NDArray[np.float64],
    source: str,
) -> NDArray[np.float64]:
    finite = np.isfinite(x) & np.isfinite(y)
    x = x[finite]
    y = y[finite]
    if len(x) < 2:
        raise VerticalEvaluationError(
            f"{source} has insufficient finite samples"
        )
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
    branches = []
    for indices in np.split(np.arange(len(x)), boundaries):
        if len(indices) < 2:
            continue
        branch_x = x[indices]
        if np.min(branch_x) <= grid[0] and np.max(branch_x) >= grid[-1]:
            branches.append(
                _interpolate_branch(branch_x, y[indices], grid, source)
            )
    if not branches:
        raise VerticalEvaluationError(
            f"{source} has no monotonic branch covering "
            f"{grid[0]:.6g}..{grid[-1]:.6g} mm"
        )
    return np.mean(branches, axis=0)


def _interpolate_branch(
    x: NDArray[np.float64],
    y: NDArray[np.float64],
    grid: NDArray[np.float64],
    source: str,
) -> NDArray[np.float64]:
    finite = np.isfinite(x) & np.isfinite(y)
    x = x[finite]
    y = y[finite]
    if len(x) < 2:
        raise VerticalEvaluationError(
            f"{source} has insufficient finite samples"
        )
    order = np.argsort(x, kind="stable")
    x = x[order]
    y = y[order]
    unique_x, starts, counts = np.unique(
        x, return_index=True, return_counts=True
    )
    unique_y = np.add.reduceat(y, starts) / counts
    if unique_x[0] > grid[0] or unique_x[-1] < grid[-1]:
        raise VerticalEvaluationError(
            f"{source} covers {unique_x[0]:.6g}..{unique_x[-1]:.6g} mm; "
            f"required {grid[0]:.6g}..{grid[-1]:.6g} mm"
        )
    return np.interp(grid, unique_x, unique_y)


def _at_zero(grid: NDArray[np.float64], values: NDArray[np.float64]) -> float:
    return float(np.interp(0.0, grid, values))
