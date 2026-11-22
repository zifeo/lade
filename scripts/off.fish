# The line that evaluates this code went through the hook: restore the
# shell now, since the after-hook is gone once it ends.
functions -q precmd_lade; and precmd_lade
functions --erase preexec_lade
functions --erase precmd_lade
