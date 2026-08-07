"""Command-line interface for KnC validation."""

import argparse
import json
from collections.abc import Sequence
from pathlib import Path

from knc_validation.hdf5 import Hdf5FormatError, inspect_hdf5


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="knc",
        description="Validate Adams/Car KnC results against physical measurements",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    inspect_parser = subparsers.add_parser(
        "inspect-h5", help="validate and summarize an adams-res HDF5 file"
    )
    inspect_parser.add_argument("path", type=Path, help="HDF5 result file")
    inspect_parser.add_argument("--json", action="store_true", help="write JSON output")
    inspect_parser.set_defaults(handler=_inspect_h5)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return int(args.handler(args))
    except (FileNotFoundError, Hdf5FormatError, OSError) as error:
        print(f"error: {error}")
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


if __name__ == "__main__":
    raise SystemExit(main())
