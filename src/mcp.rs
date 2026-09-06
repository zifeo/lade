use std::{
    collections::HashMap,
    ffi::OsString,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Instant,
};

use anyhow::{Result, bail};
use log::info;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::time::{Duration, timeout};
use url::Url;

use crate::{
    args::McpCommand, child_signals::ChildWatch, config::Config, context::InvocationContext,
    exit_codes, message_box::MessageBox, prompt,
};

/// Same bounds as `network::process`. Readiness here is not a port: an MCP
/// child is ready when spawn succeeded and the process is still running. Stdio
/// servers stay silent until `initialize`.
const INITIAL_RESTART_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const CLIENT_EOF_GRACE: Duration = Duration::from_secs(1);

pub async fn run(
    command: McpCommand,
    ctx: &InvocationContext,
    config: &Config,
    current_dir: &Path,
) -> Result<Option<i32>> {
    let target = target(&command)?;
    let rules = config.collect_for(&target, ctx.audience);
    let disclaimers = Config::disclaimers_from_rules(&rules);
    prompt::resolve_disclaimers(ctx, &disclaimers, &target).await?;
    let mut access = crate::access::acquire_attached(
        config,
        &rules,
        ctx.stderr_is_terminal && !ctx.stdin_is_terminal,
    )
    .await?;
    for warning in &access.warnings {
        MessageBox::new().warning().line(warning).print_stderr();
    }
    let result = match command.url {
        Some(raw_url) => {
            info!("mcp started transport=http");
            run_http(raw_url, access.env.clone()).await
        }
        None => {
            info!("mcp started transport=stdio");
            let mut env = access.env.clone();
            match ctx.via.child_stamp() {
                Some(value) => {
                    env.insert(crate::shell::LADE_VIA.to_string(), value.to_string());
                }
                None => {
                    env.remove(crate::shell::LADE_VIA);
                }
            }
            run_stdio(command.argv, env, current_dir.to_path_buf()).await
        }
    };
    access.cleanup()?;
    info!("mcp stopped");
    result
}

fn target(command: &McpCommand) -> Result<String> {
    match (&command.url, command.argv.is_empty()) {
        (Some(url), true) => Ok(url.clone()),
        (None, false) => canonical_argv(&command.argv),
        (Some(_), false) => bail!("use either an MCP URL or a stdio command after '--', not both"),
        (None, true) => bail!("provide an MCP HTTPS URL or a stdio command after '--'"),
    }
}

fn canonical_argv(argv: &[OsString]) -> Result<String> {
    argv.iter()
        .map(|value| {
            let value = value
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("MCP command arguments must be valid UTF-8"))?;
            if !value.is_empty()
                && value
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || "@%+=:,./_-".contains(ch))
            {
                Ok(value.to_string())
            } else {
                Ok(format!("'{}'", value.replace('\'', "'\\''")))
            }
        })
        .collect::<Result<Vec<_>>>()
        .map(|argv| argv.join(" "))
}

async fn run_stdio(
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
async fn supervise_stdio<R, W>(
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
    let mut input = BufReader::new(input);
    let mut pending = Vec::new();
    let mut forwarded_initialize = false;
    let mut backoff = INITIAL_RESTART_BACKOFF;
    let startup_deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut ready_once = false;
    let mut client_line = String::new();
    let watch = ChildWatch::new();

    loop {
        if watch.stop_requested() {
            return Ok(None);
        }
        let mut child = match spawn_stdio(&argv, &env, &current_dir) {
            Ok(child) => child,
            Err(error) => {
                if Instant::now() >= startup_deadline {
                    MessageBox::new()
                        .error()
                        .line("MCP server failed to start before initialize.")
                        .paragraph(error.to_string())
                        .print_stderr();
                    return Ok(Some(exit_codes::FAILURE));
                }
                log::warn!("mcp stdio failed to start: {error}");
                if !wait_restart_or_client(
                    &mut input,
                    &mut pending,
                    &mut client_line,
                    backoff,
                    &watch,
                )
                .await?
                {
                    return Ok(None);
                }
                backoff = (backoff * 2).min(MAX_RESTART_BACKOFF);
                continue;
            }
        };
        if let Some(pid) = child.id() {
            watch.set_pid(pid, true);
        }
        let spawned_at = Instant::now();
        let mut child_stdin = Some(child.stdin.take().expect("piped stdin"));
        let mut child_stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let mut in_flight = Vec::new();
        let mut child_line = String::new();
        let mut client_eof = false;
        let mut broken_pipe = false;

        while !pending.is_empty() {
            let Some(stdin) = child_stdin.as_mut() else {
                break;
            };
            let line = pending.remove(0);
            match write_to_child(&line, stdin, &mut forwarded_initialize, &mut in_flight).await {
                Ok(()) => {}
                Err(error) if is_broken_pipe(&error) => {
                    pending.insert(0, line);
                    broken_pipe = true;
                    break;
                }
                Err(error) => return Err(error),
            }
        }

        let status = if broken_pipe {
            drain_child_stdout(
                &mut child_stdout,
                &mut child_line,
                &mut in_flight,
                &mut output,
            )
            .await?;
            finish_child(&mut child).await?
        } else {
            loop {
                tokio::select! {
                    biased;
                    read = input.read_line(&mut client_line), if !client_eof => {
                        if read? == 0 {
                            client_eof = true;
                            drop(child_stdin.take());
                            continue;
                        }
                        let Some(stdin) = child_stdin.as_mut() else {
                            pending.push(std::mem::take(&mut client_line));
                            continue;
                        };
                        let line = std::mem::take(&mut client_line);
                        match write_to_child(
                            &line,
                            stdin,
                            &mut forwarded_initialize,
                            &mut in_flight,
                        )
                        .await
                        {
                            Ok(()) => {}
                            Err(error) if is_broken_pipe(&error) => {
                                pending.push(line);
                                drain_child_stdout(
                                    &mut child_stdout,
                                    &mut child_line,
                                    &mut in_flight,
                                    &mut output,
                                )
                                .await?;
                                break finish_child(&mut child).await?;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                    read = child_stdout.read_line(&mut child_line) => {
                        if read? == 0 {
                            if let Some(status) = child.try_wait()? {
                                break status;
                            }
                            continue;
                        }
                        forward_child_line(
                            &child_line,
                            &mut in_flight,
                            &mut output,
                        )
                        .await?;
                        child_line.clear();
                    }
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {
                        if watch.stop_requested() {
                            drain_child_stdout(
                                &mut child_stdout,
                                &mut child_line,
                                &mut in_flight,
                                &mut output,
                            )
                            .await?;
                            break finish_child(&mut child).await?;
                        }
                        if let Some(status) = child.try_wait()? {
                            drain_child_stdout(
                                &mut child_stdout,
                                &mut child_line,
                                &mut in_flight,
                                &mut output,
                            )
                            .await?;
                            break status;
                        }
                    }
                }
            }
        };

        watch.clear_pid();
        if spawned_at.elapsed() > INITIAL_RESTART_BACKOFF {
            ready_once = true;
            backoff = INITIAL_RESTART_BACKOFF;
        }

        if watch.stop_requested() {
            if forwarded_initialize {
                for id in in_flight {
                    write_server_exited(&mut output, id).await?;
                }
            }
            return Ok(exit_code(status));
        }

        if forwarded_initialize {
            for id in in_flight {
                write_server_exited(&mut output, id).await?;
            }
            MessageBox::new()
                .error()
                .line("MCP server exited after initialize.")
                .line("This lade process will exit. Starting lade mcp again will resolve secrets.")
                .print_stderr();
            return Ok(exit_code(status));
        }

        if client_eof {
            return Ok(exit_code(status));
        }

        if !ready_once && Instant::now() >= startup_deadline {
            MessageBox::new()
                .error()
                .line("MCP server exited before initialize.")
                .paragraph(format!("last status: {status}"))
                .print_stderr();
            return Ok(Some(exit_codes::FAILURE));
        }

        log::warn!("mcp stdio exited before initialize (status: {status})");
        if !wait_restart_or_client(&mut input, &mut pending, &mut client_line, backoff, &watch)
            .await?
        {
            return Ok(None);
        }
        backoff = (backoff * 2).min(MAX_RESTART_BACKOFF);
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
        .envs(std::env::vars())
        .env_remove(crate::shell::LADE_VIA)
        .envs(env.clone())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    // Own session so a tty hangup does not stop the MCP child.
    crate::child_signals::detach_session_tokio(&mut command);
    Ok(command.spawn()?)
}

async fn forward_child_line<W>(line: &str, in_flight: &mut Vec<Value>, output: &mut W) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    if let Some(id) = message_id(line.trim_end()) {
        in_flight.retain(|open| open != &id);
    }
    output.write_all(line.as_bytes()).await?;
    output.flush().await?;
    Ok(())
}

async fn drain_child_stdout<R, W>(
    child_stdout: &mut BufReader<R>,
    child_line: &mut String,
    in_flight: &mut Vec<Value>,
    output: &mut W,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    if !child_line.is_empty() {
        forward_child_line(child_line, in_flight, output).await?;
        child_line.clear();
    }
    loop {
        if child_stdout.read_line(child_line).await? == 0 {
            return Ok(());
        }
        forward_child_line(child_line, in_flight, output).await?;
        child_line.clear();
    }
}

async fn write_to_child(
    line: &str,
    child_stdin: &mut ChildStdin,
    forwarded_initialize: &mut bool,
    in_flight: &mut Vec<Value>,
) -> Result<()> {
    let payload = line.trim_end_matches(['\r', '\n']);
    child_stdin.write_all(line.as_bytes()).await?;
    if !line.ends_with('\n') {
        child_stdin.write_all(b"\n").await?;
    }
    child_stdin.flush().await?;
    if !payload.is_empty() {
        if message_method(payload).as_deref() == Some("initialize") {
            *forwarded_initialize = true;
        }
        if let Some(id) = message_id(payload) {
            in_flight.push(id);
        }
    }
    Ok(())
}

async fn wait_restart_or_client<R>(
    input: &mut BufReader<R>,
    pending: &mut Vec<String>,
    client_line: &mut String,
    backoff: Duration,
    watch: &ChildWatch,
) -> Result<bool>
where
    R: AsyncRead + Unpin,
{
    let deadline = Instant::now() + backoff;
    loop {
        if watch.stop_requested() {
            return Ok(false);
        }
        let remain = deadline.saturating_duration_since(Instant::now());
        if remain.is_zero() {
            return Ok(!watch.stop_requested());
        }
        tokio::select! {
            _ = tokio::time::sleep(remain.min(Duration::from_millis(20))) => {}
            read = input.read_line(client_line) => {
                if read? == 0 {
                    return Ok(false);
                }
                pending.push(std::mem::take(client_line));
                return Ok(!watch.stop_requested());
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

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|error| {
                error.kind() == std::io::ErrorKind::BrokenPipe
                    || error.kind() == std::io::ErrorKind::ConnectionReset
            })
    })
}

fn message_method(raw: &str) -> Option<String> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|message| message.get("method")?.as_str().map(str::to_owned))
}

fn message_id(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|message| message.get("id").cloned())
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

/// HTTP has no local child. Transport errors are answered in-process by
/// `lade_sdk::mcp::bridge_http` and do not re-resolve secrets.
async fn run_http(raw_url: String, headers: HashMap<String, String>) -> Result<Option<i32>> {
    let url = Url::parse(&raw_url)?;
    lade_sdk::mcp::bridge_http(
        lade_sdk::mcp::HttpBridgeConfig { url, headers },
        tokio::io::stdin(),
        tokio::io::stdout(),
    )
    .await?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[test]
    fn canonical_argv_quotes_only_when_needed() {
        assert_eq!(
            canonical_argv(&[OsString::from("npx"), OsString::from("with space")]).unwrap(),
            "npx 'with space'"
        );
    }

    #[test]
    fn message_method_reads_initialize() {
        assert_eq!(
            message_method(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#).as_deref(),
            Some("initialize")
        );
        assert_eq!(
            message_id(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#),
            Some(Value::from(1))
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn restarts_before_initialize_and_keeps_env() {
        let _lock = crate::child_signals::SUPERVISE_TEST_LOCK.lock().await;
        crate::child_signals::reset_for_test();
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("started");
        let script = format!(
            "if [ ! -f '{}' ]; then touch '{}'; exit 1; fi; printf '%s\\n' \"$TOKEN\"; exec cat",
            marker.display(),
            marker.display()
        );
        let (input_writer, input_reader) = tokio::io::duplex(1024);
        let (output_writer, mut output_reader) = tokio::io::duplex(1024);
        let env = HashMap::from([("TOKEN".to_string(), "cached-secret".to_string())]);
        let supervise = tokio::spawn(supervise_stdio(
            vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from(script),
            ],
            env,
            dir.path().to_path_buf(),
            input_reader,
            output_writer,
        ));
        let mut output = String::new();
        let started = Instant::now();
        while !output.contains("cached-secret") {
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "child was not restarted before initialize: {output}"
            );
            let mut buf = [0; 64];
            let read = timeout(Duration::from_secs(5), output_reader.read(&mut buf))
                .await
                .expect("timed out waiting for restarted child")
                .unwrap();
            assert_ne!(read, 0);
            output.push_str(std::str::from_utf8(&buf[..read]).unwrap());
        }
        drop(input_writer);
        let code = supervise.await.unwrap().unwrap();
        assert_eq!(code, None);
        assert!(marker.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn stop_during_pre_initialize_backoff_does_not_restart() {
        let _lock = crate::child_signals::SUPERVISE_TEST_LOCK.lock().await;
        crate::child_signals::reset_for_test();
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("starts");
        let started = dir.path().join("started");
        let script = format!(
            "printf x >> '{}'; if [ ! -f '{}' ]; then touch '{}'; exit 1; fi; exec cat",
            starts.display(),
            started.display(),
            started.display()
        );
        let (input_writer, input_reader) = tokio::io::duplex(1024);
        let (output_writer, _output_reader) = tokio::io::duplex(1024);
        let supervise = tokio::spawn(supervise_stdio(
            vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from(script),
            ],
            HashMap::new(),
            dir.path().to_path_buf(),
            input_reader,
            output_writer,
        ));
        let deadline = Instant::now() + Duration::from_secs(3);
        while !started.exists() {
            assert!(
                Instant::now() < deadline,
                "child never recorded the first crash"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        crate::child_signals::request_stop_for_test();
        let code = timeout(Duration::from_secs(2), supervise)
            .await
            .expect("supervise ignored stop during backoff")
            .unwrap()
            .unwrap();
        drop(input_writer);
        crate::child_signals::reset_for_test();
        assert!(
            code.is_none() || code == Some(1),
            "wrapper should stop without restarting, got {code:?}"
        );
        assert_eq!(std::fs::read_to_string(&starts).unwrap(), "x");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn after_initialize_answers_in_flight_ids_and_does_not_restart() {
        let _lock = crate::child_signals::SUPERVISE_TEST_LOCK.lock().await;
        crate::child_signals::reset_for_test();
        let dir = tempfile::tempdir().unwrap();
        let starts = dir.path().join("starts");
        let script = format!("printf x >> '{}'; read line; exit 1", starts.display());
        let (mut input_writer, input_reader) = tokio::io::duplex(1024);
        let (output_writer, mut output_reader) = tokio::io::duplex(1024);
        let supervise = tokio::spawn(supervise_stdio(
            vec![
                OsString::from("sh"),
                OsString::from("-c"),
                OsString::from(script),
            ],
            HashMap::new(),
            dir.path().to_path_buf(),
            input_reader,
            output_writer,
        ));
        input_writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n")
            .await
            .unwrap();
        drop(input_writer);
        let code = supervise.await.unwrap().unwrap();
        assert_eq!(code, Some(1));
        let mut output = String::new();
        output_reader.read_to_string(&mut output).await.unwrap();
        assert!(output.contains(r#""id":1"#), "{output}");
        assert!(output.contains("MCP server exited"), "{output}");
        assert_eq!(std::fs::read_to_string(&starts).unwrap(), "x");
    }
}
