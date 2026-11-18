use super::*;
use serde_json::json;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

/// Bytes written into a channel (the client's writes, read by the fake agent).
struct Into(Sender<Vec<u8>>);

impl Write for Into {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Bytes read from a channel (the fake agent's lines); its end: the agent gone.
struct From(Receiver<Vec<u8>>, Vec<u8>);

impl Read for From {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(bytes) => self.1 = bytes,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.1.len());
        buf[..n].copy_from_slice(&self.1[..n]);
        self.1.drain(..n);
        Ok(n)
    }
}

/// An agent of the test: answers `initialize` (loading sessions: `loads`), opens and loads
/// sessions (a load replays one message), streams a reply to a prompt after asking a
/// permission, ends a turn as cancelled once cancelled; what it was sent, kept.
fn fake(loads: bool, resume: Option<&str>) -> (AgentProcess, Arc<Mutex<Vec<Value>>>) {
    let (to_agent, agent_reads) = channel::<Vec<u8>>();
    let (agent_writes, from_agent) = channel::<Vec<u8>>();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut pending = String::new();
        let say = |v: Value| {
            let _ = agent_writes.send(format!("{v}\n").into_bytes());
        };
        let mut prompt_id = None;
        while let Ok(bytes) = agent_reads.recv() {
            pending.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(end) = pending.find('\n') {
                let line: String = pending.drain(..=end).collect();
                let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
                kept.lock().unwrap().push(m.clone());
                let id = m.get("id").cloned();
                match m.get("method").and_then(Value::as_str) {
                    Some("initialize") => say(json!({"jsonrpc": "2.0", "id": id,
                        "result": {"protocolVersion": 1, "agentCapabilities": {"loadSession": loads}}})),
                    Some("session/new") => {
                        say(json!({"jsonrpc": "2.0", "id": id, "result": {"sessionId": "new-1"}}))
                    }
                    Some("session/load") => {
                        say(
                            json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "old",
                            "update": {"sessionUpdate": "user_message_chunk", "content": {"type": "text", "text": "before"}}}}),
                        );
                        say(json!({"jsonrpc": "2.0", "id": id, "result": {}}));
                    }
                    Some("session/prompt") => {
                        prompt_id = id;
                        say(
                            json!({"jsonrpc": "2.0", "id": "perm-1", "method": "session/request_permission",
                            "params": {"toolCall": {"toolCallId": "t"}, "options": [{"optionId": "ok", "name": "Allow", "kind": "allow_once"}]}}),
                        );
                    }
                    Some("session/cancel") => say(
                        json!({"jsonrpc": "2.0", "id": prompt_id.take(), "result": {"stopReason": "cancelled"}}),
                    ),
                    Some("fs/read_text_file") | Some(_) => {}
                    None if m.get("result").is_some() => {
                        // The permission answered: the reply, then the turn's end.
                        say(
                            json!({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": "new-1",
                            "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": "done"}}}}),
                        );
                        say(
                            json!({"jsonrpc": "2.0", "id": prompt_id.take(), "result": {"stopReason": "end_turn"}}),
                        );
                    }
                    None => {}
                }
            }
        }
    });
    let agent = AgentProcess::over(
        From(from_agent, Vec::new()),
        Into(to_agent),
        "/p",
        resume,
        Arc::new(|| {}),
    );
    (agent, seen)
}

/// Events until `done` says enough (or a few seconds).
fn until(agent: &AgentProcess, mut done: impl FnMut(&[Event]) -> bool) -> Vec<Event> {
    let mut all = Vec::new();
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end && !done(&all) {
        all.extend(agent.events());
        std::thread::sleep(Duration::from_millis(5));
    }
    all
}

/// Started: initialized, a new session opened; a prompt asks a permission, answered, the
/// reply streams and the turn ends.
#[test]
fn a_session_opens_and_a_turn_runs() {
    let (agent, seen) = fake(true, None);
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ready { .. })));
    assert!(events.contains(&Event::Ready { session: "new-1".into() }), "{events:?}");
    assert!(agent.prompt("hi"));
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ask { .. })));
    let Some(Event::Ask { id, .. }) = events.into_iter().find(|e| matches!(e, Event::Ask { .. }))
    else {
        panic!("no permission asked")
    };
    agent.answer(&id, Some("ok"));
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::TurnEnded(_))));
    assert!(events.iter().any(|e| matches!(e, Event::Update(u) if u["content"]["text"] == "done")));
    assert!(events.contains(&Event::TurnEnded("end_turn".into())));
    let methods: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .filter_map(|m| m.get("method").and_then(Value::as_str).map(str::to_owned))
        .collect();
    assert_eq!(methods, ["initialize", "session/new", "session/prompt"]);
}

/// Taken up again: `session/load` by its id, its history replayed before it's ready; an
/// agent that can't load opens a new one instead.
#[test]
fn a_session_is_taken_up_again_when_the_agent_can() {
    let (agent, seen) = fake(true, Some("old-7"));
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ready { .. })));
    let replayed = events.iter().position(|e| matches!(e, Event::Update(_))).unwrap();
    let ready = events.iter().position(|e| matches!(e, Event::Ready { .. })).unwrap();
    assert!(replayed < ready, "the history before it's ready");
    assert_eq!(agent.session().as_deref(), Some("old-7"));
    let loaded = seen.lock().unwrap().iter().find(|m| m["method"] == "session/load").cloned();
    assert_eq!(loaded.unwrap()["params"]["sessionId"], "old-7");
    let (agent, seen) = fake(false, Some("old-7"));
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ready { .. })));
    assert!(events.contains(&Event::Ready { session: "new-1".into() }), "a new one instead");
    assert!(seen.lock().unwrap().iter().any(|m| m["method"] == "session/new"));
}

/// Cancelled: the turn's answer says so; the agent gone: said too.
#[test]
fn a_turn_is_cancelled_and_an_ending_is_said() {
    let (agent, _) = fake(true, None);
    until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ready { .. })));
    agent.prompt("long");
    until(&agent, |e| e.iter().any(|e| matches!(e, Event::Ask { .. })));
    agent.cancel();
    let events = until(&agent, |e| e.iter().any(|e| matches!(e, Event::TurnEnded(_))));
    assert!(events.contains(&Event::TurnEnded("cancelled".into())));
}
