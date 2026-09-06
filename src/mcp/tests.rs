use std::{collections::HashMap, ffi::OsString, time::Instant};

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::{Duration, timeout};

use super::stdio::{message_id, message_method, supervise_stdio};
use super::*;

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
