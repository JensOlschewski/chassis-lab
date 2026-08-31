"""Calculate SVC wheel loads and mass distribution."""

from dataclasses import dataclass
from pathlib import Path

import numpy as np

from chassis_lab.config import CaseConfig, load_config
from chassis_lab.conventions import TransformedSignal, apply_convention
from chassis_lab.hdf5 import AdamsHdf5
from chassis_lab.svc import SVC_SIGNAL_CONVENTIONS

WHEEL_LOAD_NAMES = (
    "front_left_wheel_load",
    "front_right_wheel_load",
    "rear_left_wheel_load",
    "rear_right_wheel_load",
)


class SvcEvaluationError(ValueError):
    """Raised when SVC load calculation cannot be completed."""


@dataclass(frozen=True)
class SvcLoadResult:
    """Unrounded SVC wheel loads and derived mass distribution."""

    wheel_loads_kg: dict[str, float]
    axle_loads_kg: dict[str, float]
    total_mass_kg: float
    distribution_percent: dict[str, float]
    signals: tuple[TransformedSignal, ...]
    sample_index: int

    def as_dict(self) -> dict[str, object]:
        return {
            "wheel_loads_kg": self.wheel_loads_kg,
            "axle_loads_kg": self.axle_loads_kg,
            "total_mass_kg": self.total_mass_kg,
            "distribution_percent": self.distribution_percent,
            "signals": [signal.as_dict() for signal in self.signals],
            "sample_index": self.sample_index,
        }


@dataclass(frozen=True)
class SvcGeometryResult:
    """SVC static geometry, center of gravity, and inertia values."""

    loads: SvcLoadResult
    ride_heights_mm: dict[str, float]
    wheelbase_mm: float
    tracks_mm: dict[str, float]
    cg_x_from_front_axle_mm: float
    cg_x_global_mm: float
    cg_height_mm: float
    inertias_kg_mm2: dict[str, float]
    signals: tuple[TransformedSignal, ...]
    sample_index: int

    def as_dict(self) -> dict[str, object]:
        return {
            "loads": self.loads.as_dict(),
            "ride_heights_mm": self.ride_heights_mm,
            "wheelbase_mm": self.wheelbase_mm,
            "tracks_mm": self.tracks_mm,
            "cg_x_from_front_axle_mm": self.cg_x_from_front_axle_mm,
            "cg_x_global_mm": self.cg_x_global_mm,
            "cg_height_mm": self.cg_height_mm,
            "inertias_kg_mm2": self.inertias_kg_mm2,
            "signals": [signal.as_dict() for signal in self.signals],
            "sample_index": self.sample_index,
        }


def evaluate_svc_loads(
    config: CaseConfig | Path | str,
    *,
    sample_index: int = -1,
) -> SvcLoadResult:
    """Calculate SVC values; ``-1`` means static final sample."""
    case = load_config(config) if not isinstance(config, CaseConfig) else config
    if case.domain != "svc":
        raise SvcEvaluationError(
            f"SVC evaluator requires domain 'svc', found '{case.domain}'"
        )

    transformed: list[TransformedSignal] = []
    with AdamsHdf5(case.simulation_hdf5) as result:
        for name in WHEEL_LOAD_NAMES:
            try:
                reference = case.signal(name)
                convention = SVC_SIGNAL_CONVENTIONS[name]
            except KeyError as error:
                raise SvcEvaluationError(
                    f"missing SVC wheel-load mapping: {name}"
                ) from error
            signal = result.read_signal(reference.entity, reference.component)
            if signal.unit and signal.unit != reference.unit:
                raise SvcEvaluationError(
                    f"{name}: source unit '{signal.unit}' "
                    f"!= configured '{reference.unit}'"
                )
            transformed.append(
                apply_convention(
                    signal.values,
                    name=name,
                    source=f"{case.simulation_hdf5}:{signal.entity}/{signal.component}",
                    convention=convention,
                    gravity=case.gravity,
                )
            )

    wheel_loads = {
        signal.name: _sample_value(signal, sample_index)
        for signal in transformed
    }
    axle_loads = {
        "front": wheel_loads["front_left_wheel_load"]
        + wheel_loads["front_right_wheel_load"],
        "rear": wheel_loads["rear_left_wheel_load"]
        + wheel_loads["rear_right_wheel_load"],
    }
    total_mass = axle_loads["front"] + axle_loads["rear"]
    if not np.isfinite(total_mass) or total_mass <= 0:
        raise SvcEvaluationError(
            f"total SVC mass must be finite and positive, found {total_mass}"
        )
    distribution = {
        "front": 100.0 * axle_loads["front"] / total_mass,
        "rear": 100.0 * axle_loads["rear"] / total_mass,
    }
    return SvcLoadResult(
        wheel_loads_kg=wheel_loads,
        axle_loads_kg=axle_loads,
        total_mass_kg=total_mass,
        distribution_percent=distribution,
        signals=tuple(transformed),
        sample_index=sample_index,
    )


def evaluate_svc_geometry(
    config: CaseConfig | Path | str,
    *,
    sample_index: int = -1,
) -> SvcGeometryResult:
    """Calculate SVC static geometry and inertia at selected sample."""
    case = load_config(config) if not isinstance(config, CaseConfig) else config
    if case.domain != "svc":
        raise SvcEvaluationError(
            f"SVC evaluator requires domain 'svc', found '{case.domain}'"
        )
    loads = evaluate_svc_loads(case, sample_index=sample_index)
    transformed: list[TransformedSignal] = []
    names = (
        "front_left_ride_height",
        "front_right_ride_height",
        "rear_left_ride_height",
        "rear_right_ride_height",
        "wheelbase",
        "front_track",
        "rear_track",
        "cg_height",
        "inertia_xx",
        "inertia_yy",
        "inertia_zz",
        "front_left_wheel_center_x",
    )
    with AdamsHdf5(case.simulation_hdf5) as result:
        for name in names:
            try:
                reference = case.signal(name)
                convention = SVC_SIGNAL_CONVENTIONS[name]
            except KeyError as error:
                raise SvcEvaluationError(
                    f"missing SVC geometry mapping: {name}"
                ) from error
            signal = result.read_signal(reference.entity, reference.component)
            if signal.unit and signal.unit != reference.unit:
                raise SvcEvaluationError(
                    f"{name}: source unit '{signal.unit}' "
                    f"!= configured '{reference.unit}'"
                )
            transformed.append(
                apply_convention(
                    signal.values,
                    name=name,
                    source=f"{case.simulation_hdf5}:{signal.entity}/{signal.component}",
                    convention=convention,
                    gravity=case.gravity,
                )
            )

    values = {
        signal.name: _sample_value(signal, sample_index)
        for signal in transformed
    }
    ride_heights = {
        name.removesuffix("_ride_height"): values[name] for name in names[:4]
    }
    wheelbase = values["wheelbase"]
    if wheelbase <= 0:
        raise SvcEvaluationError(
            f"wheelbase must be positive, found {wheelbase}"
        )
    cg_x_from_front_axle = (
        loads.distribution_percent["rear"] / 100.0 * wheelbase
    )
    cg_x_global = values["front_left_wheel_center_x"] + cg_x_from_front_axle
    return SvcGeometryResult(
        loads=loads,
        ride_heights_mm=ride_heights,
        wheelbase_mm=wheelbase,
        tracks_mm={
            "front": values["front_track"],
            "rear": values["rear_track"],
        },
        cg_x_from_front_axle_mm=cg_x_from_front_axle,
        cg_x_global_mm=cg_x_global,
        cg_height_mm=values["cg_height"],
        inertias_kg_mm2={
            "xx": values["inertia_xx"],
            "yy": values["inertia_yy"],
            "zz": values["inertia_zz"],
        },
        signals=tuple(transformed),
        sample_index=sample_index,
    )


def _sample_value(signal: TransformedSignal, sample_index: int) -> float:
    if signal.values.size == 0:
        raise SvcEvaluationError(
            f"SVC signal '{signal.name}' contains no values"
        )
    try:
        value = float(signal.values[sample_index])
    except IndexError as error:
        raise SvcEvaluationError(
            f"sample index {sample_index} is outside signal "
            f"'{signal.name}' with {signal.values.size} samples"
        ) from error
    if not np.isfinite(value):
        raise SvcEvaluationError(
            f"SVC signal '{signal.name}' at sample {sample_index} must be "
            f"finite, found {value}"
        )
    return value
