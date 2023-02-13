source off.zsh

preexec_lade() { 
    if [ "$1" = "source off.zsh" ]; then
        return
    fi
    LADE=$1
    echo lade on $1
    declare -g A=1
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -z ${LADE+x} ]; then
        return # ensure only runs at postexec 
    elif [ "$LADE" = "source on.zsh" ]; then
        return
    fi 
    unset -v A
    echo lade off $LADE
    unset -v LADE
}

precmd_functions+=(precmd_lade)
