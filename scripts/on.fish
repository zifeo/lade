source off.fish

function preexec_lade --on-event fish_preexec
    if [ $argv = "source off.fish" ]
        return
    end
    echo lade on $argv
    set --global A 1
end

function postexec_lade --on-event fish_postexec
    if [ $argv = "source on.fish" ]
        return
    end
    set --global --erase A
    echo lade off $argv
end
