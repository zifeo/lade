# ${arr[@]/name/} is replace, not delete, and leaves empty slots.
__lade_keep=()
for __lade_f in "${preexec_functions[@]}"; do
    [ "$__lade_f" = "preexec_lade" ] || __lade_keep+=("$__lade_f")
done
preexec_functions=("${__lade_keep[@]}")
__lade_keep=()
for __lade_f in "${precmd_functions[@]}"; do
    [ "$__lade_f" = "precmd_lade" ] || __lade_keep+=("$__lade_f")
done
precmd_functions=("${__lade_keep[@]}")
unset -v __lade_keep __lade_f
unset -f preexec_lade
unset -f precmd_lade
