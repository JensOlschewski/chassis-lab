"""Command-line interface for ChassisLab."""

import argparse
import json
import os
import sys
from collections.abc import Sequence
from pathlib import Path

from chassis_lab.acd import AcdError, AcdFile, inspect_acd
from chassis_lab.config import CaseConfig, ConfigError, load_config
from chassis_lab.converter import ConversionError, convert_res
from chassis_lab.hdf5 import (
    AdamsHdf5,
    Hdf5FormatError,
    SignalLookupError,
    inspect_hdf5,
)
from chassis_lab.runs import RunError, create_run
from chassis_lab.svc_eval import SvcEvaluationError
from chassis_lab.svc_reference import SvcReferenceError
from chassis_lab.svc_reporting import (
    SvcReportError,
    build_svc_report,
    render_svc_console,
    write_svc_outputs,
)
from chassis_lab.svc_validation import SvcValidationError, validate_svc
from chassis_lab.vertical import (
    VerticalEvaluationError,
    evaluate_vertical,
    validate_vertical_measurement,
)
from chassis_lab.vertical_reporting import (
    VerticalReportError,
    build_vertical_report,
    render_vertical_console,
    write_vertical_outputs,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="clab",
        description=(
            "Analyze and validate vehicle multibody simulation results"
        ),
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    inspect_parser = subparsers.add_parser(
        "inspect-h5", help="validate and summarize an adams-res HDF5 file"
    )
    inspect_parser.add_argument("path", type=Path, help="HDF5 result file")
    inspect_parser.add_argument(
        "--json", action="store_true", help="write JSON output"
    )
    inspect_parser.set_defaults(handler=_inspect_h5)

    list_parser = subparsers.add_parser(
        "list-h5-signals",
        help="list signals available in an adams-res HDF5 file",
    )
    list_parser.add_argument("path", type=Path, help="HDF5 result file")
    list_parser.add_argument("--match", help="case-insensitive metadata filter")
    list_parser.add_argument(
        "--json", action="store_true", help="write JSON output"
    )
    list_parser.set_defaults(handler=_list_h5_signals)

    inspect_acd_parser = subparsers.add_parser(
        "inspect-acd", help="validate and summarize a Lommel ACD file"
    )
    inspect_acd_parser.add_argument("path", type=Path, help="ACD measurement")
    inspect_acd_parser.add_argument(
        "--json", action="store_true", help="write JSON output"
    )
    inspect_acd_parser.set_defaults(handler=_inspect_acd)

    list_acd_parser = subparsers.add_parser(
        "list-acd-signals", help="list signals available in a Lommel ACD file"
    )
    list_acd_parser.add_argument("path", type=Path, help="ACD measurement")
    list_acd_parser.add_argument(
        "--match", help="case-insensitive metadata filter"
    )
    list_acd_parser.add_argument(
        "--json", action="store_true", help="write JSON output"
    )
    list_acd_parser.set_defaults(handler=_list_acd_signals)

    validate_parser = subparsers.add_parser(
        "validate-config",
        help="load and validate a TOML case configuration",
    )
    validate_parser.add_argument("path", type=Path, help="TOML config file")
    validate_parser.add_argument(
        "--json", action="store_true", help="write JSON output"
    )
    validate_parser.add_argument(
        "--strict-units",
        action="store_true",
        help="fail when source unit metadata is missing",
    )
    validate_parser.set_defaults(handler=_validate_config)

    validate_svc_parser = subparsers.add_parser(
        "validate-svc",
        help="validate an SVC result against its configured reference",
    )
    validate_svc_parser.add_argument("path", type=Path, help="TOML config file")
    validate_svc_parser.add_argument(
        "--json-output", type=Path, help="write detailed JSON report"
    )
    validate_svc_parser.add_argument(
        "--csv-output", type=Path, help="write metric CSV report"
    )
    validate_svc_parser.add_argument(
        "-f",
        "--force",
        action="store_true",
        help="replace existing report outputs",
    )
    validate_svc_parser.add_argument(
        "--color",
        choices=("auto", "always", "never"),
        default="auto",
        help="color console status (default: auto)",
    )
    validate_svc_parser.set_defaults(handler=_validate_svc)

    validate_vertical_parser = subparsers.add_parser(
        "validate-vertical",
        help="compare vertical wheel-center trajectories",
    )
    validate_vertical_parser.add_argument(
        "path", type=Path, help="unified TOML config file"
    )
    validate_vertical_parser.add_argument(
        "--plot-output", type=Path, help="write deterministic SVG plot"
    )
    validate_vertical_parser.add_argument(
        "--json-output", type=Path, help="write detailed JSON report"
    )
    validate_vertical_parser.add_argument(
        "--csv-output", type=Path, help="write metric CSV report"
    )
    validate_vertical_parser.add_argument(
        "-f",
        "--force",
        action="store_true",
        help="replace existing report outputs",
    )
    validate_vertical_parser.set_defaults(handler=_validate_vertical)

    convert_parser = subparsers.add_parser(
        "convert-res",
        help="convert an Adams/Car .res result file to HDF5",
    )
    convert_parser.add_argument("input", type=Path, help="Adams/Car .res file")
    convert_parser.add_argument(
        "-o", "--output", type=Path, help="HDF5 output path"
    )
    convert_parser.add_argument(
        "--binary",
        default="adams-res",
        help="converter binary path or name (default: adams-res)",
    )
    convert_parser.add_argument(
        "-f", "--force", action="store_true", help="replace existing output"
    )
    convert_parser.set_defaults(handler=_convert_res)

    run_parser = subparsers.add_parser(
        "create-run",
        help="create an isolated run directory and provenance manifest",
    )
    run_parser.add_argument("path", type=Path, help="TOML config file")
    run_parser.add_argument(
        "--artifacts-root", type=Path, help="root directory for run artifacts"
    )
    run_parser.add_argument(
        "--domain",
        choices=("svc", "vertical"),
        default="svc",
        help="configured analysis domain (default: svc)",
    )
    run_parser.add_argument("--run-id", help="explicit run ID")
    run_parser.set_defaults(handler=_create_run)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return int(args.handler(args))
    except (
        FileNotFoundError,
        AcdError,
        ConfigError,
        ConversionError,
        Hdf5FormatError,
        SignalLookupError,
        RunError,
        SvcEvaluationError,
        SvcReferenceError,
        SvcReportError,
        SvcValidationError,
        VerticalEvaluationError,
        VerticalReportError,
        OSError,
    ) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


def _inspect_h5(args: argparse.Namespace) -> int:
    summary = inspect_hdf5(args.path)
    if args.json:
        print(json.dumps(summary.as_dict(), indent=2))
    else:
        print(f"File: {summary.path}")
        print(f"Source: {summary.source_file}")
        print(f"Format version: {summary.format_version}")
        print(f"Samples: {summary.sample_count}")
        print(f"Entities: {summary.entity_count}")
        print(f"Signals: {summary.signal_count}")
    return 0


def _list_h5_signals(args: argparse.Namespace) -> int:
    with AdamsHdf5(args.path) as result:
        signals = result.signals(args.match)

    if args.json:
        print(json.dumps([signal.as_dict() for signal in signals], indent=2))
    else:
        print("ENTITY\tCOMPONENT\tUNIT\tADAMS_ID\tGROUP")
        for signal in signals:
            print(
                f"{signal.entity}\t{signal.component}\t{signal.unit}\t"
                f"{signal.adams_id}\t{signal.group_name}"
            )
    return 0


def _inspect_acd(args: argparse.Namespace) -> int:
    summary = inspect_acd(args.path)
    if args.json:
        print(json.dumps(summary.as_dict(), indent=2))
    else:
        print(f"File: {summary.path}")
        print(f"Encoding: {summary.encoding}")
        print(f"Dialect: {summary.dialect}")
        print(f"Data starts: line {summary.data_start_line}")
        print(f"Channels: {summary.channel_count}")
        print(f"Samples: {summary.sample_count}")
    return 0


def _list_acd_signals(args: argparse.Namespace) -> int:
    signals = AcdFile(args.path).signals(args.match)
    if args.json:
        print(json.dumps([signal.as_dict() for signal in signals], indent=2))
    else:
        print("NAME\tUNIT\tOFFSET\tDESCRIPTION")
        for signal in signals:
            print(
                f"{signal.name}\t{signal.unit}\t{signal.offset}\t"
                f"{signal.description}"
            )
    return 0


def _validate_config(args: argparse.Namespace) -> int:
    config = load_config(args.path)

    if args.json:
        _check_signals(config, quiet=True, strict_units=args.strict_units)
        _check_vertical_signals(
            config, quiet=True, strict_units=args.strict_units
        )
        print(json.dumps(_config_as_dict(config), indent=2))
    else:
        print(f"Config: {config.path}")
        print(f"Case: {config.name} ({config.domain}, {config.variant})")
        print(f"Loadcase: {config.loadcase}")
        print(f"Simulation: {config.simulation_hdf5}")
        if config.svc_signal_map is not None:
            print(f"SVC signal map: {config.svc_signal_map}")
        print(f"Gravity: {config.gravity}")
        print(f"Signals: {len(config.signals)}")
        _check_signals(config, strict_units=args.strict_units)
        if config.vertical is not None:
            print(f"Vertical measurement: {config.vertical.measurement_acd}")
            print(f"Vertical simulation: {config.vertical.simulation_hdf5}")
            if config.vertical.signal_map is not None:
                print(f"Vertical signal map: {config.vertical.signal_map}")
            print(f"Vertical signals: {len(config.vertical.signals)}")
        _check_vertical_signals(config, strict_units=args.strict_units)
    return 0


def _validate_svc(args: argparse.Namespace) -> int:
    config = load_config(args.path)
    validation = validate_svc(config)
    report = build_svc_report(config, validation)
    write_svc_outputs(
        report,
        json_path=args.json_output,
        csv_path=args.csv_output,
        force=args.force,
    )
    color = args.color == "always" or (
        args.color == "auto"
        and sys.stdout.isatty()
        and "NO_COLOR" not in os.environ
    )
    print(render_svc_console(report, color=color), end="")
    return 0 if report.passed else 1


def _validate_vertical(args: argparse.Namespace) -> int:
    config = load_config(args.path)
    result = evaluate_vertical(config)
    report = build_vertical_report(config, result)
    write_vertical_outputs(
        report,
        plot_path=args.plot_output,
        json_path=args.json_output,
        csv_path=args.csv_output,
        force=args.force,
    )
    print(render_vertical_console(report), end="")
    return 0


def _config_as_dict(config: CaseConfig) -> dict[str, object]:
    output: dict[str, object] = {
        "name": config.name,
        "domain": config.domain,
        "variant": config.variant,
        "loadcase": config.loadcase,
        "simulation_hdf5": str(config.simulation_hdf5),
        "gravity": config.gravity,
        "signals": [
            {
                "canonical": canonical,
                "entity": ref.entity,
                "component": ref.component,
                "unit": ref.unit,
            }
            for canonical, ref in config.signals
        ],
    }
    if config.svc_signal_map is not None:
        output["svc_signal_map"] = str(config.svc_signal_map)
    if config.vertical is not None:
        output["vertical"] = {
            "measurement_acd": str(config.vertical.measurement_acd),
            "simulation_hdf5": str(config.vertical.simulation_hdf5),
            "setup": {
                "code": config.vertical.setup.code,
                "brakes_applied": config.vertical.setup.brakes_applied,
                "engine_running": config.vertical.setup.engine_running,
                "front_arb_connected": (
                    config.vertical.setup.front_arb_connected
                ),
                "rear_arb_connected": config.vertical.setup.rear_arb_connected,
            },
            "window": {
                "minimum_mm": config.vertical.window.minimum_mm,
                "maximum_mm": config.vertical.window.maximum_mm,
                "step_mm": config.vertical.window.step_mm,
            },
            "signals": [
                {
                    "canonical": canonical,
                    "entity": ref.entity,
                    "component": ref.component,
                    "unit": ref.unit,
                }
                for canonical, ref in config.vertical.signals
            ],
        }
        if config.vertical.signal_map is not None:
            output["vertical"]["signal_map"] = str(config.vertical.signal_map)
    return output


def _check_signals(
    config: CaseConfig,
    *,
    quiet: bool = False,
    strict_units: bool = False,
) -> None:
    if not config.simulation_hdf5.is_file():
        if not quiet:
            print(
                "Note: simulation_hdf5 not present; skipped signal "
                f"cross-check ({config.simulation_hdf5})"
            )
        return
    with AdamsHdf5(config.simulation_hdf5) as result:
        errors: list[str] = []
        resolved: set[tuple[str, int]] = set()
        for canonical, ref in config.signals:
            try:
                info = result.resolve_signal(ref.entity, ref.component)
            except SignalLookupError as error:
                errors.append(f"{canonical}: {error}")
                if not quiet:
                    print(f"  - {canonical}: ERROR ({error})")
                continue
            if info.unit and info.unit != ref.unit:
                errors.append(
                    f"{canonical}: source unit '{info.unit}' "
                    f"!= configured '{ref.unit}'"
                )
                if not quiet:
                    print(
                        f"  - {canonical}: ERROR, source unit '{info.unit}' "
                        f"!= configured '{ref.unit}'"
                    )
            elif not info.unit:
                message = (
                    f"{canonical}: source unit missing; expected '{ref.unit}'"
                )
                if strict_units:
                    errors.append(message)
                    status = "ERROR"
                else:
                    status = "UNVERIFIED"
                if not quiet:
                    print(f"  - {canonical}: {status} [{ref.unit}]")
            else:
                if not quiet:
                    print(f"  - {canonical}: VERIFIED [{info.unit}]")
            cell = (info.group_name, info.column)
            if cell in resolved:
                errors.append(f"{canonical}: duplicate HDF5 cell {cell}")
            resolved.add(cell)
        if errors:
            raise ConfigError("signal mapping invalid: " + "; ".join(errors))


def _check_vertical_signals(
    config: CaseConfig,
    *,
    quiet: bool = False,
    strict_units: bool = False,
) -> None:
    vertical = config.vertical
    if vertical is None:
        return
    if vertical.measurement_acd.is_file():
        validate_vertical_measurement(vertical)
    elif not quiet:
        print(
            "Note: vertical measurement ACD not present; skipped cross-check "
            f"({vertical.measurement_acd})"
        )
    if not vertical.simulation_hdf5.is_file():
        if not quiet:
            print(
                "Note: vertical simulation HDF5 not present; skipped signal "
                f"cross-check ({vertical.simulation_hdf5})"
            )
        return
    with AdamsHdf5(vertical.simulation_hdf5) as result:
        errors = []
        resolved: dict[tuple[str, int], str] = {}
        for canonical, ref in vertical.signals:
            try:
                info = result.resolve_signal(ref.entity, ref.component)
            except SignalLookupError as error:
                errors.append(f"{canonical}: {error}")
                continue
            if info.unit and info.unit != ref.unit:
                errors.append(
                    f"{canonical}: source unit '{info.unit}' "
                    f"!= configured '{ref.unit}'"
                )
            elif not info.unit and strict_units:
                errors.append(
                    f"{canonical}: source unit missing; expected '{ref.unit}'"
                )
            cell = (info.group_name, info.column)
            previous = resolved.get(cell)
            if previous is not None and not _is_vertical_travel_alias(
                previous, canonical
            ):
                errors.append(f"{canonical}: duplicate HDF5 cell {cell}")
            resolved[cell] = canonical
        if errors:
            raise ConfigError(
                "vertical signal mapping invalid: " + "; ".join(errors)
            )


def _is_vertical_travel_alias(first: str, second: str) -> bool:
    names = {first, second}
    return any(
        names
        == {
            f"{corner}_travel",
            f"{corner}_wheel_center_z",
        }
        for corner in ("front_left", "front_right", "rear_left", "rear_right")
    )


def _convert_res(args: argparse.Namespace) -> int:
    result = convert_res(
        args.input,
        output=args.output,
        binary=args.binary,
        force=args.force,
    )
    print(result.h5_path)
    return 0


def _create_run(args: argparse.Namespace) -> int:
    run_dir = create_run(
        args.path,
        domain=args.domain,
        artifacts_root=args.artifacts_root,
        run_id=args.run_id,
    )
    print(run_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
