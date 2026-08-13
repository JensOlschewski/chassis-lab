"""Command-line interface for KnC validation."""

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path

from knc_validation.acd import AcdError, AcdFile, inspect_acd
from knc_validation.config import CaseConfig, ConfigError, load_config
from knc_validation.converter import ConversionError, convert_res
from knc_validation.hdf5 import (
    AdamsHdf5,
    Hdf5FormatError,
    SignalLookupError,
    inspect_hdf5,
)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="knc",
        description=(
            "Validate Adams/Car KnC results against physical measurements"
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
        print(json.dumps(_config_as_dict(config), indent=2))
    else:
        print(f"Config: {config.path}")
        print(f"Case: {config.name} ({config.domain}, {config.variant})")
        print(f"Loadcase: {config.loadcase}")
        print(f"Simulation: {config.simulation_hdf5}")
        print(f"Gravity: {config.gravity}")
        print(f"Signals: {len(config.signals)}")
        _check_signals(config, strict_units=args.strict_units)
    return 0


def _config_as_dict(config: CaseConfig) -> dict[str, object]:
    return {
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


def _convert_res(args: argparse.Namespace) -> int:
    result = convert_res(
        args.input,
        output=args.output,
        binary=args.binary,
        force=args.force,
    )
    print(result.h5_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
