# env -i PATH="$PATH" fish scripts/test.fish

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
