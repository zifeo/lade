#!/bin/sh
# Build and install lade and age-plugin-lade from this checkout.
set -e -u

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

clean=0
for arg in "$@"; do
  case "$arg" in
    --clean) clean=1 ;;
    *)
      echo "unknown argument: $arg" >&2
      echo "usage: $0 [--clean]" >&2
      exit 1
      ;;
  esac
done

if [ "$clean" -eq 1 ]; then
  cargo clean
fi
cargo install --path . --locked --force
cargo install --path crates/age-plugin-lade --locked --force
# Ad hoc, no Apple account: the hub caches secrets only behind hardened
# runtime, which cargo never sets.
if [ "$(uname -s)" = Darwin ]; then
  codesign --force --sign - --options runtime \
    "${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin/lade"
fi
