"""Read and validate the HDF5 contract produced by adams-res-cli."""

from dataclasses import dataclass
from pathlib import Path
from types import TracebackType
from typing import Any, Self

import h5py
import numpy as np
from numpy.typing import NDArray

SUPPORTED_FORMAT_VERSION = 3
REQUIRED_ENTITY_DATASETS = {"values", "component_names", "units", "adams_ids"}


class Hdf5FormatError(ValueError):
    """Raised when an Adams HDF5 file violates the supported contract."""


class SignalLookupError(LookupError):
    """Base class for Adams signal lookup failures."""


class SignalNotFoundError(SignalLookupError):
    """Raised when an entity/component pair does not exist."""


class AmbiguousSignalError(SignalLookupError):
    """Raised when an entity/component pair identifies multiple signals."""


class SignalUnitError(ValueError):
    """Raised when a signal does not have its expected source unit."""


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


@dataclass(frozen=True)
class SignalInfo:
    """Metadata identifying a component column without loading its values."""

    group_name: str
    original_name: str
    source_path: str
    component: str
    unit: str
    adams_id: int
    column: int

    @property
    def entity(self) -> str:
        return self.source_path or self.original_name or self.group_name

    def as_dict(self) -> dict[str, str | int]:
        return {
            "entity": self.entity,
            "component": self.component,
            "unit": self.unit,
            "adams_id": self.adams_id,
            "group_name": self.group_name,
            "original_name": self.original_name,
            "source_path": self.source_path,
        }


@dataclass(frozen=True)
class Signal:
    """One selected Adams signal and its source metadata."""

    info: SignalInfo
    values: NDArray[np.float64]

    @property
    def entity(self) -> str:
        return self.info.entity

    @property
    def component(self) -> str:
        return self.info.component

    @property
    def unit(self) -> str:
        return self.info.unit

    @property
    def adams_id(self) -> int:
        return self.info.adams_id


class AdamsHdf5:
    """Lazy, context-managed reader for an adams-res HDF5 file."""

    def __init__(self, path: Path | str) -> None:
        self.path = Path(path).expanduser().resolve()
        self._file: h5py.File | None = None
        self._summary: Hdf5Summary | None = None
        self._signals: tuple[SignalInfo, ...] = ()

    def __enter__(self) -> Self:
        if self._file is not None:
            raise RuntimeError("AdamsHdf5 reader is already open")
        if not self.path.is_file():
            raise FileNotFoundError(f"HDF5 file does not exist: {self.path}")

        self._file = h5py.File(self.path, "r")
        try:
            self._summary, self._signals = _validate_file(self.path, self._file)
        except Exception:
            self._file.close()
            self._file = None
            raise
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_value: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        if self._file is not None:
            self._file.close()
            self._file = None

    @property
    def summary(self) -> Hdf5Summary:
        self._require_open()
        assert self._summary is not None
        return self._summary

    def signals(self, match: str | None = None) -> tuple[SignalInfo, ...]:
        """Return metadata, optionally filtered by case-insensitive text."""
        self._require_open()
        if match is None:
            return self._signals

        query = match.casefold()
        return tuple(
            info
            for info in self._signals
            if query
            in "\t".join(
                (
                    info.group_name,
                    info.original_name,
                    info.source_path,
                    info.component,
                    info.unit,
                    str(info.adams_id),
                )
            ).casefold()
        )

    def read_signal(
        self,
        entity: str,
        component: str,
        *,
        expected_unit: str | None = None,
    ) -> Signal:
        """Read a component selected by its entity and component names."""
        h5 = self._require_open()
        info = self._resolve_signal(entity, component)

        if expected_unit is not None and info.unit != expected_unit:
            raise SignalUnitError(
                f"signal '{entity}/{component}' has unit '{info.unit}', "
                f"expected '{expected_unit}'"
            )

        values = np.asarray(
            h5[f"results/{info.group_name}/values"][:, info.column],
            dtype=np.float64,
        )
        return Signal(info=info, values=values)

    def _resolve_signal(self, entity: str, component: str) -> SignalInfo:
        component_matches = [
            info for info in self._signals if info.component == component
        ]

        if entity.startswith("/results/"):
            group_name = entity.removeprefix("/results/")
            matches = [
                info
                for info in component_matches
                if info.group_name == group_name
            ]
        else:
            matches = [
                info
                for info in component_matches
                if entity in (info.source_path, info.original_name)
                or (
                    not info.source_path
                    and not info.original_name
                    and entity == info.group_name
                )
            ]

        if not matches:
            entity_matches = [
                info
                for info in self._signals
                if entity
                in (
                    f"/results/{info.group_name}",
                    info.source_path,
                    info.original_name,
                )
            ]
            if entity_matches:
                available = ", ".join(
                    sorted({info.component for info in entity_matches})
                )
                detail = f"; available components: {available}"
            else:
                detail = ""
            raise SignalNotFoundError(
                f"signal '{entity}/{component}' was not found{detail}"
            )

        if len(matches) > 1:
            groups = ", ".join(sorted(info.group_name for info in matches))
            raise AmbiguousSignalError(
                f"signal '{entity}/{component}' is ambiguous across groups: "
                f"{groups}"
            )

        return matches[0]

    def _require_open(self) -> h5py.File:
        if self._file is None:
            raise RuntimeError("AdamsHdf5 must be used as a context manager")
        return self._file


def inspect_hdf5(path: Path | str) -> Hdf5Summary:
    """Validate an Adams HDF5 file and return its high-level metadata."""
    with AdamsHdf5(path) as result:
        return result.summary


def _validate_file(
    path: Path, h5: h5py.File
) -> tuple[Hdf5Summary, tuple[SignalInfo, ...]]:
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
    signals: list[SignalInfo] = []
    for group_name, entity in results.items():
        if not isinstance(entity, h5py.Group):
            raise Hdf5FormatError(f"/results/{group_name} is not a group")
        signals.extend(_validate_entity(group_name, entity, sample_count))

    if len(signals) != declared_signal_count:
        raise Hdf5FormatError(
            f"signal_count is {declared_signal_count}, but entity datasets "
            "contain "
            f"{len(signals)} signals"
        )

    summary = Hdf5Summary(
        path=path,
        format_version=format_version,
        sample_count=sample_count,
        signal_count=len(signals),
        entity_count=len(results),
        source_file=source_file,
    )
    return summary, tuple(signals)


def _validate_entity(
    group_name: str, entity: h5py.Group, sample_count: int
) -> list[SignalInfo]:
    missing = REQUIRED_ENTITY_DATASETS.difference(entity.keys())
    if missing:
        names = ", ".join(sorted(missing))
        raise Hdf5FormatError(
            f"/results/{group_name} is missing datasets: {names}"
        )

    for dataset_name in REQUIRED_ENTITY_DATASETS:
        if not isinstance(entity[dataset_name], h5py.Dataset):
            raise Hdf5FormatError(
                f"/results/{group_name}/{dataset_name} is not a dataset"
            )

    values = entity["values"]
    component_names = entity["component_names"]
    units = entity["units"]
    adams_ids = entity["adams_ids"]

    if values.dtype != np.dtype(np.float64):
        raise Hdf5FormatError(
            f"/results/{group_name}/values must have dtype float64, "
            f"found {values.dtype}"
        )
    if adams_ids.dtype != np.dtype(np.uint64):
        raise Hdf5FormatError(
            f"/results/{group_name}/adams_ids must have dtype uint64, "
            f"found {adams_ids.dtype}"
        )
    for dataset_name, dataset in (
        ("component_names", component_names),
        ("units", units),
    ):
        if h5py.check_string_dtype(dataset.dtype) is None:
            raise Hdf5FormatError(
                f"/results/{group_name}/{dataset_name} must contain strings, "
                f"found {dataset.dtype}"
            )

    if values.ndim != 2:
        raise Hdf5FormatError(
            f"/results/{group_name}/values must be two-dimensional"
        )

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
                f"/results/{group_name}/{dataset_name} must contain "
                f"{component_count} entries"
            )

    try:
        names = component_names.asstr()[:]
        source_units = units.asstr()[:]
        ids = adams_ids[:]
    except (OSError, TypeError, UnicodeError) as error:
        raise Hdf5FormatError(
            f"invalid metadata in /results/{group_name}: {error}"
        ) from error

    original_name = _optional_string_attribute(entity, "original_name")
    source_path = _optional_string_attribute(entity, "source_path")
    return [
        SignalInfo(
            group_name=group_name,
            original_name=original_name,
            source_path=source_path,
            component=str(name),
            unit=str(unit),
            adams_id=int(adams_id),
            column=column,
        )
        for column, (name, unit, adams_id) in enumerate(
            zip(names, source_units, ids, strict=True)
        )
    ]


def _required_int_attribute(location: h5py.Group | h5py.File, name: str) -> int:
    if name not in location.attrs:
        raise Hdf5FormatError(f"missing HDF5 attribute: {name}")
    return int(location.attrs[name])


def _string_attribute(location: h5py.Group | h5py.File, name: str) -> str:
    if name not in location.attrs:
        raise Hdf5FormatError(f"missing HDF5 attribute: {name}")
    return _attribute_as_string(location.attrs[name])


def _optional_string_attribute(location: h5py.Group, name: str) -> str:
    if name not in location.attrs:
        return ""
    return _attribute_as_string(location.attrs[name])


def _attribute_as_string(value: Any) -> str:
    return value.decode("utf-8") if isinstance(value, bytes) else str(value)
