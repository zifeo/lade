use crate::message_box::MessageBox;

#[derive(Debug)]
pub struct Error {
    lines: Vec<String>,
}

impl Error {
    pub fn box_lines(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            lines: lines.into_iter().map(Into::into).collect(),
        }
    }

    pub fn bare_version(argv0: &str, value: &str) -> Self {
        Self::box_lines([
            format!("`{argv0}: {value}` is not a pin."),
            String::new(),
            "A pin is a Lade URI: mise://<backend>/<package>@<version>.".to_string(),
            format!("Example: {argv0}: mise://aqua/owner/repo@1.2.3"),
            "Backends are the names from `mise backends ls` (aqua, core, github, …).".to_string(),
        ])
    }

    pub fn invalid_spec(detail: String) -> Self {
        Self::box_lines([
            "This pin is not a mise backend spec.".to_string(),
            String::new(),
            detail,
        ])
    }

    pub fn conflict(tool: &str, theirs: &str, ours: &str, file: &str) -> Self {
        Self::box_lines([
            format!("mise.toml and lade.yml pin {tool} to different versions."),
            String::new(),
            format!("{file} has {theirs}."),
            format!("lade.yml has {ours}."),
            String::new(),
            "Make them match. Lade will not pick one.".to_string(),
        ])
    }

    pub fn refuse(argv0: &str, spec: &str) -> Self {
        Self::box_lines([
            format!("Could not run the locked {argv0}."),
            String::new(),
            format!("mise did not install {spec}."),
            "The command was not started. Homebrew or another PATH binary is not used.".to_string(),
        ])
    }

    pub fn install(detail: String) -> Self {
        Self::box_lines([
            "mise install failed.".to_string(),
            String::new(),
            detail,
            String::new(),
            "The command was not started.".to_string(),
        ])
    }

    pub fn env(detail: String) -> Self {
        Self::box_lines([
            "mise env failed.".to_string(),
            String::new(),
            detail,
            String::new(),
            "The command was not started.".to_string(),
        ])
    }

    pub fn missing_mise(detail: String) -> Self {
        Self::box_lines([
            "Could not run mise.".to_string(),
            String::new(),
            detail,
            String::new(),
            "Install mise from https://mise.jdx.dev".to_string(),
        ])
    }

    pub fn emit(&self) {
        let mut mb = MessageBox::new().error();
        for line in &self.lines {
            mb = mb.line(line);
        }
        mb.print_stderr();
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.lines
                .iter()
                .filter(|line| !line.is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        )
    }
}

impl std::error::Error for Error {}
