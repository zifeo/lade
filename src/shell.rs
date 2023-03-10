use anyhow::{bail, Result};
use std::{collections::HashMap, env};

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
    pub fn from_env() -> Result<Shell> {
        let bin = env::var("SHELL")?;
        match bin.split('/').last() {
            Some("bash") => Ok(Shell::Bash),
            Some("zsh") => Ok(Shell::Zsh),
            Some("fish") => Ok(Shell::Fish),
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
}
