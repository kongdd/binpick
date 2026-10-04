import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "package_release", Path(__file__).resolve().parent.parent / "scripts/package_release.py"
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_all_targets_are_uncompressed_executables(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "binary"
            binary.write_bytes(b"fixture executable")
            output = root / "dist"
            for target, (platform, _) in release.TARGETS.items():
                result = release.package("0.1.1", target, binary, output)
                self.assertEqual(result.name, release.filename("0.1.1", platform))
                self.assertEqual(result.read_bytes(), binary.read_bytes())
                self.assertFalse(result.name.endswith((".zip", ".tar.gz")))
            (output / "LICENSE").write_text("license", encoding="utf-8")
            sums = release.checksums(output).read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(sums), len(release.TARGETS) + 1)
            for line in sums:
                digest, filename = line.split("  ")
                self.assertEqual(digest, hashlib.sha256((output / filename).read_bytes()).hexdigest())
            self.assertEqual(release.checksums(output).read_text().splitlines(), sums)

    def test_rejects_unsafe_version_and_missing_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for version in ("../../unsafe", "0.1.1"):
                with self.assertRaises(ValueError):
                    release.package(version, next(iter(release.TARGETS)), root / "missing", root)
            with self.assertRaises(ValueError):
                release.checksums(root)


if __name__ == "__main__":
    unittest.main()
