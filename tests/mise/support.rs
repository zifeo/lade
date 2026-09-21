use std::fs;
use std::path::Path;

#[cfg(unix)]
pub(super) fn write_cached_env(home: &Path, tool: &str, slug: &str, version: &str, env_json: &str) {
    let dir = home.join("lade-cache").join("mise-env").join(slug);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(format!("{version}.json")),
        format!(r#"{{"v":1,"tool":"{tool}","env":{env_json}}}"#),
    )
    .unwrap();
}

#[cfg(unix)]
pub(super) fn write_exec(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let body = if path.file_name().and_then(|n| n.to_str()) == Some("mise")
        && !body.contains("--version")
    {
        format!(
            r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
{body}
"#
        )
    } else {
        body.to_string()
    };
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
pub(super) fn prepend_path(dir: &Path) -> String {
    let old = std::env::var("PATH").unwrap_or_default();
    format!("{}:{old}", dir.display())
}

pub(super) fn export_value(stdout: &str, key: &str) -> Option<String> {
    let needle = format!("export {key}='");
    let rest = stdout.split(&needle).nth(1)?;
    Some(rest.split('\'').next()?.to_string())
}
