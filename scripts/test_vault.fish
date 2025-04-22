# env -i PATH="$PATH" VAULT_TOKEN="token" fish scripts/test_vault.fish

bash scripts/test_vault_setup.bash

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- on)"
echo e $E1 $E2 $E3 $E4

preexec_lade 'echo e $E1 $E2 $E3 $E4'
set out $(echo e $E1 $E2 $E3 $E4)
precmd_lade 'echo e $E1 $E2 $E3 $E4'

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- off)"
echo e $E1 $E2 $E3 $E4

set expected "e itsasecret itsanotsecret secret a\nb"

if not string match -q "*$expected*" $out
    echo "Test failed: '$out' ≠ '$expected'"
    exit 1
else
    echo "Test passed"
end

if functions -q preexec_lade
    echo "preexec_lade should not exist after lade off"
    exit 1
else
    echo "preexec_lade correctly removed"
end

if functions -q precmd_lade
    echo "precmd_lade should not exist after lade off"
    exit 1
else
    echo "precmd_lade correctly removed"
end
