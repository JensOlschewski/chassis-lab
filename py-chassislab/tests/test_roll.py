import json
from io import BytesIO
from math import atan, radians, sin
from pathlib import Path

import numpy as np
import pytest
from pypdf import PdfReader

from chassis_lab.knc_kernel import BranchOptions, index_curves, signal_values
from chassis_lab.knc_plot_kernel import _curve_xy
from chassis_lab.knc_roll_kernel import (
    PLOT_SPECS,
    _derive_roll_moments,
    metrics,
    plot,
    plot_compare,
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


def _plot_fixture() -> list[dict[str, object]]:
    curves = _curves()
    for axle in ("front", "rear"):
        for side in ("left", "right"):
            corner = f"{axle}_{side}"
            for signal in (
                "wheel_center_x",
                "wheel_center_y",
                "contact_patch_x",
            ):
                curves.append(_curve(corner, signal, ROLL))
    return curves


def _pdf_text(data: bytes) -> str:
    return PdfReader(BytesIO(data)).pages[0].extract_text()


def test_roll_corner_plot_catalog_and_secant_axes() -> None:
    reports = plot(
        json.dumps(_plot_fixture()),
        gradient_points_mm=10.0,
        gradient_points_deg=0.9,
        branch=json.dumps({"index": 2}),
    )
    assert set(reports) == {
        f"plots/knc/roll/knc_roll_{name}.pdf"
        for name in (
            "toe",
            "toe_vs_roll_angle",
            "toe_vs_suspension_roll_angle",
            "camber",
            "camber_vs_roll_angle",
            "camber_vs_suspension_roll_angle",
            "wheel_load",
            "suspension_rate",
            "ride_rate",
            "wheel_load_vs_roll_angle",
            "wheel_load_vs_suspension_roll_angle",
            "wcz_vs_wcx",
            "wcz_vs_wcy",
            "wpz_vs_cpx",
            "wpz_vs_cpy",
        )
    }
    assert all(report.startswith(b"%PDF") for report in reports.values())
    travel = _pdf_text(reports["plots/knc/roll/knc_roll_toe.pdf"])
    assert "Left front roll steer in deg/mm:" in travel
    assert "Simulation 0.01" in travel
    angle = _pdf_text(
        reports["plots/knc/roll/knc_roll_toe_vs_suspension_roll_angle.pdf"]
    )
    assert "Left front roll steer in deg/deg:" in angle
    assert "-0.9 deg and +0.9 deg suspension roll angle" in angle
    assert "Simulation 0.1111" in angle
    body = _pdf_text(reports["plots/knc/roll/knc_roll_toe_vs_roll_angle.pdf"])
    assert "Left front body-roll steer in deg/deg:" in body
    assert "-0.9 deg and +0.9 deg body roll angle" in body
    assert "Simulation 0.1" in body
    assert "body-roll camber in deg/deg:" in _pdf_text(
        reports["plots/knc/roll/knc_roll_camber_vs_roll_angle.pdf"]
    )
    assert "Roll suspension rate [newton/mm]" in _pdf_text(
        reports["plots/knc/roll/knc_roll_suspension_rate.pdf"]
    )
    assert "Roll ride rate [newton/mm]" in _pdf_text(
        reports["plots/knc/roll/knc_roll_ride_rate.pdf"]
    )


def test_roll_rate_debug_uses_body_roll_sweeps() -> None:
    curves = _plot_fixture()
    info = json.dumps(
        {
            "config": "fixture.toml",
            "creator": "test",
            "version": "0",
            "inputs": ["roll.h5"],
        }
    )
    reports = plot(
        json.dumps(curves),
        branch=json.dumps({"measurement": True}),
        debug=info,
    )
    for name in ("suspension_rate", "ride_rate"):
        path = f"plots/knc/roll/debug/measurement_knc_roll_{name}.pdf"
        assert path in reports
        text = _pdf_text(reports[path])
        assert "roll_angle [deg]" in text
        assert "roll.h5" in text

    compared = plot_compare(
        json.dumps(curves),
        json.dumps([{"label": "Candidate", "curves": curves}]),
        "Baseline",
        "Config: fixture",
        (
            Path(__file__).resolve().parents[2]
            / "crates/clab-cli/assets/flb-logo.pdf"
        ).read_bytes(),
        branches_json=json.dumps([{"measurement": True}] * 2),
        debug=json.dumps(
            {**json.loads(info), "inputs": ["baseline.h5", "candidate.h5"]}
        ),
    )
    for index in (0, 1):
        assert f"debug/series_{index}_knc_roll_ride_rate.pdf" in compared
    assert "knc_roll_ride_rate.pdf" in compared


def test_roll_local_rates_differentiate_within_body_roll_sweeps() -> None:
    curves = index_curves(_plot_fixture())
    for name in ("suspension_rate", "ride_rate"):
        spec = next(
            spec for spec in PLOT_SPECS if spec.name == f"knc_roll_{name}"
        )
        for corner, expected in (("front_left", 30.0), ("rear_right", 40.0)):
            _, rate = _curve_xy(
                spec,
                curves,
                corner,
                BranchOptions(plot_branches="selected", index=2),
            )
            assert np.isfinite(rate).any()
            assert rate[np.isfinite(rate)] == pytest.approx(expected)


def test_roll_optional_plots_skip_missing_signals_in_analyze_and_compare() -> (
    None
):
    curves = _curves()
    with pytest.warns(UserWarning, match="knc_roll_wcz_vs_wcx"):
        reports = plot(json.dumps(curves))
    assert "plots/knc/roll/knc_roll_wcz_vs_wcx.pdf" not in reports
    assert "plots/knc/roll/knc_roll_toe.pdf" in reports
    candidate = [curve for curve in curves if curve["signal"] != "toe"]
    with pytest.warns(UserWarning, match="knc_roll_toe"):
        compared = plot_compare(
            json.dumps(curves),
            json.dumps([{"label": "Curve 1", "curves": candidate}]),
            "Baseline",
            "Config: fixture",
            (
                Path(__file__).resolve().parents[2]
                / "crates/clab-cli/assets/flb-logo.pdf"
            ).read_bytes(),
            branches_json=json.dumps([{}, {}]),
        )
    assert "knc_roll_toe.pdf" not in compared
    assert "knc_roll_camber.pdf" in compared
    assert "Baseline" in _pdf_text(compared["knc_roll_camber.pdf"])
    assert "Curve 1" in _pdf_text(compared["knc_roll_camber.pdf"])


def test_roll_plots_derive_missing_suspension_angle_from_global_geometry() -> (
    None
):
    curves = [
        item
        for item in _plot_fixture()
        if item["signal"] != "suspension_roll_angle"
    ]
    for axle in ("front", "rear"):
        for side, sign in (("left", -1), ("right", 1)):
            corner = f"{axle}_{side}"
            curves.extend(
                (
                    _curve(
                        corner,
                        "wheel_center_global_y",
                        np.full_like(ROLL, sign * HALF_TRACK),
                    ),
                    _curve(corner, "wheel_center_global_z", sign * ROLL),
                )
            )
    reports = plot(json.dumps(curves), gradient_points_deg=1.0)
    assert "plots/knc/roll/knc_roll_toe_vs_suspension_roll_angle.pdf" in reports


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


def test_raw_roll_center_uses_actual_sample_angles_and_span() -> None:
    values, _ = _metrics(_curves())
    row = values["roll_center_height_change_front_mm_per_deg"]
    lower_angle, zero_angle, upper_angle = row["sampled_roll_angle_deg"]
    assert lower_angle in ROLL
    assert zero_angle in ROLL
    assert upper_angle in ROLL
    assert upper_angle != 1.0
    assert row["value"] == pytest.approx(
        (row["upper"] - row["lower"]) / (upper_angle - lower_angle)
    )


def test_raw_roll_center_does_not_average_opposite_sweep() -> None:
    from chassis_lab.knc_kernel import BranchOptions, metric_sweep

    curves = _curves()
    original, _ = _metrics(curves)
    selected = metric_sweep(ROLL, "total_roll_angle", BranchOptions(index=2))
    for curve in curves:
        if curve["signal"] == "center_of_pressure_y":
            samples = np.asarray(curve["values"])
            # Change every other sweep while keeping the selected one intact.
            outside = np.ones(len(ROLL), dtype=bool)
            outside[selected] = False
            samples[outside] *= 3.0
            curve["values"] = samples.tolist()
    changed, _ = _metrics(curves)
    for name, row in original.items():
        if name.startswith("roll_center_"):
            assert changed[name] == row


def test_raw_roll_center_rejects_nonfinite_selected_input() -> None:
    curves = _curves()
    for curve in curves:
        if curve["signal"] == "center_of_pressure_y":
            curve["values"][200] = float("nan")
    values, _ = _metrics(curves)
    row = values["roll_center_height_front_mm"]
    assert row["value"] is None
    assert "nonfinite input" in row["reason"]


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
