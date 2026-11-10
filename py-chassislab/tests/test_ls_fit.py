from dataclasses import replace

import numpy as np
import pytest

from chassis_lab.knc_kernel import (
    BranchOptions,
    KncKernelError,
    fit_slope,
    metric_cycle,
)


def _sweep(peaks, leg=400):
    return np.concatenate(
        [
            np.linspace(a, b, leg, endpoint=False)
            for a, b in zip(peaks[:-1], peaks[1:], strict=True)
        ]
    )


OPTIONS = BranchOptions(measurement=True)
# partial start branch, then reversals at -4k, +4k, -4k, +4k
FORCE = _sweep([0.0, -4000.0, 4000.0, -4000.0, 4000.0, -4000.0, 0.0])


def test_fit_slope_recovers_line_with_offset():
    x = np.linspace(-3000, 3000, 601)
    fit = fit_slope(x, 0.002 * x + 0.3, 1000.0)
    assert fit.slope == pytest.approx(0.002)
    assert fit.intercept == pytest.approx(0.3)
    assert fit.samples == 201


def test_fit_slope_pools_symmetric_hysteresis():
    x = np.concatenate([np.linspace(-2000, 2000, 400)] * 2)
    y = 0.002 * x + np.r_[np.full(400, 0.1), np.full(400, -0.1)]
    assert fit_slope(x, y, 1000.0).slope == pytest.approx(0.002)


def test_fit_slope_is_shifted_by_force_offset():
    x = np.linspace(-3000, 3000, 6001)
    y = np.where(np.abs(x) < 1500, 0.002 * x, 0.004 * x)
    centered = fit_slope(x, y, 1000.0).slope
    shifted = fit_slope(x + 600.0, y, 1000.0).slope
    assert centered == pytest.approx(0.002)
    assert shifted != pytest.approx(centered, rel=1e-3)


def test_fit_slope_ignores_non_finite_samples():
    x = np.linspace(-2000, 2000, 401)
    y = 0.002 * x
    y[200] = np.nan
    assert fit_slope(x, y, 1000.0).slope == pytest.approx(0.002)


def test_fit_slope_rejects_too_few_samples():
    x = np.linspace(-3000, 3000, 7)
    with pytest.raises(KncKernelError, match="fewer than 3"):
        fit_slope(x, x, 500.0)


def test_metric_cycle_runs_between_upper_reversals():
    cycle = metric_cycle(FORCE, replace(OPTIONS, cycle=1))
    assert FORCE[cycle][0] == pytest.approx(4000.0, abs=20)
    assert FORCE[cycle][-1] == pytest.approx(4000.0, abs=20)
    assert FORCE[cycle].min() == pytest.approx(-4000.0, abs=20)


def test_metric_cycle_selects_later_cycle():
    first = metric_cycle(FORCE, replace(OPTIONS, cycle=1))
    second = metric_cycle(FORCE, replace(OPTIONS, cycle=2))
    assert second.start >= first.stop - 1


def test_metric_cycle_requires_a_full_cycle():
    with pytest.raises(KncKernelError, match="no full cycle 3"):
        metric_cycle(FORCE, replace(OPTIONS, cycle=3))
    with pytest.raises(KncKernelError, match="no full cycle"):
        metric_cycle(_sweep([0.0, -4000.0, 4000.0]), replace(OPTIONS, cycle=1))
