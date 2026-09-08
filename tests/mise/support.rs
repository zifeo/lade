use std::fs;
use std::path::Path;

#[cfg(unix)]
pub(super) fn write_cached_env(home: &Path, tool: &str, slug: &str, version: &str, env_json: &str) {
    let dir = temp_env::with_var("HOME", Some(home), || {
        directories::ProjectDirs::from("com", "zifeo", "lade")
            .expect("project dirs")
            .cache_dir()
            .join("mise-env")
            .join(slug)
    });
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
