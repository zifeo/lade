
function preexec_lade --on-event fish_preexec
    if [ $argv = "source off.fish" ]
        return
    end
    set --global LADE "$argv"
    source (lade set $argv | psub)
end

function precmd_lade --on-event fish_postexec
    # $argv also exists here in fish, but keeping LADE for consistency
    if [ "$LADE" = "source on.fish" ]
        return
    end
    source (lade unset $argv | psub)
    set --global --erase LADE
end
