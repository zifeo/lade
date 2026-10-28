pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    #[cfg(unix)]
    {
        // Avoid inherited writable descriptors causing ETXTBSY: https://github.com/rust-lang/rust/issues/114554
        use std::io::Write;
        use std::process::{Command, Stdio};

        let path = dir.path().join(name);
        let mut child = Command::new("/bin/sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        write!(stdin, "#!/bin/sh\n{script_body}\n").unwrap();
        drop(stdin);
        assert!(child.wait().unwrap().success());
    }
}

use super::*;

fn has_work_for(scheme: &str, uri: &str) -> bool {
    let mut p = Providers::new();
    p.add(uri.to_string()).unwrap();
    p.by_scheme
        .get(scheme)
        .map(|prov| prov.has_work())
        .unwrap_or(false)
}

fn fallback_has_work(uri: &str) -> bool {
    let mut p = Providers::new();
    p.add(uri.to_string()).unwrap();
    p.fallback.has_work()
}

#[test]
fn test_dispatch_doppler() {
    assert!(has_work_for(
        "doppler",
        "doppler://api.doppler.com/proj/env/KEY"
    ));
    assert!(!fallback_has_work("doppler://api.doppler.com/proj/env/KEY"));
}

#[test]
fn test_dispatch_vault() {
    assert!(has_work_for("vault", "vault://localhost/secret/app/pass"));
    assert!(!fallback_has_work("vault://localhost/secret/app/pass"));
}

#[test]
fn test_dispatch_op() {
    assert!(has_work_for("op", "op://my.1password.com/vault/item/field"));
    assert!(!fallback_has_work("op://my.1password.com/vault/item/field"));
}

#[test]
fn test_dispatch_plain_value_to_fallback() {
    assert!(fallback_has_work("plainvalue"));
}

#[test]
fn test_dispatch_bang_escaped_to_fallback() {
    assert!(fallback_has_work("!escaped"));
}

#[test]
fn test_dispatch_unknown_scheme_to_fallback() {
    assert!(fallback_has_work("foo://some/path"));
}

#[test]
fn test_dispatch_file_without_query_falls_back_to_raw() {
    // file:// without ?query= is rejected by File::add and must land on Raw.
    assert!(fallback_has_work("file:///path/to/config.json"));
    assert!(!has_work_for("file", "file:///path/to/config.json"));
}

#[test]
fn test_dispatch_file_with_query_goes_to_file() {
    assert!(has_work_for(
        "file",
        "file:///path/to/config.json?query=.key"
    ));
    assert!(!fallback_has_work("file:///path/to/config.json?query=.key"));
}

#[test]
fn test_dispatch_awssm() {
    assert!(has_work_for("awssm", "awssm://us-east-1/myapp/db"));
    assert!(!fallback_has_work("awssm://us-east-1/myapp/db"));
}

#[test]
fn test_dispatch_age() {
    assert!(has_work_for("age", "age://YWdlLWVuY3J5cHRpb24ub3Jn"));
    assert!(!fallback_has_work("age://YWdlLWVuY3J5cHRpb24ub3Jn"));
}

#[test]
fn test_dispatch_sops() {
    assert!(has_work_for(
        "sops",
        "sops://secrets.enc.yaml?query=.password"
    ));
    assert!(!fallback_has_work(
        "sops://secrets.enc.yaml?query=.password"
    ));
}

#[test]
fn test_dispatch_azurekv() {
    assert!(has_work_for("azurekv", "azurekv://eng/db"));
    assert!(!fallback_has_work("azurekv://eng/db"));
}

#[test]
fn test_dispatch_bw() {
    assert!(has_work_for("bw", "bw://github/password"));
    assert!(!fallback_has_work("bw://github/password"));
}

#[test]
fn test_dispatch_gcpsm() {
    assert!(has_work_for("gcpsm", "gcpsm://proj/db"));
    assert!(!fallback_has_work("gcpsm://proj/db"));
}

#[test]
fn test_host_less_uris_fail_instead_of_panicking() {
    for uri in ["op://", "op://?x=1", "passbolt://", "doppler://"] {
        let mut p = Providers::new();
        assert!(p.add(uri.to_string()).is_err(), "{uri}");
    }
}

#[test]
fn test_empty_age_does_not_fall_back_to_raw() {
    let mut p = Providers::new();
    assert!(p.add("age://".to_string()).is_err());
    assert!(!p.fallback.has_work());
}

/// A provider whose CLI never answers: a shell that waits on a grandchild,
/// like `sh://op read …` waiting on Touch ID.
struct Silent {
    pid_file: std::path::PathBuf,
}

#[async_trait::async_trait]
impl Provider for Silent {
    fn add(&mut self, _: String) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Silent"
    }

    fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_secs(1)
    }

    fn install_url(&self) -> &'static str {
        "https://example.com"
    }

    fn batch_unit(&self) -> &'static str {
        "value"
    }

    async fn resolve(
        &self,
        _: &Path,
        _: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let script = format!(
            "/bin/sleep 60 & echo $! > '{}'; wait",
            self.pid_file.display()
        );
        run_cli(
            &["/bin/sh", "-c", &script],
            &HashMap::new(),
            "Silent",
            "",
            None,
        )
        .await?;
        Ok(Hydration::default())
    }
}

fn gone_within(pid: libc::pid_t, limit: std::time::Duration) -> bool {
    let deadline = std::time::Instant::now() + limit;
    while std::time::Instant::now() < deadline {
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_times_out_and_kills_the_cli_and_what_it_started() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("grandchild.pid");
    let mut p = Providers::new();
    p.by_scheme.insert(
        "silent",
        Box::new(Silent {
            pid_file: pid_file.clone(),
        }),
    );
    let started = std::time::Instant::now();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        err.to_string().contains("Silent did not answer within 1s"),
        "{err}"
    );
    let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        gone_within(pid, std::time::Duration::from_secs(2)),
        "grandchild {pid} outlived the timeout"
    );
}

#[test]
fn test_providers_that_wait_on_a_human_get_more_time() {
    let p = Providers::new();
    for scheme in ["op", "age", "sops"] {
        assert_eq!(
            p.provider(scheme).unwrap().timeout(),
            HUMAN_PROVIDER_TIMEOUT,
            "{scheme}"
        );
    }
    for scheme in ["vault", "doppler", "sh", "bw", "awssm"] {
        assert_eq!(
            p.provider(scheme).unwrap().timeout(),
            PROVIDER_TIMEOUT,
            "{scheme}"
        );
    }
}

#[test]
#[cfg(unix)]
fn test_output_within_kills_a_silent_cli() {
    let fake_bin = tempfile::tempdir().unwrap();
    fake_cli(&fake_bin, "slow", "exec sleep 60");
    let started = std::time::Instant::now();
    let err = output_within(
        std::process::Command::new(fake_bin.path().join("slow")),
        "slow",
        std::time::Duration::from_millis(200),
    )
    .unwrap_err();
    assert!(err.to_string().contains("slow did not answer"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
#[cfg(unix)]
fn test_output_within_bounds_a_pipe_a_grandchild_keeps() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("grandchild.pid");
    let fake_bin = tempfile::tempdir().unwrap();
    // The list CLI answers and exits, but leaves a helper on its stdout.
    fake_cli(
        &fake_bin,
        "lister",
        &format!(
            "/bin/sleep 60 & echo $! > '{}'\necho listed",
            pid_file.display()
        ),
    );
    let started = std::time::Instant::now();
    // Long enough for the script to start its helper and write its pid
    // under load; the test is that the helper's pipe does not hold the call.
    let err = output_within(
        std::process::Command::new(fake_bin.path().join("lister")),
        "lister",
        std::time::Duration::from_secs(3),
    )
    .unwrap_err();
    assert!(err.to_string().contains("lister did not answer"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        gone_within(pid, std::time::Duration::from_secs(2)),
        "helper {pid} outlived the timeout"
    );
}

#[test]
#[cfg(unix)]
fn test_output_within_returns_output() {
    let fake_bin = tempfile::tempdir().unwrap();
    fake_cli(&fake_bin, "fast", "printf out; printf err >&2; exit 3");
    let output = output_within(
        std::process::Command::new(fake_bin.path().join("fast")),
        "fast",
        std::time::Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(output.stdout, b"out");
    assert_eq!(output.stderr, b"err");
    assert_eq!(output.status.code(), Some(3));
}
