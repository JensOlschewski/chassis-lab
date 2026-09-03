"""Load and validate TOML case configuration for ChassisLab."""

import tomllib
from dataclasses import dataclass
from math import isfinite
from pathlib import Path
from typing import Any

LEGACY_TOP_LEVEL_KEYS = {
    "case",
    "inputs",
    "physics",
    "signal_map",
    "signals",
    "validation",
}
UNIFIED_TOP_LEVEL_KEYS = {"case", "physics", "svc", "vertical"}
CASE_KEYS = {"name", "domain", "variant", "loadcase"}
UNIFIED_CASE_KEYS = {"name", "variant", "loadcase"}
INPUTS_KEYS = {"simulation_hdf5"}
SVC_INPUT_KEYS = {
    "signal_map",
    "simulation_hdf5",
    "measurement_reference",
}
PHYSICS_KEYS = {"gravity"}
SIGNAL_KEYS = {"entity", "component", "unit"}
VALIDATION_KEYS = {"reference", "tolerances"}
SVC_VALIDATION_KEYS = {"tolerances"}
TOLERANCE_KEYS = {"absolute", "relative_percent"}
SVC_KEYS = {"inputs", "signals", "validation"}
VERTICAL_KEYS = {"inputs", "setup", "window", "signals"}
SIGNAL_MAP_KEYS = {"signals"}
VERTICAL_INPUT_KEYS = {"signal_map", "measurement_acd", "simulation_hdf5"}
VERTICAL_SETUP_CORE_KEYS = {
    "code",
    "brakes_applied",
    "engine_running",
    "front_arb_connected",
    "rear_arb_connected",
}
VERTICAL_WINDOW_KEYS = {"minimum_mm", "maximum_mm", "step_mm"}
VERTICAL_SETUP_WINDOW_KEYS = {
    "window_minimum_mm",
    "window_maximum_mm",
    "window_step_mm",
}
VERTICAL_SETUP_KEYS = VERTICAL_SETUP_CORE_KEYS | VERTICAL_SETUP_WINDOW_KEYS


class ConfigError(ValueError):
    """Raised when a case configuration is invalid."""


@dataclass(frozen=True)
class SignalRef:
    """A canonical signal mapped to an HDF5 entity and component."""

    entity: str
    component: str
    unit: str


@dataclass(frozen=True)
class ValidationTolerances:
    """Per-metric absolute and relative acceptance tolerances."""

    absolute: tuple[tuple[str, float], ...]
    relative_percent: tuple[tuple[str, float], ...]

    def absolute_for(self, metric: str) -> float | None:
        return _lookup_value(self.absolute, metric)

    def relative_percent_for(self, metric: str) -> float | None:
        return _lookup_value(self.relative_percent, metric)


@dataclass(frozen=True)
class ValidationConfig:
    """Reference input and acceptance policy for one case."""

    reference: Path
    tolerances: ValidationTolerances


@dataclass(frozen=True)
class VerticalSetup:
    code: str
    brakes_applied: bool
    engine_running: bool
    front_arb_connected: bool
    rear_arb_connected: bool | None = None


@dataclass(frozen=True)
class VerticalWindow:
    minimum_mm: float
    maximum_mm: float
    step_mm: float


@dataclass(frozen=True)
class VerticalConfig:
    measurement_acd: Path
    simulation_hdf5: Path
    setup: VerticalSetup
    window: VerticalWindow
    signals: tuple[tuple[str, SignalRef], ...]
    signal_map: Path | None = None

    def signal(self, canonical: str) -> SignalRef:
        for name, ref in self.signals:
            if name == canonical:
                return ref
        raise KeyError(canonical)


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
    validation: ValidationConfig | None = None
    vertical: VerticalConfig | None = None
    svc_signal_map: Path | None = None

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

    if "svc" in raw or "vertical" in raw:
        return _load_unified_config(raw, config_path)

    _reject_unknown(raw, LEGACY_TOP_LEVEL_KEYS, "top level")

    case = _require_table(raw, "case")
    inputs = _require_table(raw, "inputs")
    physics = _require_table(raw, "physics")
    signal_map, refs = _parse_signal_source(raw, config_path, "top level")
    validation = _parse_validation(raw.get("validation"), config_path)

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

    return CaseConfig(
        path=config_path.resolve(),
        name=name,
        domain=domain,
        variant=variant,
        loadcase=loadcase,
        simulation_hdf5=simulation,
        gravity=float(gravity),
        signals=refs,
        validation=validation,
        svc_signal_map=signal_map,
    )


def _load_unified_config(raw: dict[str, Any], config_path: Path) -> CaseConfig:
    _reject_unknown(raw, UNIFIED_TOP_LEVEL_KEYS, "top level")
    case = _require_table(raw, "case")
    physics = _require_table(raw, "physics")
    svc = _require_table(raw, "svc")
    _reject_unknown(case, UNIFIED_CASE_KEYS, "[case]")
    _require_keys(case, UNIFIED_CASE_KEYS, "[case]")
    _reject_unknown(physics, PHYSICS_KEYS, "[physics]")
    _require_keys(physics, PHYSICS_KEYS, "[physics]")
    _reject_unknown(svc, SVC_KEYS, "[svc]")

    name = _non_empty_string(case["name"], "[case].name")
    variant = _non_empty_string(case["variant"], "[case].variant")
    loadcase = _non_empty_string(case["loadcase"], "[case].loadcase")
    gravity = _positive_number(physics["gravity"], "[physics].gravity")

    inputs = _require_table(svc, "inputs")
    _reject_unknown(inputs, SVC_INPUT_KEYS, "[svc.inputs]")
    _require_keys(inputs, INPUTS_KEYS, "[svc.inputs]")
    simulation = _resolve_path(
        inputs["simulation_hdf5"],
        config_path,
        "[svc.inputs].simulation_hdf5",
    )
    signal_map, signals = _parse_signal_source(
        svc, config_path, "[svc]", inputs=inputs
    )
    validation = _parse_validation(
        svc.get("validation"),
        config_path,
        reference_value=inputs.get("measurement_reference"),
        where="[svc.validation]",
    )
    vertical = _parse_vertical(raw.get("vertical"), config_path)
    return CaseConfig(
        path=config_path.resolve(),
        name=name,
        domain="multi" if vertical is not None else "svc",
        variant=variant,
        loadcase=loadcase,
        simulation_hdf5=simulation,
        gravity=gravity,
        signals=signals,
        validation=validation,
        vertical=vertical,
        svc_signal_map=signal_map,
    )


def _parse_vertical(raw: object, config_path: Path) -> VerticalConfig | None:
    if raw is None:
        return None
    if not isinstance(raw, dict):
        raise ConfigError("[vertical] must be a table")
    _reject_unknown(raw, VERTICAL_KEYS, "[vertical]")
    inputs = _require_table(raw, "inputs")
    setup = _require_table(raw, "setup")
    window = raw.get("window")
    _reject_unknown(inputs, VERTICAL_INPUT_KEYS, "[vertical.inputs]")
    _require_keys(
        inputs,
        VERTICAL_INPUT_KEYS - {"signal_map"},
        "[vertical.inputs]",
    )
    _reject_unknown(setup, VERTICAL_SETUP_KEYS, "[vertical.setup]")
    condensed_window = any(key in setup for key in VERTICAL_SETUP_WINDOW_KEYS)
    _require_keys(
        setup,
        (
            VERTICAL_SETUP_KEYS - {"rear_arb_connected"}
            if condensed_window
            else VERTICAL_SETUP_CORE_KEYS - {"rear_arb_connected"}
        ),
        "[vertical.setup]",
    )
    if condensed_window:
        if window is not None:
            raise ConfigError(
                "[vertical] cannot define both condensed setup window and "
                "[vertical.window]"
            )
        window_values = {
            "minimum_mm": setup["window_minimum_mm"],
            "maximum_mm": setup["window_maximum_mm"],
            "step_mm": setup["window_step_mm"],
        }
        window_where = "[vertical.setup]"
    else:
        window = _require_table(raw, "window")
        _reject_unknown(window, VERTICAL_WINDOW_KEYS, "[vertical.window]")
        _require_keys(window, VERTICAL_WINDOW_KEYS, "[vertical.window]")
        window_values = window
        window_where = "[vertical.window]"
    signal_map, signals = _parse_signal_source(
        raw, config_path, "[vertical]", inputs=inputs
    )

    minimum = _finite_number(
        window_values["minimum_mm"], f"{window_where}.minimum_mm"
    )
    maximum = _finite_number(
        window_values["maximum_mm"], f"{window_where}.maximum_mm"
    )
    step = _positive_number(window_values["step_mm"], f"{window_where}.step_mm")
    if minimum >= maximum:
        raise ConfigError(
            f"{window_where}.minimum_mm must be less than maximum_mm"
        )
    return VerticalConfig(
        measurement_acd=_resolve_path(
            inputs["measurement_acd"],
            config_path,
            "[vertical.inputs].measurement_acd",
        ),
        simulation_hdf5=_resolve_path(
            inputs["simulation_hdf5"],
            config_path,
            "[vertical.inputs].simulation_hdf5",
        ),
        setup=VerticalSetup(
            code=_non_empty_string(setup["code"], "[vertical.setup].code"),
            brakes_applied=_boolean(setup["brakes_applied"], "brakes_applied"),
            engine_running=_boolean(setup["engine_running"], "engine_running"),
            front_arb_connected=_boolean(
                setup["front_arb_connected"], "front_arb_connected"
            ),
            rear_arb_connected=(
                _boolean(setup["rear_arb_connected"], "rear_arb_connected")
                if "rear_arb_connected" in setup
                else None
            ),
        ),
        window=VerticalWindow(minimum, maximum, step),
        signals=signals,
        signal_map=signal_map,
    )


def _parse_signal_source(
    container: dict[str, Any],
    config_path: Path,
    where: str,
    *,
    inputs: dict[str, Any] | None = None,
) -> tuple[Path | None, tuple[tuple[str, SignalRef], ...]]:
    signal_map_value = (inputs or container).get("signal_map")
    inline = container.get("signals")
    if signal_map_value is not None and inline is not None:
        raise ConfigError(
            f"{where} cannot define both signal_map and inline signals"
        )
    if signal_map_value is not None:
        signal_map = _resolve_path(
            signal_map_value, config_path, f"{where}.signal_map"
        )
        return signal_map, _load_signal_map(signal_map)
    if inline is not None:
        return None, _parse_signals(inline, f"{where}.signals")
    raise ConfigError(f"{where} must define signal_map or inline signals")


def _load_signal_map(path: Path) -> tuple[tuple[str, SignalRef], ...]:
    if not path.is_file():
        raise FileNotFoundError(f"signal map does not exist: {path}")
    try:
        raw = tomllib.loads(path.read_text(encoding="utf-8"))
    except (tomllib.TOMLDecodeError, UnicodeDecodeError) as error:
        raise ConfigError(
            f"invalid signal map TOML in {path}: {error}"
        ) from error
    _reject_unknown(raw, SIGNAL_MAP_KEYS, f"signal map {path}")
    return _parse_signals(raw.get("signals"), f"signal map {path} [signals]")


def _parse_signals(
    raw: object, where: str
) -> tuple[tuple[str, SignalRef], ...]:
    if not isinstance(raw, dict):
        raise ConfigError(f"{where} must be a table")
    if not raw:
        raise ConfigError(f"{where} must define at least one signal")
    return tuple(
        (canonical, _parse_signal(canonical, table))
        for canonical, table in raw.items()
    )


def _resolve_path(value: object, config_path: Path, where: str) -> Path:
    text = _non_empty_string(value, where)
    path = Path(text).expanduser()
    return (
        (config_path.parent / path).resolve()
        if not path.is_absolute()
        else path.resolve()
    )


def _non_empty_string(value: object, where: str) -> str:
    if not isinstance(value, str) or not value:
        raise ConfigError(f"{where} must be a non-empty string")
    return value


def _finite_number(value: object, where: str) -> float:
    if not isinstance(value, (int, float)) or isinstance(value, bool):
        raise ConfigError(f"{where} must be a number")
    numeric = float(value)
    if not isfinite(numeric):
        raise ConfigError(f"{where} must be finite")
    return numeric


def _positive_number(value: object, where: str) -> float:
    numeric = _finite_number(value, where)
    if numeric <= 0:
        raise ConfigError(f"{where} must be positive")
    return numeric


def _boolean(value: object, name: str) -> bool:
    if not isinstance(value, bool):
        raise ConfigError(f"[vertical.setup].{name} must be boolean")
    return value


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


def _parse_validation(
    raw: object,
    config_path: Path,
    *,
    reference_value: object = None,
    where: str = "[validation]",
) -> ValidationConfig | None:
    if raw is None:
        if reference_value is not None:
            raise ConfigError(
                f"{where} is required when [svc.inputs].measurement_reference "
                "is defined"
            )
        return None
    if not isinstance(raw, dict):
        raise ConfigError(f"{where} must be a table")
    unified = where == "[svc.validation]"
    validation_keys = SVC_VALIDATION_KEYS if unified else VALIDATION_KEYS
    _reject_unknown(raw, validation_keys, where)
    required = SVC_VALIDATION_KEYS if unified else VALIDATION_KEYS
    _require_keys(raw, required, where)

    if not unified:
        reference_value = raw["reference"]
    if not isinstance(reference_value, str) or not reference_value:
        reference_name = (
            "[svc.inputs].measurement_reference"
            if unified
            else f"{where}.reference"
        )
        raise ConfigError(f"{reference_name} must be a non-empty string")
    reference = Path(reference_value).expanduser()
    if not reference.is_absolute():
        reference = (config_path.parent / reference).resolve()
    else:
        reference = reference.resolve()

    tolerances = raw["tolerances"]
    if not isinstance(tolerances, dict):
        raise ConfigError(f"{where}.tolerances must be a table")
    _reject_unknown(tolerances, TOLERANCE_KEYS, f"{where}.tolerances")
    absolute = _parse_tolerance_table(tolerances, "absolute")
    relative = _parse_tolerance_table(tolerances, "relative_percent")
    if not absolute and not relative:
        raise ConfigError(
            f"{where}.tolerances must define at least one tolerance"
        )
    return ValidationConfig(
        reference=reference,
        tolerances=ValidationTolerances(
            absolute=absolute,
            relative_percent=relative,
        ),
    )


def _parse_tolerance_table(
    tolerances: dict[str, Any], name: str
) -> tuple[tuple[str, float], ...]:
    table = tolerances.get(name)
    if table is None:
        return ()
    if not isinstance(table, dict):
        raise ConfigError(f"[validation.tolerances.{name}] must be a table")

    parsed: list[tuple[str, float]] = []
    for metric, value in table.items():
        if not metric:
            raise ConfigError("tolerance metric names must be non-empty")
        if not isinstance(value, (int, float)) or isinstance(value, bool):
            raise ConfigError(
                f"tolerance '{metric}' in {name} must be a number"
            )
        numeric = float(value)
        if not isfinite(numeric) or numeric <= 0:
            raise ConfigError(
                f"tolerance '{metric}' in {name} must be finite and positive"
            )
        parsed.append((metric, numeric))
    return tuple(parsed)


def _lookup_value(
    values: tuple[tuple[str, float], ...], metric: str
) -> float | None:
    for name, value in values:
        if name == metric:
            return value
    return None


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
