from dataclasses import replace
from pathlib import Path

import pytest

from chassis_lab.config import load_config
from chassis_lab.svc_eval import evaluate_svc_geometry
from chassis_lab.svc_validation import (
    SvcValidationError,
    validate_svc,
    validate_svc_result,
)

_ROOT = Path(__file__).resolve().parent.parent
VARB_CONFIG_PATH = _ROOT / "configs" / "varb-lommel-curb-svc.toml"
VARC_CONFIG_PATH = _ROOT / "configs" / "varc-lommel-curb-svc.toml"
VARB_2UP_CONFIG_PATH = _ROOT / "configs" / "varb-lommel-2up-svc.toml"
VARC_2UP_CONFIG_PATH = _ROOT / "configs" / "varc-lommel-2up-svc.toml"
EXPECTED_METRIC_COUNTS = {
    "varb-lommel-curb-svc": 21,
    "varc-lommel-curb-svc": 29,
    "varb-lommel-2up-svc": 21,
    "varc-lommel-2up-svc": 25,
}
EXPECTED_FAILURES = {
    "varb-lommel-curb-svc": {
        "front_left_mass_kg",
        "ride_height_front_right_mm",
        "ride_height_rear_left_mm",
        "ride_height_rear_right_mm",
    },
    "varc-lommel-curb-svc": {
        "front_left_mass_kg",
        "wheelbase_mm",
        "ride_height_rear_left_mm",
        "ride_height_rear_right_mm",
        "toe_rear_left_deg",
        "toe_rear_right_deg",
        "camber_rear_left_deg",
        "camber_rear_right_deg",
    },
    "varb-lommel-2up-svc": {
        "front_left_mass_kg",
        "toe_front_right_deg",
        "camber_front_right_deg",
    },
    "varc-lommel-2up-svc": {
        "front_left_mass_kg",
        "ride_height_rear_left_mm",
        "ride_height_rear_right_mm",
    },
}


@pytest.fixture(
    params=(
        VARB_CONFIG_PATH,
        VARC_CONFIG_PATH,
        VARB_2UP_CONFIG_PATH,
        VARC_2UP_CONFIG_PATH,
    )
)
def svc_config(request: pytest.FixtureRequest) -> Path:
    config_path = request.param
    if not config_path.is_file():
        pytest.skip(f"SVC config not found: {config_path}")
    config = load_config(config_path)
    if not config.simulation_hdf5.is_file():
        pytest.skip(
            f"SVC HDF5 artifact not available: {config.simulation_hdf5}"
        )
    return config_path


def test_evaluate_svc_geometry_returns_all_metrics(svc_config: Path) -> None:
    result = evaluate_svc_geometry(svc_config)

    assert result.loads.total_mass_kg > 0
    assert len(result.ride_heights_mm) == 4
    assert len(result.tracks_mm) == 2
    assert len(result.inertias_kg_mm2) == 3
    assert len(result.toe_deg) == 4
    assert len(result.camber_deg) == 4
    assert result.cg_x_from_front_axle_mm > 0
    assert result.cg_height_mm > 0


def test_validate_svc_loads_and_compares(svc_config: Path) -> None:
    result = validate_svc(svc_config)

    assert result.case_name == svc_config.stem
    assert len(result.metrics) == EXPECTED_METRIC_COUNTS[result.case_name]
    assert result.metrics[0].name == "front_left_mass_kg"


def test_validate_svc_exposes_expected_failures(svc_config: Path) -> None:
    result = validate_svc(svc_config)

    assert {metric.name for metric in result.failed} == EXPECTED_FAILURES[
        result.case_name
    ]


def test_validate_svc_result_without_validation_raises() -> None:
    config = load_config(VARB_CONFIG_PATH)
    bare = replace(config, validation=None)
    geometry = evaluate_svc_geometry(VARB_CONFIG_PATH)

    with pytest.raises(SvcValidationError, match=r"no \[validation\]"):
        validate_svc_result(bare, geometry)


def test_validate_svc_roundtrip_dict(svc_config: Path) -> None:
    result = validate_svc(svc_config)

    data = result.as_dict()
    assert data["case"] == svc_config.stem
    assert isinstance(data["metrics"], list)
    assert len(data["metrics"]) == EXPECTED_METRIC_COUNTS[result.case_name]
    assert data["metrics"][0]["name"] == "front_left_mass_kg"
