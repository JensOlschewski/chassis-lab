"""Convert Adams/Car .res result files to the HDF5 contract."""

import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

DEFAULT_BINARY = "adams-res"
REPOSITORY_BINARIES = (
    Path(__file__).resolve().parents[2] / "target/release/adams-res",
    Path(__file__).resolve().parents[2] / "target/debug/adams-res",
    Path(__file__).resolve().parents[2] / "reader/target/release/adams-res",
    Path(__file__).resolve().parents[2] / "reader/target/debug/adams-res",
)


class ConversionError(RuntimeError):
    """Raised when the external result converter cannot run."""


@dataclass(frozen=True)
class ConversionResult:
    """Outcome of a .res to HDF5 conversion."""

    res_path: Path
    h5_path: Path
    binary: str


def convert_res(
    res_path: Path | str,
    *,
    output: Path | str | None = None,
    binary: str = DEFAULT_BINARY,
    force: bool = False,
) -> ConversionResult:
    """Convert an Adams/Car ``.res`` file to HDF5 using ``adams-res``.

    The converter command is discovered on ``PATH`` by default; ``binary`` may
    be an absolute path. ``output`` defaults to the input path with an ``.h5``
    suffix. Existing outputs are rejected unless ``force`` is set.
    """
    res = Path(res_path).expanduser()
    if not res.is_file():
        raise FileNotFoundError(f".res file does not exist: {res}")

    h5 = res.with_suffix(".h5") if output is None else Path(output).expanduser()
    if h5.exists() and not force:
        raise ConversionError(
            f"output already exists, use --force to replace: {h5}"
        )
    h5.parent.mkdir(parents=True, exist_ok=True)

    executable = _resolve_binary(binary)
    if executable is None:
        raise ConversionError(f"converter binary not found: {binary}")

    try:
        completed = subprocess.run(
            [executable, str(res), "-o", str(h5), *(["-f"] if force else [])],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as error:
        raise ConversionError(
            f"failed to run converter {executable}: {error}"
        ) from error

    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        reason = detail or f"exit {completed.returncode}"
        raise ConversionError(f"converter failed for {res}: {reason}")
    return ConversionResult(
        res_path=res.resolve(),
        h5_path=h5.resolve(),
        binary=executable,
    )


def _resolve_binary(binary: str) -> str | None:
    expanded = Path(binary).expanduser()
    if expanded.is_absolute() or "/" in binary:
        return str(expanded) if expanded.is_file() else None
    if binary == DEFAULT_BINARY:
        for candidate in REPOSITORY_BINARIES:
            if candidate.is_file():
                return str(candidate)
    return shutil.which(binary)


def converter_provenance(binary: str = DEFAULT_BINARY) -> dict[str, str | None]:
    """Return converter identity and reported version when available."""
    executable = _resolve_binary(binary)
    provenance: dict[str, str | None] = {
        "name": "adams-res-cli",
        "path": executable,
        "version": None,
    }
    if executable is None:
        return provenance
    try:
        completed = subprocess.run(
            [executable, "--version"],
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError:
        return provenance
    if completed.returncode == 0:
        version = (completed.stdout or completed.stderr).strip()
        provenance["version"] = version or None
    return provenance
