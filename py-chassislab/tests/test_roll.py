import json
from math import atan, radians, sin

import numpy as np
import pytest

from chassis_lab.knc_kernel import index_curves, signal_values
from chassis_lab.knc_roll_kernel import (
    _derive_roll_moments,
    metrics,
    roll_center,
)

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
                _curve(
                    corner,
                    "wheel_load",
                    4000.0 + (30.0 if axle == "front" else 40.0) * z,
                ),
                # Relative to the static contact position, left at -750 mm.
                _curve(
                    corner,
                    lateral,
                    CONTACT_SLOPE[axle] * z,
                    offset=-sign * HALF_TRACK,
                ),
            ]
            if lateral != "contact_patch_y":
                curves.append(
                    _curve(
                        corner,
                        "contact_patch_y",
                        CONTACT_SLOPE[axle] * z,
                        offset=-sign * HALF_TRACK,
                    )
                )
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
    ] == pytest.approx(40.0)
    assert values["roll_stiffness_front_nm_per_deg"]["value"] == pytest.approx(
        450.0
    )
    assert values["roll_stiffness_total_nm_per_deg"]["value"] == pytest.approx(
        1050.0
    )
    assert values["roll_stiffness_distribution_front_pct"][
        "value"
    ] == pytest.approx(100.0 * 450.0 / 1050.0)
    steer = values["roll_steer_front_left_deg_per_deg"]
    assert steer["interval_deg"] == [-1.0, 1.0]
    assert values["roll_steer_front_left_deg_per_mm"]["interval_mm"] == [
        -10.0,
        10.0,
    ]


def _with_global_wheel_centers(curves):
    for axle, ratio in (("front", 0.9), ("rear", 0.8)):
        # Rotate an axle line by body minus suspension roll. Track varies;
        # common translations must not affect the relative front-view angle.
        tilt = np.deg2rad((1.0 - ratio) * ROLL)
        radius = HALF_TRACK + 5.0 * ROLL
        for side, sign in (("left", -1.0), ("right", 1.0)):
            corner = f"{axle}_{side}"
            curves += [
                _curve(
                    corner,
                    "wheel_center_global_y",
                    12.0 * ROLL + sign * radius * np.cos(tilt),
                ),
                _curve(
                    corner,
                    "wheel_center_global_z",
                    400.0 + 3.0 * ROLL + sign * radius * np.sin(tilt),
                ),
            ]
    return curves


def test_suspension_roll_derived_from_moving_axle_geometry() -> None:
    explicit, _ = _metrics(_curves())
    curves = [c for c in _curves() if c["signal"] != "suspension_roll_angle"]
    derived, warnings = _metrics(_with_global_wheel_centers(curves))
    for name, row in explicit.items():
        if name.endswith("deg_per_deg"):
            assert derived[name]["value"] == pytest.approx(row["value"])
    assert sum("suspension roll derived" in w for w in warnings) == 2


def test_explicit_suspension_roll_takes_priority_over_geometry() -> None:
    curves = _with_global_wheel_centers(_curves())
    for c in curves:
        if c["signal"] == "wheel_center_global_z":
            c["values"] = np.zeros_like(ROLL).tolist()
    values, warnings = _metrics(curves)
    assert values["roll_steer_front_left_deg_per_deg"][
        "value"
    ] == pytest.approx(0.1 / 0.9)
    assert warnings == []


def test_invalid_suspension_roll_geometry_does_not_produce_a_metric() -> None:
    curves = _with_global_wheel_centers(
        [c for c in _curves() if c["signal"] != "suspension_roll_angle"]
    )
    for c in curves:
        if c["signal"] == "wheel_center_global_y":
            c["values"] = np.zeros_like(ROLL).tolist()
    values, warnings = _metrics(curves)
    assert values["roll_steer_front_left_deg_per_deg"]["value"] is None
    assert any("invalid wheel-center geometry" in w for w in warnings)


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
        if item["signal"]
        not in ("center_of_pressure_y", "contact_patch_y", "toe")
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


def test_total_roll_stiffness_and_distribution_use_derived_axle_sum() -> None:
    values, _ = _metrics(_curves())
    assert values["roll_stiffness_total_nm_per_deg"]["value"] == pytest.approx(
        1050.0
    )
    assert values["roll_stiffness_distribution_front_pct"][
        "value"
    ] == pytest.approx(100.0 * 450.0 / 1050.0)


def test_axle_moments_use_instantaneous_track_and_ignore_source_moments():
    curves = _curves("contact_patch_y")
    curves += [
        _curve(location, "roll_moment", np.full_like(ROLL, 99999.0))
        for location in ("front", "rear", "total")
    ]
    slopes = {
        "front_left": 400.0,
        "front_right": -200.0,
        "rear_left": 300.0,
        "rear_right": -100.0,
    }
    for c in curves:
        corner = str(c["corner"])
        if c["signal"] == "wheel_load":
            c["values"] = (2000.0 + slopes[corner] * ROLL).tolist()
            c["offset"] = 1000.0 if corner.endswith("left") else 500.0
        elif c["signal"] == "contact_patch_y":
            half_track = 750.0 if corner.startswith("front") else 900.0
            sign = -1.0 if corner.endswith("left") else 1.0
            c["offset"] = sign * half_track
            c["values"] = (sign * 100.0 * ROLL).tolist()
    values, warnings = _metrics(curves)
    expected = {
        "front": 500.0,
        "rear": 410.0,
    }
    for axle, stiffness in expected.items():
        assert values[f"roll_stiffness_{axle}_nm_per_deg"][
            "value"
        ] == pytest.approx(stiffness)
    assert values["roll_stiffness_total_nm_per_deg"]["value"] == pytest.approx(
        910.0
    )
    assert values["roll_stiffness_distribution_front_pct"][
        "value"
    ] == pytest.approx(100.0 * expected["front"] / 910.0)
    assert not any("roll moment unavailable" in w for w in warnings)


@pytest.mark.parametrize(
    "track",
    [0.0, -1500.0, float("nan"), float("inf")],
)
def test_axle_moments_reject_invalid_instantaneous_track(track):
    curves = _curves("contact_patch_y")
    for c in curves:
        if c["signal"] == "contact_patch_y" and str(c["corner"]).startswith(
            "front"
        ):
            right = str(c["corner"]).endswith("right")
            c["values"] = np.full_like(ROLL, track if right else 0.0).tolist()
            c.pop("offset", None)
    values, warnings = _metrics(curves)
    assert values["roll_stiffness_front_nm_per_deg"]["value"] is None
    assert values["roll_stiffness_total_nm_per_deg"]["value"] is None
    assert values["roll_stiffness_rear_nm_per_deg"]["value"] is not None
    assert any("instantaneous contact track" in w for w in warnings)


@pytest.mark.parametrize("signal", ["wheel_load", "contact_patch_y"])
def test_axle_moments_reject_mismatched_input_lengths(signal):
    curves = _curves("contact_patch_y")
    for c in curves:
        if c["signal"] == signal and c["corner"] == "front_left":
            c["values"] = []
    values, warnings = _metrics(curves)
    assert values["roll_stiffness_front_nm_per_deg"]["value"] is None
    assert any("roll-angle lengths differ" in w for w in warnings)


@pytest.mark.parametrize("relative", [False, True])
def test_moments_match_exported_measurement_samples_for_both_input_forms(
    relative,
):
    curves = [
        _curve("total", "roll_angle", np.array([-0.0004512117, -0.0039231931]))
    ]
    samples = {
        "front_left": ([4411.1849, 4403.7889], [-745.61386, -745.59392]),
        "front_right": ([4201.808, 4210.6962], [747.61108, 747.62909]),
        "rear_left": ([3731.018, 3722.1159], [-728.89665, -728.87053]),
        "rear_right": ([3742.695, 3752.111], [726.93068, 726.96112]),
    }
    for corner, (force, position) in samples.items():
        for signal, values, offset in (
            ("wheel_load", force, 9000.0),
            (
                "contact_patch_y",
                position,
                0.04 if corner.endswith("left") else -0.03,
            ),
        ):
            offset = offset if relative else 0.0
            curves.append(
                _curve(
                    corner,
                    signal,
                    np.asarray(values) - offset,
                    offset=offset if relative else None,
                )
            )
    indexed = index_curves(curves)
    warnings = []
    _derive_roll_moments(indexed, warnings)
    assert warnings == []
    assert signal_values(indexed, "front", "roll_moment") == pytest.approx(
        [156.3242, 144.1656], abs=0.001
    )
    assert signal_values(indexed, "rear", "roll_moment") == pytest.approx(
        [-8.4994936, -21.83374], abs=0.001
    )
    assert signal_values(indexed, "total", "roll_moment") == pytest.approx(
        [147.82471, 122.3319], abs=0.002
    )


def test_missing_contact_geometry_does_not_substitute_center_of_pressure():
    curves = [c for c in _curves() if c["signal"] != "contact_patch_y"]
    values, warnings = _metrics(curves)
    assert values["roll_stiffness_total_nm_per_deg"]["value"] is None
    assert any(
        "missing canonical signal front_left_contact_patch_y" in w
        for w in warnings
    )
