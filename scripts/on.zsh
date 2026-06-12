preexec_lade() {
    if [ "${1:0:5}" = "lade " ] || [ "$1" = "lade" ] || [ "$1" = "source off.zsh" ]; then
        return
    fi
    LADE="$1"
    eval "$(lade set $@)"
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -n "${LADE+x}" ]; then
        if [ "$LADE" != "source on.zsh" ]; then
            eval "$(lade unset $@)"
        fi
        unset -v LADE
    fi
}

precmd_functions+=(precmd_lade)
