use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub fn is_ready_for_k3d_test() -> bool {
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

pub fn ensure_cluster_context(cluster: &str, context: &str, kubeconfig: &str, config: &Path) {
    if cluster_exists(cluster) {
        start_cluster(cluster);
        write_cluster_kubeconfig(cluster, kubeconfig);
        if has_kube_context(context, kubeconfig) && api_ready(context, kubeconfig) {
            return;
        }
        delete_cluster(cluster);
    }
    let status = Command::new("k3d")
        .env("KUBECONFIG", kubeconfig)
        .arg("cluster")
        .arg("create")
        .arg("--config")
        .arg(config)
        .arg("--wait")
        .output()
        .expect("spawn k3d cluster create");
    assert!(
        status.status.success(),
        "k3d cluster create failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    write_cluster_kubeconfig(cluster, kubeconfig);
    assert!(
        has_kube_context(context, kubeconfig),
        "created cluster {cluster}, but context {context} is missing from isolated kubeconfig"
    );
    assert!(
        api_ready(context, kubeconfig),
        "created cluster {cluster}, but the API is not ready"
    );
}

fn api_ready(context: &str, kubeconfig: &str) -> bool {
    Command::new("kubectl")
        .env("KUBECONFIG", kubeconfig)
        .args([
            "--context",
            context,
            "--request-timeout=8s",
            "get",
            "--raw",
            "/readyz",
        ])
        .output()
        .is_ok_and(|output| output.status.success())
}

fn delete_cluster(cluster: &str) {
    let output = Command::new("k3d")
        .args(["cluster", "delete", cluster])
        .output()
        .expect("spawn k3d cluster delete");
    assert!(
        output.status.success(),
        "k3d cluster delete failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn start_cluster(cluster: &str) {
    let output = Command::new("k3d")
        .args(["cluster", "start", cluster, "--wait"])
        .output()
        .expect("spawn k3d cluster start");
    assert!(
        output.status.success(),
        "k3d cluster start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn write_cluster_config(dir: &Path) -> PathBuf {
    let manifest = std::env::current_dir()
        .expect("current dir")
        .join("k3d-manifests.yaml");
    let config = fs::read_to_string("k3d.yaml")
        .expect("read k3d.yaml")
        .replace("./k3d-manifests.yaml", &manifest.display().to_string());
    let path = dir.join("k3d.yaml");
    fs::write(&path, config).expect("write generated k3d config");
    path
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

pub fn run_ok(cmd: &str, kubeconfig: &str, args: &[&str]) {
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

pub fn run_capture(cmd: &str, kubeconfig: &str, args: &[&str]) -> String {
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

pub fn normalize_authority(server_url: &str) -> String {
    server_url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .expect("server authority")
        .to_string()
}

pub fn extract_lade_t(set_stdout: &str) -> Option<String> {
    for prefix in ["export LADE_T='", "LADE_T='", "export LADE_T="] {
        let Some(start) = set_stdout.find(prefix) else {
            continue;
        };
        let rest = &set_stdout[start + prefix.len()..];
        let raw = rest.split([';', '\'', '"', ' ', '\n']).next().unwrap_or("");
        if raw.len() == 4 && raw.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Some(raw.to_string());
        }
    }
    None
}

pub fn ticket_network_pid(ticket_dir: &Path, id: &str) -> Option<String> {
    let path = ticket_dir.join(format!("{id}.json"));
    let pre: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    pre["network_pids"]
        .as_array()?
        .first()?
        .as_u64()
        .map(|pid| pid.to_string())
}

pub fn is_pid_running(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
