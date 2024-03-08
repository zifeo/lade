preexec_lade() {
    if [ "$1" = "source off.zsh" ]; then
        return
    fi
    LADE=$1
    if [ "$(uname)" = "Darwin" ]; then
        # bugfix: macOs seems to triple argv
        argv=${argv:1:$((${#argv[@]} / 3))}
    fi
    eval "$(lade set $argv)"
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -z ${LADE+x} ]; then
        return # ensure only runs at postexec
    elif [ "$LADE" = "source on.zsh" ]; then
        return
    fi
    if [ "$(uname)" = "Darwin" ]; then
        # bugfix: macOs seems to triple argv
        argv=${argv:1:$((${#argv[@]} / 3))}
    fi
    eval "$(lade unset $argv)"
    unset -v LADE
}

precmd_functions+=(precmd_lade)
