# env -i PATH="$PATH" zsh scripts/test.zsh

echo "test=$TEST"
eval "$(cargo run -- on)"
# hooks seem to work in zsh scripts
preexec_functions=()
precmd_functions=()
echo "test=$TEST"

preexec_lade 'echo "test=$TEST"'
echo "test=$TEST"
precmd_lade 'echo "test=$TEST"'

echo "test=$TEST"
eval "$(cargo run -- off)"
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
