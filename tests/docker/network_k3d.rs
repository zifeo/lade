use crate::common;
use predicates::prelude::PredicateBooleanExt;
use std::env;
use std::fs;
use std::process::{Command, Stdio};
use tempfile::tempdir;

#[test]
fn network_k3d_kubectl_provider_lifecycle() {
    assert!(is_ready_for_k3d_test(), "k3d prerequisites are required");

    let cluster = env::var("LADE_K3D_CLUSTER").unwrap_or_else(|_| "lade-k3d-shared".to_string());
    let context = format!("k3d-{cluster}");
    let kube_dir = tempdir().expect("kubeconfig dir");
    let kubeconfig = kube_dir.path().join("config").display().to_string();
    ensure_cluster_context(&cluster, &context, &kubeconfig);
    let namespace = "lade-k3d-ns";
    let service = "http-echo";
    let port_local = "18080";
    let port_remote = "8080";
    let payload_arg = r#"'{"ping":"pong"}'"#;

    run_ok(
        "kubectl",
        &kubeconfig,
        &["--context", &context, "apply", "-f", "k3d-manifests.yaml"],
    );
    run_ok(
        "kubectl",
        &kubeconfig,
        &[
            "--context",
            &context,
            "-n",
            namespace,
            "rollout",
            "status",
            &format!("deployment/{service}"),
            "--timeout=120s",
        ],
    );

    let server_url = run_capture(
        "kubectl",
        &kubeconfig,
        &[
            "--context",
            &context,
            "config",
            "view",
            "--raw",
            "-o",
            &format!("jsonpath={{.clusters[?(@.name==\"{context}\")].cluster.server}}"),
        ],
    );
    let authority = normalize_authority(&server_url);

    let dir = tempdir().expect("tmp dir");
    let home = tempdir().expect("home dir");
    let rule = format!(
        "\"^curl .*http://127.0.0.1:{port_local}/$\":\n  \"{port_local}\": kubectl://{authority}/{context}/{namespace}/service/{service}/{port_remote}\n"
    );
    fs::write(dir.path().join("lade.yml"), rule).expect("write lade.yml");

    common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .args([
            "inject",
            "--no-mask",
            "curl",
            "-fsS",
            "-X",
            "POST",
            "-H",
            "content-type:application/json",
            "-d",
            payload_arg,
            &format!("http://127.0.0.1:{port_local}/"),
        ])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("\"method\": \"POST\"")
                .and(predicates::str::contains("\"ping\": \"pong\"")),
        );

    let set_output = common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .args(["set", &format!("curl http://127.0.0.1:{port_local}/")])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let set_stdout = String::from_utf8_lossy(&set_output);
    let pid = extract_pid(&set_stdout).expect("LADE_NETWORK_PIDS in set output");
    assert!(
        is_pid_running(&pid),
        "detached provider pid not running: {pid}"
    );

    common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .env("LADE_NETWORK_PIDS", &pid)
        .args(["unset", &format!("curl http://127.0.0.1:{port_local}/")])
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(
        !is_pid_running(&pid),
        "detached provider pid still running after unset: {pid}"
    );
}

fn is_ready_for_k3d_test() -> bool {
    has_cmd("k3d")
        && has_cmd("kubectl")
        && has_cmd("docker")
        && has_cmd("curl")
        && Command::new("docker")
            .arg("info")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

fn has_cmd(cmd: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .is_ok_and(|s| s.success())
}

fn ensure_cluster_context(cluster: &str, context: &str, kubeconfig: &str) {
    if has_kube_context(context, kubeconfig) {
        return;
    }
    if cluster_exists(cluster) {
        write_cluster_kubeconfig(cluster, kubeconfig);
        assert!(
            has_kube_context(context, kubeconfig),
            "exported kubeconfig for cluster {cluster}, but context {context} is missing"
        );
        return;
    }
    let config = cluster_config_path();
    let status = Command::new("k3d")
        .env("KUBECONFIG", kubeconfig)
        .args(["cluster", "create", "--config", &config, "--wait"])
        .output()
        .expect("spawn k3d cluster create");
    assert!(
        status.status.success(),
        "k3d cluster create failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert!(
        has_kube_context(context, kubeconfig),
        "created cluster {cluster}, but context {context} is missing from isolated kubeconfig"
    );
}

fn cluster_config_path() -> String {
    "k3d.yaml".to_string()
}

fn cluster_exists(cluster: &str) -> bool {
    let output = Command::new("k3d")
        .args(["cluster", "list", "-o", "json"])
        .output()
        .expect("spawn k3d cluster list");
    assert!(
        output.status.success(),
        "k3d cluster list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let clusters: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse k3d cluster list JSON");
    clusters
        .as_array()
        .expect("k3d cluster list JSON array")
        .iter()
        .any(|c| c.get("name").and_then(serde_json::Value::as_str) == Some(cluster))
}

fn write_cluster_kubeconfig(cluster: &str, kubeconfig: &str) {
    let output = Command::new("k3d")
        .args(["kubeconfig", "get", cluster])
        .output()
        .expect("spawn k3d kubeconfig get");
    assert!(
        output.status.success(),
        "k3d kubeconfig get failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    fs::write(kubeconfig, output.stdout).expect("write isolated kubeconfig");
}

fn has_kube_context(context: &str, kubeconfig: &str) -> bool {
    let output = Command::new("kubectl")
        .env("KUBECONFIG", kubeconfig)
        .args(["config", "get-contexts", "-o", "name"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == context)
}

fn run_ok(cmd: &str, kubeconfig: &str, args: &[&str]) {
    let output = Command::new(cmd)
        .env("KUBECONFIG", kubeconfig)
        .args(args)
        .output()
        .expect("spawn command");
    if output.status.success() {
        return;
    }
    panic!(
        "{cmd} {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_capture(cmd: &str, kubeconfig: &str, args: &[&str]) -> String {
    let output = Command::new(cmd)
        .env("KUBECONFIG", kubeconfig)
        .args(args)
        .output()
        .expect("spawn command");
    if !output.status.success() {
        panic!(
            "{cmd} {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn normalize_authority(server_url: &str) -> String {
    server_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .expect("server authority")
        .to_string()
}

fn extract_pid(set_stdout: &str) -> Option<String> {
    for prefix in ["LADE_NETWORK_PIDS='", "LADE_NETWORK_PIDS="] {
        let Some(start) = set_stdout.find(prefix) else {
            continue;
        };
        let rest = &set_stdout[start + prefix.len()..];
        let raw = rest
            .split(';')
            .next()
            .unwrap_or(rest)
            .trim()
            .trim_matches('\'')
            .trim_matches('"');
        if !raw.is_empty() {
            return Some(raw.to_string());
        }
    }
    None
}

fn is_pid_running(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
