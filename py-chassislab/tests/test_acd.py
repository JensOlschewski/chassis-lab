from pathlib import Path

import numpy as np
import pytest

from chassis_lab.acd import (
    AcdFile,
    AcdFormatError,
    AcdSignalNotFoundError,
    AcdSignalUnitError,
    inspect_acd,
)


def write_acd(
    path: Path,
    *,
    names: tuple[str, ...] = ("time", "lfwcz"),
    encoding: str = "utf-8",
    preamble: bool = False,
    padded: bool = False,
    post_kpoly_metadata: bool = False,
    data: tuple[tuple[str, ...], ...] = (("0", "1"), ("0.1", "2")),
) -> None:
    descriptions = ("Time", "Wheel travel")
    units = ("s", "mm")
    offsets = ("0", "10")

    def row(values: tuple[str, ...], kind: str, label: str) -> str:
        fields = (*values, kind, label)
        if padded:
            fields = tuple(f"{field:<20}" for field in fields)
        return "\t".join(fields)

    lines = []
    if preamble:
        lines.extend(
            (
                "FileInfo: ASCII3",
                "TestInfo:\tbrakeson\tbrakeson\tBrakes applied\tinteger\t\t1",
            )
        )
    lines.extend(
        (
            row(descriptions, "string", "Description"),
            row(offsets, "float", "Offset"),
            row(names, "string", "Name"),
            row(units, "string", "Units"),
            row(("NaN", "NaN"), "string", "KPoly"),
        )
    )
    if post_kpoly_metadata:
        lines.append(row(("NaN", "NaN"), "string", "ExtraMetadata"))
    lines.extend("\t".join(values) for values in data)
    path.write_bytes(("\n".join(lines) + "\n").encode(encoding))


def test_reads_selected_signals_and_metadata(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path)

    measurement = AcdFile(path)
    signals = measurement.read_signals(
        ("time", "lfwcz"), expected_units={"time": "s", "lfwcz": "mm"}
    )

    np.testing.assert_array_equal(signals["time"].values, [0.0, 0.1])
    np.testing.assert_array_equal(signals["lfwcz"].values, [1.0, 2.0])
    assert signals["lfwcz"].offset == 10.0
    assert signals["lfwcz"].info.description == "Wheel travel"


def test_detects_full_ascii3_and_cp1252(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(
        path,
        encoding="cp1252",
        preamble=True,
        padded=True,
        post_kpoly_metadata=True,
    )
    raw = path.read_bytes().replace(
        b"Wheel travel", "W\N{DEGREE SIGN}".encode("cp1252")
    )
    path.write_bytes(raw)

    summary = inspect_acd(path)

    assert summary.encoding == "cp1252"
    assert summary.dialect == "ascii3"
    assert summary.data_start_line == 9
    assert summary.sample_count == 2

    measurement = AcdFile(path)
    brakes = measurement.test_info("brakeson")
    assert brakes.name == "brakeson"
    assert brakes.description == "Brakes applied"
    assert brakes.data_type == "integer"
    assert brakes.unit == ""
    assert brakes.value == "1"


def test_preserves_non_finite_values(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path, data=(("0", "NaN"), ("1", "Inf"), ("2", "-Inf")))

    values = AcdFile(path).read_signal("lfwcz").values

    assert np.isnan(values[0])
    assert np.isposinf(values[1])
    assert np.isneginf(values[2])


def test_filters_catalog_and_normalizes_missing_unit(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path)
    text = path.read_text().replace(
        "s\tmm\tstring\tUnits", "NaN\tmm\tstring\tUnits"
    )
    path.write_text(text)

    matches = AcdFile(path).signals("wheel")

    assert [signal.name for signal in matches] == ["lfwcz"]
    assert AcdFile(path).signals()[0].unit == ""


def test_reports_missing_signal_and_wrong_unit(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path)
    measurement = AcdFile(path)

    with pytest.raises(AcdSignalNotFoundError, match="missing"):
        measurement.read_signal("missing")
    with pytest.raises(AcdSignalUnitError, match="expected 'm'"):
        measurement.read_signal("lfwcz", expected_unit="m")


@pytest.mark.parametrize("names", [("time", "time"), ("time", "")])
def test_rejects_invalid_channel_names(
    tmp_path: Path, names: tuple[str, ...]
) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path, names=names)

    with pytest.raises(AcdFormatError):
        AcdFile(path)


def test_rejects_missing_kpoly(tmp_path: Path) -> None:
    path = tmp_path / "measurement.acd"
    write_acd(path)
    path.write_text(path.read_text().replace("KPoly", "Other"))

    with pytest.raises(AcdFormatError, match="missing KPoly"):
        AcdFile(path)


def test_rejects_wrong_data_width_and_malformed_number(tmp_path: Path) -> None:
    short = tmp_path / "short.acd"
    write_acd(short, data=(("0",),))
    with pytest.raises(AcdFormatError, match="has 1 values; expected 2"):
        inspect_acd(short)

    malformed = tmp_path / "malformed.acd"
    write_acd(malformed, data=(("bad", "1"),))
    with pytest.raises(AcdFormatError, match="channel 'time' is not numeric"):
        inspect_acd(malformed)
