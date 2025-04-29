# env -i PATH="$PATH" VAULT_TOKEN="token" zsh tests/test_vault.zsh

set -e

bash tests/test_vault_setup.bash

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- on)"
# hooks seem to work in zsh scripts
preexec_functions=()
precmd_functions=()
echo e $E1 $E2 $E3 $E4

preexec_lade 'echo e $E1 $E2 $E3 $E4'
out=$(echo e $E1 $E2 $E3 $E4)
precmd_lade 'echo e $E1 $E2 $E3 $E4'

echo e $E1 $E2 $E3 $E4
eval "$(cargo run -- off)"
echo e $E1 $E2 $E3 $E4

expected=$'e itsasecret itsanotsecret secret a\nb'

if [[ "$out" != *"$expected"* ]]; then
  echo "Test zsh failed: '$out' ≠ '$expected'"
  exit 1
else
  echo "Test zsh passed"
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

