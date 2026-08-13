from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.config import load_config
from chassis_lab.svc_eval import SvcEvaluationError, evaluate_svc_loads

CONFIG = """\
[case]
name = "svc-fixture"
domain = "svc"
variant = "VARB"
loadcase = "2UP"

[inputs]
simulation_hdf5 = "result.h5"

[physics]
gravity = 10.0

[signals]
[signals.front_left_wheel_load]
entity = "forces"
component = "front_left"
unit = "newton"
[signals.front_right_wheel_load]
entity = "forces"
component = "front_right"
unit = "newton"
[signals.rear_left_wheel_load]
entity = "forces"
component = "rear_left"
unit = "newton"
[signals.rear_right_wheel_load]
entity = "forces"
component = "rear_right"
unit = "newton"
"""


def write_fixture(tmp_path: Path, values: np.ndarray) -> Path:
    config_path = tmp_path / "case.toml"
    config_path.write_text(CONFIG, encoding="utf-8")
    with h5py.File(tmp_path / "result.h5", "w") as h5:
        h5.attrs.update(
            format_version=3,
            sample_count=values.shape[0],
            signal_count=values.shape[1],
            source_file="fixture.res",
        )
        entity = h5.create_group("results/forces")
        entity.create_dataset("values", data=values)
        string_dtype = h5py.string_dtype(encoding="utf-8")
        entity.create_dataset(
            "component_names",
            data=np.asarray(
                ["front_left", "front_right", "rear_left", "rear_right"],
                dtype=string_dtype,
            ),
        )
        entity.create_dataset(
            "units",
            data=np.asarray(["newton"] * 4, dtype=string_dtype),
        )
        entity.create_dataset(
            "adams_ids", data=np.arange(1, 5, dtype=np.uint64)
        )
    return config_path


def test_evaluates_final_wheel_loads_and_distribution(tmp_path: Path) -> None:
    config_path = write_fixture(
        tmp_path,
        np.asarray([[1.0, 2.0, 3.0, 4.0], [100.0, 200.0, 300.0, 400.0]]),
    )

    result = evaluate_svc_loads(config_path)

    assert result.wheel_loads_kg == {
        "front_left_wheel_load": 10.0,
        "front_right_wheel_load": 20.0,
        "rear_left_wheel_load": 30.0,
        "rear_right_wheel_load": 40.0,
    }
    assert result.axle_loads_kg == {"front": 30.0, "rear": 70.0}
    assert result.total_mass_kg == 100.0
    assert result.distribution_percent == {"front": 30.0, "rear": 70.0}


def test_preserves_signal_provenance_and_accepts_loaded_config(
    tmp_path: Path,
) -> None:
    config_path = write_fixture(tmp_path, np.asarray([[10.0] * 4]))

    result = evaluate_svc_loads(load_config(config_path))

    assert len(result.signals) == 4
    assert result.signals[0].source_unit == "newton"
    assert result.signals[0].unit == "kg"
    assert result.signals[0].convention.corner == "front_left"
    assert result.signals[0].values[-1] == 1.0


def test_rejects_non_svc_case(tmp_path: Path) -> None:
    config_path = write_fixture(tmp_path, np.asarray([[10.0] * 4]))
    text = config_path.read_text(encoding="utf-8").replace(
        'domain = "svc"', 'domain = "other"'
    )
    config_path.write_text(text, encoding="utf-8")

    with pytest.raises(SvcEvaluationError, match="requires domain"):
        evaluate_svc_loads(config_path)


def test_rejects_non_positive_total_mass(tmp_path: Path) -> None:
    config_path = write_fixture(tmp_path, np.asarray([[0.0] * 4]))

    with pytest.raises(SvcEvaluationError, match="positive"):
        evaluate_svc_loads(config_path)


def test_rejects_missing_wheel_mapping(tmp_path: Path) -> None:
    config_path = write_fixture(tmp_path, np.asarray([[10.0] * 4]))
    missing_mapping = (
        "[signals.rear_right_wheel_load]\n"
        'entity = "forces"\n'
        'component = "rear_right"\n'
        'unit = "newton"\n'
    )
    text = config_path.read_text(encoding="utf-8").replace(missing_mapping, "")
    config_path.write_text(text, encoding="utf-8")

    with pytest.raises(SvcEvaluationError, match="rear_right_wheel_load"):
        evaluate_svc_loads(config_path)
