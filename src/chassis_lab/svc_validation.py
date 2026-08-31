"""Compare calculated SVC metrics with reviewed physical references."""

from collections.abc import Mapping
from dataclasses import dataclass
from math import isfinite
from pathlib import Path

from chassis_lab.config import CaseConfig, ValidationTolerances, load_config
from chassis_lab.svc_eval import SvcGeometryResult
from chassis_lab.svc_reference import SvcReference, load_svc_reference


class SvcValidationError(ValueError):
    """Raised when an SVC comparison cannot be completed."""


@dataclass(frozen=True)
class MetricComparison:
    """Comparison of one canonical metric against its reference."""

    name: str
    actual: float
    reference: float
    difference: float
    absolute_tolerance: float | None
    relative_percent: float | None
    allowed_difference: float
    passed: bool

    def as_dict(self) -> dict[str, object]:
        return {
            "name": self.name,
            "actual": self.actual,
            "reference": self.reference,
            "difference": self.difference,
            "absolute_tolerance": self.absolute_tolerance,
            "relative_percent": self.relative_percent,
            "allowed_difference": self.allowed_difference,
            "passed": self.passed,
        }


@dataclass(frozen=True)
class SvcValidationResult:
    """Complete comparison result for one configured SVC case."""

    case_name: str
    reference_path: Path
    metrics: tuple[MetricComparison, ...]

    @property
    def passed(self) -> bool:
        return all(metric.passed for metric in self.metrics)

    @property
    def failed(self) -> tuple[MetricComparison, ...]:
        return tuple(metric for metric in self.metrics if not metric.passed)

    def as_dict(self) -> dict[str, object]:
        return {
            "case": self.case_name,
            "reference": str(self.reference_path),
            "passed": self.passed,
            "metrics": [metric.as_dict() for metric in self.metrics],
        }


def svc_metrics(result: SvcGeometryResult) -> dict[str, float]:
    """Flatten an SVC result into canonical validation metric names."""
    loads = result.loads
    return {
        "front_left_mass_kg": loads.wheel_loads_kg["front_left_wheel_load"],
        "front_right_mass_kg": loads.wheel_loads_kg["front_right_wheel_load"],
        "rear_left_mass_kg": loads.wheel_loads_kg["rear_left_wheel_load"],
        "rear_right_mass_kg": loads.wheel_loads_kg["rear_right_wheel_load"],
        "front_mass_kg": loads.axle_loads_kg["front"],
        "rear_mass_kg": loads.axle_loads_kg["rear"],
        "total_mass_kg": loads.total_mass_kg,
        "front_distribution_percent": loads.distribution_percent["front"],
        "rear_distribution_percent": loads.distribution_percent["rear"],
        "wheelbase_mm": result.wheelbase_mm,
        "front_track_mm": result.tracks_mm["front"],
        "rear_track_mm": result.tracks_mm["rear"],
        "cg_x_from_front_axle_mm": result.cg_x_from_front_axle_mm,
        "cg_height_mm": result.cg_height_mm,
        "inertia_xx_kg_mm2": result.inertias_kg_mm2["xx"],
        "inertia_yy_kg_mm2": result.inertias_kg_mm2["yy"],
        "inertia_zz_kg_mm2": result.inertias_kg_mm2["zz"],
        "ride_height_front_left_mm": result.ride_heights_mm["front_left"],
        "ride_height_front_right_mm": result.ride_heights_mm["front_right"],
        "ride_height_rear_left_mm": result.ride_heights_mm["rear_left"],
        "ride_height_rear_right_mm": result.ride_heights_mm["rear_right"],
    }


def compare_svc_metrics(
    actual: Mapping[str, float],
    reference: SvcReference,
    tolerances: ValidationTolerances,
) -> tuple[MetricComparison, ...]:
    """Compare canonical values using configured absolute/relative limits."""
    reference_names = {name for name, _ in reference.values}
    tolerance_names = {
        name for name, _ in (*tolerances.absolute, *tolerances.relative_percent)
    }
    unknown_tolerances = tolerance_names - reference_names
    if unknown_tolerances:
        raise SvcValidationError(
            "tolerance configured for unknown reference metric(s): "
            + ", ".join(sorted(unknown_tolerances))
        )

    comparisons: list[MetricComparison] = []
    for name, expected in reference.values:
        if name not in actual:
            raise SvcValidationError(f"calculated SVC metric missing: {name}")
        absolute = tolerances.absolute_for(name)
        relative = tolerances.relative_percent_for(name)
        if absolute is None and relative is None:
            raise SvcValidationError(
                f"no acceptance tolerance configured for metric: {name}"
            )
        measured = actual[name]
        if not isinstance(measured, (int, float)) or isinstance(measured, bool):
            raise SvcValidationError(
                f"calculated SVC metric '{name}' must be numeric"
            )
        measured = float(measured)
        if not isfinite(measured):
            raise SvcValidationError(
                f"calculated SVC metric '{name}' must be finite"
            )
        allowances = []
        if absolute is not None:
            allowances.append(absolute)
        if relative is not None:
            allowances.append(abs(expected) * relative / 100.0)
        allowed = max(allowances)
        difference = abs(measured - expected)
        comparisons.append(
            MetricComparison(
                name=name,
                actual=measured,
                reference=expected,
                difference=difference,
                absolute_tolerance=absolute,
                relative_percent=relative,
                allowed_difference=allowed,
                passed=difference <= allowed,
            )
        )
    return tuple(comparisons)


def validate_svc(
    config: CaseConfig | Path | str,
    *,
    sample_index: int = -1,
) -> SvcValidationResult:
    """Evaluate SVC geometry from HDF5 and compare against reference."""
    from chassis_lab.svc_eval import evaluate_svc_geometry

    case = load_config(config) if not isinstance(config, CaseConfig) else config
    result = evaluate_svc_geometry(case, sample_index=sample_index)
    return validate_svc_result(case, result)


def validate_svc_result(
    config: CaseConfig,
    result: SvcGeometryResult,
) -> SvcValidationResult:
    """Load configured reference and compare one calculated SVC result."""
    if config.validation is None:
        raise SvcValidationError(
            f"case '{config.name}' has no [validation] configuration"
        )
    reference = load_svc_reference(config.validation.reference)
    _check_case_identity(config, reference)
    comparisons = compare_svc_metrics(
        svc_metrics(result),
        reference,
        config.validation.tolerances,
    )
    return SvcValidationResult(
        case_name=config.name,
        reference_path=reference.path,
        metrics=comparisons,
    )


def _check_case_identity(config: CaseConfig, reference: SvcReference) -> None:
    expected = (config.name, config.variant, config.loadcase)
    actual = (
        reference.case.name,
        reference.case.variant,
        reference.case.loadcase,
    )
    if actual != expected:
        raise SvcValidationError(
            "reference case identity does not match configuration: "
            f"expected {expected}, found {actual}"
        )
