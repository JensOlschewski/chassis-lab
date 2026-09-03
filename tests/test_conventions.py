from pathlib import Path

import numpy as np
import pytest

from chassis_lab.config import load_config
from chassis_lab.conventions import (
    BaselinePolicy,
    ConventionError,
    Corner,
    SignalConvention,
    apply_convention,
    convert_values,
)
from chassis_lab.svc import SVC_SIGNAL_CONVENTIONS
from chassis_lab.vertical import VERTICAL_SIGNAL_CONVENTIONS


def test_svc_mapping_has_one_convention_per_signal() -> None:
    root = Path(__file__).resolve().parents[1]
    config = load_config(root / "configs" / "varb-lommel-2up.toml")
    names = {name for name, _ in config.signals}

    assert set(SVC_SIGNAL_CONVENTIONS) == names


def test_vertical_mapping_has_one_convention_per_signal() -> None:
    root = Path(__file__).resolve().parents[1]
    config = load_config(root / "configs" / "varb-lommel-2up.toml")
    assert config.vertical is not None
    names = {name for name, _ in config.vertical.signals}

    assert set(VERTICAL_SIGNAL_CONVENTIONS) == names
    assert all(
        convention.baseline == BaselinePolicy.STATIC_SIMULATION
        for convention in VERTICAL_SIGNAL_CONVENTIONS.values()
    )
    assert all(
        convention.corner is not None
        for convention in VERTICAL_SIGNAL_CONVENTIONS.values()
    )


def test_converts_force_to_mass_without_rounding() -> None:
    values = convert_values([9.80665, 19.6133], "newton", "kg")

    np.testing.assert_allclose(values, [1.0, 2.0])
    assert values.dtype == np.float64


@pytest.mark.parametrize(
    ("source", "target", "value", "expected"),
    [
        ("mm", "m", 1000.0, 1.0),
        ("rad", "deg", np.pi, 180.0),
        ("newton-mm", "newton-meter", 1000.0, 1.0),
    ],
)
def test_converts_common_units(
    source: str, target: str, value: float, expected: float
) -> None:
    result = convert_values([value], source, target)[0]

    assert result == pytest.approx(expected)


def test_rejects_missing_source_unit() -> None:
    with pytest.raises(ConventionError, match="without source unit"):
        convert_values([1.0], "", "mm")


def test_rejects_unsupported_conversion() -> None:
    with pytest.raises(ConventionError, match="unsupported conversion"):
        convert_values([1.0], "newton", "mm")


def test_applies_scale_sign_and_provenance() -> None:
    convention = SignalConvention(
        unit="deg",
        source_unit="rad",
        scale=2.0,
        sign=-1,
        corner=Corner.FRONT_RIGHT,
        baseline=BaselinePolicy.MEASUREMENT_OFFSET,
    )

    signal = apply_convention(
        [np.pi / 2],
        name="front_right_toe",
        source="measurement.acd/toe_fr",
        convention=convention,
    )

    np.testing.assert_allclose(signal.values, [-180.0])
    assert signal.original_unit == "rad"
    assert signal.source == "measurement.acd/toe_fr"
    assert signal.convention.corner == Corner.FRONT_RIGHT
    assert signal.convention.baseline == BaselinePolicy.MEASUREMENT_OFFSET


def test_rejects_invalid_sign_and_scale() -> None:
    with pytest.raises(ConventionError, match="sign"):
        SignalConvention(unit="mm", sign=0)
    with pytest.raises(ConventionError, match="scale"):
        SignalConvention(unit="mm", scale=0)
