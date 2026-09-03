"""Create reproducible, provenance-carrying analysis run directories."""

from __future__ import annotations

import hashlib
import json
import platform
import secrets
import subprocess
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from chassis_lab import __version__
from chassis_lab.acd import inspect_acd
from chassis_lab.config import CaseConfig, load_config
from chassis_lab.hdf5 import AdamsHdf5
from chassis_lab.vertical import validate_vertical_measurement


class RunError(RuntimeError):
    """Raised when a run manifest cannot be created."""


def sha256_file(path: Path | str) -> str:
    """Return a streaming SHA-256 digest for an existing file."""
    source = Path(path)
    if not source.is_file():
        raise FileNotFoundError(f"input file does not exist: {source}")
    digest = hashlib.sha256()
    with source.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def create_run(
    config_path: Path | str,
    *,
    domain: str = "svc",
    artifacts_root: Path | str | None = None,
    run_id: str | None = None,
) -> Path:
    """Validate inputs and create isolated run directory with ``run.json``."""
    config = load_config(config_path)
    simulation_hdf5, domain_inputs, signals = _domain_inputs(config, domain)
    if not simulation_hdf5.is_file():
        raise FileNotFoundError(
            f"simulation HDF5 file does not exist: {simulation_hdf5}"
        )
    if domain == "vertical":
        measurement = domain_inputs["measurement_acd"]
        validate_vertical_measurement(config.vertical)
        inspect_acd(measurement)
    with AdamsHdf5(simulation_hdf5) as result:
        hdf5_summary = result.summary.as_dict()

    root = (
        Path(artifacts_root).expanduser().resolve()
        if artifacts_root is not None
        else config.path.parent.parent / "artifacts"
    )
    identifier = _make_run_id() if run_id is None else _validate_run_id(run_id)
    run_dir = root / config.name / identifier
    try:
        run_dir.mkdir(parents=True, exist_ok=False)
    except FileExistsError as error:
        raise RunError(f"run directory already exists: {run_dir}") from error

    manifest = _build_manifest(
        config,
        hdf5_summary,
        run_dir,
        root,
        domain=domain,
        domain_inputs=domain_inputs,
        signals=signals,
    )
    manifest_path = run_dir / "run.json"
    temporary_path = run_dir / ".run.json.tmp"
    try:
        temporary_path.write_text(
            json.dumps(manifest, indent=2, sort_keys=True) + "\n",
            encoding="utf-8",
        )
        temporary_path.replace(manifest_path)
    except OSError:
        temporary_path.unlink(missing_ok=True)
        raise
    return run_dir


def _build_manifest(
    config: CaseConfig,
    hdf5_summary: dict[str, Any],
    run_dir: Path,
    artifacts_root: Path,
    *,
    domain: str,
    domain_inputs: dict[str, Path],
    signals: tuple,
) -> dict[str, Any]:
    config_data = {
        "name": config.name,
        "domain": domain,
        "variant": config.variant,
        "loadcase": config.loadcase,
        "simulation_hdf5": str(domain_inputs["simulation_hdf5"]),
        "gravity": config.gravity,
        "signals": [
            {
                "canonical": canonical,
                "entity": ref.entity,
                "component": ref.component,
                "unit": ref.unit,
            }
            for canonical, ref in signals
        ],
    }
    if "measurement_acd" in domain_inputs:
        config_data["measurement_acd"] = str(domain_inputs["measurement_acd"])
    if domain == "vertical" and config.vertical is not None:
        config_data["setup"] = {
            "code": config.vertical.setup.code,
            "brakes_applied": config.vertical.setup.brakes_applied,
            "engine_running": config.vertical.setup.engine_running,
            "front_arb_connected": config.vertical.setup.front_arb_connected,
            "rear_arb_connected": config.vertical.setup.rear_arb_connected,
        }
        config_data["window"] = {
            "minimum_mm": config.vertical.window.minimum_mm,
            "maximum_mm": config.vertical.window.maximum_mm,
            "step_mm": config.vertical.window.step_mm,
        }
    return {
        "schema_version": 1,
        "run": {
            "id": run_dir.name,
            "case": config.name,
            "created_at": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
            "directory": str(run_dir),
            "artifacts_root": str(artifacts_root),
        },
        "config": {"path": str(config.path), "snapshot": config_data},
        "inputs": {
            "config": _file_record(config.path),
            **{
                name: _file_record(path) for name, path in domain_inputs.items()
            },
        },
        "hdf5": hdf5_summary,
        "software": {
            "chassis_lab": __version__,
            "python": platform.python_version(),
            "platform": platform.platform(),
            "converter": {"name": "adams-res-cli", "version": None},
        },
        "git": _git_metadata(config.path),
    }


def _domain_inputs(
    config: CaseConfig, domain: str
) -> tuple[Path, dict[str, Path], tuple]:
    if domain == "svc":
        simulation = config.simulation_hdf5
        inputs = {"simulation_hdf5": simulation}
        if config.svc_signal_map is not None:
            inputs["signal_map"] = config.svc_signal_map
        return simulation, inputs, config.signals
    if domain == "vertical":
        if config.vertical is None:
            raise RunError(
                f"case '{config.name}' has no [vertical] configuration"
            )
        simulation = config.vertical.simulation_hdf5
        inputs = {
            "simulation_hdf5": simulation,
            "measurement_acd": config.vertical.measurement_acd,
        }
        if config.vertical.signal_map is not None:
            inputs["signal_map"] = config.vertical.signal_map
        return simulation, inputs, config.vertical.signals
    raise RunError(f"unsupported run domain: {domain}")


def _file_record(path: Path) -> dict[str, Any]:
    return {
        "path": str(path),
        "sha256": sha256_file(path),
        "size": path.stat().st_size,
    }


def _git_metadata(path: Path) -> dict[str, Any]:
    try:
        root = _git(path, "rev-parse", "--show-toplevel")
        revision = _git(path, "rev-parse", "HEAD")
        status = subprocess.run(
            ["git", "-C", str(path.parent), "status", "--porcelain"],
            capture_output=True,
            text=True,
            check=False,
        )
    except (OSError, subprocess.CalledProcessError):
        return {"repository": None, "revision": None, "dirty": None}
    return {
        "repository": root,
        "revision": revision,
        "dirty": bool(status.stdout.strip())
        if status.returncode == 0
        else None,
    }


def _git(path: Path, *arguments: str) -> str:
    completed = subprocess.run(
        ["git", "-C", str(path.parent), *arguments],
        capture_output=True,
        text=True,
        check=True,
    )
    return completed.stdout.strip()


def _make_run_id() -> str:
    timestamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%SZ")
    return f"{timestamp}-{secrets.token_hex(4)}"


def _validate_run_id(value: str) -> str:
    if not value or value in {".", ".."} or "/" in value or "\\" in value:
        raise RunError(f"invalid run ID: {value!r}")
    return value
