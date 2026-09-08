import json
import re

from chassis_lab.vertical_kernel import evaluate


def test_kernel_evaluates_normalized_curves() -> None:
    curve = {
        "measurement_travel": [-1.0, 0.0, 1.0, 1.0, 0.0, -1.0],
        "measurement_cycle": [2.0, 2.0, 2.0, 3.0, 3.0, 3.0],
        "measurement": [-2.0, 0.0, 2.0, 2.0, 0.0, -2.0],
        "simulation_travel": [0.0, -1.0, 0.0, 1.0, 0.0, -1.0],
        "simulation": [0.5, -1.5, 0.5, 2.5, 0.5, -1.5],
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
    assert response["svg"].startswith("<svg")


def test_kernel_svg_is_deterministic_and_scales_large_curves() -> None:
    curve = {
        "measurement_travel": [-1000.0, 0.0, 1000.0, 1000.0, 0.0, -1000.0],
        "measurement_cycle": [2.0, 2.0, 2.0, 3.0, 3.0, 3.0],
        "measurement": [-1e6, 0.0, 1e6, 1e6, 0.0, -1e6],
        "simulation_travel": [-1000.0, 0.0, 1000.0],
        "simulation": [-1e6, 0.0, 1e6],
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
    first = json.loads(evaluate(request))["svg"]
    assert first == json.loads(evaluate(request))["svg"]
    points = re.findall(r'points="([^"]+)"', first)
    for point_list in points:
        for point in point_list.split():
            x, y = map(float, point.split(","))
            assert 0.0 <= x <= 900.0
            assert 0.0 <= y <= 650.0
