source bash-preexec.sh
source off.bash

preexec_lade() { 
    if [ "$1" = "source off.bash" ]; then
        return
    fi
    LADE=$1
    echo lade on $1
}

preexec_functions+=(preexec_lade)

precmd_lade() {
    if [ -z ${LADE+x} ]; then
        return # ensure only runs at postexec 
    elif [ "$LADE" = "source on.bash" ]; then
        return
    fi 
    unset -v A
    echo lade off $LADE
    unset -v LADE
}

precmd_functions+=(precmd_lade)
