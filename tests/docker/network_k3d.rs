use crate::common;
use crate::k3d::{
    echo_through, ensure_cluster_context, eventually, extract_lade_t, is_ready_for_k3d_test,
    manifests_path, normalize_authority, port_is_free, run_capture, run_ok, write_cluster_config,
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

    let manifests = manifests_path().display().to_string();
    run_ok(
        "kubectl",
        &kubeconfig,
        &[
            "--context",
            &context,
            "--request-timeout=15s",
            "apply",
            "-f",
            &manifests,
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
    let installs = tempdir().expect("mise installs");
    let kubectl = crate::common::command_path("kubectl").expect("kubectl on PATH");
    crate::common::seed_store_cli(installs.path(), "kubectl", "1.31.4", &kubectl);

    common::lade(home.path())
        .current_dir(dir.path())
        .env("KUBECONFIG", &kubeconfig)
        .env("MISE_INSTALLS_DIR", installs.path())
        // The hub socket path must fit macOS's 104-byte limit.
        .env("LADE_CACHE_DIR", home.path().join("c"))
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
    let lade = |dir: &std::path::Path| {
        let mut cmd = common::lade(home.path());
        cmd.current_dir(dir)
            .env("KUBECONFIG", &kubeconfig)
            .env("MISE_INSTALLS_DIR", installs.path())
            .env("LADE_TICKET_DIR", tickets.path())
            // The hub socket path must fit macOS's 104-byte limit.
            .env("LADE_CACHE_DIR", home.path().join("c"));
        cmd
    };
    let command = format!("curl http://127.0.0.1:{port_local}/");
    let set = |dir: &std::path::Path| {
        let out = lade(dir)
            .args(["set", &command])
            .assert()
            .success()
            .get_output()
            .clone();
        let id =
            extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T in set output");
        (id, String::from_utf8_lossy(&out.stderr).into_owned())
    };
    let unset = |dir: &std::path::Path, id: &str| {
        lade(dir)
            .env("LADE_T", id)
            .args(["unset", &command])
            .assert()
            .success();
    };

    // Pre-exec: the hub holds the tunnel after `lade set` exits.
    let (id, _) = set(dir.path());
    assert!(echo_through(port_local), "hub tunnel does not serve");

    // Another target on the same port is refused while the first holds it.
    let other = tempdir().expect("other dir");
    fs::write(
        other.path().join("lade.yml"),
        format!(
            "\"^curl .*http://127.0.0.1:{port_local}/$\":\n  \"{port_local}\": kubectl://{authority}/{context}/kube-system/service/kube-dns/53\n"
        ),
    )
    .expect("write other lade.yml");
    let waiting = common::lade_std(home.path())
        .current_dir(other.path())
        .env("KUBECONFIG", &kubeconfig)
        .env("MISE_INSTALLS_DIR", installs.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_CACHE_DIR", home.path().join("c"))
        .args(["set", &command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn waiting set");
    std::thread::sleep(std::time::Duration::from_secs(2));
    let mut waiting = waiting;
    assert!(
        waiting.try_wait().expect("poll waiting set").is_none(),
        "another target did not wait for the port"
    );
    assert!(
        echo_through(port_local),
        "the waiting tunnel broke the first one"
    );

    // Without ttl, the tunnel stops with its command, and the waiting one
    // takes the port.
    unset(dir.path(), &id);
    let out = waiting.wait_with_output().expect("waiting set output");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let other_id = extract_lade_t(&String::from_utf8_lossy(&out.stdout))
        .expect("LADE_T in the waiting set output");
    unset(other.path(), &other_id);
    eventually("port freed after unset", || port_is_free(port_local));

    // With ttl, it outlives the command and the next one reuses it.
    let persistent = tempdir().expect("persistent dir");
    fs::write(
        persistent.path().join("lade.yml"),
        format!(
            "\"^curl .*http://127.0.0.1:{port_local}/$\":\n  \".\":\n    ttl: 1h\n  \"{port_local}\": kubectl://{authority}/{context}/{namespace}/service/{service}/{port_remote}\n"
        ),
    )
    .expect("write persistent lade.yml");
    let (first, _) = set(persistent.path());
    unset(persistent.path(), &first);
    assert!(
        echo_through(port_local),
        "ttl tunnel stopped with its command"
    );
    let (second, stderr) = set(persistent.path());
    assert!(
        stderr.contains("reused"),
        "second command did not reuse: {stderr}"
    );
    unset(persistent.path(), &second);
    lade(persistent.path())
        .args(["ttl"])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "{port_local}  {port_local}  kubectl://"
        )));
    lade(persistent.path())
        .args(["ttl", "forget", port_local])
        .assert()
        .success();
    eventually("port freed after forget", || port_is_free(port_local));
    lade(persistent.path())
        .args(["ttl", "forget"])
        .assert()
        .success();
}
