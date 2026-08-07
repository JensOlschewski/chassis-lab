from pathlib import Path

import h5py
import numpy as np
import pytest

from knc_validation.hdf5 import Hdf5FormatError, inspect_hdf5


def write_hdf5_fixture(path: Path, *, format_version: int = 3) -> None:
    with h5py.File(path, "w") as h5:
        h5.attrs["format_version"] = format_version
        h5.attrs["sample_count"] = 3
        h5.attrs["signal_count"] = 2
        h5.attrs["values_per_sample"] = 2
        h5.attrs["source_file"] = "fixture.res"
        entity = h5.create_group("results/wheel")
        entity.create_dataset("values", data=np.zeros((3, 2)))
        entity.create_dataset("component_names", data=[b"X", b"Y"])
        entity.create_dataset("units", data=[b"mm", b"mm"])
        entity.create_dataset("adams_ids", data=[1, 2])


def test_inspects_format_v3_file(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path)

    summary = inspect_hdf5(path)

    assert summary.format_version == 3
    assert summary.sample_count == 3
    assert summary.signal_count == 2
    assert summary.entity_count == 1
    assert summary.source_file == "fixture.res"


def test_rejects_unknown_format_version(tmp_path: Path) -> None:
    path = tmp_path / "result.h5"
    write_hdf5_fixture(path, format_version=4)

    with pytest.raises(Hdf5FormatError, match="unsupported HDF5 format version 4"):
        inspect_hdf5(path)
