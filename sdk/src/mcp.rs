use std::collections::HashMap;

use anyhow::{Context, Result, bail};
use futures::StreamExt;
use log::info;
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
};
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use url::Url;

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

async fn write_message<W>(
    output: &mut W,
    message: &str,
    protocol_version: &mut Option<HeaderValue>,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let message = serde_json::from_str::<serde_json::Value>(message)?;
    if protocol_version.is_none() {
        *protocol_version = message
            .pointer("/result/protocolVersion")
            .and_then(serde_json::Value::as_str)
            .and_then(|version| HeaderValue::from_str(version).ok());
    }
    output.write_all(&serde_json::to_vec(&message)?).await?;
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
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .build()?;
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
                    if let Some(id) = request_id {
                        write_transport_error(&mut output, id).await?;
                    } else {
                        info!("mcp http notification failed: {error}");
                    }
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
                let body = response.text().await?;
                write_message(&mut output, &body, &mut protocol_version).await?;
            } else if content_type.starts_with("text/event-stream") {
                let mut events = Vec::new();
                let mut stream = response.bytes_stream();
                while let Some(chunk) = stream.next().await {
                    events.extend_from_slice(&chunk?);
                    while let Some(event) = next_sse_event(&mut events)? {
                        if !event.is_empty() {
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

fn is_loopback_http_url(url: &Url) -> bool {
    url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn read_request(stream: &mut TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert_ne!(read, 0);
            bytes.extend_from_slice(&buffer[..read]);
            if let Some(headers_end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..headers_end]).unwrap();
                let content_length = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length:"))
                    .or_else(|| {
                        headers.lines().find_map(|line| {
                            line.split_once(':')
                                .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                                .map(|(_, value)| value.trim())
                        })
                    })
                    .unwrap()
                    .trim()
                    .parse::<usize>()
                    .unwrap();
                if bytes.len() >= headers_end + 4 + content_length {
                    return String::from_utf8(bytes).unwrap();
                }
            }
        }
    }

    #[test]
    fn rejects_invalid_headers() {
        let error = header_map(&HashMap::from([(
            "X-Test".to_string(),
            "line\r\nbreak".to_string(),
        )]))
        .unwrap_err();
        assert!(error.to_string().contains("invalid value"));
    }

    #[test]
    fn rejects_transport_owned_headers() {
        let error = header_map(&HashMap::from([(
            "MCP-Session-Id".to_string(),
            "attacker-controlled".to_string(),
        )]))
        .unwrap_err();
        assert!(error.to_string().contains("transport owns"));
    }

    #[test]
    fn action_uses_only_the_json_rpc_method() {
        assert_eq!(
            action(r#"{"method":"tools/call","params":{"secret":"do-not-log"}}"#),
            "tools/call"
        );
        assert_eq!(action(r#"{"result":{"secret":"do-not-log"}}"#), "message");
    }

    #[test]
    fn allows_https_and_loopback_http_urls() {
        assert!(is_loopback_http_url(
            &Url::parse("http://127.0.0.1/mcp").unwrap()
        ));
        assert!(is_loopback_http_url(
            &Url::parse("http://localhost/mcp").unwrap()
        ));
        assert!(!is_loopback_http_url(
            &Url::parse("http://mcp.example.com").unwrap()
        ));
        assert!(is_allowed_http_url(
            &Url::parse("https://127.0.0.1/mcp").unwrap()
        ));
        assert!(!is_allowed_http_url(
            &Url::parse("http://mcp.example.com").unwrap()
        ));
    }

    #[test]
    fn sse_parser_handles_split_crlf_and_multiline_data() {
        let mut buffer = b"data: {\"result\":\r\ndata: 1}\r".to_vec();
        assert!(next_sse_event(&mut buffer).unwrap().is_none());
        buffer.extend_from_slice(b"\n\r\n");
        assert_eq!(
            next_sse_event(&mut buffer).unwrap().as_deref(),
            Some("{\"result\":\n1}")
        );
        assert!(buffer.is_empty());
    }

    #[tokio::test]
    async fn bridge_keeps_running_after_a_transport_error_without_replaying() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            {
                let (mut first_stream, _) = listener.accept().await.unwrap();
                let first = read_request(&mut first_stream).await;
                assert!(first.contains(r#""id":1"#));
            }
            let (mut second_stream, _) = listener.accept().await.unwrap();
            let second = read_request(&mut second_stream).await;
            let body = r#"{"jsonrpc":"2.0","id":2,"result":{}}"#;
            assert!(second.contains(r#""id":2"#));
            assert!(!second.contains(r#""id":1"#));
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            second_stream.write_all(response.as_bytes()).await.unwrap();
        });
        let (mut input_writer, input_reader) = tokio::io::duplex(1024);
        let (output_writer, mut output_reader) = tokio::io::duplex(1024);
        input_writer
            .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"first\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"second\"}\n")
            .await
            .unwrap();
        drop(input_writer);
        let bridge = tokio::spawn(bridge_http(
            HttpBridgeConfig {
                url: Url::parse(&format!("http://{address}/mcp")).unwrap(),
                headers: HashMap::new(),
            },
            input_reader,
            output_writer,
        ));
        server.await.unwrap();
        bridge.await.unwrap().unwrap();
        let mut output = String::new();
        output_reader.read_to_string(&mut output).await.unwrap();
        assert!(output.contains(r#""id":1"#));
        assert!(output.contains("MCP HTTP transport failed"));
        assert!(output.contains(r#""id":2"#));
    }
}
