"""Read and validate the HDF5 contract produced by adams-res-cli."""

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import h5py

SUPPORTED_FORMAT_VERSION = 3
REQUIRED_ENTITY_DATASETS = {"values", "component_names", "units", "adams_ids"}


class Hdf5FormatError(ValueError):
    """Raised when an Adams HDF5 file violates the supported contract."""


@dataclass(frozen=True)
class Hdf5Summary:
    path: Path
    format_version: int
    sample_count: int
    signal_count: int
    entity_count: int
    source_file: str

    def as_dict(self) -> dict[str, Any]:
        return {
            "path": str(self.path),
            "format_version": self.format_version,
            "sample_count": self.sample_count,
            "signal_count": self.signal_count,
            "entity_count": self.entity_count,
            "source_file": self.source_file,
        }


def inspect_hdf5(path: Path) -> Hdf5Summary:
    """Validate an Adams HDF5 file and return its high-level metadata."""
    path = path.expanduser().resolve()
    if not path.is_file():
        raise FileNotFoundError(f"HDF5 file does not exist: {path}")

    with h5py.File(path, "r") as h5:
        format_version = _required_int_attribute(h5, "format_version")
        if format_version != SUPPORTED_FORMAT_VERSION:
            raise Hdf5FormatError(
                f"unsupported HDF5 format version {format_version}; "
                f"expected {SUPPORTED_FORMAT_VERSION}"
            )

        sample_count = _required_int_attribute(h5, "sample_count")
        declared_signal_count = _required_int_attribute(h5, "signal_count")
        source_file = _string_attribute(h5, "source_file")

        if "results" not in h5 or not isinstance(h5["results"], h5py.Group):
            raise Hdf5FormatError("missing HDF5 group: /results")

        results = h5["results"]
        signal_count = 0
        for group_name, entity in results.items():
            if not isinstance(entity, h5py.Group):
                raise Hdf5FormatError(f"/results/{group_name} is not a group")
            signal_count += _validate_entity(group_name, entity, sample_count)

        if signal_count != declared_signal_count:
            raise Hdf5FormatError(
                f"signal_count is {declared_signal_count}, but entity datasets contain "
                f"{signal_count} signals"
            )

        return Hdf5Summary(
            path=path,
            format_version=format_version,
            sample_count=sample_count,
            signal_count=signal_count,
            entity_count=len(results),
            source_file=source_file,
        )


def _validate_entity(group_name: str, entity: h5py.Group, sample_count: int) -> int:
    missing = REQUIRED_ENTITY_DATASETS.difference(entity.keys())
    if missing:
        names = ", ".join(sorted(missing))
        raise Hdf5FormatError(f"/results/{group_name} is missing datasets: {names}")

    values = entity["values"]
    component_names = entity["component_names"]
    units = entity["units"]
    adams_ids = entity["adams_ids"]

    if values.ndim != 2:
        raise Hdf5FormatError(f"/results/{group_name}/values must be two-dimensional")

    entity_sample_count, component_count = values.shape
    if entity_sample_count != sample_count:
        raise Hdf5FormatError(
            f"/results/{group_name}/values has {entity_sample_count} samples; "
            f"expected {sample_count}"
        )

    for dataset_name, dataset in (
        ("component_names", component_names),
        ("units", units),
        ("adams_ids", adams_ids),
    ):
        if dataset.ndim != 1 or dataset.shape[0] != component_count:
            raise Hdf5FormatError(
                f"/results/{group_name}/{dataset_name} must contain {component_count} entries"
            )

    return component_count


def _required_int_attribute(location: h5py.Group | h5py.File, name: str) -> int:
    if name not in location.attrs:
        raise Hdf5FormatError(f"missing HDF5 attribute: {name}")
    return int(location.attrs[name])


def _string_attribute(location: h5py.Group | h5py.File, name: str) -> str:
    if name not in location.attrs:
        raise Hdf5FormatError(f"missing HDF5 attribute: {name}")
    value = location.attrs[name]
    return value.decode("utf-8") if isinstance(value, bytes) else str(value)
