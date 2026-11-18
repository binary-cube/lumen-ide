use super::*;

/// The agent's lines read as answers, requests and notifications; what isn't one, nothing.
#[test]
fn the_agent_s_lines_are_read() {
    assert_eq!(
        read(r#"{"jsonrpc":"2.0","id":3,"result":{"sessionId":"s"}}"#),
        Some(Inbound::Response { id: 3, result: Ok(json!({"sessionId": "s"})) })
    );
    assert_eq!(
        read(r#"{"jsonrpc":"2.0","id":4,"error":{"code":-32000,"message":"not logged in"}}"#),
        Some(Inbound::Response { id: 4, result: Err("not logged in".into()) })
    );
    let asked = read(
        r#"{"jsonrpc":"2.0","id":"p1","method":"session/request_permission","params":{"a":1}}"#,
    );
    assert_eq!(
        asked,
        Some(Inbound::Request {
            id: json!("p1"),
            method: "session/request_permission".into(),
            params: json!({"a": 1})
        })
    );
    let told = read(r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s"}}"#);
    assert!(
        matches!(told, Some(Inbound::Notification { method, .. }) if method == "session/update")
    );
    assert_eq!(read("not json"), None);
    assert_eq!(read("{}"), None);
}

/// What the editor sends: one line each, the shapes the protocol asks.
#[test]
fn the_editor_s_messages_are_written() {
    let line = request(1, "session/prompt", prompt("s", "hi"));
    assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
    let sent: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(sent["params"]["prompt"][0], json!({"type": "text", "text": "hi"}));
    assert_eq!(initialize("editor", "1")["protocolVersion"], json!(VERSION));
    assert!(loads_sessions(&json!({"agentCapabilities": {"loadSession": true}})));
    assert!(!loads_sessions(&json!({})));
    assert_eq!(load_session("abc", "/p")["sessionId"], json!("abc"));
    let allowed: Value =
        serde_json::from_str(&response(&json!(7), permission(Some("allow-once")))).unwrap();
    assert_eq!(
        allowed["result"]["outcome"],
        json!({"outcome": "selected", "optionId": "allow-once"})
    );
    assert_eq!(permission(None)["outcome"]["outcome"], json!("cancelled"));
    let refused: Value = serde_json::from_str(&error(&json!(2), -32601, "no")).unwrap();
    assert_eq!(refused["error"]["code"], json!(-32601));
}
