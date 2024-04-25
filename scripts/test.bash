# env -i PATH="$PATH" bash scripts/test.bash

echo "test=$TEST"
eval "$(cargo run -- on)"
echo "test=$TEST"

preexec_lade 'echo "test=$TEST"'
echo "test=$TEST"
precmd_lade 'echo "test=$TEST"'

echo "test=$TEST"
eval "$(cargo run -- off)"
echo "test=$TEST"

preexec_lade
precmd_lade
