from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.cli import main
from chassis_lab.config import (
    CaseConfig,
    ConfigError,
    SignalRef,
    load_config,
)

VALID_TOML = """\
[case]
name = "varb-lommel-2up-svc"
domain = "svc"
variant = "VARB"
loadcase = "2UP"

[inputs]
simulation_hdf5 = "sub/result.h5"

[physics]
gravity = 9.80665

[signals]
[signals.front_left_wheel_load]
entity = "til_wheel_tire_forces"
component = "normal_front"
unit = "newton"
[signals.rear_left_wheel_load]
entity = "/results/til_wheel_tire_forces_2"
component = "normal_rear"
unit = "newton"
"""


def write_config(tmp_path: Path, text: str, name: str = "case.toml") -> Path:
    path = tmp_path / name
    path.write_text(text)
    return path


def test_loads_config_and_resolves_relative_path(tmp_path: Path) -> None:
    config = load_config(write_config(tmp_path, VALID_TOML))

    assert isinstance(config, CaseConfig)
    assert config.name == "varb-lommel-2up-svc"
    assert config.domain == "svc"
    assert config.variant == "VARB"
    assert config.loadcase == "2UP"
    assert config.gravity == 9.80665
    assert config.simulation_hdf5 == (tmp_path / "sub/result.h5").resolve()
    assert config.signals == (
        (
            "front_left_wheel_load",
            SignalRef(
                entity="til_wheel_tire_forces",
                component="normal_front",
                unit="newton",
            ),
        ),
        (
            "rear_left_wheel_load",
            SignalRef(
                entity="/results/til_wheel_tire_forces_2",
                component="normal_rear",
                unit="newton",
            ),
        ),
    )


def test_preserves_absolute_simulation_path(tmp_path: Path) -> None:
    absolute = tmp_path / "other" / "result.h5"
    config = load_config(
        write_config(
            tmp_path,
            VALID_TOML.replace("sub/result.h5", str(absolute)),
        )
    )

    assert config.simulation_hdf5 == absolute.resolve()


def test_svc_uses_shared_mapping_when_signals_are_omitted(
    tmp_path: Path,
) -> None:
    content = VALID_TOML.split("[signals]", 1)[0]
    config = load_config(write_config(tmp_path, content))

    assert len(config.signals) == 16
    assert config.signal("rear_left_wheel_load").entity == (
        "/results/til_wheel_tire_forces_2"
    )


@pytest.mark.parametrize(
    "mutation",
    [
        "gravity = 0",
        "gravity = -1",
    ],
)
def test_rejects_non_positive_gravity(tmp_path: Path, mutation: str) -> None:
    content = VALID_TOML.replace("gravity = 9.80665", mutation)
    with pytest.raises(ConfigError, match="gravity must be positive"):
        load_config(write_config(tmp_path, content))


def test_rejects_missing_required_tables(tmp_path: Path) -> None:
    without_signals = VALID_TOML.replace(
        'domain = "svc"', 'domain = "vertical"'
    )
    without_signals = without_signals.split("[signals]", 1)[0]
    with pytest.raises(ConfigError, match=r"\[signals\] is required"):
        load_config(write_config(tmp_path, without_signals))


def test_rejects_unknown_top_level_key(tmp_path: Path) -> None:
    content = "extra = 1\n\n" + VALID_TOML
    with pytest.raises(ConfigError, match=r"unknown top level key"):
        load_config(write_config(tmp_path, content))


def test_rejects_signal_without_component(tmp_path: Path) -> None:
    content = VALID_TOML.replace('component = "normal_front"\n', "")
    with pytest.raises(ConfigError, match=r"missing key"):
        load_config(write_config(tmp_path, content))


def test_rejects_empty_signals_table(tmp_path: Path) -> None:
    content = VALID_TOML.split("[signals]", 1)[0] + "[signals]\n"
    with pytest.raises(ConfigError, match="at least one signal"):
        load_config(write_config(tmp_path, content))


def test_rejects_invalid_toml(tmp_path: Path) -> None:
    with pytest.raises(ConfigError, match="invalid TOML"):
        load_config(write_config(tmp_path, "[case"))


def test_missing_file_raises_file_not_found(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        load_config(tmp_path / "missing.toml")


def test_rejects_non_string_case_field(tmp_path: Path) -> None:
    content = VALID_TOML.replace('domain = "svc"', "domain = 5")
    with pytest.raises(ConfigError, match=r"\[case\].domain"):
        load_config(write_config(tmp_path, content))


def test_rejects_non_string_simulation_path(tmp_path: Path) -> None:
    content = VALID_TOML.replace(
        'simulation_hdf5 = "sub/result.h5"', "simulation_hdf5 = 5"
    )
    with pytest.raises(ConfigError, match=r"simulation_hdf5"):
        load_config(write_config(tmp_path, content))


def test_rejects_empty_simulation_path(tmp_path: Path) -> None:
    content = VALID_TOML.replace(
        'simulation_hdf5 = "sub/result.h5"', 'simulation_hdf5 = ""'
    )
    with pytest.raises(ConfigError, match=r"simulation_hdf5"):
        load_config(write_config(tmp_path, content))


def write_hdf5_with_entity(
    path: Path,
    group: str,
    *,
    source_path: str,
    component: str,
    unit: str,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with h5py.File(path, "w") as h5:
        h5.attrs["format_version"] = 3
        h5.attrs["sample_count"] = 1
        h5.attrs["signal_count"] = 1
        h5.attrs["values_per_sample"] = 1
        h5.attrs["source_file"] = "fixture.res"
        results = h5.create_group("results")
        entity = results.create_group(group)
        entity.attrs["source_path"] = source_path
        entity.attrs["original_name"] = source_path
        string_type = h5py.string_dtype(encoding="utf-8")
        entity.create_dataset("values", data=np.asarray([[1.0]]))
        entity.create_dataset(
            "component_names",
            data=np.asarray([component], dtype=string_type),
        )
        entity.create_dataset(
            "units", data=np.asarray([unit], dtype=string_type)
        )
        entity.create_dataset(
            "adams_ids", data=np.asarray([1], dtype=np.uint64)
        )


def test_cli_validates_config_json(tmp_path: Path) -> None:
    path = write_config(tmp_path, VALID_TOML)

    assert main(["validate-config", str(path), "--json"]) == 0


def test_cli_validates_config_cross_checks_signals(tmp_path: Path) -> None:
    h5 = tmp_path / "sub" / "result.h5"
    write_hdf5_with_entity(
        h5,
        "til_wheel_tire_forces",
        source_path="til_wheel_tire_forces",
        component="normal_front",
        unit="newton",
    )
    content = VALID_TOML.split("[signals.rear", 1)[0]
    path = write_config(tmp_path, content)

    assert main(["validate-config", str(path)]) == 0


def test_cli_marks_missing_source_unit_unverified(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    h5 = tmp_path / "sub" / "result.h5"
    write_hdf5_with_entity(
        h5,
        "til_wheel_tire_forces",
        source_path="til_wheel_tire_forces",
        component="normal_front",
        unit="",
    )
    content = VALID_TOML.split("[signals.rear", 1)[0]
    path = write_config(tmp_path, content)

    assert main(["validate-config", str(path)]) == 0
    assert "UNVERIFIED [newton]" in capsys.readouterr().out


def test_cli_strict_units_rejects_missing_source_unit(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    h5 = tmp_path / "sub" / "result.h5"
    write_hdf5_with_entity(
        h5,
        "til_wheel_tire_forces",
        source_path="til_wheel_tire_forces",
        component="normal_front",
        unit="",
    )
    content = VALID_TOML.split("[signals.rear", 1)[0]
    path = write_config(tmp_path, content)

    assert main(["validate-config", str(path), "--strict-units"]) == 2
    assert "source unit missing" in capsys.readouterr().err


def test_cli_rejects_duplicate_resolved_cells(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    h5 = tmp_path / "sub" / "result.h5"
    write_hdf5_with_entity(
        h5,
        "til_wheel_tire_forces",
        source_path="til_wheel_tire_forces",
        component="normal_front",
        unit="newton",
    )
    content = (
        VALID_TOML.replace(
            "[signals.rear_left_wheel_load]",
            "[signals.front_right_wheel_load]",
        )
        .replace(
            'entity = "/results/til_wheel_tire_forces_2"',
            'entity = "til_wheel_tire_forces"',
        )
        .replace('component = "normal_rear"', 'component = "normal_front"')
    )
    path = write_config(tmp_path, content)

    assert main(["validate-config", str(path)]) == 2
    assert "duplicate HDF5 cell" in capsys.readouterr().err


def test_cli_notes_missing_simulation_hdf5(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = write_config(tmp_path, VALID_TOML)

    assert main(["validate-config", str(path)]) == 0
    assert "skipped signal cross-check" in capsys.readouterr().out
