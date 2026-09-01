from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.config import load_config
from chassis_lab.svc_eval import (
    SvcEvaluationError,
    evaluate_svc_geometry,
    evaluate_svc_loads,
)

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


def write_geometry_fixture(tmp_path: Path) -> Path:
    config_path = write_fixture(
        tmp_path, np.asarray([[100.0] * 4, [100.0] * 4])
    )
    config_path.write_text(
        config_path.read_text(encoding="utf-8")
        + """
[signals.front_left_ride_height]
entity = "ride_heights"
component = "front_left"
unit = "mm"
[signals.front_right_ride_height]
entity = "ride_heights"
component = "front_right"
unit = "mm"
[signals.rear_left_ride_height]
entity = "ride_heights"
component = "rear_left"
unit = "mm"
[signals.rear_right_ride_height]
entity = "ride_heights"
component = "rear_right"
unit = "mm"
[signals.wheelbase]
entity = "svc_output"
component = "wheelbase"
unit = "mm"
[signals.front_track]
entity = "svc_output"
component = "front_track"
unit = "mm"
[signals.rear_track]
entity = "svc_output"
component = "rear_track"
unit = "mm"
[signals.cg_height]
entity = "svc_output"
component = "cg_height"
unit = "mm"
[signals.inertia_xx]
entity = "svc_output"
component = "ixx"
unit = "kg*mm^2"
[signals.inertia_yy]
entity = "svc_output"
component = "iyy"
unit = "kg*mm^2"
[signals.inertia_zz]
entity = "svc_output"
component = "izz"
unit = "kg*mm^2"
[signals.front_left_wheel_center_x]
entity = "front_wheel"
component = "x"
unit = "mm"
[signals.front_left_toe]
entity = "toe_camber_body"
component = "left_toe_angle_front"
unit = "deg"
[signals.front_right_toe]
entity = "toe_camber_body"
component = "right_toe_angle_front"
unit = "deg"
[signals.rear_left_toe]
entity = "/results/toe_camber_body_2"
component = "left_toe_angle_rear"
unit = "deg"
[signals.rear_right_toe]
entity = "/results/toe_camber_body_2"
component = "right_toe_angle_rear"
unit = "deg"
[signals.front_left_camber]
entity = "toe_camber_body"
component = "left_camber_angle_front"
unit = "deg"
[signals.front_right_camber]
entity = "toe_camber_body"
component = "right_camber_angle_front"
unit = "deg"
[signals.rear_left_camber]
entity = "/results/toe_camber_body_2"
component = "left_camber_angle_rear"
unit = "deg"
[signals.rear_right_camber]
entity = "/results/toe_camber_body_2"
component = "right_camber_angle_rear"
unit = "deg"
""",
        encoding="utf-8",
    )
    with h5py.File(tmp_path / "result.h5", "a") as h5:
        results = h5["results"]
        string_dtype = h5py.string_dtype(encoding="utf-8")
        metadata = {
            "ride_heights": (
                ["front_left", "front_right", "rear_left", "rear_right"],
                [10.0, 11.0, 12.0, 13.0],
                ["mm"] * 4,
            ),
            "svc_output": (
                [
                    "wheelbase",
                    "front_track",
                    "rear_track",
                    "cg_height",
                    "ixx",
                    "iyy",
                    "izz",
                ],
                [2800.0, 1600.0, 1580.0, 700.0, 1.0, 2.0, 3.0],
                ["mm"] * 4 + ["kg*mm^2"] * 3,
            ),
            "front_wheel": (["x"], [1000.0], ["mm"]),
            "toe_camber_body": (
                [
                    "left_toe_angle_front",
                    "right_toe_angle_front",
                    "left_camber_angle_front",
                    "right_camber_angle_front",
                ],
                [0.1, -0.1, -1.0, -1.1],
                [""] * 4,
            ),
            "toe_camber_body_2": (
                [
                    "left_toe_angle_rear",
                    "right_toe_angle_rear",
                    "left_camber_angle_rear",
                    "right_camber_angle_rear",
                ],
                [0.2, -0.2, -1.2, -1.3],
                [""] * 4,
            ),
        }
        h5.attrs["signal_count"] = 24
        for group_name, (components, row, units) in metadata.items():
            group = results.create_group(group_name)
            group.create_dataset("values", data=np.asarray([row, row]))
            group.create_dataset(
                "component_names",
                data=np.asarray(components, dtype=string_dtype),
            )
            group.create_dataset(
                "units", data=np.asarray(units, dtype=string_dtype)
            )
            group.create_dataset(
                "adams_ids", data=np.arange(len(components), dtype=np.uint64)
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


def test_evaluates_static_geometry_from_final_sample(tmp_path: Path) -> None:
    config_path = write_geometry_fixture(tmp_path)

    result = evaluate_svc_geometry(config_path)

    assert result.sample_index == -1
    assert result.ride_heights_mm == {
        "front_left": 10.0,
        "front_right": 11.0,
        "rear_left": 12.0,
        "rear_right": 13.0,
    }
    assert result.wheelbase_mm == 2800.0
    assert result.tracks_mm == {"front": 1600.0, "rear": 1580.0}
    assert result.cg_x_from_front_axle_mm == 1400.0
    assert result.cg_x_global_mm == 2400.0
    assert result.cg_height_mm == 700.0
    assert result.inertias_kg_mm2 == {"xx": 1.0, "yy": 2.0, "zz": 3.0}
    assert result.toe_deg == {
        "front_left": 0.1,
        "front_right": -0.1,
        "rear_left": 0.2,
        "rear_right": -0.2,
    }
    assert result.camber_deg == {
        "front_left": -1.0,
        "front_right": -1.1,
        "rear_left": -1.2,
        "rear_right": -1.3,
    }

    data = result.as_dict()
    assert data["cg_x_from_front_axle_mm"] == 1400.0
    assert data["cg_x_global_mm"] == 2400.0


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
