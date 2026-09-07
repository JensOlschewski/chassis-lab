"""Read Lommel ASCII channel-data (ACD) measurement files."""

from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
from numpy.typing import NDArray

REQUIRED_METADATA = {"Description", "Offset", "Name", "Units"}


class AcdError(Exception):
    """Base class for ACD reader failures."""


class AcdFormatError(AcdError, ValueError):
    """Raised when an ACD file is malformed."""


class AcdSignalNotFoundError(AcdError, LookupError):
    """Raised when an ACD channel does not exist."""


class AcdSignalUnitError(AcdError, ValueError):
    """Raised when an ACD channel has an unexpected unit."""


@dataclass(frozen=True)
class AcdSignalInfo:
    name: str
    description: str
    unit: str
    offset: float
    column: int

    def as_dict(self) -> dict[str, str | float | int]:
        return {
            "name": self.name,
            "description": self.description,
            "unit": self.unit,
            "offset": self.offset,
            "column": self.column,
        }


@dataclass(frozen=True)
class AcdTestInfo:
    name: str
    description: str
    data_type: str
    unit: str
    value: str

    def as_dict(self) -> dict[str, str]:
        return {
            "name": self.name,
            "description": self.description,
            "data_type": self.data_type,
            "unit": self.unit,
            "value": self.value,
        }


@dataclass(frozen=True)
class AcdSignal:
    info: AcdSignalInfo
    values: NDArray[np.float64]

    @property
    def name(self) -> str:
        return self.info.name

    @property
    def unit(self) -> str:
        return self.info.unit

    @property
    def offset(self) -> float:
        return self.info.offset


@dataclass(frozen=True)
class AcdSummary:
    path: Path
    encoding: str
    dialect: str
    channel_count: int
    sample_count: int
    data_start_line: int

    def as_dict(self) -> dict[str, Any]:
        return {
            "path": str(self.path),
            "encoding": self.encoding,
            "dialect": self.dialect,
            "channel_count": self.channel_count,
            "sample_count": self.sample_count,
            "data_start_line": self.data_start_line,
        }


class AcdFile:
    """Metadata-indexed reader that loads requested ACD channels in one pass."""

    def __init__(self, path: Path | str) -> None:
        self.path = Path(path).expanduser().resolve()
        if not self.path.is_file():
            raise FileNotFoundError(f"ACD file does not exist: {self.path}")

        (
            self.encoding,
            self.dialect,
            self.data_start_line,
            self._signals,
            self._test_info,
        ) = _read_header(self.path)
        self._by_name = {signal.name: signal for signal in self._signals}
        self._test_info_by_name = {info.name: info for info in self._test_info}

    def test_info(
        self, name: str | None = None
    ) -> AcdTestInfo | tuple[AcdTestInfo, ...]:
        """Return embedded test metadata or one named metadata field."""
        if name is None:
            return self._test_info
        try:
            return self._test_info_by_name[name]
        except KeyError as error:
            raise KeyError(f"ACD test metadata not found: {name}") from error

    def signals(self, match: str | None = None) -> tuple[AcdSignalInfo, ...]:
        if match is None:
            return self._signals

        query = match.casefold()
        return tuple(
            signal
            for signal in self._signals
            if query
            in "\t".join(
                (
                    signal.name,
                    signal.description,
                    signal.unit,
                    str(signal.offset),
                )
            ).casefold()
        )

    def read_signal(
        self,
        name: str,
        *,
        expected_unit: str | None = None,
    ) -> AcdSignal:
        return self.read_signals([name], expected_units={name: expected_unit})[
            name
        ]

    def read_signals(
        self,
        names: list[str] | tuple[str, ...],
        *,
        expected_units: dict[str, str | None] | None = None,
    ) -> dict[str, AcdSignal]:
        requested = tuple(dict.fromkeys(names))
        missing = [name for name in requested if name not in self._by_name]
        if missing:
            raise AcdSignalNotFoundError(
                f"ACD signal(s) not found: {', '.join(missing)}"
            )

        expected_units = expected_units or {}
        for name in requested:
            expected = expected_units.get(name)
            actual = self._by_name[name].unit
            if expected is not None and actual != expected:
                raise AcdSignalUnitError(
                    f"signal '{name}' has unit '{actual}', "
                    f"expected '{expected}'"
                )

        columns = {self._by_name[name].column: name for name in requested}
        values = {name: [] for name in requested}
        self._read_data(columns, values)
        return {
            name: AcdSignal(
                info=self._by_name[name],
                values=np.asarray(values[name], dtype=np.float64),
            )
            for name in requested
        }

    def inspect(self) -> AcdSummary:
        sample_count = self._read_data({}, {})
        return AcdSummary(
            path=self.path,
            encoding=self.encoding,
            dialect=self.dialect,
            channel_count=len(self._signals),
            sample_count=sample_count,
            data_start_line=self.data_start_line,
        )

    def _read_data(
        self,
        columns: dict[int, str],
        output: dict[str, list[float]],
    ) -> int:
        try:
            return self._read_data_decoded(columns, output)
        except UnicodeDecodeError as error:
            raise AcdFormatError(
                f"cannot decode ACD data as {self.encoding}: {error}"
            ) from error

    def _read_data_decoded(
        self,
        columns: dict[int, str],
        output: dict[str, list[float]],
    ) -> int:
        sample_count = 0
        with self.path.open(encoding=self.encoding, newline="") as stream:
            for line_number, line in enumerate(stream, start=1):
                if line_number < self.data_start_line:
                    continue
                if not line.strip():
                    continue

                fields = [
                    field.strip() for field in line.rstrip("\r\n").split("\t")
                ]
                if len(fields) != len(self._signals):
                    raise AcdFormatError(
                        f"line {line_number} has {len(fields)} values; "
                        f"expected {len(self._signals)}"
                    )

                for column, field in enumerate(fields):
                    try:
                        value = float(field)
                    except ValueError as error:
                        name = self._signals[column].name
                        raise AcdFormatError(
                            f"line {line_number}, channel '{name}' is not "
                            "numeric: "
                            f"{field!r}"
                        ) from error
                    if column in columns:
                        output[columns[column]].append(value)
                sample_count += 1

        if sample_count == 0:
            raise AcdFormatError("ACD file contains no numerical samples")
        return sample_count


def inspect_acd(path: Path | str) -> AcdSummary:
    return AcdFile(path).inspect()


def _read_header(
    path: Path,
) -> tuple[
    str,
    str,
    int,
    tuple[AcdSignalInfo, ...],
    tuple[AcdTestInfo, ...],
]:
    errors: list[UnicodeDecodeError] = []
    for encoding in ("utf-8-sig", "cp1252"):
        try:
            return _parse_header(path, encoding)
        except UnicodeDecodeError as error:
            errors.append(error)
    raise AcdFormatError(
        f"ACD file is neither UTF-8 nor Windows-1252: {errors[-1]}"
    )


def _parse_header(
    path: Path,
    encoding: str,
) -> tuple[
    str,
    str,
    int,
    tuple[AcdSignalInfo, ...],
    tuple[AcdTestInfo, ...],
]:
    metadata: dict[str, list[str]] = {}
    test_info: list[AcdTestInfo] = []
    first_content = ""
    kpoly_line = 0
    data_start_line = 0

    with path.open(encoding=encoding, newline="") as stream:
        for line_number, line in enumerate(stream, start=1):
            if not first_content and line.strip():
                first_content = line.strip()
            fields = [
                field.strip() for field in line.rstrip("\r\n").split("\t")
            ]
            if len(fields) >= 7 and fields[0] == "TestInfo:":
                test_info.append(
                    AcdTestInfo(
                        name=fields[1],
                        description=fields[3],
                        data_type=fields[4],
                        unit=_normalize_unit(fields[5]),
                        value=_normalize_unit("\t".join(fields[6:])),
                    )
                )
            label = fields[-1] if fields else ""
            if label in REQUIRED_METADATA:
                metadata[label] = fields[:-2]
            if label == "KPoly":
                kpoly_line = line_number
                continue
            if (
                kpoly_line
                and "Name" in metadata
                and (
                    len(fields) != len(metadata["Name"]) + 2
                    or _is_float(fields[-1])
                )
            ):
                data_start_line = line_number
                break

    if kpoly_line and not data_start_line:
        raise AcdFormatError("ACD file contains no channel-width data row")

    if not kpoly_line:
        raise AcdFormatError("missing KPoly metadata row")

    missing = REQUIRED_METADATA.difference(metadata)
    if missing:
        raise AcdFormatError(
            f"missing required metadata row(s): {', '.join(sorted(missing))}"
        )

    names = metadata["Name"]
    channel_count = len(names)
    if not channel_count:
        raise AcdFormatError("Name metadata row contains no channels")
    for label in REQUIRED_METADATA - {"Name"}:
        if len(metadata[label]) != channel_count:
            raise AcdFormatError(
                f"{label} metadata contains {len(metadata[label])} channels; "
                f"expected {channel_count}"
            )

    blank_columns = [
        str(index + 1) for index, name in enumerate(names) if not name
    ]
    if blank_columns:
        raise AcdFormatError(
            f"blank channel name(s) in column(s): {', '.join(blank_columns)}"
        )
    duplicates = sorted({name for name in names if names.count(name) > 1})
    if duplicates:
        raise AcdFormatError(
            f"duplicate channel name(s): {', '.join(duplicates)}"
        )

    signals = tuple(
        AcdSignalInfo(
            name=name,
            description=metadata["Description"][column],
            unit=_normalize_unit(metadata["Units"][column]),
            offset=_parse_offset(metadata["Offset"][column], name),
            column=column,
        )
        for column, name in enumerate(names)
    )
    dialect = "ascii3" if first_content.startswith("FileInfo:") else "stripped"
    test_names = [info.name for info in test_info]
    if len(test_names) != len(set(test_names)):
        raise AcdFormatError("ACD test metadata names must be unique")
    return encoding, dialect, data_start_line, signals, tuple(test_info)


def _normalize_unit(unit: str) -> str:
    return "" if unit.casefold() == "nan" else unit


def _is_float(value: str) -> bool:
    try:
        float(value)
    except ValueError:
        return False
    return True


def _parse_offset(value: str, name: str) -> float:
    try:
        offset = float(value)
    except ValueError as error:
        raise AcdFormatError(
            f"offset for channel '{name}' is not numeric: {value!r}"
        ) from error
    if not np.isfinite(offset):
        raise AcdFormatError(f"offset for channel '{name}' must be finite")
    return offset
