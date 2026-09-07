import stat
from pathlib import Path

import pytest

import chassis_lab.converter as converter
from chassis_lab.converter import ConversionError, convert_res
from chassis_lab.hdf5 import AdamsHdf5

MINIMAL_RES = (
    "<Result>\n"
    '  <Entity type="request" name="ride_heights" '
    'entity="vehicle.ride_heights" objectId="42">\n'
    '    <Component name="front_left" unitsValue="mm" id="2" />\n'
    '    <Component name="front_right" unitsValue="mm" id="1" />\n'
    "  </Entity>\n"
    '  <Entity type="request" name="svc_output">\n'
    '    <Component name="Wheelbase" unitsValue="mm" id="3" />\n'
    "  </Entity>\n"
    '  <Step type="input">\n'
    "    360.0 361.0 2490.0\n"
    "  </Step>\n"
    '  <Step type="input">\n'
    "    359.5 360.5 2490.0\n"
    "  </Step>\n"
    "</Result>\n"
)


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


def test_prefers_repository_converter(tmp_path: Path, monkeypatch) -> None:
    res = tmp_path / "run.res"
    res.write_text("dummy")
    fake = make_fake_converter(tmp_path)
    monkeypatch.setattr(converter, "REPOSITORY_BINARIES", (fake,))

    result = convert_res(res)

    assert result.binary == str(fake)


def test_repository_converter_round_trip_to_python_reader(
    tmp_path: Path,
) -> None:
    binary_available = any(
        path.is_file() for path in converter.REPOSITORY_BINARIES
    )
    if not binary_available:
        pytest.skip("build converter before running integration test")
    source = tmp_path / "minimal.res"
    source.write_text(MINIMAL_RES)

    result = convert_res(source, output=tmp_path / "result.h5")

    with AdamsHdf5(result.h5_path) as hdf5:
        assert hdf5.summary.format_version == 3
        assert hdf5.summary.sample_count == 2
        assert hdf5.summary.signal_count == 3
        front_left = hdf5.read_signal(
            "vehicle.ride_heights", "front_left", expected_unit="mm"
        )
        wheelbase = hdf5.read_signal(
            "svc_output", "Wheelbase", expected_unit="mm"
        )
    assert front_left.values.tolist() == [361.0, 360.5]
    assert wheelbase.values.tolist() == [2490.0, 2490.0]
