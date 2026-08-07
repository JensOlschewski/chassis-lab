import stat
from pathlib import Path

import pytest

from knc_validation.converter import ConversionError, convert_res


def make_fake_converter(path: Path, exit_code: int = 0) -> Path:
    binary = path / "adams-res"
    binary.write_text(
        "#!/bin/sh\n"
        'printf "%s\\n" "$@" > "$(dirname "$0")/args.log"\n'
        "out=\n"
        "prev=\n"
        'for arg in "$@"; do\n'
        '  if [ "$prev" = "-o" ]; then out="$arg"; fi\n'
        '  prev="$arg"\n'
        "done\n"
        f"exit {exit_code}\n"
    )
    binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
    return binary


def read_args(tmp_path: Path) -> list[str]:
    return (tmp_path / "args.log").read_text().strip().splitlines()


def test_converts_res_with_default_output(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    fake = make_fake_converter(tmp_path)

    result = convert_res(res, binary=str(fake))

    assert result.res_path == res.resolve()
    assert result.h5_path == (tmp_path / "run.h5").resolve()
    assert result.binary == str(fake.resolve())


def test_converts_res_with_explicit_output(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    fake = make_fake_converter(tmp_path)
    output = tmp_path / "out" / "result.h5"
    output.parent.mkdir()

    result = convert_res(res, output=output, binary=str(fake))

    assert result.h5_path == output.resolve()


def test_rejects_existing_output_without_force(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    (tmp_path / "run.h5").write_text("existing")
    fake = make_fake_converter(tmp_path)

    with pytest.raises(ConversionError, match="force"):
        convert_res(res, binary=str(fake))


def test_forces_existing_output(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    (tmp_path / "run.h5").write_text("existing")
    fake = make_fake_converter(tmp_path)

    result = convert_res(res, binary=str(fake), force=True)

    assert result.h5_path == (tmp_path / "run.h5").resolve()
    assert "-f" in read_args(tmp_path)


def test_default_run_does_not_pass_force(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    fake = make_fake_converter(tmp_path)

    convert_res(res, binary=str(fake))

    assert "-f" not in read_args(tmp_path)


def test_raises_on_converter_failure(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    fake = make_fake_converter(tmp_path, exit_code=1)

    with pytest.raises(ConversionError, match="failed"):
        convert_res(res, binary=str(fake))


def test_raises_on_missing_input(tmp_path: Path) -> None:
    with pytest.raises(FileNotFoundError):
        convert_res(tmp_path / "missing.res", binary="adams-res")


def test_raises_on_missing_binary(tmp_path: Path) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")

    with pytest.raises(ConversionError, match="not found"):
        convert_res(res, binary=str(tmp_path / "nonexistent"))
