function preexec_lade --on-event fish_preexec
    if test (string sub -l 5 -- "$argv") = "lade "; or test "$argv" = "lade"; or test "$argv" = "source off.fish"
        return
    end
    set --global LADE "$argv[1]"
    source (lade set -- "$LADE" </dev/null | psub)
end

function precmd_lade --on-event fish_postexec
    if set -q LADE
        if test "$LADE" != "source on.fish"
            source (lade unset -- "$LADE" </dev/null | psub)
        end
        set --global --erase LADE
    end
end
