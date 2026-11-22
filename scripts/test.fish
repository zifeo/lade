# env -i PATH="$PATH" fish scripts/test.fish
# cargo test sets LADE_BIN so this does not rebuild target/debug/lade.

function lade_cli
  if set -q LADE_BIN
    $LADE_BIN $argv
  else
    cargo run -- $argv
  end
end

function preexec_other --on-event fish_preexec; end

echo "test=$TEST"
eval "$(lade_cli on)"
echo "test=$TEST"

preexec_lade 'echo "test=$TEST"'
echo "test=$TEST"
precmd_lade

preexec_lade 'lade status'
if set -q LADE
    echo "preexec_lade should skip a lade line"
    exit 1
end

echo "test=$TEST"
eval "$(lade_cli off)"
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

if not functions -q preexec_other
    echo "lade off should keep another hook"
    exit 1
end
