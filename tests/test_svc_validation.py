import json
from dataclasses import replace
from pathlib import Path

import pytest

from chassis_lab.config import (
    CaseConfig,
    ValidationConfig,
    ValidationTolerances,
)
from chassis_lab.svc_eval import SvcGeometryResult, SvcLoadResult
from chassis_lab.svc_reference import load_svc_reference
from chassis_lab.svc_validation import (
    SvcValidationError,
    compare_svc_metrics,
    svc_metrics,
    validate_svc_result,
)


def geometry_result() -> SvcGeometryResult:
    loads = SvcLoadResult(
        wheel_loads_kg={
            "front_left_wheel_load": 400.0,
            "front_right_wheel_load": 390.0,
            "rear_left_wheel_load": 350.0,
            "rear_right_wheel_load": 360.0,
        },
        axle_loads_kg={"front": 790.0, "rear": 710.0},
        total_mass_kg=1500.0,
        distribution_percent={"front": 52.666, "rear": 47.334},
        signals=(),
        sample_index=-1,
    )
    return SvcGeometryResult(
        loads=loads,
        ride_heights_mm={
            "front_left": 356.0,
            "front_right": 352.0,
            "rear_left": 348.0,
            "rear_right": 350.0,
        },
        wheelbase_mm=2500.0,
        tracks_mm={"front": 1500.0, "rear": 1470.0},
        cg_x_from_front_axle_mm=1183.35,
        cg_x_global_mm=2983.35,
        cg_height_mm=490.0,
        inertias_kg_mm2={"xx": 500e6, "yy": 1900e6, "zz": 2000e6},
        signals=(),
        sample_index=-1,
    )


def reference_data(values: dict[str, float]) -> dict[str, object]:
    return {
        "schema_version": 1,
        "case": {
            "name": "varb-lommel-curb-svc",
            "variant": "VARB",
            "loadcase": "CURB",
        },
        "coordinate_system": {
            "cg_x_origin": "front_axle_wheel_center",
            "cg_x_positive": "rearward",
        },
        "sources": [
            {
                "file": "source.txt",
                "sha256": "a" * 64,
                "result": "reviewed",
            }
        ],
        "values": values,
    }


def write_reference(tmp_path: Path, values: dict[str, float]) -> Path:
    path = tmp_path / "reference.json"
    path.write_text(json.dumps(reference_data(values)), encoding="utf-8")
    return path


def tolerances(
    *,
    absolute: tuple[tuple[str, float], ...] = (),
    relative: tuple[tuple[str, float], ...] = (),
) -> ValidationTolerances:
    return ValidationTolerances(
        absolute=absolute,
        relative_percent=relative,
    )


def test_flattens_geometry_to_canonical_metrics() -> None:
    metrics = svc_metrics(geometry_result())

    assert len(metrics) == 21
    assert metrics["front_left_mass_kg"] == 400.0
    assert metrics["front_mass_kg"] == 790.0
    assert metrics["front_distribution_percent"] == 52.666
    assert metrics["cg_x_from_front_axle_mm"] == 1183.35
    assert metrics["inertia_zz_kg_mm2"] == 2000e6
    assert metrics["ride_height_rear_right_mm"] == 350.0
    assert "cg_x_global_mm" not in metrics


def test_combines_absolute_and_relative_tolerances(tmp_path: Path) -> None:
    reference = load_svc_reference(
        write_reference(tmp_path, {"total_mass_kg": 100.0})
    )
    policy = tolerances(
        absolute=(("total_mass_kg", 2.0),),
        relative=(("total_mass_kg", 5.0),),
    )

    comparison = compare_svc_metrics(
        {"total_mass_kg": 104.0}, reference, policy
    )[0]

    assert comparison.difference == 4.0
    assert comparison.allowed_difference == 5.0
    assert comparison.passed


def test_supports_absolute_only_and_reports_failure(tmp_path: Path) -> None:
    reference = load_svc_reference(
        write_reference(tmp_path, {"wheelbase_mm": 2500.0})
    )

    comparison = compare_svc_metrics(
        {"wheelbase_mm": 2506.0},
        reference,
        tolerances(absolute=(("wheelbase_mm", 5.0),)),
    )[0]

    assert not comparison.passed
    assert comparison.relative_percent is None


def test_supports_relative_only(tmp_path: Path) -> None:
    reference = load_svc_reference(
        write_reference(tmp_path, {"inertia_xx_kg_mm2": 500e6})
    )

    comparison = compare_svc_metrics(
        {"inertia_xx_kg_mm2": 524e6},
        reference,
        tolerances(relative=(("inertia_xx_kg_mm2", 5.0),)),
    )[0]

    assert comparison.allowed_difference == 25e6
    assert comparison.passed


@pytest.mark.parametrize(
    ("actual", "policy", "message"),
    [
        ({}, tolerances(absolute=(("total_mass_kg", 1.0),)), "missing"),
        ({"total_mass_kg": 100.0}, tolerances(), "no acceptance"),
        (
            {"total_mass_kg": 100.0},
            tolerances(absolute=(("wheelbase_mm", 1.0),)),
            "unknown reference metric",
        ),
        (
            {"total_mass_kg": float("nan")},
            tolerances(absolute=(("total_mass_kg", 1.0),)),
            "finite",
        ),
    ],
)
def test_rejects_incomplete_comparison_inputs(
    tmp_path: Path,
    actual: dict[str, float],
    policy: ValidationTolerances,
    message: str,
) -> None:
    reference = load_svc_reference(
        write_reference(tmp_path, {"total_mass_kg": 100.0})
    )

    with pytest.raises(SvcValidationError, match=message):
        compare_svc_metrics(actual, reference, policy)


def test_validates_result_and_exposes_failures(tmp_path: Path) -> None:
    reference_path = write_reference(
        tmp_path,
        {"total_mass_kg": 1490.0, "wheelbase_mm": 2490.0},
    )
    config = CaseConfig(
        path=tmp_path / "case.toml",
        name="varb-lommel-curb-svc",
        domain="svc",
        variant="VARB",
        loadcase="CURB",
        simulation_hdf5=tmp_path / "result.h5",
        gravity=9.80665,
        signals=(),
        validation=ValidationConfig(
            reference=reference_path,
            tolerances=tolerances(
                absolute=(
                    ("total_mass_kg", 20.0),
                    ("wheelbase_mm", 5.0),
                )
            ),
        ),
    )

    result = validate_svc_result(config, geometry_result())

    assert not result.passed
    assert [metric.name for metric in result.failed] == ["wheelbase_mm"]
    assert result.as_dict()["passed"] is False


def test_rejects_missing_validation_and_case_mismatch(tmp_path: Path) -> None:
    config = CaseConfig(
        path=tmp_path / "case.toml",
        name="varb-lommel-curb-svc",
        domain="svc",
        variant="VARB",
        loadcase="CURB",
        simulation_hdf5=tmp_path / "result.h5",
        gravity=9.80665,
        signals=(),
    )
    with pytest.raises(SvcValidationError, match=r"no \[validation\]"):
        validate_svc_result(config, geometry_result())

    reference_path = write_reference(tmp_path, {"total_mass_kg": 1500.0})
    config = replace(
        config,
        variant="VARC",
        validation=ValidationConfig(
            reference=reference_path,
            tolerances=tolerances(absolute=(("total_mass_kg", 20.0),)),
        ),
    )
    with pytest.raises(SvcValidationError, match="does not match"):
        validate_svc_result(config, geometry_result())
