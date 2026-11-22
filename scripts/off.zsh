# The line that evaluates this code went through the hook: restore the
# shell now, since the after-hook is gone once it ends.
(( $+functions[precmd_lade] )) && precmd_lade
# [@] filters per element. Unquoted so an unset array does not become ("").
preexec_functions=(${preexec_functions[@]:#preexec_lade})
precmd_functions=(${precmd_functions[@]:#precmd_lade})
(( $+functions[preexec_lade] )) && unset -f preexec_lade
(( $+functions[precmd_lade] )) && unset -f precmd_lade
