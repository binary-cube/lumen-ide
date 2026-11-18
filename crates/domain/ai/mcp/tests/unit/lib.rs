use super::*;

#[derive(Debug)]
struct Echo;

impl McpTool for Echo {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn description(&self) -> &'static str {
        "Says it back."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "properties": {"text": {"type": "string"}}})
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Read
    }
    fn call(&self, args: &Value, _host: &mut dyn workbench_model::WorkbenchHost, reply: Reply) {
        reply.send(match text_arg(args, "text") {
            Ok(text) => ToolOutcome::Text(text.to_owned()),
            Err(why) => why,
        });
    }
}

const INFO: ServerInfo<'static> = ServerInfo { name: "editor", version: "1" };

fn ask(message: Value) -> Option<Value> {
    let tools: Vec<Arc<dyn McpTool>> = vec![Arc::new(Echo)];
    let mut call = |name: &str, args: &Value| {
        (name == "echo").then(|| ToolOutcome::Text(format!("called with {args}")))
    };
    answer(&message, INFO, &tools, &mut call)
}

/// Introduced in the version asked when it's one it speaks, else its newest; listed tools
/// with their schemas; a call's text; notifications unanswered; the rest refused.
#[test]
fn the_protocol_s_requests_are_answered() {
    let init = ask(json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-03-26"}}))
    .unwrap();
    assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(init["result"]["serverInfo"]["name"], "editor");
    let unknown =
        ask(json!({"id": 2, "method": "initialize", "params": {"protocolVersion": "1999"}}));
    assert_eq!(unknown.unwrap()["result"]["protocolVersion"], VERSIONS[0]);
    let list = ask(json!({"id": 3, "method": "tools/list"})).unwrap();
    assert_eq!(list["result"]["tools"][0]["name"], "echo");
    assert_eq!(list["result"]["tools"][0]["inputSchema"]["type"], "object");
    let called = ask(
        json!({"id": 4, "method": "tools/call", "params": {"name": "echo", "arguments": {"a": 1}}}),
    );
    let called = called.unwrap();
    assert_eq!(called["result"]["content"][0]["text"], "called with {\"a\":1}");
    assert_eq!(called["result"]["isError"], false);
    let missing =
        ask(json!({"id": 5, "method": "tools/call", "params": {"name": "nope"}})).unwrap();
    assert_eq!(missing["error"]["code"], -32602);
    assert!(ask(json!({"method": "notifications/initialized"})).is_none());
    assert_eq!(ask(json!({"id": 6, "method": "resources/list"})).unwrap()["error"]["code"], -32601);
    assert!(ask(json!({"id": 7})).is_none(), "not a request");
}

/// A failure is the agent's to read, not an error of the protocol's.
#[test]
fn a_failed_call_is_said_as_its_result() {
    let failed = result(&ToolOutcome::Failed("no such file".into()));
    assert_eq!(failed["isError"], true);
    assert_eq!(failed["content"][0]["text"], "no such file");
    assert!(text_arg(&json!({"x": 1}), "path").is_err());
}

/// Registered tools are found by name; one with the same name replaces it.
#[test]
fn tools_are_found_by_name() {
    register(Arc::new(Echo));
    register(Arc::new(Echo));
    assert_eq!(tools().iter().filter(|t| t.name() == "echo").count(), 1);
    assert!(tool("echo").is_some() && tool("nope").is_none());
}

/// A reply answers once; one dropped unanswered says it was refused.
#[test]
fn a_reply_dropped_says_so() {
    let long = std::time::Duration::from_secs(5);
    let (reply, outcome) = Reply::channel();
    reply.send(ToolOutcome::Text("done".into()));
    assert_eq!(outcome.wait(long), Some(ToolOutcome::Text("done".into())));
    let (reply, outcome) = Reply::channel();
    drop(reply);
    assert!(matches!(outcome.wait(long), Some(ToolOutcome::Failed(_))));
}

/// A call waited for too long is given up: whoever holds its reply knows not to run it.
#[test]
fn a_call_waited_for_too_long_is_given_up() {
    let (reply, outcome) = Reply::channel();
    assert!(!reply.given_up());
    assert_eq!(outcome.wait(std::time::Duration::from_millis(1)), None);
    assert!(reply.given_up());
}
