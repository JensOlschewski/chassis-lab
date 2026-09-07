from dataclasses import replace
from pathlib import Path
from typing import cast

import pytest

from chassis_lab.config import load_config
from chassis_lab.svc_eval import SvcGeometryResult
from chassis_lab.svc_validation import (
    SvcValidationError,
    validate_svc_result,
)

_ROOT = Path(__file__).resolve().parents[2]
EXAMPLE_CONFIG_PATH = _ROOT / "examples" / "example-config.toml"


def test_example_config_loads_without_private_data() -> None:
    config = load_config(EXAMPLE_CONFIG_PATH)

    assert config.name == "example-config"
    assert config.simulation_hdf5.name == "example-svc.h5"
    assert config.validation is not None
    assert config.validation.reference.name == "example-measures.json"


def test_validate_svc_result_without_validation_raises() -> None:
    config = load_config(EXAMPLE_CONFIG_PATH)
    bare = replace(config, validation=None)

    with pytest.raises(SvcValidationError, match=r"no \[validation\]"):
        validate_svc_result(bare, cast(SvcGeometryResult, None))
