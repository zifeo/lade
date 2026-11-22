# The line that evaluates this code went through the hook: restore the
# shell now, since the after-hook is gone once it ends.
# § one jfxr4rmae.b@94z7j
if declare -F precmd_lade >/dev/null; then
    precmd_lade
fi
# ${arr[@]/name/} is replace, not delete, and leaves empty slots.
# § vqrcg1d6p.b@pctz3
__lade_keep=()
for __f in "${preexec_functions[@]}"; do
    [ "$__f" = "preexec_lade" ] || __lade_keep+=("$__f")
done
preexec_functions=("${__lade_keep[@]}")
__lade_keep=()
for __f in "${precmd_functions[@]}"; do
    [ "$__f" = "precmd_lade" ] || __lade_keep+=("$__f")
done
precmd_functions=("${__lade_keep[@]}")
unset -v __lade_keep __f
unset -f preexec_lade
unset -f precmd_lade
# § end vqrcg1d6p.b
