"""Branch-aware linear fits of a response against a driven force.

Force is the independent variable: branches are the monotone force sweeps found
by the shared reversal detection, fitted separately so hysteresis stays visible.
"""

from dataclasses import dataclass

import numpy as np

from chassis_lab.knc_kernel import BranchOptions, KncKernelError, branch_ranges


@dataclass(frozen=True)
class ForceWindow:
    """Force range to fit and the minimum evidence a fit needs.

    Fits never extrapolate: a branch must reach both ends of the range.
    """

    low: float
    high: float
    min_span: float
    min_samples: int = 10

    def __post_init__(self) -> None:
        if not (self.high - self.low >= self.min_span > 0):
            raise KncKernelError(
                "force window must be at least min_span wide and min_span > 0"
            )
        if self.min_samples < 2:
            raise KncKernelError("min_samples must be at least 2")


@dataclass(frozen=True)
class BranchFit:
    """One branch fit; `reason` replaces the numbers when it was rejected."""

    branch: str
    requested: tuple[float, float]
    actual: tuple[float, float] | None = None
    samples: int = 0
    slope: float | None = None
    intercept: float | None = None
    residual: float | None = None
    reason: str | None = None


def force_branches(force: np.ndarray) -> list[tuple[str, slice]]:
    """Rising and falling force sweeps, numbered per direction in time order.

    The first sweep is dropped like for measured travel: it starts from rest.
    """
    counts = {1: 0, -1: 0}
    branches = []
    for segment, sign in branch_ranges(force, BranchOptions(measurement=True)):
        counts[sign] += 1
        name = "rise" if sign > 0 else "fall"
        branches.append((f"{name} {counts[sign]}", segment))
    return branches


def fit_branches(
    force: np.ndarray, response: np.ndarray, window: ForceWindow
) -> list[BranchFit]:
    """Fit `response` against `force` on every branch, in time order."""
    requested = (window.low, window.high)
    fits = []
    for name, segment in force_branches(force):
        x, y = force[segment], response[segment]
        finite = np.isfinite(x) & np.isfinite(y)
        x, y = x[finite], y[finite]
        if len(x) == 0 or x.min() > window.low or x.max() < window.high:
            fits.append(BranchFit(name, requested, reason="range not covered"))
            continue
        inside = (x >= window.low) & (x <= window.high)
        x, y = x[inside], y[inside]
        actual = (float(x.min()), float(x.max()))
        if len(x) < window.min_samples:
            fits.append(
                BranchFit(
                    name,
                    requested,
                    actual,
                    len(x),
                    reason=f"fewer than {window.min_samples} samples",
                )
            )
            continue
        slope, intercept = np.polyfit(x, y, 1)
        residual = float(np.sqrt(np.mean((y - (slope * x + intercept)) ** 2)))
        fits.append(
            BranchFit(
                name,
                requested,
                actual,
                len(x),
                float(slope),
                float(intercept),
                residual,
            )
        )
    return fits
