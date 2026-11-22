preexec_lade() {
    # § one vqrcg1d6p.a@txndp
    if [ "${1:0:5}" = "lade " ] || [ "$1" = "lade" ] || [ "$1" = "source off.bash" ] || [ "$1" = 'eval "$(lade off)"' ]; then
        return
    fi
    LADE="$1"
    eval "$(lade set -- "$LADE")"
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    # § jfxr4rmae.b@1c32z
    if [ -n "${LADE+x}" ]; then
        if [ "$LADE" != "source on.bash" ]; then
            eval "$(lade unset -- "$LADE")"
        fi
        unset -v LADE
    fi
}

precmd_functions+=(precmd_lade)
