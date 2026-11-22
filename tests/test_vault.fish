# Run by `cargo test --test docker vault`, from a directory whose lade.yml holds
# the `^echo e` rule of tests/docker/vault.rs.

set -e

set -x LADE_VAULT_HTTP 1
bash (dirname (status filename))/test_vault_setup.bash
# A `cd` in a fish command substitution moves the shell itself.
set lade_bin (path resolve (dirname (status filename))/..)/target/debug/lade

echo e $E1 $E2 $E3 $E4
# The test runs under a temp HOME without cargo's state: the debug hooks call
# the built binary instead of `cargo run`.
$lade_bin on | string replace -a 'cargo run -- ' "$lade_bin " | source
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
