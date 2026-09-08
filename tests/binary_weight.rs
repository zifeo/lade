use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::Command;

/// Runtime crate ceiling for `lade` on the host triple.
/// Bump this in the same PR as an intentional provider or crate add.
/// Do not raise it to hide a second TLS or OpenSSL stack.
const MAX_RUNTIME_CRATES: usize = 450;

/// Explosion guard used only when `target/release/lade` already exists.
/// `cargo test` does not build release. cargo-bloat is the microscope
/// when this or the crate ceiling trips.
const MAX_RELEASE_BYTES: u64 = 32 * 1024 * 1024;

const FORBIDDEN: &[&str] = &[
    "hyper-tls",
    "native-tls",
    "openssl",
    "openssl-sys",
    "tokio-native-tls",
];

#[test]
fn runtime_graph_stays_on_one_rustls_stack() {
    let meta = cargo_metadata();
    let pkgs = runtime_packages(&meta);
    let names: BTreeSet<&str> = pkgs.keys().copied().collect();

    assert!(
        pkgs.len() <= MAX_RUNTIME_CRATES,
        "lade runtime graph is {} crates (max {MAX_RUNTIME_CRATES}). If this is a new provider, bump MAX_RUNTIME_CRATES. If not, a Cargo feature probably pulled a second HTTP/TLS stack. Diagnose with `cargo tree -e features` and `cargo bloat --release --crates`.",
        pkgs.len()
    );

    let extra: Vec<&str> = FORBIDDEN
        .iter()
        .copied()
        .filter(|name| names.contains(name))
        .collect();
    assert!(
        extra.is_empty(),
        "forbidden runtime TLS/OpenSSL crates: {extra:?}. rustls-native-certs may pull openssl-probe on Linux to find the CA bundle. That is not a second TLS stack. Azure must keep enable_reqwest_rustls only. AWS must not enable the secretsmanager `rustls` feature (legacy 0.21 stack)."
    );

    let rustls = pkgs.get("rustls").map(Vec::as_slice).unwrap_or(&[]);
    assert_eq!(
        rustls.len(),
        1,
        "expected one rustls, got {rustls:?}. A feature unification pulled a second TLS stack."
    );
    assert!(
        rustls[0].starts_with("0.23."),
        "rustls {} is outside 0.23.*",
        rustls[0]
    );
}

#[test]
fn release_binary_under_budget_when_present() {
    let Some(path) = release_lade() else {
        return;
    };
    let size = path.metadata().expect("stat release lade").len();
    assert!(
        size <= MAX_RELEASE_BYTES,
        "{} is {size} bytes (max {MAX_RELEASE_BYTES}). Feature flags or a new crate probably bloated the binary. Run `cargo bloat --release --crates`.",
        path.display()
    );
}

fn rustc_host() -> String {
    let out = Command::new("rustc")
        .arg("-vV")
        .output()
        .expect("run rustc -vV");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("rustc -vV utf8")
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_string))
        .expect("rustc -vV host")
}

fn cargo_metadata() -> Value {
    let out = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            &rustc_host(),
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo metadata");
    assert!(
        out.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("parse cargo metadata")
}

fn runtime_packages(meta: &Value) -> BTreeMap<&str, Vec<&str>> {
    let packages = meta["packages"].as_array().expect("packages");
    let by_id: BTreeMap<&str, (&str, &str)> = packages
        .iter()
        .map(|p| {
            (
                p["id"].as_str().expect("id"),
                (
                    p["name"].as_str().expect("name"),
                    p["version"].as_str().expect("version"),
                ),
            )
        })
        .collect();
    let nodes = meta["resolve"]["nodes"].as_array().expect("nodes");
    let deps_of: BTreeMap<&str, &Value> = nodes
        .iter()
        .map(|n| (n["id"].as_str().expect("node id"), n))
        .collect();
    let root = meta["resolve"]["root"].as_str().expect("resolve.root");
    assert_eq!(by_id[root].0, "lade");

    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = deps_of.get(id) else {
            continue;
        };
        for dep in node["deps"].as_array().expect("deps") {
            if !is_runtime_dep(dep) {
                continue;
            }
            stack.push(dep["pkg"].as_str().expect("dep.pkg"));
        }
    }

    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in seen {
        let (name, version) = by_id[id];
        by_name.entry(name).or_default().push(version);
    }
    by_name
}

fn is_runtime_dep(dep: &Value) -> bool {
    let Some(kinds) = dep.get("dep_kinds").and_then(Value::as_array) else {
        return true;
    };
    if kinds.is_empty() {
        return true;
    }
    kinds.iter().any(|kind| match kind.get("kind") {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.is_empty() || s == "normal",
        _ => false,
    })
}

fn release_lade() -> Option<PathBuf> {
    let mut dirs = vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/release")];
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        dirs.push(PathBuf::from(target).join("release"));
    }
    dirs.into_iter()
        .map(|dir| dir.join("lade"))
        .find(|path| path.is_file())
}
