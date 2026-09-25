#!/usr/bin/env bash
# Unit test for scripts/changelog-section.sh.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRIPT="$ROOT/scripts/changelog-section.sh"

fail() {
  echo "FAIL: $*" >&2
  exit 1
}

if command -v shellcheck >/dev/null 2>&1; then
  shellcheck "$SCRIPT" || fail "shellcheck reported issues"
else
  fail "shellcheck not installed"
fi

notes="$("$SCRIPT" "$ROOT/CHANGELOG.md" 0.19.0)"
first="$(printf '%s\n' "$notes" | head -n 1)"
third="$(printf '%s\n' "$notes" | sed -n '3p')"
last="$(printf '%s\n' "$notes" | tail -n 1)"
[[ "$first" == "## [0.19.0]" ]] || fail "0.19.0 heading: $first"
[[ "$third" == "Since 0.18.0, a repo gets its CLIs from mise in one of two ways." ]] || fail "0.19.0 body: $third"
[[ "$last" == "[0.19.0]: https://github.com/zifeo/lade/compare/v0.18.0...v0.19.0" ]] || fail "0.19.0 link: $last"
printf '%s\n' "$notes" | grep -q '^## \[0.18.0\]' && fail "0.19.0 leaked the next section"

short="$("$SCRIPT" "$ROOT/CHANGELOG.md" 0.14.2)"
[[ "$short" == $'## [0.14.2]\n\n[0.14.2]: https://github.com/zifeo/lade/compare/v0.14.1...v0.14.2' ]] || fail "0.14.2 body: $short"

if "$SCRIPT" "$ROOT/CHANGELOG.md" 9.9.9 >/dev/null; then
  fail "missing section should fail"
fi

work="$(mktemp)"
trap 'rm -f "$work"' EXIT
cat >"$work" <<'EOF'
## [0.19.0] - 2026-09-25

body 0.19.0

## [0.19] - 2026-01-01

body 0.19

## [1.2.3] - 2026-01-01

## [1.2.0] - 2026-01-01

body 1.2.0
EOF

prefix="$("$SCRIPT" "$work" 0.19)"
[[ "$prefix" == $'## [0.19]\n\nbody 0.19' ]] || fail "prefix match: $prefix"

if "$SCRIPT" "$work" 1.2.3 >/dev/null; then
  fail "empty section should fail"
fi

echo "ok"
