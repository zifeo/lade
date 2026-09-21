# env -i PATH="$PATH" VAULT_TOKEN="token" LADE_VAULT_HTTP=1 fish tests/test_vault.fish

set -e

set -x LADE_VAULT_HTTP 1
bash tests/test_vault_setup.bash
set lade_bin (cd (dirname (status filename))/.. && pwd)/target/debug/lade

echo e $E1 $E2 $E3 $E4
eval "$($lade_bin on)"
echo e $E1 $E2 $E3 $E4

preexec_lade 'echo e $E1 $E2 $E3 $E4'
set out $(echo e $E1 $E2 $E3 $E4)
precmd_lade 'echo e $E1 $E2 $E3 $E4'

echo e $E1 $E2 $E3 $E4
eval "$($lade_bin off)"
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
