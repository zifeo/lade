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

preexec_lade
precmd_lade
