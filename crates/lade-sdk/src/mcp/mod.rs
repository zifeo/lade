use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use log::info;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use url::{Host, Url};

/// Configuration for a one-process stdio-to-HTTP MCP bridge.
///
/// The caller owns process lifecycle and diagnostics. This module only moves
/// JSON-RPC bytes between the standard stream framing and the HTTP transport.
#[derive(Debug, Clone)]
pub struct HttpBridgeConfig {
    pub url: Url,
    pub headers: HashMap<String, String>,
}

fn header_map(headers: &HashMap<String, String>) -> Result<HeaderMap> {
    let mut result = HeaderMap::new();
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .with_context(|| format!("invalid HTTP header name '{name}'"))?;
        if matches!(
            name.as_str(),
            "accept"
                | "content-type"
                | "content-length"
                | "connection"
                | "host"
                | "origin"
                | "mcp-session-id"
                | "mcp-protocol-version"
                | "last-event-id"
        ) {
            // § aeffh6p49.c@nm5n7
            // The configuration author is trusted to choose arbitrary service
            // headers. These names are different: reqwest or MCP owns their
            // value for framing, routing, or per-request session state, so
            // accepting one here would make a valid secret mapping produce an
            // invalid or desynchronised transport.
            bail!("MCP transport owns HTTP header '{name}'");
        }
        let value = HeaderValue::from_str(value)
            .with_context(|| format!("invalid value for HTTP header '{name}'"))?;
        result.insert(name, value);
    }
    Ok(result)
}

fn action(message: &str) -> String {
    serde_json::from_str::<serde_json::Value>(message)
        .ok()
        .and_then(|message| message.get("method")?.as_str().map(str::to_owned))
        .unwrap_or_else(|| "message".to_string())
}

fn request_id(message: &str) -> Result<Option<serde_json::Value>> {
    let message = serde_json::from_str::<serde_json::Value>(message)?;
    Ok(message.get("id").cloned())
}

async fn write_transport_error<W>(output: &mut W, id: serde_json::Value) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let error = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": -32000,
            "message": "MCP HTTP transport failed",
        },
    });
    output.write_all(&serde_json::to_vec(&error)?).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

/// A request whose exchange failed in transport, before or while its answer
/// arrived, gets an error in its place; a notification has no one to tell.
async fn answer_transport_failure<W>(
    output: &mut W,
    request_id: Option<serde_json::Value>,
    error: &reqwest::Error,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    // § hkqw5dexk.c@xsx43
    match request_id {
        Some(id) => write_transport_error(output, id).await,
        None => {
            info!("mcp http notification failed: {error}");
            Ok(())
        }
    }
}

/// Whether `event` is the server's answer to the request `id`.
fn answers(event: &str, id: Option<&serde_json::Value>) -> bool {
    let Some(id) = id else {
        return false;
    };
    serde_json::from_str::<serde_json::Value>(event).is_ok_and(|message| {
        message.get("id") == Some(id)
            && (message.get("result").is_some() || message.get("error").is_some())
    })
}

async fn write_message<W>(
    output: &mut W,
    message: &str,
    protocol_version: &mut Option<HeaderValue>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    // § aeffh6p49.a@qq4tt
    let parsed = serde_json::from_str::<serde_json::Value>(message)?;
    if protocol_version.is_none() {
        *protocol_version = parsed
            .pointer("/result/protocolVersion")
            .and_then(serde_json::Value::as_str)
            .and_then(|version| HeaderValue::from_str(version).ok());
    }
    // Write the server's own bytes, not a re-serialisation that would rewrite
    // numbers. Valid JSON holds no raw line break inside a string, so dropping
    // them only joins the message onto the one line stdio framing needs.
    let line: Vec<u8> = message
        .bytes()
        .filter(|byte| !matches!(byte, b'\n' | b'\r'))
        .collect();
    output.write_all(&line).await?;
    output.write_all(b"\n").await?;
    output.flush().await?;
    Ok(())
}

fn next_sse_event(buffer: &mut Vec<u8>) -> Result<Option<String>> {
    let delimiter = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| (position, 4))
        .or_else(|| {
            buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| (position, 2))
        });
    let Some((position, length)) = delimiter else {
        return Ok(None);
    };
    let event = buffer.drain(..position + length).collect::<Vec<_>>();
    let event = std::str::from_utf8(&event[..position])?;
    let data = event
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(data))
}

async fn cleanup_session(
    client: &Client,
    url: Url,
    headers: HeaderMap,
    session_id: Option<HeaderValue>,
) {
    let Some(session_id) = session_id else {
        return;
    };
    if let Err(error) = client
        .delete(url)
        .headers(headers)
        .header("MCP-Session-Id", session_id)
        .send()
        .await
    {
        info!("mcp http session cleanup failed: {error}");
    }
}

/// Relay newline-delimited JSON-RPC from `input` to a Streamable HTTP MCP
/// endpoint and write its JSON-RPC responses as newline-delimited bytes.
///
/// This intentionally does not understand tools or schemas. It preserves each
/// input message verbatim and only parses JSON enough to track the negotiated
/// protocol version returned by legacy initialization.
pub async fn bridge_http<R, W>(config: HttpBridgeConfig, input: R, mut output: W) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    // § aeffh6p49.b@hxtgr
    if !is_allowed_http_url(&config.url) {
        bail!("MCP HTTP URLs must use https unless they target a loopback address");
    }
    if !config.url.username().is_empty()
        || config.url.password().is_some()
        || config.url.fragment().is_some()
    {
        bail!("MCP HTTP URL must not contain credentials or a fragment");
    }
    let headers = header_map(&config.headers)?;
    // § hkqw5dexk.c@0vdbx
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()?;
    // § end hkqw5dexk.c
    // § end aeffh6p49.b
    let mut reader = BufReader::new(input);
    let mut line = String::new();
    let mut session_id: Option<HeaderValue> = None;
    let mut protocol_version: Option<HeaderValue> = None;

    let result = async {
        loop {
            line.clear();
            if reader.read_line(&mut line).await? == 0 {
                break;
            }
            let message = line.trim_end_matches(['\r', '\n']);
            if message.is_empty() {
                continue;
            }
            let action = action(message);
            let request_id = request_id(message)?;
            info!("mcp http -> {action}");
            let started = Instant::now();
            let mut request = client
                .post(config.url.clone())
                .headers(headers.clone())
                .header(ACCEPT, "application/json, text/event-stream")
                .header(CONTENT_TYPE, "application/json")
                .body(message.to_owned());
            if let Some(session) = &session_id {
                request = request.header("MCP-Session-Id", session.clone());
            }
            if let Some(version) = &protocol_version {
                request = request.header("MCP-Protocol-Version", version.clone());
            }
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    // § hkqw5dexk.c@4vwkx
                    answer_transport_failure(&mut output, request_id, &error).await?;
                    continue;
                }
            };
            info!(
                "mcp http <- {} ({} ms)",
                response.status(),
                started.elapsed().as_millis()
            );
            if response.status() == StatusCode::ACCEPTED {
                continue;
            }
            if !response.status().is_success() {
                bail!("MCP HTTP server returned {}", response.status());
            }
            if session_id.is_none() {
                session_id = response.headers().get("MCP-Session-Id").cloned();
            }
            let content_type = response
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string();
            if content_type.starts_with("application/json") {
                match response.text().await {
                    Ok(body) => write_message(&mut output, &body, &mut protocol_version).await?,
                    Err(error) => {
                        answer_transport_failure(&mut output, request_id, &error).await?;
                    }
                }
            } else if content_type.starts_with("text/event-stream") {
                let mut events = Vec::new();
                let mut answered = false;
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            if !answered {
                                answer_transport_failure(&mut output, request_id, &error).await?;
                            }
                            break;
                        }
                    };
                    events.extend_from_slice(&chunk);
                    while let Some(event) = next_sse_event(&mut events)? {
                        if !event.is_empty() {
                            answered |= answers(&event, request_id.as_ref());
                            write_message(&mut output, &event, &mut protocol_version).await?;
                        }
                    }
                }
            } else {
                bail!("MCP HTTP server returned unsupported content type '{content_type}'");
            }
        }
        Ok(())
    }
    .await;
    cleanup_session(&client, config.url, headers, session_id).await;
    result
}

fn is_allowed_http_url(url: &Url) -> bool {
    url.scheme() == "https" || is_loopback_http_url(url)
}

/// `host_str` brackets an IPv6 host, so it is matched as a parsed host.
fn is_loopback_http_url(url: &Url) -> bool {
    url.scheme() == "http"
        && match url.host() {
            Some(Host::Domain(domain)) => domain == "localhost",
            Some(Host::Ipv4(ip)) => ip == Ipv4Addr::LOCALHOST,
            Some(Host::Ipv6(ip)) => ip == Ipv6Addr::LOCALHOST,
            None => false,
        }
}

#[cfg(test)]
mod tests;
