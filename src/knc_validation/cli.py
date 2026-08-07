"""Command-line interface for KnC validation."""

import argparse
import json
import sys
from collections.abc import Sequence
from pathlib import Path

from knc_validation.acd import AcdError, AcdFile, inspect_acd
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
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return int(args.handler(args))
    except (
        FileNotFoundError,
        AcdError,
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


if __name__ == "__main__":
    raise SystemExit(main())
