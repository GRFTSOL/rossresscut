#!/usr/bin/env bash
# Generates an import library from libmpv-2.dll.
#
# rsmpv-sys links with -lmpv, but the official Windows builds ship only the
# DLL, so the linker needs a matching import library. Point MPV_LIBRARY_DIR at
# the output directory:
#
#   ./scripts/mpv-import-lib.sh /path/to/libmpv-2.dll
#   MPV_LIBRARY_DIR=$PWD/dist/mpv-lib cargo build --release
#
# The MSVC tools (dumpbin + lib) are used when present; otherwise the LLVM
# tools (llvm-readobj + llvm-dlltool) are used, which works from Linux and
# macOS too. Both produce mpv.lib for the x86_64-pc-windows-msvc target.
set -euo pipefail

dll=${1:-}
out=${2:-dist/mpv-lib}
if [[ -z $dll ]]; then
    echo "usage: $0 <libmpv-2.dll> [output-directory]" >&2
    exit 2
fi
if [[ ! -f $dll ]]; then
    echo "error: $dll not found" >&2
    exit 1
fi

mkdir -p "$out"
def="$out/mpv.def"
lib="$out/mpv.lib"
dll_name=$(basename "$dll")

# 1. Collect the exported names.
#
# `dumpbin /exports` prints "ordinal hint RVA name" rows; the ordinal column
# keeps the table header and summary lines out of the result.
if command -v dumpbin >/dev/null 2>&1; then
    # MSYS rewrites the /exports argument into a path; keep it literal.
    MSYS_NO_PATHCONV=1 dumpbin /exports "$dll" |
        awk '$1 ~ /^[0-9]+$/ && NF == 4 && $4 ~ /^[A-Za-z_]/ { print "  " $4 }' > "$def.body"
elif command -v llvm-readobj >/dev/null 2>&1; then
    llvm-readobj --coff-exports "$dll" |
        awk '/^ *Name: / { print "  " $2 }' > "$def.body"
else
    echo "error: need dumpbin (MSVC) or llvm-readobj (LLVM) to read $dll" >&2
    exit 1
fi

if [[ ! -s $def.body ]]; then
    echo "error: no exports were parsed from $dll" >&2
    exit 1
fi

{
    echo "LIBRARY $dll_name"
    echo "EXPORTS"
    cat "$def.body"
} > "$def"
exports=$(wc -l < "$def.body")
rm -f "$def.body"

# 2. Build the import library.
if [[ ${OS:-} == Windows_NT ]] && command -v lib >/dev/null 2>&1; then
    MSYS_NO_PATHCONV=1 lib /def:"$def" /machine:x64 /out:"$lib"
elif command -v llvm-dlltool >/dev/null 2>&1; then
    llvm-dlltool -m i386:x86-64 -d "$def" -l "$lib"
elif command -v lib >/dev/null 2>&1; then
    lib /def:"$def" /machine:x64 /out:"$lib"
else
    echo "error: need lib.exe (MSVC) or llvm-dlltool (LLVM) to create $lib" >&2
    exit 1
fi

if [[ ! -s $lib ]]; then
    echo "error: $lib was not produced" >&2
    exit 1
fi

echo "import library: $lib ($exports exports)"
echo "next: MPV_LIBRARY_DIR=$PWD/$out cargo build --release"
