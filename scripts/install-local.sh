#!/bin/sh
# Build and install lade and age-plugin-lade from this checkout.
set -e -u

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo clean
cargo install --path . --locked --force
cargo install --path crates/age-plugin-lade --locked --force
