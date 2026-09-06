use std::process::ExitStatus;

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin};
use tokio::time::Duration;

use crate::child_signals::ChildWatch;

pub(super) struct PumpOutcome {
    pub status: ExitStatus,
    pub client_eof: bool,
    pub in_flight: Vec<Value>,
}

pub(super) async fn pump_stdio<R, W>(
    child: &mut Child,
    input: &mut BufReader<R>,
    output: &mut W,
    pending: &mut Vec<String>,
    client_line: &mut String,
    forwarded_initialize: &mut bool,
    watch: &ChildWatch,
) -> Result<PumpOutcome>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
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
        match write_to_child(&line, stdin, forwarded_initialize, &mut in_flight).await {
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
        drain_child_stdout(&mut child_stdout, &mut child_line, &mut in_flight, output).await?;
        super::finish_child(child).await?
    } else {
        loop {
            tokio::select! {
                biased;
                read = input.read_line(client_line), if !client_eof => {
                    if read? == 0 {
                        client_eof = true;
                        drop(child_stdin.take());
                        continue;
                    }
                    let Some(stdin) = child_stdin.as_mut() else {
                        pending.push(std::mem::take(client_line));
                        continue;
                    };
                    let line = std::mem::take(client_line);
                    match write_to_child(
                        &line,
                        stdin,
                        forwarded_initialize,
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
                                output,
                            )
                            .await?;
                            break super::finish_child(child).await?;
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
                        output,
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
                            output,
                        )
                        .await?;
                        break super::finish_child(child).await?;
                    }
                    if let Some(status) = child.try_wait()? {
                        drain_child_stdout(
                            &mut child_stdout,
                            &mut child_line,
                            &mut in_flight,
                            output,
                        )
                        .await?;
                        break status;
                    }
                }
            }
        }
    };

    Ok(PumpOutcome {
        status,
        client_eof,
        in_flight,
    })
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

pub fn message_method(raw: &str) -> Option<String> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|message| message.get("method")?.as_str().map(str::to_owned))
}

pub fn message_id(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|message| message.get("id").cloned())
}
