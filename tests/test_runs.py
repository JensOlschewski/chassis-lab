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
