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
    assert!(is_loopback_http_url(
        &Url::parse("http://[::1]/mcp").unwrap()
    ));
    assert!(is_loopback_http_url(
        &Url::parse("http://[::1]:8080/mcp").unwrap()
    ));
    assert!(!is_loopback_http_url(
        &Url::parse("http://[::2]/mcp").unwrap()
    ));
    assert!(!is_loopback_http_url(
        &Url::parse("http://localhost.example.com/mcp").unwrap()
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

/// Runs the bridge on one request against a server that sends `head` and
/// `body`, then closes the connection.
async fn bridge_against_cut_response(
    head: &'static str,
    body: &'static str,
) -> (Result<()>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_request(&mut stream).await;
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(body.as_bytes()).await.unwrap();
        stream.flush().await.unwrap();
    });
    let (mut input_writer, input_reader) = tokio::io::duplex(4096);
    let (output_writer, mut output_reader) = tokio::io::duplex(4096);
    input_writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\"}\n")
        .await
        .unwrap();
    drop(input_writer);
    let result = bridge_http(
        HttpBridgeConfig {
            url: Url::parse(&format!("http://{address}/mcp")).unwrap(),
            headers: HashMap::new(),
        },
        input_reader,
        output_writer,
    )
    .await;
    server.await.unwrap();
    let mut output = String::new();
    output_reader.read_to_string(&mut output).await.unwrap();
    (result, output)
}

#[tokio::test]
async fn a_body_cut_in_transport_answers_the_request() {
    let (result, output) = bridge_against_cut_response(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n",
        r#"{"jsonrpc":"2.0","#,
    )
    .await;
    result.unwrap();
    assert!(output.contains(r#""id":1"#), "{output}");
    assert!(output.contains("MCP HTTP transport failed"), "{output}");
}

#[tokio::test]
async fn an_event_stream_cut_before_its_answer_answers_the_request() {
    let (result, output) = bridge_against_cut_response(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n",
        "3b\r\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\n\r\n",
    )
    .await;
    result.unwrap();
    assert!(output.contains("notifications/progress"), "{output}");
    assert!(output.contains(r#""id":1"#), "{output}");
    assert!(output.contains("MCP HTTP transport failed"), "{output}");
}

/// Runs the bridge on one request line against `url`, with one header.
async fn bridge_once(url: &str) -> (Result<()>, String) {
    let (mut input_writer, input_reader) = tokio::io::duplex(4096);
    let (output_writer, mut output_reader) = tokio::io::duplex(4096);
    input_writer
        .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\"}\n")
        .await
        .unwrap();
    drop(input_writer);
    let result = bridge_http(
        HttpBridgeConfig {
            url: Url::parse(url).unwrap(),
            headers: HashMap::from([("X-Token".to_string(), "tok-secret-value".to_string())]),
        },
        input_reader,
        output_writer,
    )
    .await;
    let mut output = String::new();
    output_reader.read_to_string(&mut output).await.unwrap();
    (result, output)
}

/// Answers one request with `response`, and returns the request it read.
fn answer_once(listener: TcpListener, response: String) -> tokio::task::JoinHandle<String> {
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_request(&mut stream).await;
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    })
}

#[tokio::test]
async fn bridge_keeps_the_server_numbers_as_written() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let body = "{\"jsonrpc\":\"2.0\",\"id\":1,\r\n\"result\":{\"big\":123456789012345678901234567890,\"e\":1e2,\"f\":0.10000000000000000001}}\n";
    let server = answer_once(
        listener,
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let (result, output) = bridge_once(&format!("http://{address}/mcp")).await;
    result.unwrap();
    server.await.unwrap();
    assert_eq!(
        output,
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"big\":123456789012345678901234567890,\"e\":1e2,\"f\":0.10000000000000000001}}\n"
    );
}

#[tokio::test]
async fn refuses_credentials_and_fragment() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    for url in [
        format!("http://u:p@{address}/mcp"),
        format!("http://u@{address}/mcp"),
        format!("http://{address}/mcp#x"),
    ] {
        let (result, output) = bridge_once(&url).await;
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("credentials or a fragment"),
            "{url}: {error}"
        );
        assert!(output.is_empty(), "{url}: {output}");
    }
    let accepted =
        tokio::time::timeout(std::time::Duration::from_millis(200), listener.accept()).await;
    assert!(accepted.is_err(), "a refused URL reached the server");
}

#[tokio::test]
async fn does_not_follow_a_redirect() {
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target_address = target.local_addr().unwrap();
    let origin = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin = answer_once(
        origin,
        format!(
            "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{target_address}/mcp\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        ),
    );
    let (result, _) = bridge_once(&format!("http://{origin_address}/mcp")).await;
    assert!(result.unwrap_err().to_string().contains("307"));
    assert!(
        origin
            .await
            .unwrap()
            .to_ascii_lowercase()
            .contains("x-token: tok-secret-value")
    );
    let followed =
        tokio::time::timeout(std::time::Duration::from_millis(200), target.accept()).await;
    assert!(followed.is_err(), "the redirect target got a request");
}

#[tokio::test]
async fn bridge_passes_messages_unchanged() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let body = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"token is tok-secret-value"}]}}"#;
    let server = answer_once(
        listener,
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    let (result, output) = bridge_once(&format!("http://{address}/mcp")).await;
    result.unwrap();
    server.await.unwrap();
    assert!(output.contains("token is tok-secret-value"), "{output}");
    let sent: serde_json::Value = serde_json::from_str(output.trim_end()).unwrap();
    assert_eq!(
        sent,
        serde_json::from_str::<serde_json::Value>(body).unwrap()
    );
}
