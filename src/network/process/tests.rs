use super::logs::dedupe_lines;
use super::*;
use anyhow::anyhow;
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::Ordering;

#[test]
#[cfg(unix)]
fn child_output_files_capture_stdout_and_stderr() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf 'out\nout'; printf 'err\nerr' >&2"]);
    let logs = ChildOutputFiles::capture(&mut command).unwrap();
    let status = command.spawn().unwrap().wait().unwrap();
    assert!(status.success());
    let text = logs.read_text();
    logs.cleanup();
    assert!(text.contains("stdout:\nout"));
    assert!(text.contains("stderr:\nerr"));
    assert!(!text.contains("out\nout"));
    assert!(!text.contains("err\nerr"));
}

#[test]
fn dedupe_lines_collapses_kubernetes_memcache_retries() {
    let raw = concat!(
        "E0705 00:01:52.834950   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
        "E0705 00:01:52.894498   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
        "Unable to connect to the server: same\n",
    );

    let text = dedupe_lines(raw);
    assert_eq!(text.matches("memcache.go").count(), 1);
    assert!(text.contains("Unable to connect to the server: same"));
}

#[test]
#[cfg(unix)]
fn dropping_supervisor_terminates_the_provider_process_group() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (forward, pid) = RunningForward::supervise(
        "test forward".to_string(),
        "127.0.0.1".to_string(),
        address.port(),
        || {
            let mut command = Command::new("sh");
            command.args(["-c", "while :; do :; done"]);
            Ok(command)
        },
    )
    .unwrap();
    drop(forward);
    assert_eq!(kill(Pid::from_raw(pid as i32), None), Err(Errno::ESRCH));
}

#[test]
#[cfg(unix)]
fn supervisor_retries_a_startup_failure() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let command_attempts = Arc::clone(&attempts);
    let (forward, _) = RunningForward::supervise(
        "test forward".to_string(),
        "127.0.0.1".to_string(),
        address.port(),
        move || {
            let attempt = command_attempts.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                return Err(anyhow!("simulated startup failure"));
            }
            let mut command = Command::new("sh");
            command.args(["-c", "while :; do :; done"]);
            Ok(command)
        },
    )
    .unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    drop(forward);
}
