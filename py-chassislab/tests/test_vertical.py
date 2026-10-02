import json
import subprocess
import sys
from math import atan, degrees
from pathlib import Path

import pytest

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


def test_metrics_use_secants_at_symmetric_travel_points() -> None:
    response = json.loads(
        metrics({"gradient_mm": 5.0, "curves": _metric_curves()})
    )
    values = {item["metric"]: item for item in response["metrics"]}
    assert len(values) == 24
    assert values["front_left_bump_steer_deg_per_mm"]["value"] == pytest.approx(
        0.01
    )
    assert values["front_left_bump_camber_deg_per_mm"][
        "value"
    ] == pytest.approx(-0.02)
    assert values["rear_right_wheel_rate_n_per_mm"]["value"] == pytest.approx(
        25.0
    )
    assert values["front_left_wheel_center_x_change_mm"][
        "value"
    ] == pytest.approx(-1.0)
    assert values["front_left_wheel_center_y_change_mm"][
        "value"
    ] == pytest.approx(2.0)
    assert values["front_left_wheel_recession_angle_deg"][
        "value"
    ] == pytest.approx(degrees(atan(0.1)))
    toe = values["front_left_bump_steer_deg_per_mm"]
    assert (toe["lower"], toe["upper"]) == pytest.approx((-0.05, 0.05))


def test_metrics_report_unavailable_reasons() -> None:
    curves = json.loads(_metric_curves())
    curves = [item for item in curves if item["signal"] != "toe"]
    response = json.loads(
        metrics({"gradient_mm": 20.0, "curves": json.dumps(curves)})
    )
    values = {item["metric"]: item for item in response["metrics"]}
    toe = values["front_left_bump_steer_deg_per_mm"]
    assert toe["value"] is None
    assert toe["reason"] == "missing canonical signal front_left_toe"
    camber = values["front_left_bump_camber_deg_per_mm"]
    assert camber["value"] is None
    assert "does not cover wheel-center travel -20..20 mm" in camber["reason"]


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
    reports = json.loads(plot(json.dumps(curves)))
    assert set(reports) == {
        "plots/knc/vertical/wheel-center-z-vs-wheel-center-y.svg",
        "plots/knc/vertical/wheel-center-y-vs-wheel-center-x.svg",
        "plots/knc/vertical/wheel-center-z-vs-wheel-center-x.svg",
    }
    assert (
        "Simulation"
        in reports["plots/knc/vertical/wheel-center-z-vs-wheel-center-y.svg"]
    )


def test_plot_adds_optional_direct_vertical_curves_and_wheel_rate() -> None:
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
        )
    ]
    reports = json.loads(plot(json.dumps(curves)))
    assert "plots/knc/vertical/wheel-load-vs-wheel-center-z.svg" in reports
    assert "plots/knc/vertical/wheel-rate-vs-wheel-center-z.svg" in reports
    assert (
        "[newton/mm]"
        in reports["plots/knc/vertical/wheel-rate-vs-wheel-center-z.svg"]
    )


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
    reports = json.loads(
        plot_compare(
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
                / "crates/clab-cli/assets/flb-logo.png"
            ).read_bytes(),
        )
    )
    svg = reports["wheel-center-z-vs-wheel-center-y.svg"]
    for color in ("#000000", "#e41a1c", "#377eb8", "#e7298a", "#00a6a6"):
        assert color in svg
    assert "Baseline: fixture | loadcase | input.h5" in svg
    assert "Generated:" in svg
