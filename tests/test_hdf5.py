from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.cli import main
from chassis_lab.hdf5 import (
    AdamsHdf5,
    AmbiguousSignalError,
    Hdf5FormatError,
    SignalNotFoundError,
    SignalUnitError,
    inspect_hdf5,
)


def add_entity(
    results: h5py.Group,
    group_name: str,
    *,
    source_path: str,
    original_name: str,
    components: list[str],
    units: list[str],
    adams_ids: list[int],
    values: np.ndarray,
) -> None:
    entity = results.create_group(group_name)
    entity.attrs["source_path"] = source_path
    entity.attrs["original_name"] = original_name
    entity.create_dataset("values", data=values)
    string_type = h5py.string_dtype(encoding="utf-8")
    entity.create_dataset(
        "component_names", data=np.asarray(components, dtype=string_type)
    )
    entity.create_dataset("units", data=np.asarray(units, dtype=string_type))
    entity.create_dataset(
        "adams_ids", data=np.asarray(adams_ids, dtype=np.uint64)
    )


def write_hdf5_fixture(
    path: Path,
    *,
    format_version: int = 3,
    duplicate_x: bool = False,
) -> None:
    with h5py.File(path, "w") as h5:
        h5.attrs["format_version"] = format_version
        h5.attrs["sample_count"] = 3
        h5.attrs["signal_count"] = 4 if duplicate_x else 3
        h5.attrs["values_per_sample"] = 4 if duplicate_x else 3
        h5.attrs["source_file"] = "fixture.res"
        results = h5.create_group("results")
        add_entity(
            results,
            "wheel",
            source_path="vehicle.wheel",
            original_name="wheel_xform",
            components=["X", "Y"],
            units=["mm", "mm"],
            adams_ids=[1, 2],
            values=np.asarray([[1.0, 10.0], [2.0, 20.0], [3.0, 30.0]]),
        )
        add_entity(
            results,
            "wheel_2",
            source_path="vehicle.wheel",
            original_name="wheel_xform",
            components=["Z", "X"] if duplicate_x else ["Z"],
            units=["mm", "mm"] if duplicate_x else ["mm"],
            adams_ids=[3, 4] if duplicate_x else [3],
            values=(
                np.asarray([[100.0, 1000.0], [200.0, 2000.0], [300.0, 3000.0]])
                if duplicate_x
                else np.asarray([[100.0], [200.0], [300.0]])
            ),
        )


def test_inspects_format_v3_file(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    summary = inspect_hdf5(path)

    assert summary.format_version == 3
    assert summary.sample_count == 3
    assert summary.signal_count == 3
    assert summary.entity_count == 2
    assert summary.source_file == "fixture.res"


def test_reads_signal_by_source_entity_and_component(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with AdamsHdf5(path) as result:
        signal = result.read_signal("vehicle.wheel", "Z", expected_unit="mm")

    np.testing.assert_array_equal(signal.values, [100.0, 200.0, 300.0])
    assert signal.entity == "vehicle.wheel"
    assert signal.component == "Z"
    assert signal.unit == "mm"
    assert signal.adams_id == 3
    assert signal.info.group_name == "wheel_2"
    assert signal.info.original_name == "wheel_xform"


def test_reads_signal_by_original_entity_name(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with AdamsHdf5(path) as result:
        signal = result.read_signal("wheel_xform", "Y")

    np.testing.assert_array_equal(signal.values, [10.0, 20.0, 30.0])


def test_exact_group_name_disambiguates_aliases(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path, duplicate_x=True)

    with AdamsHdf5(path) as result:
        signal = result.read_signal("/results/wheel_2", "X")

    np.testing.assert_array_equal(signal.values, [1000.0, 2000.0, 3000.0])


def test_rejects_ambiguous_entity_component_pair(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path, duplicate_x=True)

    with (
        AdamsHdf5(path) as result,
        pytest.raises(AmbiguousSignalError, match=r"wheel, wheel_2"),
    ):
        result.read_signal("vehicle.wheel", "X")


def test_missing_component_lists_available_components(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with (
        AdamsHdf5(path) as result,
        pytest.raises(
            SignalNotFoundError, match=r"available components: X, Y, Z"
        ),
    ):
        result.read_signal("vehicle.wheel", "VX")


def test_reports_missing_entity_component_pair(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with (
        AdamsHdf5(path) as result,
        pytest.raises(
            SignalNotFoundError, match=r"signal 'missing/X' was not found$"
        ),
    ):
        result.read_signal("missing", "X")


def test_validates_expected_source_unit(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with (
        AdamsHdf5(path) as result,
        pytest.raises(SignalUnitError, match="expected 'm'"),
    ):
        result.read_signal("vehicle.wheel", "Y", expected_unit="m")


def test_filters_signal_catalog_without_loading_values(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with AdamsHdf5(path) as result:
        matches = result.signals("wheel_2")

    assert [match.component for match in matches] == ["Z"]


def test_requires_context_manager(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    with pytest.raises(RuntimeError, match="must be used as a context manager"):
        AdamsHdf5(path).signals()


def test_rejects_reentering_open_reader(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)
    result = AdamsHdf5(path)

    with result, pytest.raises(RuntimeError, match="reader is already open"):
        result.__enter__()


def test_lists_filtered_signals_from_cli(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    exit_code = main(["list-h5-signals", str(path), "--match", "wheel_2"])

    assert exit_code == 0
    assert capsys.readouterr().out.splitlines() == [
        "ENTITY\tCOMPONENT\tUNIT\tADAMS_ID\tGROUP",
        "vehicle.wheel\tZ\tmm\t3\twheel_2",
    ]


def test_cli_writes_errors_to_stderr(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    exit_code = main(["inspect-h5", str(tmp_path / "missing.h5"), "--json"])

    captured = capsys.readouterr()
    assert exit_code == 2
    assert captured.out == ""
    assert "error: HDF5 file does not exist" in captured.err


def test_rejects_unknown_format_version(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path, format_version=4)

    with pytest.raises(
        Hdf5FormatError, match="unsupported HDF5 format version 4"
    ):
        inspect_hdf5(path)


def test_rejects_incorrect_values_shape(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)
    with h5py.File(path, "r+") as h5:
        del h5["results/wheel/values"]
        h5["results/wheel"].create_dataset("values", data=np.zeros((2, 2)))

    with pytest.raises(Hdf5FormatError, match="has 2 samples; expected 3"):
        inspect_hdf5(path)


def test_rejects_required_group_instead_of_dataset(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)
    with h5py.File(path, "r+") as h5:
        del h5["results/wheel/units"]
        h5["results/wheel"].create_group("units")

    with pytest.raises(
        Hdf5FormatError, match="/results/wheel/units is not a dataset"
    ):
        inspect_hdf5(path)


def test_rejects_non_float64_values(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)
    with h5py.File(path, "r+") as h5:
        values = h5["results/wheel/values"][:].astype(np.float32)
        del h5["results/wheel/values"]
        h5["results/wheel"].create_dataset("values", data=values)

    with pytest.raises(
        Hdf5FormatError, match="values must have dtype float64, found float32"
    ):
        inspect_hdf5(path)
