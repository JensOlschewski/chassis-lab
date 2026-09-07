from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.cli import main
from chassis_lab.config import (
    CaseConfig,
    SignalRef,
    VerticalConfig,
    VerticalSetup,
    VerticalWindow,
)
from chassis_lab.conventions import Corner
from chassis_lab.vertical import VerticalEvaluationError, evaluate_vertical
from chassis_lab.vertical_reporting import (
    build_vertical_report,
    write_vertical_outputs,
)

PREFIXES = {
    Corner.FRONT_LEFT: "lf",
    Corner.FRONT_RIGHT: "rf",
    Corner.REAR_LEFT: "lr",
    Corner.REAR_RIGHT: "rr",
}


def write_vertical_acd(path: Path, *, engine_on: bool = False) -> None:
    names = []
    descriptions = []
    units = []
    offsets = []
    baselines = {}
    for index, (corner, prefix) in enumerate(PREFIXES.items()):
        base = 100.0 * (index + 1)
        baselines[corner] = (base + 10.0, base + 20.0, base + 30.0)
        names.extend(
            (f"{prefix}slope", f"{prefix}wcx", f"{prefix}wcy", f"{prefix}wcz")
        )
        descriptions.extend(("Cycle", "WC X", "WC Y", "WC Z"))
        units.extend(("NaN", "mm", "mm", "mm"))
        offsets.extend(("0", *(str(value) for value in baselines[corner])))

    def metadata(values: list[str], kind: str, label: str) -> str:
        return "\t".join((*values, kind, label))

    lines = [
        "FileInfo: ASCII3",
        "TestInfo:\ttesttype\ttesttype\tTest type\tstring\t\tvert",
        "TestInfo:\tbrakeson\tbrakeson\tBrakes applied\tinteger\t\t1",
        "TestInfo:\tengineon\tengineon\tEngine running\tinteger\t\t"
        + str(int(engine_on)),
        "TestInfo:\tfarbon\tfarbon\tFront ARB\tinteger\t\t1",
        "TestInfo:\trarbon\trarbon\tRear ARB\tinteger\t\tNaN",
        metadata(descriptions, "string", "Description"),
        metadata(offsets, "float", "Offset"),
        metadata(names, "string", "Name"),
        metadata(units, "string", "Units"),
        metadata(["NaN"] * len(names), "string", "KPoly"),
    ]
    for cycle, travel_values in (
        (2.0, (-2.0, -1.0, 0.0, 1.0, 2.0)),
        (3.0, (2.0, 1.0, 0.0, -1.0, -2.0)),
    ):
        for travel in travel_values:
            row = []
            for corner in Corner:
                x, y, z = baselines[corner]
                row.extend(
                    (
                        cycle,
                        x + 2.0 * travel,
                        y + 3.0 * travel,
                        z + travel,
                    )
                )
            lines.append("\t".join(str(value) for value in row))
    path.write_text("\n".join(lines) + "\n")


def write_vertical_hdf5(path: Path) -> None:
    travel = np.asarray(
        (0.0, -1.0, -2.0, -1.0, 0.0, 1.0, 2.0, 1.0, 0.0, -1.0, -2.0)
    )
    with h5py.File(path, "w") as h5:
        h5.attrs["format_version"] = 3
        h5.attrs["sample_count"] = len(travel)
        h5.attrs["signal_count"] = 16
        h5.attrs["source_file"] = "vertical.res"
        results = h5.create_group("results")
        string_type = h5py.string_dtype(encoding="utf-8")
        for index, corner in enumerate(Corner):
            base = 100.0 * (index + 1)
            entity = results.create_group(corner.value)
            entity.attrs["source_path"] = corner.value
            entity.create_dataset(
                "values",
                data=np.column_stack(
                    (
                        travel,
                        base + 10.5 + 2.25 * travel,
                        base + 20.5 + 3.0 * travel,
                        base + 30.5 + travel,
                    )
                ),
            )
            entity.create_dataset(
                "component_names",
                data=np.asarray(("travel", "x", "y", "z"), dtype=string_type),
            )
            entity.create_dataset(
                "units", data=np.asarray(("mm",) * 4, dtype=string_type)
            )
            entity.create_dataset(
                "adams_ids",
                data=np.arange(index * 4 + 1, index * 4 + 5, dtype=np.uint64),
            )


def make_config(tmp_path: Path, *, engine_running: bool = False) -> CaseConfig:
    config_path = tmp_path / "case.toml"
    config_path.write_text("# vertical fixture\n")
    measurement = tmp_path / "measurement.acd"
    simulation = tmp_path / "result.h5"
    write_vertical_acd(measurement)
    write_vertical_hdf5(simulation)
    signals = []
    for corner in Corner:
        signals.append(
            (
                f"{corner.value}_travel",
                SignalRef(corner.value, "travel", "mm"),
            )
        )
        for axis in ("x", "y", "z"):
            signals.append(
                (
                    f"{corner.value}_wheel_center_{axis}",
                    SignalRef(corner.value, axis, "mm"),
                )
            )
    return CaseConfig(
        path=config_path,
        name="varc-lommel-2up",
        domain="multi",
        variant="VARC",
        loadcase="2UP",
        simulation_hdf5=simulation,
        gravity=9.80665,
        signals=(),
        vertical=VerticalConfig(
            measurement_acd=measurement,
            simulation_hdf5=simulation,
            setup=VerticalSetup(
                code="b1e0f1",
                brakes_applied=True,
                engine_running=engine_running,
                front_arb_connected=True,
            ),
            window=VerticalWindow(-2.0, 2.0, 1.0),
            signals=tuple(signals),
        ),
    )


def test_evaluates_all_corners_and_coordinates(tmp_path: Path) -> None:
    result = evaluate_vertical(make_config(tmp_path))

    assert len(result.comparisons) == 12
    front_left_x = result.comparisons[0]
    rear_right_z = result.comparisons[-1]
    assert front_left_x.corner is Corner.FRONT_LEFT
    assert front_left_x.axis == "x"
    assert front_left_x.measurement_baseline_mm == 110.0
    assert front_left_x.simulation_baseline_mm == 110.5
    assert front_left_x.baseline_error_mm == 0.5
    assert front_left_x.rmse_mm == pytest.approx(np.sqrt(0.125))
    assert front_left_x.maximum_error_mm == pytest.approx(0.5)
    assert rear_right_z.corner is Corner.REAR_RIGHT
    assert rear_right_z.measurement_baseline_mm == 430.0
    assert [
        comparison.simulation_baseline_mm
        for comparison in result.comparisons
        if comparison.axis == "x"
    ] == [110.5, 210.5, 310.5, 410.5]


def test_rejects_setup_mismatch(tmp_path: Path) -> None:
    with pytest.raises(VerticalEvaluationError, match="engineon=False"):
        evaluate_vertical(make_config(tmp_path, engine_running=True))


def test_writes_deterministic_reports_and_svg(tmp_path: Path) -> None:
    config = make_config(tmp_path)
    report = build_vertical_report(config, evaluate_vertical(config))
    json_path = tmp_path / "report.json"
    csv_path = tmp_path / "report.csv"
    plot_path = tmp_path / "trajectories.svg"

    write_vertical_outputs(
        report,
        json_path=json_path,
        csv_path=csv_path,
        plot_path=plot_path,
    )
    first_svg = plot_path.read_bytes()
    write_vertical_outputs(report, plot_path=plot_path, force=True)

    assert plot_path.read_bytes() == first_svg
    assert '"setup_code": "b1e0f1"' in json_path.read_text()
    assert "front_left,x,110.0,110.5,0.5,0.3535533905932738,0.5" in (
        csv_path.read_text()
    )
    assert b"Lommel" in first_svg
    assert b"Adams" in first_svg


def test_cli_requires_existing_vertical_inputs(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    path = tmp_path / "case.toml"
    path.write_text(
        """\
[case]
name = "fixture"
variant = "VARC"
loadcase = "2UP"
[physics]
gravity = 9.80665
[svc.inputs]
simulation_hdf5 = "svc.h5"
[svc.signals.signal]
entity = "svc"
component = "value"
unit = "mm"
[vertical.inputs]
measurement_acd = "missing.acd"
simulation_hdf5 = "vertical.h5"
[vertical.setup]
code = "b1e0f1"
brakes_applied = true
engine_running = false
front_arb_connected = true
[vertical.window]
minimum_mm = -2
maximum_mm = 2
step_mm = 1
[vertical.signals.front_left_travel]
entity = "front_left"
component = "travel"
unit = "mm"
"""
    )

    assert main(["validate-vertical", str(path)]) == 2
    assert "ACD file does not exist" in capsys.readouterr().err
