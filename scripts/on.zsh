preexec_lade() {
    if [ "$1" = "source off.zsh" ]; then
        return
    fi
    LADE="$1"
    eval "$(lade set $@)"
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -z ${LADE+x} ]; then
        return # ensure only runs at postexec
    elif [ "$LADE" = "source on.zsh" ]; then
        return
    fi
    eval "$(lade unset $@)"
    unset -v LADE
}

precmd_functions+=(precmd_lade)
