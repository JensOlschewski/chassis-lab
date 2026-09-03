import hashlib
import json
from pathlib import Path

import h5py
import numpy as np
import pytest

from chassis_lab.cli import main
from chassis_lab.runs import RunError, create_run, sha256_file

CONFIG = """\
[case]
name = "demo"
domain = "svc"
variant = "VARB"
loadcase = "2UP"

[inputs]
simulation_hdf5 = "result.h5"

[physics]
gravity = 9.80665

[signals]
[signals.signal]
entity = "entity"
component = "value"
unit = "mm"
"""


def write_inputs(tmp_path: Path) -> Path:
    config_path = tmp_path / "case.toml"
    config_path.write_text(CONFIG, encoding="utf-8")
    with h5py.File(tmp_path / "result.h5", "w") as h5:
        h5.attrs.update(
            format_version=3,
            sample_count=2,
            signal_count=1,
            source_file="x.res",
        )
        group = h5.create_group("results/entity")
        group.create_dataset("values", data=np.array([[1.0], [2.0]]))
        string_dtype = h5py.string_dtype(encoding="utf-8")
        group.create_dataset(
            "component_names", data=["value"], dtype=string_dtype
        )
        group.create_dataset("units", data=["mm"], dtype=string_dtype)
        group.create_dataset("adams_ids", data=np.array([1], dtype=np.uint64))
    return config_path


def test_sha256_file_stream_matches_hashlib(tmp_path: Path) -> None:
    path = tmp_path / "input.bin"
    path.write_bytes(b"manifest input")
    expected = hashlib.sha256(path.read_bytes()).hexdigest()
    assert sha256_file(path) == expected


def test_create_run_writes_manifest_and_preserves_inputs(
    tmp_path: Path,
) -> None:
    config_path = write_inputs(tmp_path)
    result_path = tmp_path / "result.h5"
    run_dir = create_run(
        config_path, artifacts_root=tmp_path / "artifacts", run_id="run-1"
    )
    manifest = json.loads((run_dir / "run.json").read_text(encoding="utf-8"))
    assert run_dir == tmp_path / "artifacts/demo/run-1"
    assert manifest["schema_version"] == 1
    assert manifest["run"]["id"] == "run-1"
    assert manifest["config"]["snapshot"]["name"] == "demo"
    assert manifest["inputs"]["simulation_hdf5"]["sha256"] == sha256_file(
        result_path
    )
    assert manifest["hdf5"]["format_version"] == 3
    assert result_path.is_file()


def test_create_run_rejects_existing_run_and_invalid_id(tmp_path: Path) -> None:
    config_path = write_inputs(tmp_path)
    create_run(
        config_path, artifacts_root=tmp_path / "artifacts", run_id="run-1"
    )
    with pytest.raises(RunError, match="already exists"):
        create_run(
            config_path, artifacts_root=tmp_path / "artifacts", run_id="run-1"
        )
    with pytest.raises(RunError, match="invalid run ID"):
        create_run(
            config_path, artifacts_root=tmp_path / "artifacts", run_id="../bad"
        )


def test_create_run_cli_prints_directory(tmp_path: Path, capsys) -> None:
    config_path = write_inputs(tmp_path)
    assert (
        main(
            [
                "create-run",
                str(config_path),
                "--artifacts-root",
                str(tmp_path / "artifacts"),
                "--run-id",
                "cli-run",
            ]
        )
        == 0
    )
    assert capsys.readouterr().out.strip().endswith("artifacts/demo/cli-run")


def test_create_vertical_run_records_measurement_provenance(
    tmp_path: Path,
) -> None:
    config_path = write_inputs(tmp_path)
    measurement = tmp_path / "measurement.acd"
    names = [
        f"{prefix}{suffix}"
        for prefix in ("lf", "rf", "lr", "rr")
        for suffix in ("slope", "wcx", "wcy", "wcz")
    ]
    measurement.write_text(
        "\n".join(
            (
                "FileInfo: ASCII3",
                "TestInfo:\ttesttype\ttesttype\tTest type\tstring\t\tvert",
                "TestInfo:\tbrakeson\tbrakeson\tBrakes applied\tinteger\t\t1",
                "TestInfo:\tengineon\tengineon\tEngine running\tinteger\t\t0",
                "TestInfo:\tfarbon\tfarbon\tFront ARB\tinteger\t\t1",
                "TestInfo:\trarbon\trarbon\tRear ARB\tinteger\t\tNaN",
                "\t".join(
                    ["Cycle", "WC X", "WC Y", "WC Z"] * 4
                    + ["string", "Description"]
                ),
                "\t".join(["0"] * len(names) + ["float", "Offset"]),
                "\t".join(names + ["string", "Name"]),
                "\t".join(["NaN", "mm", "mm", "mm"] * 4 + ["string", "Units"]),
                "\t".join(["NaN"] * len(names) + ["string", "KPoly"]),
                "\t".join(["0"] * len(names)),
                "\t".join(["0"] * len(names)),
            )
        )
        + "\n",
        encoding="utf-8",
    )
    config_path.write_text(
        """\
[case]
name = "demo"
variant = "VARB"
loadcase = "2UP"
[physics]
gravity = 9.80665
[svc.inputs]
simulation_hdf5 = "result.h5"
[svc.signals.signal]
entity = "entity"
component = "value"
unit = "mm"
[vertical.inputs]
measurement_acd = "measurement.acd"
simulation_hdf5 = "result.h5"
[vertical.setup]
code = "b1e0f1"
brakes_applied = true
engine_running = false
front_arb_connected = true
[vertical.window]
minimum_mm = -1
maximum_mm = 1
step_mm = 1
[vertical.signals.front_left_travel]
entity = "entity"
component = "value"
unit = "mm"
"""
    )

    run_dir = create_run(
        config_path,
        domain="vertical",
        artifacts_root=tmp_path / "artifacts",
        run_id="vertical-run",
    )
    manifest = json.loads((run_dir / "run.json").read_text())

    assert manifest["config"]["snapshot"]["domain"] == "vertical"
    assert manifest["config"]["snapshot"]["setup"] == {
        "code": "b1e0f1",
        "brakes_applied": True,
        "engine_running": False,
        "front_arb_connected": True,
        "rear_arb_connected": None,
    }
    assert manifest["config"]["snapshot"]["window"] == {
        "minimum_mm": -1,
        "maximum_mm": 1,
        "step_mm": 1,
    }
    assert manifest["inputs"]["measurement_acd"]["sha256"] == sha256_file(
        measurement
    )
