use anyhow::{bail, Result};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use sysinfo::{get_current_pid, ProcessExt, System, SystemExt};

#[cfg(debug_assertions)]
macro_rules! import {
    ($x:expr) => {
        include_str!($x)
            .replace("lade set", "cargo run -- set")
            .replace("lade unset", "cargo run -- unset")
    };
}

#[cfg(not(debug_assertions))]
macro_rules! import {
    ($x:expr) => {
        include_str!($x).to_string()
    };
}

pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn detect() -> Result<Shell> {
        let sys = System::new_all();
        let process = sys
            .process(get_current_pid().expect("no pid"))
            .expect("pid does not exist");
        let parent = sys
            .process(process.parent().expect("no parent pid"))
            .expect("parent pid does not exist");
        let shell = parent.name().trim().to_lowercase();
        let shell = shell.strip_suffix(".exe").unwrap_or(&shell); // windows bad

        match shell {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            _ => bail!("Unsupported shell"),
        }
    }

    pub fn on(&self) -> String {
        match self {
            Shell::Bash => format!(
                "{}\n{}",
                import!("../scripts/bash-preexec.sh"),
                import!("../scripts/on.bash")
            ),
            Shell::Zsh => import!("../scripts/on.zsh"),
            Shell::Fish => import!("../scripts/on.fish"),
        }
    }

    pub fn off(&self) -> String {
        match self {
            Shell::Bash => import!("../scripts/off.bash"),
            Shell::Zsh => import!("../scripts/off.zsh"),
            Shell::Fish => import!("../scripts/off.fish"),
        }
    }

    pub fn set(&self, env: HashMap<String, String>) -> String {
        env.into_iter()
            .map(|(k, v)| match self {
                Shell::Zsh | Shell::Bash => {
                    format!("declare -g -x {k}='{v}'")
                }
                Shell::Fish => {
                    format!("set --global --export {k} '{v}'")
                }
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn unset(&self, keys: Vec<String>) -> String {
        let format = match self {
            Shell::Zsh | Shell::Bash => |k| format!("unset -v ${k}"),
            Shell::Fish => |k| format!("set --global --erase {k}"),
        };
        keys.into_iter().map(format).collect::<Vec<_>>().join(";")
    }

    pub fn install(&self) -> String {
        self.configure_auto_launch(true).display().to_string()
    }

    pub fn uninstall(&self) -> String {
        self.configure_auto_launch(false).display().to_string()
    }

    fn configure_auto_launch(&self, install: bool) -> PathBuf {
        let user = directories::UserDirs::new().expect("cannot get HOME location");
        let home_dir = user.home_dir();
        let curr_exe = std::env::current_exe().expect("cannot get current executable path");
        let command = match self {
            Shell::Bash => format!("source <(echo \"$({} on)\")", curr_exe.display()),
            Shell::Zsh => format!("eval \"$({} on)\"", curr_exe.display()),
            Shell::Fish => format!("eval \"$({} on)\"", curr_exe.display()),
        };
        let marker = "lade-do-not-edit".to_string();
        let config_file = match self {
            Shell::Bash => home_dir.join(".bashrc"),
            Shell::Zsh => home_dir.join(".zshrc"),
            Shell::Fish => home_dir.join(".config/fish/config.fish"),
        };
        edit_config(&config_file, command, marker, install);
        config_file
    }
}

fn edit_config<P: AsRef<Path>>(config_file: P, line: String, marker: String, install: bool) {
    let old_config = std::fs::read_to_string(&config_file).unwrap_or_default();
    let mut new_config = old_config
        .lines()
        .filter(|l| !l.contains(&marker))
        .collect::<Vec<_>>();
    let new_line = format!("{}  # {}", line, marker);
    if install {
        new_config.push(&new_line);
    }
    std::fs::write(config_file, new_config.join("\n")).expect("cannot write to config file");
}
