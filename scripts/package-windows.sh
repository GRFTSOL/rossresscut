#!/usr/bin/env bash
# Builds the Windows release binary and produces the portable zip.
#
# Run on Windows from Git Bash, from a clean checkout. Environment:
#
#   MPV_LIBRARY_DIR  directory containing mpv.lib (scripts/mpv-import-lib.sh)
#   MPV_DLL          libmpv-2.dll to ship (default target/release/libmpv-2.dll)
#   FFMPEG_BIN       ffmpeg.exe (default: ffmpeg found on PATH)
#   FFPROBE_BIN      ffprobe.exe (default: ffprobe found on PATH)
#
# Output: dist/rossresscut-<version>-windows-x64/ and the matching .zip,
# plus a .sha256 file when a checksum tool is available.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
    echo "error: commit or stash tracked changes before packaging" >&2
    exit 1
fi

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [[ -z $version ]]; then
    echo "error: cannot read the version from Cargo.toml" >&2
    exit 1
fi

if [[ -z ${MPV_LIBRARY_DIR:-} ]]; then
    echo "error: set MPV_LIBRARY_DIR to the directory containing mpv.lib" >&2
    echo "       generate it with scripts/mpv-import-lib.sh" >&2
    exit 1
fi

mpv_dll=${MPV_DLL:-}
if [[ -z $mpv_dll ]]; then
    for candidate in target/release/libmpv-2.dll libmpv-2.dll; do
        if [[ -f $candidate ]]; then
            mpv_dll=$candidate
            break
        fi
    done
fi
if [[ -z $mpv_dll || ! -s $mpv_dll ]]; then
    echo "error: set MPV_DLL to the libmpv-2.dll that should ship" >&2
    exit 1
fi

ffmpeg_bin=${FFMPEG_BIN:-$(command -v ffmpeg || true)}
ffprobe_bin=${FFPROBE_BIN:-$(command -v ffprobe || true)}
if [[ -z $ffmpeg_bin ]]; then
    echo "error: ffmpeg.exe not found; set FFMPEG_BIN" >&2
    exit 1
fi
if [[ -z $ffprobe_bin ]]; then
    echo "error: ffprobe.exe not found; set FFPROBE_BIN" >&2
    exit 1
fi

# Integration test binaries live in target/debug/deps and load the DLL from
# their own directory, so place it there before running the test suite.
mkdir -p target/debug/deps
cp "$mpv_dll" target/debug/deps/libmpv-2.dll
cp "$mpv_dll" target/debug/libmpv-2.dll

# The release workflow sets PACKAGE_SKIP_CHECKS=1: its runner has no Vulkan
# runtime, so tests that load libmpv cannot start there. It runs
# `cargo test --no-default-features` instead.
if [[ ${PACKAGE_SKIP_CHECKS:-0} != 1 ]]; then
    ./scripts/check.sh
fi
cargo build --release --locked

if [[ $(head -c 2 target/release/rossresscut.exe) != "MZ" ]]; then
    echo "error: target/release/rossresscut.exe is not a Windows binary" >&2
    echo "       run this script on Windows" >&2
    exit 1
fi

stage="dist/rossresscut-$version-windows-x64"
rm -rf "$stage"
mkdir -p "$stage"
cp target/release/rossresscut.exe "$stage/"
cp "$mpv_dll" "$stage/libmpv-2.dll"
cp "$ffmpeg_bin" "$stage/ffmpeg.exe"
cp "$ffprobe_bin" "$stage/ffprobe.exe"
cp LICENSE ATTRIBUTION.md "$stage/"
sed -e "s/@VERSION@/$version/g" -e "s/@DATE@/$(date +%Y-%m-%d)/g" \
    packaging/README-WINDOWS.txt > "$stage/README-WINDOWS.txt"

for file in rossresscut.exe libmpv-2.dll ffmpeg.exe ffprobe.exe LICENSE \
    ATTRIBUTION.md README-WINDOWS.txt; do
    if [[ ! -s "$stage/$file" ]]; then
        echo "error: packaging did not produce $stage/$file" >&2
        exit 1
    fi
done

zip_path="dist/rossresscut-$version-windows-x64.zip"
rm -f "$zip_path"
if command -v bsdtar >/dev/null 2>&1; then
    bsdtar -a -c -f "$zip_path" -C dist "$(basename "$stage")"
elif [[ -x /c/Windows/System32/tar.exe ]]; then
    # Windows ships bsdtar as tar.exe; the MSYS tar cannot write zip files.
    /c/Windows/System32/tar.exe -a -c -f "$zip_path" -C dist "$(basename "$stage")"
elif command -v zip >/dev/null 2>&1; then
    (cd dist && zip -qr "$(basename "$zip_path")" "$(basename "$stage")")
else
    echo "error: need bsdtar, Windows tar.exe or zip to create the archive" >&2
    exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$zip_path" > "$zip_path.sha256"
elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$zip_path" > "$zip_path.sha256"
fi

echo "package: $zip_path"
if [[ -f $zip_path.sha256 ]]; then
    cat "$zip_path.sha256"
fi
