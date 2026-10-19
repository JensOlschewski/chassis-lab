import json
from math import atan, radians, sin

import numpy as np
import pytest

from chassis_lab.knc_roll_kernel import metrics, roll_center

# Two and a half roll cycles of +-3 deg: rising and falling sweep 2 both cover
# the full amplitude.
ROLL = 3.0 * np.sin(np.linspace(0.0, 5.0 * np.pi, 501))
TRAVEL_PER_DEG = 10.0  # [mm/deg] wheel-center travel, left up
HALF_TRACK = 750.0
# d(lateral contact position)/d(wheel pad z) per axle.
CONTACT_SLOPE = {"front": 0.1, "rear": 0.2}


def _curve(corner: str, signal: str, values: np.ndarray, offset=None):
    item = {
        "corner": corner,
        "signal": signal,
        "unit": "",
        "values": np.asarray(values).tolist(),
    }
    if offset is not None:
        item["offset"] = offset
    return item


def _curves(lateral: str = "center_of_pressure_y") -> list[dict[str, object]]:
    curves = [
        _curve("total", "roll_angle", ROLL),
        _curve("front", "suspension_roll_angle", 0.9 * ROLL),
        _curve("rear", "suspension_roll_angle", 0.8 * ROLL),
        _curve("front", "roll_moment", 500.0 * ROLL),
        _curve("rear", "roll_moment", 700.0 * ROLL),
        _curve("total", "roll_moment", 1200.0 * ROLL),
    ]
    for axle in ("front", "rear"):
        for side, sign in (("left", 1.0), ("right", -1.0)):
            corner = f"{axle}_{side}"
            z = sign * TRAVEL_PER_DEG * ROLL
            toe = 0.01 if axle == "front" else 0.02
            curves += [
                _curve(corner, "wheel_center_z", z),
                _curve(corner, "wheel_pad_z", z),
                _curve(corner, "toe", toe * z),
                _curve(corner, "camber", -0.03 * z),
                _curve(corner, "wheel_load", 4000.0 + 30.0 * z),
                # Relative to the static contact position, left at -750 mm.
                _curve(
                    corner,
                    lateral,
                    CONTACT_SLOPE[axle] * z,
                    offset=-sign * HALF_TRACK,
                ),
            ]
    return curves


def _metrics(
    curves: list[dict[str, object]], **request: object
) -> tuple[dict[str, dict[str, object]], list[str]]:
    response = json.loads(
        metrics(
            {
                "gradient_mm": 10.0,
                "gradient_deg": 1.0,
                "curves": json.dumps(curves),
                "branch": {"index": 2},
                "arb_connected": json.dumps({"front": False, "rear": True}),
                **request,
            }
        )
    )
    values = {item["metric"]: item for item in response["metrics"]}
    return values, response["warnings"]


def test_roll_secant_metrics_follow_linear_fixture() -> None:
    values, warnings = _metrics(_curves())
    assert warnings == []
    assert values["roll_steer_front_left_deg_per_mm"]["value"] == pytest.approx(
        0.01
    )
    assert values["roll_steer_rear_right_deg_per_mm"]["value"] == pytest.approx(
        0.02
    )
    # Front toe 0.01 deg/mm * 10 mm/deg over 0.9 deg suspension roll per deg.
    assert values["roll_steer_front_left_deg_per_deg"][
        "value"
    ] == pytest.approx(0.1 / 0.9)
    assert values["roll_steer_rear_right_deg_per_deg"][
        "value"
    ] == pytest.approx(-0.2 / 0.8)
    assert values["roll_camber_front_right_deg_per_mm"][
        "value"
    ] == pytest.approx(-0.03)
    assert values["roll_wheel_rate_rear_left_n_per_mm"][
        "value"
    ] == pytest.approx(30.0)
    assert values["roll_stiffness_front_nm_per_deg"]["value"] == pytest.approx(
        500.0
    )
    assert values["roll_stiffness_total_nm_per_deg"]["value"] == pytest.approx(
        1200.0
    )
    assert values["roll_stiffness_distribution_front_pct"][
        "value"
    ] == pytest.approx(100.0 * 500.0 / 1200.0)
    steer = values["roll_steer_front_left_deg_per_deg"]
    assert steer["interval_deg"] == [-1.0, 1.0]
    assert values["roll_steer_front_left_deg_per_mm"]["interval_mm"] == [
        -10.0,
        10.0,
    ]


def test_roll_wheel_rate_without_arb_follows_configured_arb_state() -> None:
    values, _ = _metrics(_curves())
    front = values["roll_wheel_rate_without_arb_front_left_n_per_mm"]
    assert front["value"] == pytest.approx(30.0)
    rear = values["roll_wheel_rate_without_arb_rear_left_n_per_mm"]
    assert rear["value"] is None
    assert rear["reason"] == "rear anti-roll bar is connected"
    values, _ = _metrics(_curves(), arb_connected=json.dumps({"front": True}))
    rear = values["roll_wheel_rate_without_arb_rear_left_n_per_mm"]
    assert rear["reason"] == "rear_arb_connected is not configured"


def test_roll_center_matches_contact_path_geometry_per_axle() -> None:
    values, _ = _metrics(_curves())
    # Symmetric contact paths at angle theta: rc_z = track / 2 * tan(theta)
    # at 0 deg roll, on the vehicle center line.
    for axle, slope in CONTACT_SLOPE.items():
        height = values[f"roll_center_height_{axle}_mm"]
        assert height["value"] == pytest.approx(HALF_TRACK * slope, rel=1e-3)
        assert height["lateral_signal"] == "center_of_pressure_y"
        migration = values[f"roll_center_lateral_migration_{axle}_mm_per_deg"]
        assert migration["lower"] is not None
    change = values["roll_center_height_change_front_mm_per_deg"]
    assert change["interval_deg"] == [-1.0, 1.0]


def test_roll_center_formula_reproduces_tilted_triangle() -> None:
    theta = atan(0.1)
    roll = radians(2.0)
    height, position = roll_center(
        np.array([0.1]),
        np.array([-0.1]),
        np.array([-750.0]),
        np.array([0.0]),
        np.array([750.0]),
        np.array([0.0]),
        np.array([roll]),
    )
    alpha, beta = theta + roll, theta - roll
    expected = 1500.0 * sin(alpha) * sin(beta) / sin(np.pi - alpha - beta)
    assert height[0] == pytest.approx(expected)
    assert position[0] == pytest.approx(-750.0 + expected / np.tan(alpha))


def test_roll_center_falls_back_to_contact_patch_with_warning() -> None:
    values, warnings = _metrics(_curves("contact_patch_y"))
    height = values["roll_center_height_rear_mm"]
    assert height["value"] == pytest.approx(150.0, rel=1e-3)
    assert height["lateral_signal"] == "contact_patch_y"
    assert warnings == [
        "front roll center uses contact_patch_y; center_of_pressure_y is not "
        "mapped",
        "rear roll center uses contact_patch_y; center_of_pressure_y is not "
        "mapped",
    ]


def test_roll_metrics_report_unavailable_reasons() -> None:
    curves = [
        item
        for item in _curves()
        if item["signal"] not in ("center_of_pressure_y", "toe")
        and not (item["corner"] == "total" and item["signal"] == "roll_moment")
    ]
    values, _ = _metrics(curves, gradient_deg=5.0)
    assert values["roll_steer_front_left_deg_per_mm"]["reason"] == (
        "missing canonical signal front_left_toe"
    )
    assert "does not cover suspension_roll_angle travel -5..5 deg" in str(
        values["roll_camber_front_left_deg_per_deg"]["reason"]
    )
    assert values["roll_stiffness_distribution_front_pct"]["reason"] == (
        "front and total roll stiffness unavailable"
    )
    assert values["roll_center_height_front_mm"]["reason"] == (
        "missing canonical signal front_left_center_of_pressure_y or "
        "front_left_contact_patch_y"
    )


def test_roll_stiffness_distribution_falls_back_to_axle_sum() -> None:
    curves = [
        item
        for item in _curves()
        if not (item["corner"] == "total" and item["signal"] == "roll_moment")
    ]
    values, _ = _metrics(curves)
    assert values["roll_stiffness_total_nm_per_deg"]["value"] is None
    assert values["roll_stiffness_distribution_front_pct"][
        "value"
    ] == pytest.approx(100.0 * 500.0 / 1200.0)
