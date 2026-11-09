import json
from io import BytesIO

import numpy as np
import pytest
from pypdf import PdfReader

from chassis_lab.knc_longitudinal_kernel import (
    PLOT_SPECS,
    metrics,
    plot,
    plot_jobs,
)

CORNERS = ("front_left", "front_right", "rear_left", "rear_right")


def _force(leg=400):
    """Rest, then -4 kN, +4 kN, -4 kN, +4 kN, 0: four clean branches."""
    peaks = [0.0, -4000.0, 4000.0, -4000.0, 4000.0, 0.0]
    return np.concatenate(
        [
            np.linspace(a, b, leg, endpoint=False)
            for a, b in zip(peaks[:-1], peaks[1:], strict=True)
        ]
    )


def _curves(slopes, force=None, signs=None):
    """Linear responses per corner with per-signal slope per kN."""
    force = _force() if force is None else force
    curves = []
    for corner in CORNERS:
        curves.append(
            {"corner": corner, "signal": "force_x", "values": force.tolist()}
        )
        for signal, slope in slopes.items():
            curves.append(
                {
                    "corner": corner,
                    "signal": signal,
                    "values": (slope * force / 1000.0).tolist(),
                }
            )
    return curves


def _request(curves, brakes):
    return {
        "curves": json.dumps(curves),
        "branch": json.dumps({"measurement": True}),
        "brakes": brakes,
    }


def _by_name(raw):
    return {
        row["metric"]: row
        for row in json.loads(raw)["metrics"]
        if row.get("status") != "not_covered"
    }


def test_acceleration_reports_longitudinal_compliance_per_kn():
    result = _by_name(
        metrics(_request(_curves({"wheel_center_x": 2.0}), brakes=False))
    )
    assert set(result) == {
        f"longitudinal_compliance_{corner}_mm_per_kn" for corner in CORNERS
    }
    row = result["longitudinal_compliance_rear_left_mm_per_kn"]
    assert row["value"] == pytest.approx(2.0)
    assert row["unit"] == "mm/kN"
    assert row["interval_N"] == [-1000.0, 1000.0]
    assert (row["lower"], row["upper"]) == pytest.approx((-2.0, 2.0))


def test_braking_reports_brake_steer_and_windup_with_sign():
    curves = _curves({"toe": 0.13, "spin": -0.64})
    result = _by_name(metrics(_request(curves, brakes=True)))
    assert len(result) == 2 * len(CORNERS)
    assert result["brake_steer_front_left_deg_per_kn"]["value"] == (
        pytest.approx(0.13)
    )
    assert result["windup_compliance_rear_right_deg_per_kn"]["value"] == (
        pytest.approx(-0.64)
    )


def test_secant_uses_the_configured_sweep_and_keeps_hysteresis():
    force = _force()
    sign = np.sign(np.gradient(force))
    curves = []
    for corner in CORNERS:
        curves += [
            {"corner": corner, "signal": "force_x", "values": force.tolist()},
            {
                "corner": corner,
                "signal": "wheel_center_x",
                "values": (force / 1000.0 + 0.3 * sign).tolist(),
            },
        ]
    request = _request(curves, brakes=False)
    rising = _by_name(metrics(request))
    request["branch"] = json.dumps(
        {"measurement": True, "direction": "fall", "index": 1}
    )
    falling = _by_name(metrics(request))
    name = "longitudinal_compliance_front_left_mm_per_kn"
    # Offset cancels in the secant; both branches have the same slope.
    assert rising[name]["value"] == pytest.approx(1.0)
    assert falling[name]["value"] == pytest.approx(1.0)
    assert rising[name]["lower"] == pytest.approx(-0.7, abs=2e-2)
    assert falling[name]["lower"] == pytest.approx(-1.3, abs=2e-2)


@pytest.mark.parametrize(
    ("brakes", "covered", "other", "needs"),
    [
        (False, "longitudinal_compliance", "brake_steer", "B1"),
        (True, "brake_steer", "longitudinal_compliance", "B0"),
    ],
)
def test_other_setup_metrics_are_listed_as_not_covered(
    brakes, covered, other, needs
):
    curves = _curves({"wheel_center_x": 2.0, "toe": 0.1, "spin": -0.6})
    rows = json.loads(metrics(_request(curves, brakes=brakes)))["metrics"]
    for row in rows:
        if row["metric"].startswith(other):
            assert row["value"] is None
            assert row["status"] == "not_covered"
            assert row["needs_setup"] == needs
            assert row["reason"] is None
        elif row["metric"].startswith(covered):
            assert "status" not in row
    assert any(row["metric"].startswith(other) for row in rows)


def test_missing_channel_is_unavailable_with_reason_not_zero():
    curves = [c for c in _curves({"toe": 0.1}) if c["corner"] != "rear_left"]
    result = _by_name(metrics(_request(curves, brakes=True)))
    row = result["windup_compliance_front_left_deg_per_kn"]
    assert row["value"] is None
    assert "missing canonical signal front_left_spin" in row["reason"]
    assert result["brake_steer_front_left_deg_per_kn"]["value"] is not None
    assert result["brake_steer_rear_left_deg_per_kn"]["value"] is None


def test_force_range_not_reached_is_unavailable():
    force = _force() * 0.2
    result = _by_name(
        metrics(_request(_curves({"wheel_center_x": 2.0}, force), brakes=False))
    )
    row = result["longitudinal_compliance_front_left_mm_per_kn"]
    assert row["value"] is None
    assert "does not cover force_x travel -1000..1000 N" in row["reason"]


def _plots(brakes, debug=None):
    curves = _curves(
        {
            "camber": 0.05,
            "toe": 0.1,
            "spin": -0.6,
            "projected_contact_patch_x": 1.0,
            "wheel_center_x": 2.0,
            "wheel_center_y": 0.1,
        }
    )
    return plot(
        json.dumps(curves),
        "Measurement",
        branch=json.dumps({"measurement": True}),
        brakes=brakes,
        series=True,
        debug=debug,
    )


@pytest.mark.parametrize(
    ("brakes", "expected"),
    [(False, {"wc_x"}), (True, {"toe", "castor"})],
)
def test_debug_plots_show_the_secant_branches_of_the_setup(brakes, expected):
    info = json.dumps(
        {
            "config": "c.toml",
            "inputs": ["f.h5"],
            "creator": "test",
            "version": "0",
        }
    )
    files = _plots(brakes, debug=info)
    debug = {
        name.rsplit("/", 1)[1]
        for name in files
        if "/debug/" in name and name.endswith(".pdf")
    }
    assert debug == {
        f"measurement_knc_longitudinal_{plot}_vs_force_x.pdf"
        for plot in expected
    }
    for name in files:
        if "/debug/" in name:
            assert PdfReader(BytesIO(files[name])).pages


def test_plot_catalog_names_match_the_report_files():
    assert [job.removeprefix("plot:") for job in plot_jobs(False)] == [
        f"knc_longitudinal_{name}_vs_force_x"
        for name in (
            "camber",
            "toe",
            "castor",
            "proj_cp_x",
            "wc_x",
            "wc_y",
        )
    ]
    assert len(PLOT_SPECS) == 6


@pytest.mark.parametrize(
    ("brakes", "marked"),
    [
        (False, {"wc_x"}),
        (True, {"toe", "castor"}),
    ],
)
def test_plots_mark_only_the_metrics_of_the_setup(brakes, marked):
    files = _plots(brakes)
    pdfs = {name for name in files if name.endswith(".pdf")}
    assert len(pdfs) == (6 if brakes else 4)
    for name in pdfs:
        assert name.startswith("plots/knc/longitudinal/")
        assert PdfReader(BytesIO(files[name])).pages
    shown = set()
    for name, content in files.items():
        if not name.endswith(".plot.json"):
            continue
        document = json.loads(content)
        if document["metrics"]:
            shown.add(
                name.split("knc_longitudinal_")[1].split("_vs_force_x")[0]
            )
            metric = document["panels"][0]["sources"][0]["metric"]
            assert metric is not None
    assert shown == marked
