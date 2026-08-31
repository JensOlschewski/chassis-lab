"""Load reviewed physical reference values for SVC validation."""

import json
from dataclasses import dataclass
from datetime import date
from math import isclose, isfinite
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
TOP_LEVEL_KEYS = {
    "schema_version",
    "case",
    "coordinate_system",
    "sources",
    "values",
}
CASE_KEYS = {"name", "variant", "loadcase"}
COORDINATE_KEYS = {"cg_x_origin", "cg_x_positive"}
SOURCE_KEYS = {"file", "sha256", "test_date", "result"}
REQUIRED_SOURCE_KEYS = {"file", "sha256", "result"}
SVC_REFERENCE_METRICS = {
    "front_left_mass_kg",
    "front_right_mass_kg",
    "rear_left_mass_kg",
    "rear_right_mass_kg",
    "front_mass_kg",
    "rear_mass_kg",
    "total_mass_kg",
    "front_distribution_percent",
    "rear_distribution_percent",
    "wheelbase_mm",
    "front_track_mm",
    "rear_track_mm",
    "cg_x_from_front_axle_mm",
    "cg_height_mm",
    "inertia_xx_kg_mm2",
    "inertia_yy_kg_mm2",
    "inertia_zz_kg_mm2",
    "ride_height_front_left_mm",
    "ride_height_front_right_mm",
    "ride_height_rear_left_mm",
    "ride_height_rear_right_mm",
}


class SvcReferenceError(ValueError):
    """Raised when an SVC reference file violates its schema."""


@dataclass(frozen=True)
class SvcReferenceCase:
    name: str
    variant: str
    loadcase: str


@dataclass(frozen=True)
class SvcCoordinateSystem:
    cg_x_origin: str
    cg_x_positive: str


@dataclass(frozen=True)
class SvcReferenceSource:
    file: str
    sha256: str
    result: str
    test_date: str | None = None


@dataclass(frozen=True)
class SvcReference:
    path: Path
    schema_version: int
    case: SvcReferenceCase
    coordinate_system: SvcCoordinateSystem
    sources: tuple[SvcReferenceSource, ...]
    values: tuple[tuple[str, float], ...]

    def value(self, metric: str) -> float:
        for name, value in self.values:
            if name == metric:
                return value
        raise KeyError(metric)


def load_svc_reference(path: Path | str) -> SvcReference:
    """Load and validate one canonical SVC reference JSON file."""
    reference_path = Path(path).expanduser().resolve()
    if not reference_path.is_file():
        raise FileNotFoundError(
            f"SVC reference file does not exist: {reference_path}"
        )
    try:
        raw = json.loads(reference_path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        raise SvcReferenceError(
            f"invalid JSON in {reference_path}: {error}"
        ) from error
    if not isinstance(raw, dict):
        raise SvcReferenceError("SVC reference root must be an object")
    _require_exact_keys(raw, TOP_LEVEL_KEYS, "SVC reference")

    schema_version = raw["schema_version"]
    if (
        not isinstance(schema_version, int)
        or isinstance(schema_version, bool)
        or schema_version != SCHEMA_VERSION
    ):
        raise SvcReferenceError(
            f"unsupported SVC reference schema version: {schema_version!r}"
        )

    case = _parse_case(raw["case"])
    coordinate_system = _parse_coordinate_system(raw["coordinate_system"])
    sources = _parse_sources(raw["sources"])
    values = _parse_values(raw["values"])
    _validate_consistency(dict(values))
    return SvcReference(
        path=reference_path,
        schema_version=schema_version,
        case=case,
        coordinate_system=coordinate_system,
        sources=sources,
        values=values,
    )


def _parse_case(raw: object) -> SvcReferenceCase:
    table = _require_object(raw, "case")
    _require_exact_keys(table, CASE_KEYS, "case")
    values = {
        key: _require_nonempty_string(table[key], f"case.{key}")
        for key in CASE_KEYS
    }
    return SvcReferenceCase(
        name=values["name"],
        variant=values["variant"],
        loadcase=values["loadcase"],
    )


def _parse_coordinate_system(raw: object) -> SvcCoordinateSystem:
    table = _require_object(raw, "coordinate_system")
    _require_exact_keys(table, COORDINATE_KEYS, "coordinate_system")
    origin = _require_nonempty_string(
        table["cg_x_origin"], "coordinate_system.cg_x_origin"
    )
    positive = _require_nonempty_string(
        table["cg_x_positive"], "coordinate_system.cg_x_positive"
    )
    if origin != "front_axle_wheel_center" or positive != "rearward":
        raise SvcReferenceError(
            "SVC CG-X coordinates must use front_axle_wheel_center origin "
            "and rearward-positive direction"
        )
    return SvcCoordinateSystem(
        cg_x_origin=origin,
        cg_x_positive=positive,
    )


def _parse_sources(raw: object) -> tuple[SvcReferenceSource, ...]:
    if not isinstance(raw, list) or not raw:
        raise SvcReferenceError("sources must be a non-empty array")
    sources: list[SvcReferenceSource] = []
    filenames: set[str] = set()
    for index, item in enumerate(raw):
        table = _require_object(item, f"sources[{index}]")
        _reject_unknown(table, SOURCE_KEYS, f"sources[{index}]")
        missing = REQUIRED_SOURCE_KEYS - set(table)
        if missing:
            raise SvcReferenceError(
                f"sources[{index}] missing key(s): {', '.join(sorted(missing))}"
            )
        filename = _require_nonempty_string(
            table["file"], f"sources[{index}].file"
        )
        if filename in filenames:
            raise SvcReferenceError(f"duplicate reference source: {filename}")
        filenames.add(filename)
        checksum = _require_nonempty_string(
            table["sha256"], f"sources[{index}].sha256"
        )
        if len(checksum) != 64 or any(
            character not in "0123456789abcdef" for character in checksum
        ):
            raise SvcReferenceError(
                f"sources[{index}].sha256 must be lowercase SHA-256 hex"
            )
        result = _require_nonempty_string(
            table["result"], f"sources[{index}].result"
        )
        test_date = table.get("test_date")
        if test_date is not None:
            test_date = _require_nonempty_string(
                test_date, f"sources[{index}].test_date"
            )
            try:
                date.fromisoformat(test_date)
            except ValueError as error:
                raise SvcReferenceError(
                    f"sources[{index}].test_date must use YYYY-MM-DD"
                ) from error
        sources.append(
            SvcReferenceSource(
                file=filename,
                sha256=checksum,
                result=result,
                test_date=test_date,
            )
        )
    return tuple(sources)


def _parse_values(raw: object) -> tuple[tuple[str, float], ...]:
    table = _require_object(raw, "values")
    if not table:
        raise SvcReferenceError("values must define at least one metric")
    unknown = set(table) - SVC_REFERENCE_METRICS
    if unknown:
        raise SvcReferenceError(
            f"unknown SVC reference metric(s): {', '.join(sorted(unknown))}"
        )
    values: list[tuple[str, float]] = []
    for metric, value in table.items():
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise SvcReferenceError(
                f"reference metric '{metric}' must be numeric"
            )
        numeric = float(value)
        if not isfinite(numeric):
            raise SvcReferenceError(
                f"reference metric '{metric}' must be finite"
            )
        values.append((metric, numeric))
    return tuple(values)


def _validate_consistency(values: dict[str, float]) -> None:
    _check_sum(
        values,
        ("front_left_mass_kg", "front_right_mass_kg"),
        "front_mass_kg",
    )
    _check_sum(
        values,
        ("rear_left_mass_kg", "rear_right_mass_kg"),
        "rear_mass_kg",
    )
    _check_sum(
        values,
        ("front_mass_kg", "rear_mass_kg"),
        "total_mass_kg",
    )
    _check_sum(
        values,
        ("front_distribution_percent", "rear_distribution_percent"),
        expected=100.0,
    )
    if {
        "front_mass_kg",
        "rear_mass_kg",
        "total_mass_kg",
        "front_distribution_percent",
        "rear_distribution_percent",
    }.issubset(values):
        total = values["total_mass_kg"]
        if total <= 0:
            raise SvcReferenceError("total_mass_kg must be positive")
        for axle in ("front", "rear"):
            actual = values[f"{axle}_distribution_percent"]
            expected = 100.0 * values[f"{axle}_mass_kg"] / total
            if not isclose(actual, expected, abs_tol=0.051):
                raise SvcReferenceError(
                    f"{axle}_distribution_percent is inconsistent with masses"
                )
    if {
        "rear_mass_kg",
        "total_mass_kg",
        "wheelbase_mm",
        "cg_x_from_front_axle_mm",
    }.issubset(values):
        total = values["total_mass_kg"]
        if total <= 0:
            raise SvcReferenceError("total_mass_kg must be positive")
        expected = values["rear_mass_kg"] / total * values["wheelbase_mm"]
        if not isclose(
            values["cg_x_from_front_axle_mm"], expected, abs_tol=0.001
        ):
            raise SvcReferenceError(
                "cg_x_from_front_axle_mm is inconsistent with mass distribution"
            )


def _check_sum(
    values: dict[str, float],
    parts: tuple[str, str],
    result: str | None = None,
    *,
    expected: float | None = None,
) -> None:
    required = {*parts, *((result,) if result is not None else ())}
    if not required.issubset(values):
        return
    target = values[result] if result is not None else expected
    assert target is not None
    if not isclose(sum(values[name] for name in parts), target, abs_tol=1e-6):
        description = result or " + ".join(parts)
        raise SvcReferenceError(
            f"{description} is inconsistent with components"
        )


def _require_object(raw: object, where: str) -> dict[str, Any]:
    if not isinstance(raw, dict):
        raise SvcReferenceError(f"{where} must be an object")
    return raw


def _require_exact_keys(
    table: dict[str, Any], expected: set[str], where: str
) -> None:
    _reject_unknown(table, expected, where)
    missing = expected - set(table)
    if missing:
        raise SvcReferenceError(
            f"{where} missing key(s): {', '.join(sorted(missing))}"
        )


def _reject_unknown(
    table: dict[str, Any], allowed: set[str], where: str
) -> None:
    unknown = set(table) - allowed
    if unknown:
        raise SvcReferenceError(
            f"unknown {where} key(s): {', '.join(sorted(unknown))}"
        )


def _require_nonempty_string(raw: object, where: str) -> str:
    if not isinstance(raw, str) or not raw:
        raise SvcReferenceError(f"{where} must be a non-empty string")
    return raw
