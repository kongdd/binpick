import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location(
    "package_release", Path(__file__).resolve().parent.parent / "scripts/package_release.py"
)
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def test_all_targets_contain_only_binary_license_and_readme(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "LICENSE").write_text("license", encoding="utf-8")
            (root / "README.md").write_text("readme", encoding="utf-8")
            binary = root / "binary"
            binary.write_bytes(b"fixture executable")
            output = root / "dist"
            for target, (platform, _) in release.TARGETS.items():
                archive = release.package("0.1.0", target, binary, output, root)
                executable = "prex.exe" if platform.startswith("windows") else "prex"
                expected = {executable, "LICENSE", "README.md"}
                if archive.suffix == ".zip":
                    with zipfile.ZipFile(archive) as reader:
                        self.assertEqual(set(reader.namelist()), expected)
                        self.assertEqual(reader.read(executable), binary.read_bytes())
                else:
                    with tarfile.open(archive) as reader:
                        self.assertEqual(set(reader.getnames()), expected)
                        self.assertEqual(reader.getmember(executable).mode, 0o755)
                        self.assertEqual(reader.extractfile(executable).read(), binary.read_bytes())
            sums = release.checksums(output).read_text(encoding="utf-8").splitlines()
            self.assertEqual(len(sums), len(release.TARGETS))
            for line in sums:
                digest, filename = line.split("  ")
                self.assertEqual(digest, hashlib.sha256((output / filename).read_bytes()).hexdigest())

    def test_rejects_unsafe_version_and_missing_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(ValueError):
                release.package("../../unsafe", next(iter(release.TARGETS)), root / "missing", root)
            with self.assertRaises(ValueError):
                release.package("0.1.0", next(iter(release.TARGETS)), root / "missing", root)
            with self.assertRaises(ValueError):
                release.checksums(root)


if __name__ == "__main__":
    unittest.main()
