from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path

SCRIPT_PATH = Path(__file__).with_name("package_loadable_extension.py")
SPEC = importlib.util.spec_from_file_location("package_loadable_extension", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PackageLoadableExtensionTests(unittest.TestCase):
    def test_appends_duckdb_v1_metadata_footer(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            extension = Path(temp_dir) / "pgquack.duckdb_extension"
            extension.write_bytes(b"shared-library-bytes")

            MODULE.append_duckdb_metadata(
                extension,
                platform="windows_amd64",
                duckdb_version="v1.2.2",
                extension_version="v0.3.0",
                abi_type="C_STRUCT",
            )

            payload = extension.read_bytes()
            footer = payload[len(b"shared-library-bytes") :]
            self.assertEqual(footer[:4], b"\x00\x93\x04\x10")
            self.assertEqual(footer[4:20], b"duckdb_signature")
            self.assertEqual(len(footer), 534)
            self.assertEqual(footer[118:150].rstrip(b"\x00"), b"C_STRUCT")
            self.assertEqual(footer[150:182].rstrip(b"\x00"), b"v0.3.0")
            self.assertEqual(footer[182:214].rstrip(b"\x00"), b"v1.2.2")
            self.assertEqual(footer[214:246].rstrip(b"\x00"), b"windows_amd64")
            self.assertEqual(footer[246:278].rstrip(b"\x00"), b"4")


if __name__ == "__main__":
    unittest.main()
