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

type -q preexec_lade
if test $status -eq 0
    echo "preexec_lade should not exist after lade off"
    exit 1
else
    echo "preexec_lade correctly removed"
end

type -q precmd_lade
if test $status -eq 0
    echo "precmd_lade should not exist after lade off"
    exit 1
else
    echo "precmd_lade correctly removed"
end
