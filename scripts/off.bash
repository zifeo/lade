preexec_functions=("${preexec_functions[@]/preexec_lade/}")
precmd_functions=("${precmd_functions[@]/precmd_lade/}")
unset -f preexec_lade
unset -f precmd_lade
