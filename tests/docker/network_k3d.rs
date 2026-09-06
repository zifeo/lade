use crate::common;
use crate::k3d::{
    ensure_cluster_context, extract_lade_t, is_pid_running, is_ready_for_k3d_test,
    normalize_authority, run_capture, run_ok, ticket_network_pid, write_cluster_config,
};
use predicates::prelude::PredicateBooleanExt;
use std::env;
use std::fs;
use tempfile::tempdir;

#[test]
fn network_k3d_kubectl_provider_lifecycle() {
    assert!(is_ready_for_k3d_test(), "k3d prerequisites are required");

    let cluster = env::var("LADE_K3D_CLUSTER").unwrap_or_else(|_| "lade-k3d-shared".to_string());
    let context = format!("k3d-{cluster}");
    let kube_dir = tempdir().expect("kubeconfig dir");
    let kubeconfig = kube_dir.path().join("config").display().to_string();
    let cluster_config = write_cluster_config(kube_dir.path());
    ensure_cluster_context(&cluster, &context, &kubeconfig, &cluster_config);
    let namespace = "lade-k3d-ns";
    let service = "http-echo";
    let port_local = "18080";
    let port_remote = "8080";
    let payload_arg = r#"'{"ping":"pong"}'"#;

    run_ok(
        "kubectl",
        &kubeconfig,
        &[
            "--context",
            &context,
            "--request-timeout=15s",
            "apply",
            "-f",
            "k3d-manifests.yaml",
        ],
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

    let tickets = tempdir().expect("ticket dir");
    let set_output = common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", &format!("curl http://127.0.0.1:{port_local}/")])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let set_stdout = String::from_utf8_lossy(&set_output);
    let id = extract_lade_t(&set_stdout).expect("LADE_T in set output");
    let pid = ticket_network_pid(tickets.path(), &id).expect("network_pids on ticket");
    assert!(
        is_pid_running(&pid),
        "detached provider pid not running: {pid}"
    );

    common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &id)
        .args(["unset", &format!("curl http://127.0.0.1:{port_local}/")])
        .assert()
        .success();

    std::thread::sleep(std::time::Duration::from_secs(1));
    assert!(
        !is_pid_running(&pid),
        "detached provider pid still running after unset: {pid}"
    );
}
