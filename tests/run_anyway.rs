#![cfg(unix)]

//! After a failed hydrate, a human at a terminal has 5 s to press Enter and
//! run the command without Lade. Silence cancels it, and so does having no
//! terminal at all.

mod common;

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nix::pty::openpty;
use nix::sys::termios::tcgetattr;
use tempfile::{TempDir, tempdir};

const PROMPT: &str = "Press Enter within 5 seconds";
const FAILING: &str = "\"file:///nonexistent/lade-missing.json?query=.a\"";
const FAILING_RULE: &str =
    "\"touch.*\":\n  KEY: \"file:///nonexistent/lade-missing.json?query=.a\"\n";

/// Keys typed on the terminal once the output shows `after`, or at once
/// when `after` is empty.
struct Typed<'a> {
    after: &'a str,
    keys: &'a [u8],
}

struct Run {
    home: TempDir,
    status: std::process::ExitStatus,
    /// The terminal: stderr, and stdout unless `preexec`.
    output: String,
    /// Lade's stdout under `preexec`.
    stdout: String,
}

/// Runs `lade <args>` in a fresh `HOME` holding `yaml`, on a fresh pty
/// that is its controlling terminal, so `/dev/tty` is that pty. With
/// `preexec`, bash starts Lade and its stdout goes to a file, as the hook
/// runs `lade set`.
fn on_a_terminal(
    yaml: &str,
    args: impl FnOnce(&Path) -> Vec<String>,
    typed: Option<Typed>,
    preexec: bool,
) -> Run {
    on_a_terminal_in(yaml, args, typed, preexec.then_some("bash"), |_| Vec::new())
}

/// [`on_a_terminal`] with the shell that starts Lade in pre-exec, bash or
/// fish, and `envs` for Lade, written in the fresh `HOME`.
fn on_a_terminal_in(
    yaml: &str,
    args: impl FnOnce(&Path) -> Vec<String>,
    typed: Option<Typed>,
    preexec: Option<&str>,
    envs: impl FnOnce(&Path) -> Vec<(&'static str, std::ffi::OsString)>,
) -> Run {
    let home = tempdir().unwrap();
    common::write_yml_raw(home.path(), yaml);
    let stdout_path = home.path().join("stdout");
    let pty = openpty(None, None).unwrap();
    let before = tcgetattr(&pty.master).unwrap().local_flags;
    let mut cmd = match preexec {
        Some(shell) => {
            let line = match shell {
                "fish" => "$argv[1] $argv[2..-1]; and true",
                _ => "\"$0\" \"$@\" && true",
            };
            let mut parent = std::process::Command::new(shell);
            parent
                .args(["-c", line])
                .arg(assert_cmd::cargo::cargo_bin("lade"));
            common::with_lade_env(parent, home.path())
        }
        None => common::lade_std(home.path()),
    };
    let stdout = if preexec.is_some() {
        Stdio::from(File::create(&stdout_path).unwrap())
    } else {
        Stdio::from(pty.slave.try_clone().unwrap())
    };
    cmd.current_dir(home.path())
        .envs(envs(home.path()))
        .args(args(home.path()))
        .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
        .stdout(stdout)
        .stderr(Stdio::from(pty.slave.try_clone().unwrap()));
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid()?;
            if libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();
    drop(pty.slave);
    let mut master = File::from(pty.master);
    let mut reader = master.try_clone().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut captured = String::new();
    let mut typed = typed;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(Typed { after, keys }) = typed
            && captured.contains(after)
        {
            master.write_all(keys).unwrap();
            typed = None;
        }
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "lade never exited: {captured}");
        if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
            captured.push_str(&String::from_utf8_lossy(&chunk));
        }
    };
    while let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
        captured.push_str(&String::from_utf8_lossy(&chunk));
    }
    let after = tcgetattr(&master).unwrap().local_flags;
    assert_eq!(before, after, "terminal mode not restored: {captured}");
    let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();
    Run {
        home,
        status,
        output: captured,
        stdout,
    }
}

fn touch_marker(home: &Path) -> Vec<String> {
    vec![
        "inject".into(),
        format!("touch {}", home.join("ran").display()),
    ]
}

/// `lade inject touch <marker>` on the failing rule. `answer` is typed once
/// the prompt shows. Returns whether the command ran, the exit status, and
/// the output.
fn run_on_a_terminal(answer: Option<&[u8]>) -> (bool, std::process::ExitStatus, String) {
    let typed = answer.map(|keys| Typed {
        after: PROMPT,
        keys,
    });
    let run = on_a_terminal(FAILING_RULE, touch_marker, typed, false);
    (run.home.path().join("ran").exists(), run.status, run.output)
}

#[test]
fn enter_runs_the_command_without_lade() {
    let (ran, status, output) = run_on_a_terminal(Some(b"\n"));
    assert!(output.contains(PROMPT), "{output}");
    assert!(ran, "{output}");
    assert!(status.success(), "{output}");
}

#[test]
fn a_command_run_anyway_gets_no_binding_and_no_row() {
    let yaml = format!("\"^env\":\n  \".\":\n    log: true\n  OTHER: x\n  KEY: {FAILING}\n");
    let run = on_a_terminal(
        &yaml,
        |home| {
            vec![
                "inject".into(),
                format!("env > {}", home.join("env").display()),
            ]
        },
        Some(Typed {
            after: PROMPT,
            keys: b"\n",
        }),
        false,
    );
    assert!(run.status.success(), "{}", run.output);
    let env = std::fs::read_to_string(run.home.path().join("env")).unwrap();
    assert!(!env.lines().any(|line| line.starts_with("OTHER=")), "{env}");
    let rows = common::log_rows(run.home.path(), run.home.path());
    assert_eq!(rows.as_array().map(Vec::len), Some(0), "{rows}");
}

/// An installed mise pin, read from the cached env: no mise runs.
fn installed_kubectl_pin(home: &Path) -> Vec<(&'static str, std::ffi::OsString)> {
    use std::os::unix::fs::PermissionsExt;
    let installs = home.join("installs");
    let bin = installs.join("kubectl/1.37.0");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("kubectl"), "#!/bin/sh\necho PINNED\n").unwrap();
    std::fs::set_permissions(bin.join("kubectl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let sidecar = home.join("lade-cache/mise-env/aqua-kubernetes-kubectl");
    std::fs::create_dir_all(&sidecar).unwrap();
    std::fs::write(
        sidecar.join("1.37.0.json"),
        r#"{"v":1,"tool":"aqua:kubernetes/kubectl","env":{}}"#,
    )
    .unwrap();
    vec![("MISE_INSTALLS_DIR", installs.into_os_string())]
}

#[test]
fn a_command_run_anyway_gets_no_pinned_path() {
    let yaml =
        format!("\"^env\":\n  kubectl: mise://aqua/kubernetes/kubectl@1.37.0\n  KEY: {FAILING}\n");
    let run = on_a_terminal_in(
        &yaml,
        |home| {
            vec![
                "inject".into(),
                format!("env > {}", home.join("env").display()),
            ]
        },
        Some(Typed {
            after: PROMPT,
            keys: b"\n",
        }),
        None,
        installed_kubectl_pin,
    );
    assert!(run.status.success(), "{}", run.output);
    let env = std::fs::read_to_string(run.home.path().join("env")).unwrap();
    let path = env
        .lines()
        .find(|line| line.starts_with("PATH="))
        .unwrap_or_else(|| panic!("{env}"));
    // The pin's own directory: the developer's PATH may hold a kubectl
    // 1.37.0 of their own.
    let pinned = run.home.path().join("installs/kubectl/1.37.0");
    assert!(!path.contains(pinned.to_str().unwrap()), "{path}");
}

// fish aborts its hook on SIGINT and runs the line: a pin that installs is
// part of preparing the command, so Ctrl-C there is a key too.
#[test]
fn ctrl_c_while_fish_installs_a_pin_cancels_the_command() {
    let run = on_a_terminal_in(
        "\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
        |_| vec!["set".into(), "--".into(), "jq .".into()],
        Some(Typed {
            after: "MISE-BUSY",
            keys: b"\x03",
        }),
        Some("fish"),
        |home| {
            use std::os::unix::fs::PermissionsExt;
            let mise = home.join("fake-mise");
            std::fs::write(
                &mise,
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'mise 2024.8.12'; exit 0; fi\nprintf 'MISE-BUSY\\n' >/dev/tty\nexec sleep 30\n",
            )
            .unwrap();
            std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755)).unwrap();
            vec![
                ("LADE_MISE", mise.into_os_string()),
                ("MISE_INSTALLS_DIR", home.join("installs").into_os_string()),
            ]
        },
    );
    assert!(run.output.contains("MISE-BUSY"), "{}", run.output);
    assert!(run.stdout.contains("__lade_unblock"), "{}", run.stdout);
    assert_eq!(run.status.code(), Some(130), "{}", run.output);
}

#[test]
fn enter_in_pre_exec_gives_the_shell_nothing() {
    let run = on_a_terminal(
        FAILING_RULE,
        |_| vec!["set".into(), "--".into(), "touch x".into()],
        Some(Typed {
            after: PROMPT,
            keys: b"\n",
        }),
        true,
    );
    assert!(run.output.contains(PROMPT), "{}", run.output);
    assert_eq!(run.status.code(), Some(0), "{}", run.output);
    assert!(
        run.stdout.contains("unset -v LADE_RESTORE"),
        "{}",
        run.stdout
    );
    assert!(!run.stdout.contains("export"), "{}", run.stdout);
    assert!(!run.stdout.contains("PATH="), "{}", run.stdout);
}

#[test]
fn a_file_of_the_user_survives_a_run_anyway_and_the_unset_after_it() {
    let yaml = "\"touch.*\":\n  \".\":\n    file: out.json\n  KEY: value\n";
    let run = on_a_terminal_in(
        yaml,
        |_| vec!["set".into(), "--".into(), "touch x".into()],
        Some(Typed {
            after: PROMPT,
            keys: b"\n",
        }),
        Some("bash"),
        |home| {
            std::fs::write(home.join("out.json"), "mine\n").unwrap();
            Vec::new()
        },
    );
    assert!(run.output.contains("file already exists"), "{}", run.output);
    assert_eq!(run.status.code(), Some(0), "{}", run.output);
    let mut unset = common::lade(run.home.path());
    unset
        .current_dir(run.home.path())
        .env_remove("LADE_T")
        .args(["unset", "--", "touch x"])
        .assert()
        .success();
    let out = run.home.path().join("out.json");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "mine\n");
}

#[test]
fn a_key_typed_before_the_question_is_no_answer() {
    let started = Instant::now();
    let yaml = format!("\"touch.*\":\n  SLOW: \"sh://sleep 1\"\n  KEY: {FAILING}\n");
    let run = on_a_terminal(
        &yaml,
        touch_marker,
        Some(Typed {
            after: "",
            keys: b"\n",
        }),
        false,
    );
    assert!(run.output.contains(PROMPT), "{}", run.output);
    assert!(!run.home.path().join("ran").exists(), "{}", run.output);
    assert_eq!(run.status.code(), Some(1), "{}", run.output);
    assert!(
        started.elapsed() >= Duration::from_secs(5),
        "{}",
        run.output
    );
}

#[test]
fn a_disclaimer_asks_for_yes() {
    let yaml = "\"touch.*\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n";
    for (answer, runs) in [(&b"yes\n"[..], true), (&b"no\n"[..], false)] {
        let run = on_a_terminal(
            yaml,
            touch_marker,
            Some(Typed {
                after: "Type \"yes\"",
                keys: answer,
            }),
            false,
        );
        assert!(run.output.contains("Danger!"), "{}", run.output);
        assert_eq!(run.home.path().join("ran").exists(), runs, "{}", run.output);
        assert_eq!(
            run.status.code(),
            Some(if runs { 0 } else { 1 }),
            "{}",
            run.output
        );
    }
}

#[test]
fn no_answer_cancels_the_command() {
    let started = Instant::now();
    let (ran, status, output) = run_on_a_terminal(None);
    assert!(output.contains(PROMPT), "{output}");
    assert!(!ran, "{output}");
    assert_eq!(status.code(), Some(1), "{output}");
    assert!(started.elapsed() >= Duration::from_secs(5), "{output}");
}

#[test]
fn ctrl_c_cancels_the_command_at_once() {
    let started = Instant::now();
    let (ran, status, output) = run_on_a_terminal(Some(b"\x03"));
    assert!(output.contains(PROMPT), "{output}");
    assert!(!ran, "{output}");
    assert_eq!(status.code(), Some(130), "{output}");
    assert!(started.elapsed() < Duration::from_secs(5), "{output}");
}

#[test]
fn without_a_terminal_the_command_is_cancelled_at_once() {
    let home = tempdir().unwrap();
    let marker = home.path().join("ran");
    common::write_yml_raw(home.path(), FAILING_RULE);
    let started = Instant::now();
    let output = common::lade_std(home.path())
        .current_dir(home.path())
        .args(["inject", &format!("touch {}", marker.display())])
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(1), "{stderr}");
    assert!(!marker.exists(), "{stderr}");
    assert!(!stderr.contains(PROMPT), "{stderr}");
    assert!(started.elapsed() < Duration::from_secs(5), "{stderr}");
}
