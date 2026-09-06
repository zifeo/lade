use std::collections::BTreeMap;

use serde_json::{Value, json};

pub(super) fn is_post_event(input: &Value) -> bool {
    matches!(hook_event(input), Some("afterMCPExecution" | "PostToolUse"))
}

pub(super) fn is_verb(input: &Value) -> bool {
    if hook_event(input) == Some("beforeMCPExecution") {
        return true;
    }
    if let Some(name) = tool_name(input)
        && (name.starts_with("MCP:") || name.starts_with("mcp__"))
    {
        return true;
    }
    is_opencode_mcp(input)
}

fn is_opencode_mcp(input: &Value) -> bool {
    input.get("hook_source").and_then(Value::as_str) == Some("opencode-plugin")
        && tool_name(input).is_some_and(|name| !name.eq_ignore_ascii_case("bash"))
}

pub(super) fn hook_event(input: &Value) -> Option<&str> {
    input.get("hook_event_name").and_then(Value::as_str)
}

pub(super) fn tool_name(input: &Value) -> Option<&str> {
    input
        .get("tool_name")
        .and_then(Value::as_str)
        .or_else(|| input.get("tool").and_then(Value::as_str))
        .filter(|name| !name.is_empty())
}

pub(super) fn tool_input(input: &Value) -> Option<&Value> {
    input.get("tool_input").or_else(|| input.get("args"))
}

pub(super) fn normalize(input: &Value) -> String {
    verb_prefix(
        tool_name(input).unwrap_or(""),
        input.get("mcp_server_name").and_then(Value::as_str),
    )
}

pub(super) fn verb_args(input: &Value) -> Option<Value> {
    let obj = tool_input(input)?.as_object()?;
    if obj.is_empty() {
        return None;
    }
    let sorted: BTreeMap<&str, &Value> = obj.iter().map(|(key, val)| (key.as_str(), val)).collect();
    Some(json!(sorted))
}

fn verb_prefix(tool_name: &str, mcp_server: Option<&str>) -> String {
    if let Some(rest) = tool_name.strip_prefix("mcp__") {
        let mut parts = rest.split("__").filter(|part| !part.is_empty());
        let Some(first) = parts.next() else {
            return String::new();
        };
        let rest: Vec<&str> = parts.collect();
        if rest.is_empty() {
            return first.to_string();
        }
        return format!("{first}.{}", rest.join("."));
    }
    let tool = tool_name.strip_prefix("MCP:").unwrap_or(tool_name);
    match mcp_server.filter(|server| !server.is_empty()) {
        Some(server) if !tool.is_empty() => format!("{server}.{tool}"),
        _ => tool.to_string(),
    }
}

pub(super) fn verb_agent(mut base: Value, input: &Value) -> Value {
    let obj = match base.as_object_mut() {
        Some(obj) => obj,
        None => {
            base = json!({});
            base.as_object_mut().expect("object")
        }
    };
    if let Some((server, tool)) = verb_parts(input) {
        if let Some(tool) = tool {
            obj.insert("tool".to_string(), json!(tool));
        }
        if let Some(server) = server {
            obj.insert("mcp_server".to_string(), json!(server));
        }
    }
    if let Some(event) = hook_event(input) {
        obj.insert("hook".to_string(), json!(event));
    }
    if let Some(id) = input.get("tool_use_id").and_then(Value::as_str)
        && !id.is_empty()
    {
        obj.insert("tool_use_id".to_string(), json!(id));
    }
    if hook_event(input) == Some("beforeMCPExecution")
        && let Some(launch) = launch_from(input)
    {
        obj.insert("launch".to_string(), json!(launch));
    }
    if obj.is_empty() { Value::Null } else { base }
}

fn verb_parts(input: &Value) -> Option<(Option<String>, Option<String>)> {
    let prefix = verb_prefix(
        tool_name(input).unwrap_or(""),
        input.get("mcp_server_name").and_then(Value::as_str),
    );
    if prefix.is_empty() {
        return None;
    }
    match prefix.split_once('.') {
        Some((server, tool)) => Some((Some(server.to_string()), Some(tool.to_string()))),
        None => Some((None, Some(prefix))),
    }
}

fn launch_from(input: &Value) -> Option<String> {
    input
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| input.get("url").and_then(Value::as_str))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_prefixes_and_before_mcp() {
        assert!(is_verb(&json!({"tool_name":"MCP:mem_stats"})));
        assert!(is_verb(&json!({"tool_name":"mcp__engram__mem_stats"})));
        assert!(is_verb(
            &json!({"hook_event_name":"beforeMCPExecution","command":"engram"})
        ));
        assert!(!is_verb(
            &json!({"tool_name":"Shell","tool_input":{"command":"echo"}})
        ));
        assert!(is_post_event(&json!({"hook_event_name":"PostToolUse"})));
        assert!(is_post_event(
            &json!({"hook_event_name":"afterMCPExecution"})
        ));
    }

    #[test]
    fn normalize_table() {
        assert_eq!(
            normalize(&json!({
                "tool_name":"MCP:mem_stats",
                "mcp_server_name":"engram"
            })),
            "engram.mem_stats"
        );
        assert_eq!(
            normalize(&json!({
                "tool_name":"mem_stats",
                "mcp_server_name":"engram"
            })),
            "engram.mem_stats"
        );
        assert_eq!(
            normalize(&json!({"tool_name":"MCP:mem_stats"})),
            "mem_stats"
        );
        assert_eq!(
            normalize(&json!({"tool_name":"mcp__engram__mem_stats"})),
            "engram.mem_stats"
        );
        assert_eq!(
            normalize(&json!({"tool_name":"mcp__engram__foo__bar"})),
            "engram.foo.bar"
        );
        assert_eq!(
            normalize(&json!({"tool_name":"mcp__mem_stats"})),
            "mem_stats"
        );
        assert_eq!(
            normalize(&json!({
                "tool_name":"MCP:mem_stats",
                "mcp_server_name":"engram",
                "tool_input":{"project":"lade","z":1}
            })),
            "engram.mem_stats"
        );
        assert_eq!(
            verb_args(&json!({
                "tool_name":"MCP:mem_stats",
                "mcp_server_name":"engram",
                "tool_input":{"project":"lade","z":1}
            })),
            Some(json!({"project":"lade","z":1}))
        );
        assert_eq!(
            normalize(&json!({
                "tool_name":"MCP:mem_stats",
                "mcp_server_name":"engram",
                "tool_input":{}
            })),
            "engram.mem_stats"
        );
        assert_eq!(
            verb_args(&json!({
                "tool_name":"MCP:mem_stats",
                "mcp_server_name":"engram",
                "tool_input":{}
            })),
            None
        );
    }

    #[test]
    fn opencode_single_token_is_a_verb() {
        let input = json!({
            "hook_source":"opencode-plugin",
            "tool":"mem_stats",
            "args":{"project":"lade"}
        });
        assert!(is_verb(&input));
        assert_eq!(normalize(&input), "mem_stats");
        assert_eq!(verb_args(&input), Some(json!({"project":"lade"})));
    }
}
