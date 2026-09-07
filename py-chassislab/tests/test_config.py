from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.cli import main
from chassis_lab.config import (
    CaseConfig,
    ConfigError,
    SignalRef,
    ValidationConfig,
    ValidationTolerances,
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
entity = "til_wheel_tire_forces"
component = "normal_rear"
unit = "newton"
"""

VALIDATION_TOML = (
    VALID_TOML
    + """
[validation]
reference = "../measures/reference.json"

[validation.tolerances.absolute]
total_mass_kg = 20.0
wheelbase_mm = 5.0

[validation.tolerances.relative_percent]
total_mass_kg = 1.0
inertia_xx_kg_mm2 = 5.0
"""
)

UNIFIED_TOML = """\
[case]
name = "varb-lommel-2up"
variant = "VARB"
loadcase = "2UP"

[physics]
gravity = 9.80665

[svc.inputs]
simulation_hdf5 = "svc.h5"
measurement_reference = "reference.json"

[svc.validation.tolerances.absolute]
total_mass_kg = 20.0

[svc.signals.signal]
entity = "svc"
component = "value"
unit = "mm"

[vertical.inputs]
measurement_acd = "bounce.acd"
simulation_hdf5 = "vertical.h5"

[vertical.setup]
code = "b1e0f1"
brakes_applied = true
engine_running = false
front_arb_connected = true
window_minimum_mm = -75
window_maximum_mm = 75
window_step_mm = 1

[vertical.signals.front_left_travel]
entity = "wheel_center_rise_front_wrt_body"
component = "left_z"
unit = "mm"
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
                entity="til_wheel_tire_forces",
                component="normal_rear",
                unit="newton",
            ),
        ),
    )
    assert config.validation is None


def test_loads_unified_svc_and_vertical_config(tmp_path: Path) -> None:
    config = load_config(write_config(tmp_path, UNIFIED_TOML))

    assert config.name == "varb-lommel-2up"
    assert config.domain == "multi"
    assert config.simulation_hdf5 == (tmp_path / "svc.h5").resolve()
    assert config.validation is not None
    assert (
        config.validation.reference == (tmp_path / "reference.json").resolve()
    )
    assert len(config.signals) == 1
    assert config.vertical is not None
    assert (
        config.vertical.measurement_acd == (tmp_path / "bounce.acd").resolve()
    )
    assert (
        config.vertical.simulation_hdf5 == (tmp_path / "vertical.h5").resolve()
    )
    assert config.vertical.setup.code == "b1e0f1"
    assert config.vertical.setup.rear_arb_connected is None
    assert config.vertical.window.minimum_mm == -75.0
    assert config.vertical.signal("front_left_travel").component == "left_z"


def test_loads_unified_signal_maps_from_domain_inputs(tmp_path: Path) -> None:
    signal_map = tmp_path / "signals.toml"
    signal_map.write_text(
        "[signals.signal]\n"
        'entity = "entity"\ncomponent = "value"\nunit = "mm"\n'
    )
    content = UNIFIED_TOML.replace(
        'simulation_hdf5 = "svc.h5"',
        'signal_map = "signals.toml"\nsimulation_hdf5 = "svc.h5"',
    ).replace(
        'simulation_hdf5 = "vertical.h5"',
        'signal_map = "signals.toml"\nsimulation_hdf5 = "vertical.h5"',
    )
    content = content.replace(
        "[svc.signals.signal]\n"
        'entity = "svc"\ncomponent = "value"\nunit = "mm"\n\n',
        "",
    ).replace(
        "[vertical.signals.front_left_travel]\n"
        'entity = "wheel_center_rise_front_wrt_body"\n'
        'component = "left_z"\n'
        'unit = "mm"\n',
        "",
    )

    config = load_config(write_config(tmp_path, content))

    assert config.svc_signal_map == signal_map.resolve()
    assert config.vertical is not None
    assert config.vertical.signal_map == signal_map.resolve()


def test_rejects_unified_signal_map_outside_inputs(tmp_path: Path) -> None:
    content = UNIFIED_TOML.replace(
        "[svc.inputs]", '[svc]\nsignal_map = "signals.toml"\n\n[svc.inputs]'
    )

    with pytest.raises(ConfigError, match=r"unknown \[svc\] key"):
        load_config(write_config(tmp_path, content))


def test_rejects_reference_inside_unified_validation(tmp_path: Path) -> None:
    content = UNIFIED_TOML.replace(
        "[svc.validation.tolerances.absolute]",
        '[svc.validation]\nreference = "other.json"\n\n'
        "[svc.validation.tolerances.absolute]",
    )

    with pytest.raises(ConfigError, match=r"unknown \[svc.validation\] key"):
        load_config(write_config(tmp_path, content))


def test_requires_unified_measurement_reference_for_validation(
    tmp_path: Path,
) -> None:
    content = UNIFIED_TOML.replace(
        'measurement_reference = "reference.json"\n', ""
    )

    with pytest.raises(ConfigError, match="measurement_reference"):
        load_config(write_config(tmp_path, content))


def test_loads_validation_config_and_resolves_reference(tmp_path: Path) -> None:
    config = load_config(write_config(tmp_path, VALIDATION_TOML))

    assert config.validation == ValidationConfig(
        reference=(tmp_path / "../measures/reference.json").resolve(),
        tolerances=ValidationTolerances(
            absolute=(("total_mass_kg", 20.0), ("wheelbase_mm", 5.0)),
            relative_percent=(
                ("total_mass_kg", 1.0),
                ("inertia_xx_kg_mm2", 5.0),
            ),
        ),
    )
    assert config.validation.tolerances.absolute_for("total_mass_kg") == 20.0
    assert (
        config.validation.tolerances.relative_percent_for("total_mass_kg")
        == 1.0
    )
    assert config.validation.tolerances.absolute_for("missing") is None


def test_validation_allows_only_absolute_tolerances(tmp_path: Path) -> None:
    content = VALIDATION_TOML.split(
        "[validation.tolerances.relative_percent]", 1
    )[0]

    config = load_config(write_config(tmp_path, content))

    assert config.validation is not None
    assert config.validation.tolerances.relative_percent == ()


@pytest.mark.parametrize(
    ("replacement", "message"),
    [
        ("", "missing key"),
        ('reference = ""', "reference must be a non-empty string"),
    ],
)
def test_rejects_missing_or_empty_validation_reference(
    tmp_path: Path, replacement: str, message: str
) -> None:
    content = VALIDATION_TOML.replace(
        'reference = "../measures/reference.json"', replacement
    )

    with pytest.raises(ConfigError, match=message):
        load_config(write_config(tmp_path, content))


def test_rejects_validation_without_tolerances(tmp_path: Path) -> None:
    content = (
        VALID_TOML
        + """
[validation]
reference = "../measures/reference.json"
[validation.tolerances]
"""
    )

    with pytest.raises(ConfigError, match="at least one tolerance"):
        load_config(write_config(tmp_path, content))


@pytest.mark.parametrize("value", ["0", "-1", "nan", "true", '"wide"'])
def test_rejects_invalid_tolerance_values(tmp_path: Path, value: str) -> None:
    content = VALIDATION_TOML.replace(
        "total_mass_kg = 20.0", f"total_mass_kg = {value}", 1
    )

    with pytest.raises(ConfigError, match="must be"):
        load_config(write_config(tmp_path, content))


def test_rejects_unknown_validation_key(tmp_path: Path) -> None:
    content = VALIDATION_TOML.replace(
        "[validation]\n", "[validation]\nunknown = true\n"
    )

    with pytest.raises(ConfigError, match=r"unknown \[validation\] key"):
        load_config(write_config(tmp_path, content))


def test_preserves_absolute_simulation_path(tmp_path: Path) -> None:
    absolute = tmp_path / "other" / "result.h5"
    config = load_config(
        write_config(
            tmp_path,
            VALID_TOML.replace("sub/result.h5", str(absolute)),
        )
    )

    assert config.simulation_hdf5 == absolute.resolve()


def test_loads_reusable_signal_map(
    tmp_path: Path,
) -> None:
    content = VALID_TOML.split("[signals]", 1)[0]
    signal_map = tmp_path / "signals.toml"
    signal_map.write_text(
        """\
[signals.rear_left_wheel_load]
entity = "til_wheel_tire_forces"
component = "normal_rear"
unit = "newton"
"""
    )
    config = load_config(
        write_config(tmp_path, f'signal_map = "signals.toml"\n{content}')
    )

    assert config.svc_signal_map == signal_map.resolve()
    assert len(config.signals) == 1
    assert config.signal("rear_left_wheel_load").entity == (
        "til_wheel_tire_forces"
    )


def test_rejects_missing_or_conflicting_signal_map(tmp_path: Path) -> None:
    without_inline = VALID_TOML.split("[signals]", 1)[0]
    with_missing_map = f'signal_map = "missing.toml"\n{without_inline}'
    with_both = f'signal_map = "signals.toml"\n{VALID_TOML}'
    (tmp_path / "signals.toml").write_text("[signals]\n")

    with pytest.raises(FileNotFoundError, match="signal map does not exist"):
        load_config(
            write_config(tmp_path, with_missing_map, "case-missing.toml")
        )
    with pytest.raises(ConfigError, match="cannot define both"):
        load_config(write_config(tmp_path, with_both, "both.toml"))


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
    with pytest.raises(ConfigError, match="signal_map or inline signals"):
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
            'entity = "til_wheel_tire_forces"',
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
