use anyhow::{Context, Result, bail};
use std::{collections::HashMap, path::PathBuf, str::FromStr};

/// A `PATH` with no directory in it, while Lade blocks a command.
const BLOCKED_PATH: &str = "/lade-blocked-this-command";
const CANCELLED: &str = "Lade cancelled";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Sh,
}

impl FromStr for Shell {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        match name {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            "sh" => Ok(Shell::Sh),
            _ => bail!("Unsupported shell: {name}"),
        }
    }
}

impl Shell {
    pub fn bin(&self) -> &str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Sh => "sh",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Shell::Bash => "Bash",
            Shell::Zsh => "Zsh",
            Shell::Fish => "Fish",
            Shell::Sh => "sh",
        }
    }

    /// Argv after the binary for a non-interactive `-c` that must not source
    /// user startup files. `fish -c` reads `config.fish` and `zsh -c` reads
    /// `.zshenv`; either can overwrite resolved secrets after process env is
    /// applied. `bash -c` does not read `.bashrc` / `.bash_profile`; its
    /// overwrite vector is `$BASH_ENV`, cleared on the child `Command`.
    /// `--norc --noprofile` are omitted: they do not skip `BASH_ENV`, and
    /// `bash -c` does not read `.bashrc` or login profiles anyway. Preexec
    /// does not use this: it evals `set()` in the already-running
    /// interactive shell.
    pub fn noninteractive_args<'a>(&self, command: &'a str) -> Vec<&'a str> {
        match self {
            Shell::Fish => vec!["--no-config", "-c", command],
            Shell::Zsh => vec!["-f", "-c", command],
            Shell::Bash | Shell::Sh => vec!["-c", command],
        }
    }

    pub fn prepare_command(&self, command: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.bin());
        cmd.args(self.noninteractive_args(command));
        cmd
    }

    /// Startup file `inject` skips when present. `None` for bash/sh: those
    /// `-c` invocations do not read a user file unless `$BASH_ENV` is set.
    pub fn wrap_startup_file(&self) -> Option<PathBuf> {
        let home = directories::UserDirs::new()?.home_dir().to_path_buf();
        match self {
            Shell::Fish => Some(home.join(".config/fish/config.fish")),
            Shell::Zsh => Some(home.join(".zshenv")),
            Shell::Bash | Shell::Sh => None,
        }
    }

    pub fn detect() -> Result<Shell> {
        if let Ok(shell_env) = std::env::var("LADE_SHELL") {
            let path = std::path::Path::new(&shell_env);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(shell_env.as_str());
            return Shell::from_str(name);
        }

        let name = parent_process_name()?;
        shell_from_comm(&name)
    }

    pub(super) fn export_lade_shell(&self) -> String {
        match self {
            Shell::Bash | Shell::Zsh | Shell::Sh => {
                format!("export LADE_SHELL={}", self.bin())
            }
            Shell::Fish => format!("set --global --export LADE_SHELL {}", self.bin()),
        }
    }

    pub(super) fn unset_lade_shell(&self) -> String {
        match self {
            Shell::Bash | Shell::Zsh | Shell::Sh => "unset -v LADE_SHELL".to_string(),
            Shell::Fish => "set --global --erase LADE_SHELL".to_string(),
        }
    }

    pub fn set(&self, env: HashMap<String, String>) -> String {
        env.into_iter()
            .map(|(k, v)| match self {
                Shell::Bash | Shell::Zsh | Shell::Sh => {
                    format!("export {k}='{}'", v.replace('\'', "'\\''"))
                }
                // Inside fish single quotes, `\` escapes `\` and `'`.
                Shell::Fish => format!(
                    "set --global --export {k} '{}'",
                    v.replace('\\', "\\\\").replace('\'', "\\'")
                ),
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn unset(&self, keys: Vec<String>) -> String {
        let format = match self {
            Shell::Zsh | Shell::Bash | Shell::Sh => |k: String| format!("unset -v {k}"),
            Shell::Fish => |k: String| format!("set --global --erase {k}"),
        };
        keys.into_iter().map(format).collect::<Vec<_>>().join(";")
    }

    /// Code for the pre-exec hook to eval so the pending command does not
    /// run: no external command is found until it ends, then `PATH` comes
    /// back. The shell's command-not-found handler says Lade cancelled it,
    /// then the user's handler comes back. Erasing `LADE` makes the hook
    /// skip `lade unset`, which would find no `lade` on that `PATH` and has
    /// nothing to undo: a refused `lade set` exported nothing.
    pub fn block_command(&self) -> String {
        match self {
            Shell::Zsh => format!(
                "if [ -z \"${{__lade_path+x}}\" ]; then\n\
                 __lade_path=\"$PATH\"\n\
                 (( $+functions[command_not_found_handler] )) && functions[__lade_cnf]=$functions[command_not_found_handler]\n\
                 fi\n\
                 PATH={BLOCKED_PATH}\n\
                 unset -v LADE\n\
                 command_not_found_handler() {{ print -ru2 -- \"{CANCELLED} $1.\"; return 127; }}\n\
                 __lade_unblock() {{ PATH=\"$__lade_path\"; unset __lade_path; unfunction command_not_found_handler; \
                 if (( $+functions[__lade_cnf] )); then functions[command_not_found_handler]=$functions[__lade_cnf]; unfunction __lade_cnf; fi; \
                 precmd_functions=(${{precmd_functions:#__lade_unblock}}); unfunction __lade_unblock; }}\n\
                 precmd_functions=(__lade_unblock $precmd_functions)"
            ),
            Shell::Bash => format!(
                "if [ -z \"${{__lade_path+x}}\" ]; then\n\
                 __lade_path=\"$PATH\"\n\
                 __lade_def=$(declare -f command_not_found_handle) && eval \"${{__lade_def/command_not_found_handle/__lade_cnf}}\"\n\
                 unset -v __lade_def\n\
                 fi\n\
                 PATH={BLOCKED_PATH}\n\
                 unset -v LADE\n\
                 command_not_found_handle() {{ printf '{CANCELLED} %s.\\n' \"$1\" >&2; return 127; }}\n\
                 __lade_unblock() {{ PATH=\"$__lade_path\"; unset __lade_path; unset -f command_not_found_handle; local def kept=() f; \
                 if def=$(declare -f __lade_cnf); then eval \"${{def/__lade_cnf/command_not_found_handle}}\"; unset -f __lade_cnf; fi; \
                 for f in \"${{precmd_functions[@]}}\"; do [ \"$f\" = __lade_unblock ] || kept+=(\"$f\"); done; \
                 precmd_functions=(\"${{kept[@]}}\"); unset -f __lade_unblock; }}\n\
                 precmd_functions=(__lade_unblock \"${{precmd_functions[@]}}\")"
            ),
            // psub removes the file this code is sourced from once `source`
            // returns, with `command rm`: run that now, while `rm` is found.
            Shell::Fish => format!(
                "for __lade_f in (functions -a -n | string match '__fish_psub_*'); $__lade_f; end\n\
                 set -e __lade_f\n\
                 if not set -q __lade_path\n\
                 set -g __lade_path $PATH\n\
                 functions -q fish_command_not_found; and functions -c fish_command_not_found __lade_cnf\n\
                 end\n\
                 set -gx PATH {BLOCKED_PATH}\n\
                 set -e LADE\n\
                 function fish_command_not_found; printf '{CANCELLED} %s.\\n' $argv[1] >&2; end\n\
                 function __lade_unblock --on-event fish_postexec\n\
                 set -q __lade_path; and set -gx PATH $__lade_path\n\
                 set -e __lade_path\n\
                 functions -e fish_command_not_found\n\
                 functions -q __lade_cnf; and functions -c __lade_cnf fish_command_not_found; and functions -e __lade_cnf\n\
                 functions -e __lade_unblock\n\
                 end"
            ),
            // No pre-exec hook runs under sh.
            Shell::Sh => String::new(),
        }
    }

    pub fn restore(&self, previous: HashMap<String, Option<String>>) -> String {
        let mut set = HashMap::new();
        let mut unset = Vec::new();
        for (key, value) in previous {
            match value {
                Some(value) => {
                    set.insert(key, value);
                }
                None => unset.push(key),
            }
        }
        let mut parts = Vec::new();
        if !set.is_empty() {
            parts.push(self.set(set));
        }
        if !unset.is_empty() {
            parts.push(self.unset(unset));
        }
        parts.join(";")
    }
}

pub(super) fn shell_from_comm(name: &str) -> Result<Shell> {
    let shell = name.trim().to_lowercase();
    let shell = shell.strip_suffix(".exe").unwrap_or(shell.as_str());
    let shell = std::path::Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(shell);
    Shell::from_str(shell)
}

fn parent_process_name() -> Result<String> {
    #[cfg(not(unix))]
    {
        bail!("set LADE_SHELL; parent detection is Unix-only");
    }
    #[cfg(unix)]
    {
        parent_comm(nix::unistd::getppid())
    }
}

#[cfg(target_os = "linux")]
fn parent_comm(ppid: nix::unistd::Pid) -> Result<String> {
    let path = format!("/proc/{}/comm", ppid.as_raw());
    let name = std::fs::read_to_string(&path).with_context(|| format!("cannot read {path}"))?;
    Ok(name.trim().to_string())
}

#[cfg(target_os = "macos")]
fn parent_comm(ppid: nix::unistd::Pid) -> Result<String> {
    let mut buf = [0u8; 256];
    let n = unsafe {
        libc::proc_name(
            ppid.as_raw(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if n <= 0 {
        bail!("cannot read parent process name");
    }
    let nbytes = (n as usize).min(buf.len());
    let end = buf[..nbytes].iter().position(|&b| b == 0).unwrap_or(nbytes);
    String::from_utf8(buf[..end].to_vec()).context("parent process name is not utf-8")
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn parent_comm(_ppid: nix::unistd::Pid) -> Result<String> {
    bail!("set LADE_SHELL; parent detection is Linux/macOS-only");
}
