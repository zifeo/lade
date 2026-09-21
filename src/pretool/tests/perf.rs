use super::{test_config, with_cursor_env, with_ticket_tmpdir};
use crate::config::Audience;
use crate::pretool::handle;
use std::time::Instant;

#[test]
fn handle_typical_cursor_payload_stays_under_half_a_second() {
    with_cursor_env(|| {
        with_ticket_tmpdir(|| {
            let (config, _dir) = test_config("^echo");
            let input = r#"{"tool_name":"Shell","tool_input":{"command":"echo hello"},"hook_event_name":"preToolUse"}"#;
            let started = Instant::now();
            for _ in 0..20 {
                let result = handle(&config, input, Audience::Agent, None).unwrap();
                assert!(result.contains("updated_input"));
            }
            let elapsed = started.elapsed();
            assert!(
                elapsed.as_millis() < 500,
                "20 pretool handle calls took {elapsed:?}"
            );
        });
    });
}
