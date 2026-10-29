#!/usr/bin/env bash
# Local CI gate. Mirrors the checks required by docs/development.md.
set -euo pipefail

cargo fmt --all --check
cargo check
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cargo test
cargo build
