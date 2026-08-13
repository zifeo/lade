preexec_lade() {
    case ${ZSH_EVAL_CONTEXT:-} in
        *cmdsubst*|*precmd*) return ;;
    esac
    if [ -z "$1" ] || [ "${1:0:5}" = "lade " ] || [ "$1" = "lade" ] || [ "$1" = "source off.zsh" ]; then
        return
    fi
    LADE="$1"
    eval "$(lade set -- "$LADE" </dev/null)"
}

precmd_lade() {
    if [ -n "${LADE+x}" ]; then
        if [ "$LADE" != "source on.zsh" ]; then
            eval "$(lade unset -- "$LADE" </dev/null)"
        fi
        unset -v LADE
    fi
}

autoload -Uz add-zsh-hook
add-zsh-hook preexec preexec_lade
add-zsh-hook precmd precmd_lade
