# ${arr[@]/name/} is replace, not delete, and leaves empty slots.
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
