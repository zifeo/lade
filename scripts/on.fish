function preexec_lade --on-event fish_preexec
    if test (string sub -l 5 -- "$argv") = "lade "; or test "$argv" = "lade"; or test "$argv" = "source off.fish"
        return
    end
    set --global LADE "$argv"
    source (lade set $argv | psub)
end

function precmd_lade --on-event fish_postexec
    if set -q LADE
        if test "$LADE" != "source on.fish"
            source (lade unset $argv | psub)
        end
        set --global --erase LADE
    end
end
