
preexec_lade() { 
    if [ "$1" = "source off.bash" ]; then
        return
    fi
    LADE="$1"
    eval $(lade set $argv)
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -z ${LADE+x} ]; then
        return # ensure only runs at postexec 
    elif [ "$LADE" = "source on.bash" ]; then
        return
    fi 
    eval $(lade unset $argv)
    unset -v LADE
}

precmd_functions+=(precmd_lade)
