
function preexec_lade --on-event fish_preexec
    if [ $argv = "source off.fish" ]
        return
    end
    set --global LADE "$argv"
    eval (lade set $argv)
end

function postexec_lade --on-event fish_postexec
    # $argv also exists here in fish, but keeping LADE for consistency
    if [ "$LADE" = "source on.fish" ]
        return
    end
    eval (lade unset $argv)
    set --global --erase LADE
end
