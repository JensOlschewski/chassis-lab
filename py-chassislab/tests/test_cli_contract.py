import argparse
import csv
import json
from pathlib import Path

import pytest

import chassis_lab.cli as cli
from chassis_lab.config import ConfigError
from chassis_lab.svc_reporting import (
    CSV_FIELDS as SVC_CSV_FIELDS,
)
from chassis_lab.svc_reporting import (
    REPORT_SCHEMA_VERSION as SVC_REPORT_SCHEMA_VERSION,
)
from chassis_lab.vertical_reporting import (
    CSV_FIELDS as VERTICAL_CSV_FIELDS,
)
from chassis_lab.vertical_reporting import (
    REPORT_SCHEMA_VERSION as VERTICAL_REPORT_SCHEMA_VERSION,
)

COMMAND_CONTRACT = {
    "inspect-h5": (("path",), (("--json",),)),
    "list-h5-signals": (
        ("path",),
        (("--match",), ("--json",)),
    ),
    "inspect-acd": (("path",), (("--json",),)),
    "list-acd-signals": (
        ("path",),
        (("--match",), ("--json",)),
    ),
    "validate-config": (
        ("path",),
        (("--json",), ("--strict-units",)),
    ),
    "validate-svc": (
        ("path",),
        (
            ("--json-output",),
            ("--csv-output",),
            ("-f", "--force"),
            ("--color",),
        ),
    ),
    "validate-vertical": (
        ("path",),
        (
            ("--plot-output",),
            ("--json-output",),
            ("--csv-output",),
            ("-f", "--force"),
        ),
    ),
    "convert-res": (
        ("input",),
        (
            ("-o", "--output"),
            ("--binary",),
            ("-f", "--force"),
        ),
    ),
    "create-run": (
        ("path",),
        (("--artifacts-root",), ("--domain",), ("--run-id",)),
    ),
}

SVC_JSON_FIELDS = {
    "case",
    "failed_count",
    "inputs",
    "metrics",
    "passed",
    "reference_sources",
    "sample_index",
    "schema_version",
    "software",
}
SVC_METRIC_FIELDS = {
    "absolute_tolerance",
    "actual",
    "allowed_difference",
    "difference",
    "name",
    "passed",
    "reference",
    "relative_percent",
    "unit",
}


def test_v031_command_and_option_contract() -> None:
    parser = cli.build_parser()
    command_action = next(
        action for action in parser._actions if action.dest == "command"
    )
    actual = {}
    for name, subparser in command_action.choices.items():
        actions = [
            action for action in subparser._actions if action.dest != "help"
        ]
        positionals = tuple(
            action.dest for action in actions if not action.option_strings
        )
        options = tuple(
            tuple(action.option_strings)
            for action in actions
            if action.option_strings
        )
        actual[name] = (positionals, options)

    assert actual == COMMAND_CONTRACT
    help_output = parser.format_help()
    assert "Analyze and validate vehicle multibody simulation results" in (
        help_output
    )
    assert all(command in help_output for command in COMMAND_CONTRACT)


@pytest.mark.parametrize("exit_code", (0, 1))
def test_main_preserves_success_and_validation_exit_codes(
    exit_code: int, monkeypatch: pytest.MonkeyPatch
) -> None:
    parser = _StubParser(lambda _: exit_code)
    monkeypatch.setattr(cli, "build_parser", lambda: parser)

    assert cli.main([]) == exit_code


def test_main_maps_operational_errors_to_two(
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    def fail(_: argparse.Namespace) -> int:
        raise ConfigError("invalid contract fixture")

    monkeypatch.setattr(cli, "build_parser", lambda: _StubParser(fail))

    assert cli.main([]) == 2
    captured = capsys.readouterr()
    assert captured.out == ""
    assert captured.err == "error: invalid contract fixture\n"


def test_inspection_commands_are_read_only(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    summary = _Summary()
    monkeypatch.setattr(cli, "inspect_hdf5", lambda _: summary)
    monkeypatch.setattr(cli, "inspect_acd", lambda _: summary)
    monkeypatch.setattr(cli, "AdamsHdf5", _SignalReader)
    monkeypatch.setattr(cli, "AcdFile", _SignalReader)
    missing = tmp_path / "input"

    commands = (
        ["inspect-h5", str(missing), "--json"],
        ["list-h5-signals", str(missing), "--json"],
        ["inspect-acd", str(missing), "--json"],
        ["list-acd-signals", str(missing), "--json"],
    )
    for command in commands:
        assert cli.main(command) == 0

    capsys.readouterr()
    assert tuple(tmp_path.iterdir()) == ()


def test_svc_report_schema_contract(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    root = Path(__file__).resolve().parents[2]
    config = root / "configs" / "example-config.toml"
    json_output = tmp_path / "svc.json"
    csv_output = tmp_path / "svc.csv"

    assert (
        cli.main(
            [
                "validate-svc",
                str(config),
                "--json-output",
                str(json_output),
                "--csv-output",
                str(csv_output),
            ]
        )
        == 0
    )
    capsys.readouterr()

    report = json.loads(json_output.read_text(encoding="utf-8"))
    assert report["schema_version"] == SVC_REPORT_SCHEMA_VERSION == 2
    assert set(report) == SVC_JSON_FIELDS
    assert set(report["metrics"][0]) == SVC_METRIC_FIELDS
    with csv_output.open(encoding="utf-8", newline="") as stream:
        assert tuple(csv.DictReader(stream).fieldnames or ()) == SVC_CSV_FIELDS


def test_vertical_report_schema_contract() -> None:
    assert VERTICAL_REPORT_SCHEMA_VERSION == 1
    assert VERTICAL_CSV_FIELDS == (
        "case",
        "variant",
        "loadcase",
        "setup_code",
        "corner",
        "axis",
        "measurement_baseline_mm",
        "simulation_baseline_mm",
        "baseline_error_mm",
        "rmse_mm",
        "maximum_error_mm",
        "config_sha256",
        "signal_map_sha256",
        "measurement_sha256",
        "simulation_sha256",
    )


class _StubParser:
    def __init__(self, handler) -> None:
        self.handler = handler

    def parse_args(self, _: object) -> argparse.Namespace:
        return argparse.Namespace(handler=self.handler)


class _Summary:
    def as_dict(self) -> dict[str, int]:
        return {"sample_count": 0}


class _SignalReader:
    def __init__(self, _: Path) -> None:
        pass

    def __enter__(self):
        return self

    def __exit__(self, *_: object) -> None:
        pass

    def signals(self, _: str | None = None) -> tuple[()]:
        return ()
