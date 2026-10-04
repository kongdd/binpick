#!/usr/bin/env python3
"""Package standalone prex binaries and generate sha256sum-compatible checksums."""

import argparse
import hashlib
from pathlib import Path
import re
import tarfile
import zipfile

TARGETS = {
    "x86_64-unknown-linux-gnu": ("linux-amd64-gnu", "ubuntu-22.04"),
    "aarch64-unknown-linux-gnu": ("linux-arm64-gnu", "ubuntu-24.04-arm"),
    "x86_64-unknown-linux-musl": ("linux-amd64-musl", "ubuntu-22.04"),
    "aarch64-unknown-linux-musl": ("linux-arm64-musl", "ubuntu-24.04-arm"),
    "x86_64-apple-darwin": ("darwin-amd64", "macos-15-intel"),
    "aarch64-apple-darwin": ("darwin-arm64", "macos-15"),
    "x86_64-pc-windows-msvc": ("windows-amd64", "windows-2022"),
    "aarch64-pc-windows-msvc": ("windows-arm64", "windows-11-arm"),
}
ROOT = Path(__file__).resolve().parent.parent


def package(version, target, binary, output, source=ROOT):
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?", version):
        raise ValueError("version must be a safe semver without the leading v")
    platform, _ = TARGETS[target]
    windows = platform.startswith("windows-")
    executable = "prex.exe" if windows else "prex"
    files = [(Path(binary), executable), (source / "LICENSE", "LICENSE"),
             (source / "README.md", "README.md")]
    for path, _ in files:
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"missing or non-regular release input: {path}")
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    extension = "zip" if windows else "tar.gz"
    archive = output / f"prex-{version}-{platform}.{extension}"
    if windows:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as writer:
            for path, name in files:
                writer.write(path, arcname=name)
    else:
        with tarfile.open(archive, "w:gz") as writer:
            for path, name in files:
                info = writer.gettarinfo(str(path), arcname=name)
                info.mode = 0o755 if name == executable else 0o644
                info.uid = info.gid = 0
                info.uname = info.gname = ""
                with path.open("rb") as reader:
                    writer.addfile(info, reader)
    return archive


def checksums(directory):
    directory = Path(directory)
    archives = sorted(list(directory.glob("prex-*.tar.gz")) + list(directory.glob("prex-*.zip")))
    if not archives:
        raise ValueError("no release archives found")
    lines = []
    for archive in archives:
        digest = hashlib.sha256()
        with archive.open("rb") as reader:
            for chunk in iter(lambda: reader.read(1024 * 1024), b""):
                digest.update(chunk)
        lines.append(f"{digest.hexdigest()}  {archive.name}\n")
    path = directory / "SHA256SUMS.txt"
    path.write_text("".join(lines), encoding="utf-8", newline="\n")
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build = commands.add_parser("package")
    build.add_argument("--version", required=True)
    build.add_argument("--target", choices=TARGETS, required=True)
    build.add_argument("--binary", type=Path)
    build.add_argument("--out-dir", type=Path, default=ROOT / "dist")
    sums = commands.add_parser("checksums")
    sums.add_argument("--dir", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    if args.command == "checksums":
        print(checksums(args.dir))
    else:
        executable = "prex.exe" if "windows" in args.target else "prex"
        binary = args.binary or ROOT / "target" / args.target / "release" / executable
        print(package(args.version, args.target, binary, args.out_dir))


if __name__ == "__main__":
    main()
