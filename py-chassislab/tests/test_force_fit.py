import numpy as np
import pytest

from chassis_lab.knc_force_fit import (
    ForceWindow,
    fit_branches,
    force_branches,
)
from chassis_lab.knc_kernel import KncKernelError

WINDOW = ForceWindow(low=-3.0, high=3.0, min_span=1.0)


def _force(peaks=(-4.0, 4.0, -4.0, 4.0, 0.0), leg=500, dither=0.002):
    """Triangle wave from rest through the peaks, dithering at each turn."""
    rng = np.random.default_rng(1)
    points = np.r_[0.0, peaks]
    force = np.concatenate(
        [
            np.linspace(start, end, leg, endpoint=False)
            for start, end in zip(points[:-1], points[1:], strict=True)
        ]
    )
    for turn in range(1, len(peaks)):
        at = turn * leg
        force[at - 5 : at + 5] += rng.normal(0, dither, 10)
    return force


def _response(force, slope=0.5, loop=0.1):
    """Linear response, shifted up on rising and down on falling force."""
    sign = np.sign(np.gradient(force))
    return slope * force + loop * sign


def test_branches_are_numbered_per_direction_and_skip_first_sweep():
    names = [name for name, _ in force_branches(_force())]
    assert names == ["rise 1", "fall 1", "rise 2", "fall 2"]


def test_fits_known_slope_and_keeps_hysteresis_per_branch():
    force = _force()
    fits = fit_branches(force, _response(force), WINDOW)
    usable = {fit.branch: fit for fit in fits if fit.reason is None}
    assert set(usable) == {"rise 1", "fall 1", "rise 2"}
    for fit in usable.values():
        assert fit.slope == pytest.approx(0.5, abs=1e-2)
        assert fit.residual < 0.02
        assert fit.actual[0] >= -3.0 and fit.actual[1] <= 3.0
    assert usable["rise 1"].intercept == pytest.approx(0.1, abs=1e-2)
    assert usable["fall 1"].intercept == pytest.approx(-0.1, abs=1e-2)


def test_partial_branch_is_rejected_not_extrapolated():
    force = _force()
    fall_2 = fit_branches(force, _response(force), WINDOW)[-1]
    assert fall_2.branch == "fall 2"
    assert fall_2.reason == "range not covered"
    assert fall_2.slope is None


def test_too_few_samples_is_rejected():
    force = _force(leg=40)
    fits = fit_branches(force, _response(force), ForceWindow(-3, 3, 1, 100))
    assert fits
    assert all(fit.reason for fit in fits)


def test_plateau_inside_window_does_not_change_the_slope():
    force = np.insert(_force(), 750, np.full(60, _force()[750]))
    fits = fit_branches(force, 0.5 * force, WINDOW)
    assert [fit.slope for fit in fits if fit.reason is None]
    for fit in fits:
        if fit.reason is None:
            assert fit.slope == pytest.approx(0.5, abs=1e-2)


def test_nan_response_samples_are_ignored():
    force = _force()
    response = 0.5 * force
    response[600:620] = np.nan
    fit = fit_branches(force, response, WINDOW)[0]
    assert fit.slope == pytest.approx(0.5, abs=1e-6)


def test_window_must_have_positive_span():
    with pytest.raises(KncKernelError):
        ForceWindow(low=1.0, high=1.0, min_span=0.5)
