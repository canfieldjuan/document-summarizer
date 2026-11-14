"""Offline packaging regression: retained vendor bytes, enforced app formatting.

Run: python3 scripts/test_vendor_formatting.py
Uses real vendor configs and the unchanged CI cargo-fmt command in a tiny
throwaway path-dependency graph. No application files or network are touched.
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BAD = b"pub fn probe( ){let _x=1;}\n"
GOOD = b"pub fn probe() {\n    let _x = 1;\n}\n"


class VendorFormatting(unittest.TestCase):
    def test_vendor_preservation_does_not_exempt_application(self):
        with tempfile.TemporaryDirectory(prefix="docsum-format-") as directory:
            root = Path(directory)
            app = root / "src/lib.rs"
            app.parent.mkdir()
            app.write_bytes(GOOD)
            dependencies = []
            vendor_sources = []
            for index, name in enumerate(("pdf-extract", "lopdf")):
                vendor = root / "vendor" / name
                (vendor / "src").mkdir(parents=True)
                config = ROOT / "src-tauri/vendor" / name / "rustfmt.toml"
                if config.exists():
                    (vendor / "rustfmt.toml").write_bytes(config.read_bytes())
                (vendor / "Cargo.toml").write_text(
                    f'[package]\nname = "fixture{index}"\nversion = "0.0.0"\nedition = "2021"\n'
                )
                source = vendor / "src/lib.rs"
                source.write_bytes(BAD)
                vendor_sources.append(source)
                dependencies.append(f'fixture{index} = {{ path = "vendor/{name}" }}')
            (root / "Cargo.toml").write_text(
                '[package]\nname = "format-probe"\nversion = "0.0.0"\nedition = "2021"\n'
                '[dependencies]\n' + "\n".join(dependencies) + "\n[workspace]\n"
            )

            def check():
                return subprocess.run(
                    ["cargo", "fmt", "--all", "--", "--check"], cwd=root,
                    env={**os.environ, "CARGO_NET_OFFLINE": "true"},
                    capture_output=True, text=True, timeout=60,
                )

            result = check()
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            app.write_bytes(BAD)
            result = check()
            self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
            self.assertIn(str(app), result.stdout)
            self.assertNotIn("Diff in " + str(root / "vendor"), result.stdout)
            app.write_bytes(GOOD)
            result = check()
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            for source in vendor_sources:
                self.assertEqual(source.read_bytes(), BAD)


if __name__ == "__main__":
    unittest.main()
