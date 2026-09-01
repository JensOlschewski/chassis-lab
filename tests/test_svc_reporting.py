import csv
import io
import json
from dataclasses import replace
from pathlib import Path

import pytest

from chassis_lab.cli import main
from chassis_lab.config import (
    CaseConfig,
    SignalRef,
    ValidationConfig,
    ValidationTolerances,
)
from chassis_lab.runs import sha256_file
from chassis_lab.svc_reporting import (
    CSV_FIELDS,
    SvcReportError,
    build_svc_report,
    metric_group,
    metric_unit,
    render_svc_console,
    render_svc_csv,
    render_svc_json,
    write_svc_outputs,
)
from chassis_lab.svc_validation import (
    MetricComparison,
    SvcValidationResult,
)


def write_inputs(tmp_path: Path) -> CaseConfig:
    config_path = tmp_path / "case.toml"
    simulation_path = tmp_path / "result.h5"
    reference_path = tmp_path / "reference.json"
    config_path.write_text("synthetic SVC config\n", encoding="utf-8")
    simulation_path.write_bytes(b"synthetic HDF5")
    reference_path.write_text(
        json.dumps(
            {
                "schema_version": 1,
                "case": {
                    "name": "svc-report",
                    "variant": "VARB",
                    "loadcase": "CURB",
                },
                "coordinate_system": {
                    "cg_x_origin": "front_axle_wheel_center",
                    "cg_x_positive": "rearward",
                },
                "sources": [
                    {
                        "file": "veh_inf.htm",
                        "sha256": "a" * 64,
                        "test_date": "2026-08-31",
                        "result": "reviewed",
                    }
                ],
                "values": {
                    "total_mass_kg": 1500.0,
                    "wheelbase_mm": 2500.0,
                },
            }
        ),
        encoding="utf-8",
    )
    return CaseConfig(
        path=config_path,
        name="svc-report",
        domain="svc",
        variant="VARB",
        loadcase="CURB",
        simulation_hdf5=simulation_path,
        gravity=9.80665,
        signals=(("unused", SignalRef("entity", "component", "mm")),),
        validation=ValidationConfig(
            reference=reference_path,
            tolerances=ValidationTolerances(
                absolute=(
                    ("total_mass_kg", 20.0),
                    ("wheelbase_mm", 5.0),
                ),
                relative_percent=(),
            ),
        ),
    )


def validation_result(*, passed: bool = False) -> SvcValidationResult:
    wheelbase = MetricComparison(
        name="wheelbase_mm",
        actual=2500.0 if passed else 2510.0,
        reference=2500.0,
        difference=0.0 if passed else 10.0,
        absolute_tolerance=5.0,
        relative_percent=None,
        allowed_difference=5.0,
        passed=passed,
    )
    return SvcValidationResult(
        case_name="svc-report",
        reference_path=Path("reference.json"),
        metrics=(
            MetricComparison(
                name="total_mass_kg",
                actual=1510.0,
                reference=1500.0,
                difference=10.0,
                absolute_tolerance=20.0,
                relative_percent=None,
                allowed_difference=20.0,
                passed=True,
            ),
            wheelbase,
        ),
    )


def test_builds_report_with_units_and_provenance(tmp_path: Path) -> None:
    config = write_inputs(tmp_path)
    report = build_svc_report(config, validation_result())

    assert not report.passed
    assert [metric.unit for metric in report.metrics] == ["kg", "mm"]
    assert report.config_input.sha256 == sha256_file(config.path)
    assert report.simulation_input.sha256 == sha256_file(config.simulation_hdf5)
    assert report.reference_sources == (
        {
            "file": "veh_inf.htm",
            "sha256": "a" * 64,
            "test_date": "2026-08-31",
            "result": "reviewed",
        },
    )


def test_console_groups_and_lists_all_metrics(tmp_path: Path) -> None:
    config = write_inputs(tmp_path)
    report = build_svc_report(config, validation_result())

    assert render_svc_console(report) == (
        "SVC validation\n"
        "case: svc-report (VARB, CURB)\n"
        f"simulation: {config.simulation_hdf5}\n"
        f"reference: {config.validation.reference}\n"
        "\n"
        "running 2 metrics\n"
        "\n"
        "svc::loads\n"
        "  total_mass_kg ... ok (actual 1510; reference 1500; "
        "diff 10 <= 20 kg)\n"
        "\n"
        "svc::geometry\n"
        "  wheelbase_mm ... FAILED (actual 2510; reference 2500; "
        "diff 10 > 5 mm)\n"
        "\n"
        "validation result: FAILED. 1 passed; 1 failed; 2 total\n"
    )


def test_assigns_metric_units_and_console_groups() -> None:
    assert metric_unit("toe_front_left_deg") == "deg"
    assert metric_group("front_left_mass_kg") == "loads"
    assert metric_group("wheelbase_mm") == "geometry"
    assert metric_group("ride_height_front_left_mm") == "ride_height"
    assert metric_group("camber_rear_right_deg") == "alignment"


def test_console_colors_status_only_when_requested(tmp_path: Path) -> None:
    report = build_svc_report(write_inputs(tmp_path), validation_result())

    plain = render_svc_console(report)
    colored = render_svc_console(report, color=True)

    assert "\x1b[" not in plain
    assert "\x1b[32mok\x1b[0m" in colored
    assert "\x1b[31mFAILED\x1b[0m" in colored


def test_json_and_csv_are_deterministic(tmp_path: Path) -> None:
    config = write_inputs(tmp_path)
    report = build_svc_report(config, validation_result())

    json_output = render_svc_json(report)
    assert json_output == render_svc_json(report)
    data = json.loads(json_output)
    assert data["schema_version"] == 1
    assert data["failed_count"] == 1
    assert data["metrics"][0]["unit"] == "kg"
    assert data["inputs"]["simulation_hdf5"]["sha256"] == sha256_file(
        config.simulation_hdf5
    )

    csv_output = render_svc_csv(report)
    assert csv_output == render_svc_csv(report)
    assert "\r\n" not in csv_output
    reader = csv.DictReader(io.StringIO(csv_output))
    assert tuple(reader.fieldnames or ()) == CSV_FIELDS
    rows = list(reader)
    assert [row["metric"] for row in rows] == [
        "total_mass_kg",
        "wheelbase_mm",
    ]
    assert rows[1]["passed"] == "false"
    assert rows[1]["simulation_sha256"] == sha256_file(config.simulation_hdf5)


def test_writes_outputs_and_requires_force(tmp_path: Path) -> None:
    report = build_svc_report(write_inputs(tmp_path), validation_result())
    json_path = tmp_path / "reports" / "metrics.json"
    csv_path = tmp_path / "reports" / "summary.csv"

    write_svc_outputs(report, json_path=json_path, csv_path=csv_path)

    assert json_path.read_text(encoding="utf-8") == render_svc_json(report)
    assert csv_path.read_text(encoding="utf-8") == render_svc_csv(report)
    with pytest.raises(SvcReportError, match="already exists"):
        write_svc_outputs(report, json_path=json_path)
    write_svc_outputs(report, json_path=json_path, force=True)


def test_rejects_unknown_unit_and_mismatched_case(tmp_path: Path) -> None:
    with pytest.raises(SvcReportError, match="no report unit"):
        metric_unit("unknown")

    config = write_inputs(tmp_path)
    result = replace(validation_result(), case_name="other")
    with pytest.raises(SvcReportError, match="does not match"):
        build_svc_report(config, result)


def test_validate_svc_cli_writes_reports_and_returns_one(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    config = write_inputs(tmp_path)
    config.path.write_text(
        f"""\
[case]
name = "svc-report"
domain = "svc"
variant = "VARB"
loadcase = "CURB"
[inputs]
simulation_hdf5 = "{config.simulation_hdf5.name}"
[physics]
gravity = 9.80665
[validation]
reference = "{config.validation.reference.name}"
[validation.tolerances.absolute]
total_mass_kg = 20.0
wheelbase_mm = 5.0
""",
        encoding="utf-8",
    )
    monkeypatch.setattr(
        "chassis_lab.cli.validate_svc", lambda loaded: validation_result()
    )
    json_path = tmp_path / "metrics.json"
    csv_path = tmp_path / "summary.csv"

    exit_code = main(
        [
            "validate-svc",
            str(config.path),
            "--json-output",
            str(json_path),
            "--csv-output",
            str(csv_path),
        ]
    )

    assert exit_code == 1
    output = capsys.readouterr().out
    assert output.startswith("SVC validation\n")
    assert "total_mass_kg ... ok" in output
    assert "wheelbase_mm ... FAILED" in output
    assert json.loads(json_path.read_text())["failed_count"] == 1
    with csv_path.open(encoding="utf-8", newline="") as stream:
        assert len(list(csv.DictReader(stream))) == 2

    assert (
        main(
            ["validate-svc", str(config.path), "--json-output", str(json_path)]
        )
        == 2
    )
    assert "already exists" in capsys.readouterr().err


def test_validate_svc_cli_returns_zero_for_pass(
    tmp_path: Path,
    monkeypatch: pytest.MonkeyPatch,
    capsys: pytest.CaptureFixture[str],
) -> None:
    config = write_inputs(tmp_path)
    config.path.write_text(
        f"""\
[case]
name = "svc-report"
domain = "svc"
variant = "VARB"
loadcase = "CURB"
[inputs]
simulation_hdf5 = "{config.simulation_hdf5.name}"
[physics]
gravity = 9.80665
[validation]
reference = "{config.validation.reference.name}"
[validation.tolerances.absolute]
total_mass_kg = 20.0
wheelbase_mm = 5.0
""",
        encoding="utf-8",
    )
    monkeypatch.setattr(
        "chassis_lab.cli.validate_svc",
        lambda loaded: validation_result(passed=True),
    )

    assert main(["validate-svc", str(config.path), "--color", "always"]) == 0
    output = capsys.readouterr().out
    assert output.startswith("SVC validation\n")
    assert "total_mass_kg ... \x1b[32mok\x1b[0m" in output
    assert "wheelbase_mm ... \x1b[32mok\x1b[0m" in output
    assert "validation result: \x1b[32mok\x1b[0m." in output
