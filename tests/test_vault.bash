# env -i PATH="$PATH" VAULT_TOKEN="token" bash tests/test_vault.bash
set -e

bash tests/test_vault_setup.bash

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- on)"
echo e $E1 $E2 $E3 $E4

preexec_lade 'echo e $E1 $E2 $E3 $E4'
out=$(echo e $E1 $E2 $E3 $E4)
precmd_lade 'echo e $E1 $E2 $E3 $E4'

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- off)"
echo e $E1 $E2 $E3 $E4

expected="e itsasecret itsanotsecret secret a\nb"

if [[ "$out" != *"$expected"* ]]; then
  echo "Test failed: '$out' ≠ '$expected'"
  exit 1
else
  echo "Test passed bash"
fi

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
