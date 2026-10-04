#!/usr/bin/env python3
"""Prepare uncompressed prex executables and SHA-256 checksums."""

import argparse
import hashlib
from pathlib import Path
import re
import shutil

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


def filename(version, platform):
    return f"prex-{version}-{platform}" + (".exe" if platform.startswith("windows-") else "")


def package(version, target, binary, output):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("version must be a safe semver without the leading v")
    binary, output = Path(binary), Path(output)
    if not binary.is_file() or binary.is_symlink():
        raise ValueError(f"missing or non-regular executable: {binary}")
    output.mkdir(parents=True, exist_ok=True)
    destination = output / filename(version, TARGETS[target][0])
    shutil.copyfile(binary, destination)
    destination.chmod(0o755)
    return destination


def checksums(directory):
    directory = Path(directory)
    files = sorted(p for p in directory.iterdir() if p.is_file() and p.name != "SHA256SUMS.txt")
    if not files:
        raise ValueError("no release files found")
    lines = []
    for path in files:
        digest = hashlib.sha256()
        with path.open("rb") as reader:
            for chunk in iter(lambda: reader.read(1024 * 1024), b""):
                digest.update(chunk)
        lines.append(f"{digest.hexdigest()}  {path.name}\n")
    output = directory / "SHA256SUMS.txt"
    output.write_text("".join(lines), encoding="utf-8", newline="\n")
    return output


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
