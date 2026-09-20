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
