# env -i PATH="$PATH" zsh scripts/test.zsh
# cargo test sets LADE_BIN so this does not rebuild target/debug/lade.

lade_cli() {
  if [ -n "${LADE_BIN:-}" ]; then
    "$LADE_BIN" "$@"
  else
    cargo run -- "$@"
  fi
}

echo "test=$TEST"
eval "$(lade_cli on)"
# hooks seem to work in zsh scripts
preexec_functions=()
precmd_functions=()
echo "test=$TEST"

preexec_lade 'echo "test=$TEST"'
echo "test=$TEST"
precmd_lade

echo "test=$TEST"
eval "$(lade_cli off)"
echo "test=$TEST"

if type preexec_lade >/dev/null 2>&1; then
  echo "preexec_lade should not exist after lade off"
  exit 1
else
  echo "preexec_lade correctly removed"
fi

if type precmd_lade >/dev/null 2>&1; then
  echo "precmd_lade should not exist after lade off"
  exit 1
else
  echo "precmd_lade correctly removed"
fi
