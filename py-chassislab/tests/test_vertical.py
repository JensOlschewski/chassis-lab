import json
import subprocess
import sys
from pathlib import Path

from chassis_lab.knc_vertical_kernel import evaluate, plot, plot_compare


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
