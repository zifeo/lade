mod hooks;

use anyhow::{Result, bail};
use std::{collections::HashMap, str::FromStr};
use sysinfo::{System, get_current_pid};

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

    pub fn detect() -> Result<Shell> {
        if let Ok(shell_env) = std::env::var("LADE_SHELL") {
            let path = std::path::Path::new(&shell_env);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(shell_env.as_str());
            return Shell::from_str(name);
        }

        let sys = System::new_all();
        let process = sys
            .process(get_current_pid().expect("no pid"))
            .expect("pid does not exist");
        let parent = sys
            .process(process.parent().expect("no parent pid"))
            .expect("parent pid does not exist");
        let shell = parent.name().to_string_lossy().trim().to_lowercase();
        let shell = shell.strip_suffix(".exe").unwrap_or(&shell);
        Shell::from_str(shell)
    }

    pub fn set(&self, env: HashMap<String, String>) -> String {
        env.into_iter()
            .map(|(k, v)| match self {
                Shell::Bash | Shell::Zsh | Shell::Sh => format!("export {k}='{v}'"),
                Shell::Fish => format!("set --global --export {k} '{v}'"),
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn unset(&self, keys: Vec<String>) -> String {
        let format = match self {
            Shell::Zsh | Shell::Bash | Shell::Sh => |k| format!("unset -v {k}"),
            Shell::Fish => |k| format!("set --global --erase {k}"),
        };
        keys.into_iter().map(format).collect::<Vec<_>>().join(";")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_set_bash_single_key() {
        let result = Shell::Bash.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
        assert_eq!(result, "export KEY='value'");
    }

    #[test]
    fn test_set_zsh_single_key() {
        assert_eq!(
            Shell::Zsh.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
            "export KEY='value'"
        );
    }

    #[test]
    fn test_set_fish_single_key() {
        assert_eq!(
            Shell::Fish.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
            "set --global --export KEY 'value'"
        );
    }

    #[test]
    fn test_set_empty_map() {
        assert_eq!(Shell::Bash.set(HashMap::new()), "");
    }

    #[test]
    fn test_set_multiple_keys_contains() {
        let result = Shell::Bash.set(HashMap::from([
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]));
        assert!(result.contains("export A='1'") && result.contains("export B='2'"));
        assert!(result.contains(';'));
    }

    #[test]
    fn test_unset_bash_single_key() {
        assert_eq!(Shell::Bash.unset(vec!["KEY".to_string()]), "unset -v KEY");
    }

    #[test]
    fn test_unset_fish_single_key() {
        assert_eq!(
            Shell::Fish.unset(vec!["KEY".to_string()]),
            "set --global --erase KEY"
        );
    }

    #[test]
    fn test_unset_multiple_keys_order_preserved() {
        assert_eq!(
            Shell::Bash.unset(vec!["KEY1".to_string(), "KEY2".to_string()]),
            "unset -v KEY1;unset -v KEY2"
        );
    }
}
