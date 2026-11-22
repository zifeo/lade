use super::super::opencode::{OpenCodeApi, api_from_version};

#[test]
fn api_follows_the_opencode_major() {
    assert_eq!(api_from_version("1.19.0\n").unwrap(), OpenCodeApi::V1);
    assert_eq!(api_from_version("0.15.4").unwrap(), OpenCodeApi::V1);
    assert_eq!(
        api_from_version("opencode v2.0.29\n").unwrap(),
        OpenCodeApi::V2
    );
    assert_eq!(api_from_version("3.1.0").unwrap(), OpenCodeApi::V2);
    assert!(api_from_version("").is_err());
    assert!(api_from_version("opencode dev").is_err());
}

#[test]
fn each_snapshot_is_read_back_as_its_own_api() {
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        assert_eq!(OpenCodeApi::of(api.snapshot()), Some(api));
    }
    assert_eq!(OpenCodeApi::of("export default {}"), None);
}

/// Lade 0.18 to 0.19.2 rewrite any file holding these markers with v1.
#[test]
fn v2_snapshot_has_no_marker_older_lade_rewrites() {
    let v2 = OpenCodeApi::V2.snapshot();
    for marker in ["LadePretool", "lade hook", "[\"hook\"]", "[\"hook\","] {
        assert!(!v2.contains(marker), "v2 plugin contains {marker}");
    }
}

#[test]
fn v1_snapshot_is_the_published_plugin() {
    let v1 = OpenCodeApi::V1.snapshot();
    assert!(v1.contains("export const LadePretool"));
    assert!(v1.contains(r#""tool.execute.before""#));
}

#[test]
fn both_plugins_pass_lade_stderr_on() {
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        let plugin = api.snapshot();
        assert!(plugin.contains("result.error"), "{api:?}");
        assert!(
            plugin.contains("process.stderr.write(result.stderr)"),
            "{api:?}"
        );
    }
}

/// Loads each plugin in node with a fake `lade`, as OpenCode calls it.
#[test]
fn both_plugins_send_command_and_session_and_apply_the_answer() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    let cases = [
        (OpenCodeApi::V1, "bash"),
        (OpenCodeApi::V2, "bash"),
        (OpenCodeApi::V2, "shell"),
    ];
    for (api, tool) in cases {
        let run = node::run(
            api,
            tool,
            node::Lade::Answers(r#"{"command":"wrapped && true"}"#),
        );
        assert_eq!(run.command, "wrapped && true", "{api:?} {tool}");
        assert_eq!(run.argv, "hook\n--harness\nopencode\n", "{api:?} {tool}");
        let sent: serde_json::Value = serde_json::from_str(&run.stdin).unwrap();
        assert_eq!(
            sent,
            serde_json::json!({ "command": "echo hi", "session_id": "s1" }),
            "{api:?} {tool}"
        );
    }
    let ignored = node::run(OpenCodeApi::V1, "shell", node::Lade::Answers("{}"));
    assert_eq!(ignored.command, "echo hi");
    assert_eq!(ignored.argv, "", "v1 matches only the bash tool");
}

#[test]
fn both_plugins_show_lade_stderr_and_a_missing_lade() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        let failed = node::run(api, "bash", node::Lade::Fails);
        assert_eq!(failed.command, "echo hi", "{api:?}");
        assert!(
            failed.stderr.contains("lade: a warning"),
            "{api:?}: {}",
            failed.stderr
        );

        let answered = node::run(api, "bash", node::Lade::Answers(r#"{"command":"x"}"#));
        assert!(
            answered.stderr.contains("lade: a warning"),
            "{api:?}: {}",
            answered.stderr
        );

        let missing = node::run(api, "bash", node::Lade::Missing);
        assert_eq!(missing.command, "echo hi", "{api:?}");
        assert!(
            missing.stderr.contains("lade: cannot start"),
            "{api:?}: {}",
            missing.stderr
        );
    }
}

#[test]
fn both_plugins_run_lade_bin_over_path() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        let on_path = node::run(api, "bash", node::Lade::Answers(r#"{"command":"x"}"#));
        assert_eq!(on_path.command, "x", "{api:?}");
        assert!(!on_path.lade_bin_ran, "{api:?}");

        let explicit = node::run(api, "bash", node::Lade::AtLadeBin(r#"{"command":"y"}"#));
        assert_eq!(explicit.command, "y", "{api:?}");
        assert!(explicit.lade_bin_ran, "{api:?}: the lade on PATH ran");
    }
}

#[test]
fn both_plugins_fail_on_a_lade_bin_that_is_not_a_file() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        let run = node::run(api, "bash", node::Lade::BinNotAFile);
        assert_eq!(run.argv, "", "{api:?}: the lade on PATH ran");
        assert!(
            run.stderr.contains("lade: cannot start") && run.stderr.contains("no-such-lade"),
            "{api:?}: {}",
            run.stderr
        );
    }
}

#[test]
fn a_home_plugin_runs_the_line_when_lade_cannot_run() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        for (lade, said) in [
            (node::Lade::Missing, "lade: cannot start"),
            (node::Lade::Fails, "failed with exit 1"),
            (node::Lade::Garbled, "lade: cannot read the answer"),
        ] {
            let run = node::run(api, "bash", lade);
            assert_eq!(run.blocked, None, "{api:?}");
            assert_eq!(run.command, "echo hi", "{api:?}");
            assert!(run.stderr.contains(said), "{api:?}: {}", run.stderr);
        }
    }
}

#[test]
fn a_repo_plugin_blocks_the_call_when_lade_cannot_run() {
    if !node::available() {
        eprintln!("node not installed: skipped");
        return;
    }
    for api in [OpenCodeApi::V1, OpenCodeApi::V2] {
        for (lade, said) in [
            (node::Lade::Missing, "lade: cannot start"),
            (node::Lade::BinNotAFile, "no-such-lade"),
            (node::Lade::Fails, "failed with exit 1"),
            (node::Lade::Garbled, "lade: cannot read the answer"),
        ] {
            let run = node::run_in_repo(api, lade);
            let blocked = run.blocked.unwrap_or_else(|| panic!("{api:?}: {said} ran"));
            assert!(blocked.contains(said), "{api:?}: {blocked}");
            assert!(
                blocked.contains("This repo requires Lade"),
                "{api:?}: {blocked}"
            );
            assert_eq!(run.command, "echo hi", "{api:?}");
        }
        let wrapped = node::run_in_repo(api, node::Lade::Answers(r#"{"command":"x"}"#));
        assert_eq!(wrapped.blocked, None, "{api:?}");
        assert_eq!(wrapped.command, "x", "{api:?}");
        let allowed = node::run_in_repo(api, node::Lade::Answers("{}"));
        assert_eq!(allowed.blocked, None, "{api:?}");
        assert_eq!(allowed.command, "echo hi", "{api:?}");
    }
}

mod node {
    use super::OpenCodeApi;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    pub(super) fn available() -> bool {
        Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|out| out.status.success())
    }

    /// `lade` on `PATH` and `LADE_BIN` unset, except for `AtLadeBin`
    /// (`LADE_BIN` names another one), `BinNotAFile` (`LADE_BIN` names no
    /// file) and `Missing` (no `lade` on `PATH`). `Garbled` exits 0 with an
    /// answer that is not JSON.
    pub(super) enum Lade {
        Answers(&'static str),
        Fails,
        Garbled,
        AtLadeBin(&'static str),
        BinNotAFile,
        Missing,
    }

    pub(super) struct Run {
        pub(super) command: String,
        /// The message of the error the hook threw, which blocks the call.
        pub(super) blocked: Option<String>,
        pub(super) argv: String,
        pub(super) stdin: String,
        pub(super) stderr: String,
        pub(super) lade_bin_ran: bool,
    }

    const FAKE_LADE: &str = r#"#!/bin/sh
case "$0" in *lade-bin) : > "$FAKE_DIR/lade-bin-ran" ;; esac
printf '%s\n' "$@" > "$FAKE_DIR/argv"
cat > "$FAKE_DIR/stdin"
printf 'lade: a warning\n' >&2
printf '%s' "$FAKE_STDOUT"
exit "$FAKE_STATUS"
"#;

    const DRIVER: &str = r#"import { pathToFileURL } from "node:url";
const [plugin, api, tool] = process.argv.slice(2);
const mod = await import(pathToFileURL(plugin).href);
let command;
let blocked = null;
try {
  if (api === "v1") {
    const hooks = await mod.LadePretool();
    const output = { args: { command: "echo hi" } };
    try {
      await hooks["tool.execute.before"]({ tool, sessionID: "s1" }, output);
    } finally {
      command = output.args.command;
    }
  } else {
    let hook;
    await mod.default.setup({
      tool: { hook: async (name, fn) => { if (name === "execute.before") hook = fn; } },
    });
    const event = { tool, sessionID: "s1", input: { command: "echo hi" } };
    try {
      await hook(event);
    } finally {
      command = event.input.command;
    }
  }
} catch (error) {
  blocked = error.message;
}
process.stdout.write(JSON.stringify({ command, blocked }));
"#;

    /// The plugin as a home plugin, outside any `.opencode/plugins/`.
    pub(super) fn run(api: OpenCodeApi, tool: &str, lade: Lade) -> Run {
        run_at(api, tool, lade, &["opencode", "plugins"])
    }

    /// The plugin in a repo's `.opencode/plugins/`.
    pub(super) fn run_in_repo(api: OpenCodeApi, lade: Lade) -> Run {
        run_at(api, "bash", lade, &["repo", ".opencode", "plugins"])
    }

    fn run_at(api: OpenCodeApi, tool: &str, lade: Lade, plugin_dir: &[&str]) -> Run {
        let dir = tempfile::tempdir().unwrap();
        let plugin_dir = plugin_dir
            .iter()
            .fold(dir.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(&plugin_dir).unwrap();
        let plugin = plugin_dir.join("lade-pretool.mjs");
        fs::write(&plugin, api.snapshot()).unwrap();
        let driver = dir.path().join("driver.mjs");
        fs::write(&driver, DRIVER).unwrap();
        let path_dir = dir.path().join("bin");
        fs::create_dir(&path_dir).unwrap();
        let on_path = path_dir.join("lade");
        let lade_bin = dir.path().join("lade-bin");
        for fake in [&on_path, &lade_bin] {
            fs::write(fake, FAKE_LADE).unwrap();
            fs::set_permissions(fake, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let (stdout, status) = match lade {
            Lade::Answers(stdout) | Lade::AtLadeBin(stdout) => (stdout, "0"),
            Lade::Garbled => ("not json", "0"),
            Lade::Fails | Lade::BinNotAFile | Lade::Missing => ("", "1"),
        };
        if let Lade::Missing = lade {
            fs::remove_file(&on_path).unwrap();
        }
        let mut node = Command::new(node_bin());
        match lade {
            Lade::AtLadeBin(_) => node.env("LADE_BIN", &lade_bin),
            Lade::BinNotAFile => node.env("LADE_BIN", dir.path().join("no-such-lade")),
            Lade::Answers(_) | Lade::Fails | Lade::Garbled | Lade::Missing => {
                node.env_remove("LADE_BIN")
            }
        };
        let out = node
            .arg(&driver)
            .arg(&plugin)
            .arg(api.label())
            .arg(tool)
            .env("HOME", dir.path())
            .env(
                "PATH",
                std::env::join_paths([path_dir.as_path(), "/usr/bin".as_ref(), "/bin".as_ref()])
                    .unwrap(),
            )
            .env("FAKE_DIR", dir.path())
            .env("FAKE_STDOUT", stdout)
            .env("FAKE_STATUS", status)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "node failed: {stderr}");
        let read = |name: &str| fs::read_to_string(dir.path().join(name)).unwrap_or_default();
        let answer: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        Run {
            command: answer["command"].as_str().unwrap().to_string(),
            blocked: answer["blocked"].as_str().map(str::to_string),
            argv: read("argv"),
            stdin: read("stdin"),
            stderr,
            lade_bin_ran: dir.path().join("lade-bin-ran").exists(),
        }
    }

    /// The plugin runs under a `PATH` of the test's fakes only.
    fn node_bin() -> std::path::PathBuf {
        let path = std::env::var_os("PATH").expect("PATH");
        std::env::split_paths(&path)
            .map(|dir| dir.join("node"))
            .find(|candidate| candidate.is_file())
            .expect("node on PATH")
    }
}
