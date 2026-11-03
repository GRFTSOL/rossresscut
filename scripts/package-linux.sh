#!/usr/bin/env bash
# Builds the Linux release binary and packages it as a pacman archive.
#
# Run from a clean checkout.  The package is built from that checkout, so the
# binary, the version in Cargo.toml/packaging/PKGBUILD and the commit embedded
# by build.rs stay in sync.
set -euo pipefail

cd "$(dirname "$0")/.."
root=$PWD

if [[ -n $(git status --porcelain --untracked-files=no) ]]; then
    echo "error: commit or stash tracked changes before packaging" >&2
    exit 1
fi

cargo_version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
pkg_version=$(sed -n 's/^pkgver=//p' packaging/PKGBUILD)
if [[ -z $cargo_version || $cargo_version != "$pkg_version" ]]; then
    echo "error: Cargo.toml version ($cargo_version) and packaging/PKGBUILD pkgver ($pkg_version) differ" >&2
    exit 1
fi

./scripts/check.sh
cargo build --release --locked

mkdir -p dist
(
    cd packaging
    PKGEXT='.pkg.tar.zst' PKGDEST="$root/dist" makepkg --force
)

package=$(find "$root/dist" -maxdepth 1 -name "rossresscut-$cargo_version-*.pkg.tar.zst" -print -quit)
if [[ -z $package ]]; then
    echo "error: makepkg did not produce a package" >&2
    exit 1
fi

pacman -Qip "$package"
echo "package: $package"
