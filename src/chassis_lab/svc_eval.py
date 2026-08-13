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

    def as_dict(self) -> dict[str, object]:
        return {
            "wheel_loads_kg": self.wheel_loads_kg,
            "axle_loads_kg": self.axle_loads_kg,
            "total_mass_kg": self.total_mass_kg,
            "distribution_percent": self.distribution_percent,
            "signals": [signal.as_dict() for signal in self.signals],
        }


def evaluate_svc_loads(config: CaseConfig | Path | str) -> SvcLoadResult:
    """Calculate SVC wheel loads, axle loads, mass, and distribution."""
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
            signal = result.read_signal(
                reference.entity,
                reference.component,
                expected_unit=reference.unit,
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

    wheel_loads = {signal.name: _final_value(signal) for signal in transformed}
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
    )


def _final_value(signal: TransformedSignal) -> float:
    if signal.values.size == 0:
        raise SvcEvaluationError(
            f"SVC signal '{signal.name}' contains no values"
        )
    value = float(signal.values[-1])
    if not np.isfinite(value):
        raise SvcEvaluationError(
            f"final SVC signal '{signal.name}' must be finite, found {value}"
        )
    return value
