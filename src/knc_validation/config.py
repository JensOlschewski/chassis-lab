"""Load and validate TOML case configuration for KnC validation."""

import tomllib
from dataclasses import dataclass
from pathlib import Path
from typing import Any

TOP_LEVEL_KEYS = {"case", "inputs", "physics", "signals"}
CASE_KEYS = {"name", "domain", "variant", "loadcase"}
INPUTS_KEYS = {"simulation_hdf5"}
PHYSICS_KEYS = {"gravity"}
SIGNAL_KEYS = {"entity", "component", "unit"}


class ConfigError(ValueError):
    """Raised when a case configuration is invalid."""


@dataclass(frozen=True)
class SignalRef:
    """A canonical signal mapped to an HDF5 entity and component."""

    entity: str
    component: str
    unit: str


@dataclass(frozen=True)
class CaseConfig:
    """Validated TOML case configuration with resolved paths."""

    path: Path
    name: str
    domain: str
    variant: str
    loadcase: str
    simulation_hdf5: Path
    gravity: float
    signals: tuple[tuple[str, SignalRef], ...]

    def signal(self, canonical: str) -> SignalRef:
        for name, ref in self.signals:
            if name == canonical:
                return ref
        raise KeyError(canonical)


def load_config(path: Path | str) -> CaseConfig:
    """Load and validate a TOML case configuration file.

    Relative paths are resolved against the configuration file's parent
    directory so a run never depends on the current working directory.
    """
    config_path = Path(path).expanduser()
    if not config_path.is_file():
        raise FileNotFoundError(f"config file does not exist: {config_path}")
    try:
        raw = tomllib.loads(config_path.read_text(encoding="utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        raise ConfigError(f"invalid TOML in {config_path}: {error}") from error

    _reject_unknown(raw, TOP_LEVEL_KEYS, "top level")

    case = _require_table(raw, "case")
    inputs = _require_table(raw, "inputs")
    physics = _require_table(raw, "physics")
    signals = raw.get("signals")
    if signals is not None and not isinstance(signals, dict):
        raise ConfigError("[signals] must be a table")

    _require_keys(case, CASE_KEYS, "[case]")
    _require_keys(inputs, INPUTS_KEYS, "[inputs]")
    _require_keys(physics, PHYSICS_KEYS, "[physics]")

    name = case["name"]
    domain = case["domain"]
    variant = case["variant"]
    loadcase = case["loadcase"]
    for key, value in (
        ("name", name),
        ("domain", domain),
        ("variant", variant),
        ("loadcase", loadcase),
    ):
        if not isinstance(value, str) or not value:
            raise ConfigError(f"[case].{key} must be a non-empty string")

    simulation_value = inputs["simulation_hdf5"]
    if not isinstance(simulation_value, str) or not simulation_value:
        raise ConfigError("[inputs].simulation_hdf5 must be a non-empty string")
    simulation = Path(simulation_value).expanduser()
    if not simulation.is_absolute():
        simulation = (config_path.parent / simulation).resolve()
    else:
        simulation = simulation.resolve()

    gravity = physics["gravity"]
    if not isinstance(gravity, (int, float)) or isinstance(gravity, bool):
        raise ConfigError("[physics].gravity must be a number")
    if gravity <= 0:
        raise ConfigError("[physics].gravity must be positive")

    if signals is None:
        if domain != "svc":
            raise ConfigError("[signals] is required outside the svc domain")
        from knc_validation.svc import SVC_SIGNAL_MAPPING

        refs = SVC_SIGNAL_MAPPING
    else:
        if not signals:
            raise ConfigError("[signals] must define at least one signal")
        refs = tuple(
            (canonical, _parse_signal(canonical, table))
            for canonical, table in signals.items()
        )
    return CaseConfig(
        path=config_path.resolve(),
        name=name,
        domain=domain,
        variant=variant,
        loadcase=loadcase,
        simulation_hdf5=simulation,
        gravity=float(gravity),
        signals=refs,
    )


def _parse_signal(canonical: str, table: object) -> SignalRef:
    if not isinstance(canonical, str) or not canonical:
        raise ConfigError("signal names must be non-empty strings")
    if not isinstance(table, dict):
        raise ConfigError(f"signal '{canonical}' must be a table")
    _reject_unknown(table, SIGNAL_KEYS, f"signal '{canonical}'")
    _require_keys(table, SIGNAL_KEYS, f"signal '{canonical}'")

    entity = table["entity"]
    component = table["component"]
    unit = table["unit"]
    if not isinstance(entity, str) or not entity:
        raise ConfigError(
            f"signal '{canonical}'.entity must be a non-empty string"
        )
    if not isinstance(component, str) or not component:
        raise ConfigError(
            f"signal '{canonical}'.component must be a non-empty string"
        )
    if not isinstance(unit, str):
        raise ConfigError(f"signal '{canonical}'.unit must be a string")
    return SignalRef(entity=entity, component=component, unit=unit)


def _require_table(raw: dict[str, Any], key: str) -> dict[str, Any]:
    table = raw.get(key)
    if not isinstance(table, dict):
        if key not in raw:
            raise ConfigError(f"missing required [{key}] table")
        raise ConfigError(f"[{key}] must be a table")
    return table


def _reject_unknown(
    table: dict[str, Any], allowed: set[str], where: str
) -> None:
    unknown = set(table) - allowed
    if unknown:
        names = ", ".join(sorted(unknown))
        raise ConfigError(f"unknown {where} key(s): {names}")


def _require_keys(
    table: dict[str, Any], required: set[str], where: str
) -> None:
    missing = sorted(required - set(table))
    if missing:
        names = ", ".join(missing)
        raise ConfigError(f"{where} is missing key(s): {names}")
