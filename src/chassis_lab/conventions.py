"""Shared units, corner, sign, and baseline conventions."""

from dataclasses import dataclass
from enum import StrEnum
from math import pi
from typing import Literal

import numpy as np
from numpy.typing import ArrayLike, NDArray


class ConventionError(ValueError):
    """Raised when a convention or conversion is invalid."""


class Corner(StrEnum):
    """Canonical vehicle corner names."""

    FRONT_LEFT = "front_left"
    FRONT_RIGHT = "front_right"
    REAR_LEFT = "rear_left"
    REAR_RIGHT = "rear_right"


class BaselinePolicy(StrEnum):
    """Policy describing how a signal's baseline is interpreted."""

    ABSOLUTE = "absolute"
    MEASUREMENT_OFFSET = "measurement_offset"
    STATIC_SIMULATION = "static_simulation"


Unit = Literal[
    "newton",
    "kg",
    "mm",
    "m",
    "rad",
    "deg",
    "newton-mm",
    "newton-meter",
    "kg*mm^2",
]


@dataclass(frozen=True)
class SignalConvention:
    """Semantic convention applied to one canonical signal."""

    unit: Unit
    source_unit: str = ""
    scale: float = 1.0
    sign: int = 1
    corner: Corner | None = None
    baseline: BaselinePolicy = BaselinePolicy.ABSOLUTE

    def __post_init__(self) -> None:
        if self.sign not in (-1, 1):
            raise ConventionError("sign must be either 1 or -1")
        if not np.isfinite(self.scale) or self.scale == 0:
            raise ConventionError("scale must be finite and non-zero")


@dataclass(frozen=True)
class TransformedSignal:
    """Signal values plus source and convention provenance."""

    name: str
    values: NDArray[np.float64]
    unit: Unit
    source: str
    source_unit: str
    convention: SignalConvention

    @property
    def original_unit(self) -> str:
        return self.source_unit

    def as_dict(self) -> dict[str, object]:
        return {
            "name": self.name,
            "unit": self.unit,
            "source": self.source,
            "original_unit": self.original_unit,
            "scale": self.convention.scale,
            "sign": self.convention.sign,
            "corner": self.convention.corner,
            "baseline": self.convention.baseline,
        }


def convert_values(
    values: ArrayLike,
    source_unit: str,
    target_unit: Unit,
    *,
    gravity: float = 9.80665,
) -> NDArray[np.float64]:
    """Convert numeric values between supported units.

    Empty source units are rejected. Semantic knowledge from a configuration
    must not silently turn missing metadata into a numeric conversion.
    """
    if not source_unit:
        raise ConventionError(
            f"cannot convert to '{target_unit}' without source unit metadata"
        )
    if gravity <= 0 or not np.isfinite(gravity):
        raise ConventionError("gravity must be finite and positive")
    factor = _conversion_factor(source_unit, target_unit, gravity)
    return np.asarray(values, dtype=np.float64) * factor


def apply_convention(
    values: ArrayLike,
    *,
    name: str,
    source: str,
    convention: SignalConvention,
    gravity: float = 9.80665,
) -> TransformedSignal:
    """Convert and sign a signal while retaining transformation provenance."""
    converted = convert_values(
        values,
        convention.source_unit,
        convention.unit,
        gravity=gravity,
    )
    transformed = converted * convention.scale * convention.sign
    return TransformedSignal(
        name=name,
        values=np.asarray(transformed, dtype=np.float64),
        unit=convention.unit,
        source=source,
        source_unit=convention.source_unit,
        convention=convention,
    )


def _conversion_factor(
    source_unit: str, target_unit: Unit, gravity: float
) -> float:
    if source_unit == target_unit:
        return 1.0
    factors = {
        ("newton", "kg"): 1.0 / gravity,
        ("kg", "newton"): gravity,
        ("mm", "m"): 0.001,
        ("m", "mm"): 1000.0,
        ("rad", "deg"): 180.0 / pi,
        ("deg", "rad"): pi / 180.0,
        ("newton-mm", "newton-meter"): 0.001,
        ("newton-meter", "newton-mm"): 1000.0,
    }
    try:
        return factors[(source_unit, target_unit)]
    except KeyError as error:
        raise ConventionError(
            f"unsupported conversion: '{source_unit}' -> '{target_unit}'"
        ) from error
