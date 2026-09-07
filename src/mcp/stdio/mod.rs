use std::{
    collections::HashMap,
    ffi::OsString,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Instant,
};

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::Child;
use tokio::time::{Duration, timeout};

use crate::{child_signals::ChildWatch, exit_codes, message_box::MessageBox};

use super::{CLIENT_EOF_GRACE, INITIAL_RESTART_BACKOFF, MAX_RESTART_BACKOFF, STARTUP_TIMEOUT};

mod pump;

#[cfg(test)]
pub(super) use pump::{message_id, message_method};

enum SpawnAttempt {
    Child(Child),
    Retry,
    Exit(Option<i32>),
}

struct Session<R> {
    input: BufReader<R>,
    pending: Vec<String>,
    forwarded_initialize: bool,
    backoff: Duration,
    startup_deadline: Instant,
    ready_once: bool,
    client_line: String,
    watch: ChildWatch,
}

impl<R: AsyncRead + Unpin> Session<R> {
    fn new(input: R) -> Self {
        Self {
            input: BufReader::new(input),
            pending: Vec::new(),
            forwarded_initialize: false,
            backoff: INITIAL_RESTART_BACKOFF,
            startup_deadline: Instant::now() + STARTUP_TIMEOUT,
            ready_once: false,
            client_line: String::new(),
            watch: ChildWatch::new(),
        }
    }
}

pub(super) async fn run_stdio(
    argv: Vec<OsString>,
    env: HashMap<String, String>,
    current_dir: std::path::PathBuf,
) -> Result<Option<i32>> {
    supervise_stdio(
        argv,
        env,
        current_dir,
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await
}

/// Restart the child only until the client handshake has been copied through.
///
/// `initialize` is not a Unix signal. It is the first JSON-RPC line the MCP
/// client writes to our stdin (`"method":"initialize"`). We write those same
/// bytes to the child's stdin. After that the session is the client's.
pub(super) async fn supervise_stdio<R, W>(
    argv: Vec<OsString>,
    env: HashMap<String, String>,
    current_dir: std::path::PathBuf,
    input: R,
    mut output: W,
) -> Result<Option<i32>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut session = Session::new(input);
    loop {
        if session.watch.stop_requested() {
            return Ok(None);
        }
        let mut child = match spawn_or_retry(&argv, &env, &current_dir, &mut session).await? {
            SpawnAttempt::Child(child) => child,
            SpawnAttempt::Retry => continue,
            SpawnAttempt::Exit(code) => return Ok(code),
        };
        if let Some(pid) = child.id() {
            session.watch.set_pid(pid, true);
        }
        let spawned_at = Instant::now();
        let outcome = pump::pump_stdio(
            &mut child,
            &mut session.input,
            &mut output,
            &mut session.pending,
            &mut session.client_line,
            &mut session.forwarded_initialize,
            &session.watch,
        )
        .await?;
        session.watch.clear_pid();
        if spawned_at.elapsed() > INITIAL_RESTART_BACKOFF {
            session.ready_once = true;
            session.backoff = INITIAL_RESTART_BACKOFF;
        }
        if let Some(code) = restart_or_exit(outcome, &mut session, &mut output).await? {
            return Ok(code);
        }
    }
}

async fn spawn_or_retry<R>(
    argv: &[OsString],
    env: &HashMap<String, String>,
    current_dir: &Path,
    session: &mut Session<R>,
) -> Result<SpawnAttempt>
where
    R: AsyncRead + Unpin,
{
    match spawn_stdio(argv, env, current_dir) {
        Ok(child) => Ok(SpawnAttempt::Child(child)),
        Err(error) => {
            if Instant::now() >= session.startup_deadline {
                MessageBox::new()
                    .error()
                    .line("MCP server failed to start before initialize.")
                    .paragraph(error.to_string())
                    .print_stderr();
                return Ok(SpawnAttempt::Exit(Some(exit_codes::FAILURE)));
            }
            log::warn!("mcp stdio failed to start: {error}");
            if restart_after_crash(session).await? {
                Ok(SpawnAttempt::Retry)
            } else {
                Ok(SpawnAttempt::Exit(None))
            }
        }
    }
}

fn spawn_stdio(
    argv: &[OsString],
    env: &HashMap<String, String>,
    current_dir: &Path,
) -> Result<Child> {
    let program = argv
        .first()
        .ok_or_else(|| anyhow::anyhow!("missing MCP stdio command"))?;
    let mut command = tokio::process::Command::new(program);
    command
        .args(&argv[1..])
        .current_dir(current_dir)
        .envs(std::env::vars());
    crate::shell::strip_child_protocol_tokio(&mut command);
    command
        .envs(env.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    // Own session so a tty hangup does not stop the MCP child.
    crate::child_signals::detach_session_tokio(&mut command);
    Ok(command.spawn()?)
}

async fn restart_or_exit<R, W>(
    outcome: pump::PumpOutcome,
    session: &mut Session<R>,
    output: &mut W,
) -> Result<Option<Option<i32>>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let pump::PumpOutcome {
        status,
        client_eof,
        in_flight,
    } = outcome;
    if session.watch.stop_requested() {
        if session.forwarded_initialize {
            write_in_flight_exits(output, in_flight).await?;
        }
        return Ok(Some(exit_code(status)));
    }
    if session.forwarded_initialize {
        write_in_flight_exits(output, in_flight).await?;
        MessageBox::new()
            .error()
            .line("MCP server exited after initialize.")
            .line("This process will exit. Re-run `lade mcp` to hydrate again.")
            .print_stderr();
        return Ok(Some(exit_code(status)));
    }
    if client_eof {
        return Ok(Some(exit_code(status)));
    }
    if !session.ready_once && Instant::now() >= session.startup_deadline {
        MessageBox::new()
            .error()
            .line("MCP server exited before initialize.")
            .paragraph(format!("last status: {status}"))
            .print_stderr();
        return Ok(Some(Some(exit_codes::FAILURE)));
    }
    log::warn!("mcp stdio exited before initialize (status: {status})");
    if !restart_after_crash(session).await? {
        return Ok(Some(None));
    }
    Ok(None)
}

async fn restart_after_crash<R>(session: &mut Session<R>) -> Result<bool>
where
    R: AsyncRead + Unpin,
{
    if !wait_restart_or_client(session).await? {
        return Ok(false);
    }
    session.backoff = (session.backoff * 2).min(MAX_RESTART_BACKOFF);
    Ok(true)
}

async fn write_in_flight_exits<W>(output: &mut W, in_flight: Vec<Value>) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    for id in in_flight {
        write_server_exited(output, id).await?;
    }
    Ok(())
}

async fn wait_restart_or_client<R>(session: &mut Session<R>) -> Result<bool>
where
    R: AsyncRead + Unpin,
{
    let deadline = Instant::now() + session.backoff;
    loop {
        if session.watch.stop_requested() {
            return Ok(false);
        }
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            return Ok(!session.watch.stop_requested());
        }
        tokio::select! {
            _ = tokio::time::sleep(remain.min(Duration::from_millis(20))) => {}
            read = session.input.read_line(&mut session.client_line) => {
                if read? == 0 {
                    return Ok(false);
                }
                session.pending.push(std::mem::take(&mut session.client_line));
                return Ok(!session.watch.stop_requested());
            }
        }
    }
}

async fn finish_child(child: &mut Child) -> Result<ExitStatus> {
    match timeout(CLIENT_EOF_GRACE, child.wait()).await {
        Ok(status) => Ok(status?),
        Err(_) => {
            let _ = child.kill().await;
            Ok(child.wait().await?)
        }
    }
}

fn exit_code(status: ExitStatus) -> Option<i32> {
    (!status.success()).then_some(status.code().unwrap_or(1))
}

async fn write_server_exited<W>(output: &mut W, id: Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": "MCP server exited",
        },
    });
    output.write_all(&serde_json::to_vec(&error)?).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}
