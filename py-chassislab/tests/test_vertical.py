import json
import subprocess
import sys
from io import BytesIO
from math import atan, degrees
from pathlib import Path

import numpy as np
import pytest
from pypdf import PdfReader

from chassis_lab.knc_plot_kernel import (
    BranchOptions,
    branch_ranges,
    gradient_curve,
)
from chassis_lab.knc_vertical_kernel import (
    evaluate,
    metrics,
    plot,
    plot_compare,
)


def _metric_curves(**overrides: list[float]) -> str:
    # Up and down branches with hysteresis-free linear responses.
    z = [-10.0, -5.0, 0.0, 5.0, 10.0, 10.0, 5.0, 0.0, -5.0, -10.0]
    responses = {
        "wheel_center_z": z,
        "wheel_center_x": [-0.1 * value for value in z],
        "wheel_center_y": [0.2 * value for value in z],
        "toe": [0.01 * value for value in z],
        "camber": [-0.02 * value for value in z],
        "wheel_load": [3000.0 + 25.0 * value for value in z],
        "tire_deflection": [0.1 * value for value in z],
    }
    responses.update(overrides)
    return json.dumps(
        [
            {"corner": corner, "signal": signal, "unit": "", "values": values}
            for corner in (
                "front_left",
                "front_right",
                "rear_left",
                "rear_right",
            )
            for signal, values in responses.items()
        ]
    )


def test_rates_follow_each_branch_without_mixing_hysteresis() -> None:
    # Two rising passes at different loads; global differentiation spikes.
    travel = np.array(
        [
            -10.0,
            -5.0,
            0.0,
            5.0,
            10.0,
            10.0,
            5.0,
            0.0,
            -5.0,
            -10.0,
            -10.0,
            -5.0,
            0.0,
            5.0,
            10.0,
        ]
    )
    force = 3000 + 25 * travel
    force[10:] += 100 + 15 * travel[10:]
    ranges = branch_ranges(travel, BranchOptions())
    x, rate = gradient_curve(
        travel, force, None, [segment for segment, _ in ranges], BranchOptions()
    )
    # Repeated travel at reversals has an undefined pointwise derivative.
    assert np.isnan(rate).sum() >= len(ranges)
    assert np.nanmin(rate) == pytest.approx(25.0)
    assert np.nanmax(np.abs(rate)) == pytest.approx(40.0)
    assert np.isfinite(x).sum() > 0
    curves = json.loads(_metric_curves())
    for curve in curves:
        if curve["signal"] == "wheel_load":
            curve["values"] = force.tolist()
        else:
            curve["values"] = (
                (0.1 * travel).tolist()
                if curve["signal"] == "tire_deflection"
                else travel.tolist()
            )
    response = json.loads(
        metrics(
            {
                "curves": json.dumps(curves),
                "gradient_mm": 5,
                "branch": {"index": 2},
            }
        )
    )
    rate_metric = next(
        m
        for m in response["metrics"]
        if m["metric"] == "suspension_rate_front_left_n_per_mm"
    )
    assert rate_metric["value"] == pytest.approx(40.0)


def test_metrics_use_secants_at_symmetric_travel_points() -> None:
    response = json.loads(
        metrics(
            {
                "gradient_mm": 5.0,
                "curves": _metric_curves(),
                "branch": {
                    "index": 1,
                },
            }
        )
    )
    values = {item["metric"]: item for item in response["metrics"]}
    assert len(values) == 24
    assert values["bump_steer_front_left_deg_per_mm"]["value"] == pytest.approx(
        0.01
    )
    assert values["bump_camber_front_left_deg_per_mm"][
        "value"
    ] == pytest.approx(-0.02)
    assert values["suspension_rate_rear_right_n_per_mm"][
        "value"
    ] == pytest.approx(25.0)
    # Pad travel is 1.1 x wheel-center travel: ride = 25 / 1.1, tire = 25 / 0.1.
    assert values["ride_rate_rear_right_n_per_mm"]["value"] == pytest.approx(
        25.0 / 1.1
    )
    assert values["tire_rate_rear_right_n_per_mm"]["value"] == pytest.approx(
        250.0
    )
    ride = values["ride_rate_rear_right_n_per_mm"]
    assert (ride["lower"], ride["upper"]) == pytest.approx(
        (3000.0 - 5.0 * 25.0 / 1.1, 3000.0 + 5.0 * 25.0 / 1.1)
    )
    assert values["wheel_recession_angle_front_left_deg"][
        "value"
    ] == pytest.approx(degrees(atan(0.1)))
    toe = values["bump_steer_front_left_deg_per_mm"]
    assert (toe["lower"], toe["upper"]) == pytest.approx((-0.05, 0.05))


def test_metrics_report_unavailable_reasons() -> None:
    curves = json.loads(_metric_curves())
    curves = [item for item in curves if item["signal"] != "toe"]
    response = json.loads(
        metrics(
            {
                "gradient_mm": 20.0,
                "curves": json.dumps(curves),
                "branch": {
                    "index": 1,
                },
            }
        )
    )
    values = {item["metric"]: item for item in response["metrics"]}
    toe = values["bump_steer_front_left_deg_per_mm"]
    assert toe["value"] is None
    assert toe["reason"] == "missing canonical signal front_left_toe"
    camber = values["bump_camber_front_left_deg_per_mm"]
    assert camber["value"] is None
    assert "does not cover wheel_center_z travel -20..20 mm" in camber["reason"]


def test_numeric_kernel_import_does_not_require_matplotlib() -> None:
    result = subprocess.run(
        [
            sys.executable,
            "-c",
            "import sys; import chassis_lab.knc_vertical_kernel; "
            "assert 'matplotlib' not in sys.modules",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stderr


def test_kernel_evaluates_normalized_curves() -> None:
    curve = {
        "baseline": {"travel": [-1.0, 0.0, 1.0], "values": [-2.0, 0.0, 2.0]},
        "candidate": {"travel": [-1.0, 0.0, 1.0], "values": [-1.5, 0.5, 2.5]},
    }
    response = json.loads(
        evaluate(
            {
                "grid": [-1.0, 0.0, 1.0],
                "curves": json.dumps(
                    {
                        f"{corner}:{axis}": curve
                        for corner in (
                            "front_left",
                            "front_right",
                            "rear_left",
                            "rear_right",
                        )
                        for axis in ("x", "y", "z")
                    }
                ),
            }
        )
    )
    assert len(response["comparisons"]) == 12
    assert response["comparisons"][0]["baseline_error_mm"] == 0.5
    assert response["comparisons"][0]["maximum_error_mm"] == 0.0


def test_kernel_comparison_is_deterministic() -> None:
    curve = {
        "baseline": {
            "travel": [-1000.0, 0.0, 1000.0],
            "values": [-1e6, 0.0, 1e6],
        },
        "candidate": {
            "travel": [-1000.0, 0.0, 1000.0],
            "values": [-1e6, 0.0, 1e6],
        },
    }
    request = {
        "grid": [-1000.0, 0.0, 1000.0],
        "curves": json.dumps(
            {
                f"{corner}:{axis}": curve
                for corner in (
                    "front_left",
                    "front_right",
                    "rear_left",
                    "rear_right",
                )
                for axis in ("x", "y", "z")
            }
        ),
    }
    assert evaluate(request) == evaluate(request)


def test_plot_groups_direct_vertical_corner_panels() -> None:
    curves = [
        {
            "corner": corner,
            "signal": signal,
            "values": [-1.0, 0.0, 1.0],
            "unit": "mm",
        }
        for corner in (
            "front_left",
            "front_right",
            "rear_left",
            "rear_right",
        )
        for signal in ("wheel_center_x", "wheel_center_y", "wheel_center_z")
    ]
    reports = plot(json.dumps(curves))
    assert set(reports) == {
        "plots/knc/vertical/knc_vertical_wcz_vs_wcy.pdf",
        "plots/knc/vertical/knc_vertical_wcz_vs_wcx.pdf",
        "plots/knc/vertical/knc_vertical_recession_angle.pdf",
    }
    assert all(report.startswith(b"%PDF") for report in reports.values())


def test_plot_adds_optional_direct_vertical_curves_and_suspension_rate() -> (
    None
):
    curves = [
        {
            "corner": corner,
            "signal": signal,
            "values": [-1.0, 0.0, 1.0],
            "unit": unit,
        }
        for corner in (
            "front_left",
            "front_right",
            "rear_left",
            "rear_right",
        )
        for signal, unit in (
            ("wheel_center_x", "mm"),
            ("wheel_center_y", "mm"),
            ("wheel_center_z", "mm"),
            ("wheel_load", "newton"),
            ("toe", "deg"),
            ("camber", "deg"),
            ("tire_deflection", "mm"),
        )
    ]
    reports = plot(json.dumps(curves), gradient_points_mm=0.5)
    assert "plots/knc/vertical/knc_vertical_wheel_load.pdf" in reports
    assert "plots/knc/vertical/knc_vertical_suspension_rate.pdf" in reports
    assert "plots/knc/vertical/knc_vertical_toe.pdf" in reports
    assert "plots/knc/vertical/knc_vertical_ride_rate.pdf" in reports
    assert "plots/knc/vertical/knc_vertical_tire_rate.pdf" in reports


def test_comparison_plot_uses_ordered_curve_colors_and_footer() -> None:
    curves = [
        {
            "corner": corner,
            "signal": signal,
            "values": [-1.0, 0.0, 1.0],
            "unit": "mm",
        }
        for corner in ("front_left", "front_right", "rear_left", "rear_right")
        for signal in ("wheel_center_x", "wheel_center_y", "wheel_center_z")
    ]
    curves.extend(
        {
            "corner": corner,
            "signal": signal,
            "values": [-1.0, 0.0, 1.0],
            "unit": unit,
        }
        for corner in ("front_left", "front_right", "rear_left", "rear_right")
        for signal, unit in (
            ("wheel_load", "newton"),
            ("toe", "deg"),
            ("camber", "deg"),
        )
    )
    reports = plot_compare(
        json.dumps(curves),
        json.dumps(
            [
                {"label": f"Curve {index}", "curves": curves}
                for index in range(1, 5)
            ]
        ),
        "Baseline",
        "Baseline: fixture | loadcase | input.h5",
        (
            Path(__file__).resolve().parents[2]
            / "crates/clab-cli/assets/flb-logo.pdf"
        ).read_bytes(),
        0.5,
    )
    assert reports["knc_vertical_wcz_vs_wcy.pdf"].startswith(b"%PDF")
    assert "knc_vertical_toe.pdf" in reports
    assert "knc_vertical_suspension_rate.pdf" in reports
    assert "knc_vertical_recession_angle.pdf" in reports


def _pdf_text(report: bytes) -> str:
    return PdfReader(BytesIO(report)).pages[0].extract_text()


def test_gradient_overlay_and_metric_table_only_on_selected_plots() -> None:
    reports = plot(
        _metric_curves(),
        "Simulation",
        "Config: case.toml",
        None,
        5.0,
        json.dumps({"index": 1}),
    )
    tables = {
        "knc_vertical_wheel_load": "Left front suspension rate in N/mm:",
        "knc_vertical_camber": "Right front bump camber in deg/mm:",
        "knc_vertical_toe": "Left rear bump steer in deg/mm:",
        "knc_vertical_wcz_vs_wcx": "Right rear wheel recession angle in deg:",
    }
    for path, report in reports.items():
        text = _pdf_text(report)
        name = Path(path).stem
        if name in tables:
            assert tables[name] in text
            assert "Simulation" in text
        else:
            assert "secant between" not in text


def test_metric_table_lists_every_source_with_metric_values() -> None:
    reports = plot_compare(
        _metric_curves(),
        json.dumps(
            [
                {
                    "label": "Curve_1",
                    "curves": json.loads(
                        _metric_curves(
                            wheel_load=[
                                3000.0 + 50.0 * value
                                for value in (
                                    -10.0,
                                    -5.0,
                                    0.0,
                                    5.0,
                                    10.0,
                                    10.0,
                                    5.0,
                                    0.0,
                                    -5.0,
                                    -10.0,
                                )
                            ]
                        )
                    ),
                }
            ]
        ),
        "Baseline",
        "Config: case.toml",
        None,
        5.0,
        json.dumps([{"index": 1}, {"index": 1}]),
    )
    text = _pdf_text(reports["knc_vertical_wheel_load.pdf"])
    assert "Left front suspension rate in N/mm:" in text
    assert "Baseline 25" in text
    assert "Curve_1 50" in text


def test_branch_detection_discards_short_turnaround_jitter() -> None:
    rise = np.linspace(0.0, 10.0, 51)
    jitter = 10.0 + np.array([0.001, -0.001, 0.002, -0.002, 0.001, -0.001])
    fall = np.linspace(10.0, 0.0, 51)
    travel = np.concatenate([rise, jitter, fall])
    branches = branch_ranges(travel, BranchOptions(measurement=True))
    # The first (partial) measurement sweep is dropped.
    assert [(segment.start, sign) for segment, sign in branches] == [(57, -1)]
