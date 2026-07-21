#!/usr/bin/env python
"""Package a Rust cdylib as an unsigned DuckDB C API extension."""

from __future__ import annotations

import argparse
import shutil
from pathlib import Path

_METADATA_FIELD_SIZE = 32
_SIGNATURE_SIZE = 256
_METADATA_VERSION = "4"


def _metadata_field(value: str) -> bytes:
    encoded = value.encode("ascii")
    if len(encoded) > _METADATA_FIELD_SIZE:
        raise ValueError(f"DuckDB extension metadata field is too long: {value!r}")
    return encoded.ljust(_METADATA_FIELD_SIZE, b"\0")


def append_duckdb_metadata(
    extension: Path,
    *,
    platform: str,
    duckdb_version: str,
    extension_version: str,
    abi_type: str,
) -> None:
    """Append the unsigned DuckDB v1 extension metadata footer in-place.

    This is the same binary layout used by DuckDB's
    ``scripts/append_metadata.cmake``. It makes a shared library loadable by
    a matching DuckDB CLI; it does not sign the extension for registry use.
    """

    fields = [
        "",  # metadata 8
        "",  # metadata 7
        "",  # metadata 6
        abi_type,  # metadata 5
        extension_version,  # metadata 4
        duckdb_version,  # metadata 3
        platform,  # metadata 2
        _METADATA_VERSION,  # metadata 1
    ]
    footer = (
        b"\0\x93\x04\x10duckdb_signature\x80\x04"
        + b"".join(_metadata_field(field) for field in fields)
        + (b"\0" * _SIGNATURE_SIZE)
    )
    with extension.open("ab") as output:
        output.write(footer)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--duckdb-version", required=True)
    parser.add_argument("--extension-version", required=True)
    parser.add_argument("--abi-type", default="C_STRUCT")
    args = parser.parse_args()

    if not args.source.is_file():
        parser.error(f"source shared library does not exist: {args.source}")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(args.source, args.output)
    append_duckdb_metadata(
        args.output,
        platform=args.platform,
        duckdb_version=args.duckdb_version,
        extension_version=args.extension_version,
        abi_type=args.abi_type,
    )
    print(args.output)


if __name__ == "__main__":
    main()
