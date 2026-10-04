#!/usr/bin/env bash
set -euo pipefail

repository="${PREX_REPOSITORY:-kongdd/prex}"
version=latest
install_dir="${PREX_ROOT:-$HOME/.prex}/bin"
platform=""

usage() {
    cat <<'EOF'
Install prex from GitHub Releases after SHA-256 verification.
Usage: bash install.sh [--version v0.1.0] [--dir DIR] [--platform PLATFORM]

Defaults: latest release; ${PREX_ROOT:-$HOME/.prex}/bin.
Linux uses the portable static musl build; macOS uses its native architecture.
PREX_REPOSITORY overrides the GitHub owner/repository (default: kongdd/prex).
EOF
}
fail() { printf 'error: %s\n' "$*" >&2; exit 1; }
while (($#)); do
    case "$1" in
        --version|--dir|--platform)
            (($# >= 2)) || fail "$1 requires a value"
            case "$1" in
                --version) version="$2" ;;
                --dir) install_dir="$2" ;;
                --platform) platform="$2" ;;
            esac
            shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown option: $1" ;;
    esac
done
[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || fail 'invalid PREX_REPOSITORY'
[[ -n "$install_dir" ]] || fail 'installation directory cannot be empty'
for command in curl tar mktemp; do
    command -v "$command" >/dev/null || fail "missing required command: $command"
done
if command -v sha256sum >/dev/null; then
    hash_command=sha256sum
elif command -v shasum >/dev/null; then
    hash_command=shasum
else
    fail 'SHA-256 verification requires sha256sum or shasum'
fi

if [[ -z "$platform" ]]; then
    case "$(uname -m)" in
        x86_64|amd64) arch=amd64 ;;
        arm64|aarch64) arch=arm64 ;;
        *) fail "unsupported architecture: $(uname -m)" ;;
    esac
    case "$(uname -s)" in
        Linux) platform="linux-$arch-musl" ;;
        Darwin) platform="darwin-$arch" ;;
        *) fail 'use install.ps1 on Windows; this installer supports Linux and macOS' ;;
    esac
fi
case "$platform" in
    linux-amd64-gnu|linux-arm64-gnu|linux-amd64-musl|linux-arm64-musl|darwin-amd64|darwin-arm64) ;;
    *) fail "unsupported installer platform: $platform" ;;
esac

base="https://github.com/$repository/releases"
curl_options=(--fail --location --silent --show-error --retry 3 --connect-timeout 20 --max-time 300)
if [[ "$version" == latest ]]; then
    latest_url=$(curl "${curl_options[@]}" --output /dev/null --write-out '%{url_effective}' "$base/latest")
    version="${latest_url##*/}"
else
    version="v${version#v}"
fi
[[ "$version" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "invalid release version: $version"
asset="prex-${version#v}-$platform.tar.gz"
work=$(mktemp -d)
staged=""
cleanup() {
    rm -rf -- "$work"
    if [[ -n "$staged" ]]; then rm -f -- "$staged"; fi
}
trap cleanup EXIT
printf 'Downloading prex %s (%s)...\n' "$version" "$platform"
curl "${curl_options[@]}" --output "$work/$asset" "$base/download/$version/$asset"
curl "${curl_options[@]}" --output "$work/SHA256SUMS.txt" "$base/download/$version/SHA256SUMS.txt"
expected=$(awk -v name="$asset" '$2 == name {print $1}' "$work/SHA256SUMS.txt")
[[ "$expected" =~ ^[a-fA-F0-9]{64}$ ]] || fail "missing or ambiguous checksum for $asset"
if [[ "$hash_command" == sha256sum ]]; then
    actual=$(sha256sum "$work/$asset")
else
    actual=$(shasum -a 256 "$work/$asset")
fi
actual="${actual%% *}"
[[ "$actual" == "$expected" ]] || fail 'SHA-256 mismatch; nothing was installed'
tar -xzf "$work/$asset" -C "$work" prex
[[ -f "$work/prex" && ! -L "$work/prex" ]] || fail 'release does not contain a regular prex executable'
mkdir -p -- "$install_dir"
staged=$(mktemp "$install_dir/.prex-install.XXXXXX")
cp "$work/prex" "$staged"
chmod 755 "$staged"
mv -f -- "$staged" "$install_dir/prex"
staged=""
printf 'Installed %s to %s/prex\n' "$version" "$install_dir"
printf 'Add to PATH (and persist in your shell profile):\n  export PATH=%q:"$PATH"\n' "$install_dir"
printf 'Then run: prex init\n'
