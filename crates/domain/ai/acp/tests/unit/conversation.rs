use super::*;
use serde_json::json;

fn text(kind: &str, text: &str) -> Value {
    json!({"sessionUpdate": kind, "content": {"type": "text", "text": text}})
}

/// Chunks join their message while it's the latest; another author starts another.
#[test]
fn chunks_make_messages() {
    let mut c = Conversation::default();
    c.typed("fix it");
    assert!(c.running);
    assert!(c.update(&text("agent_thought_chunk", "Looking")));
    c.update(&text("agent_message_chunk", "Done "));
    c.update(&text("agent_message_chunk", "now."));
    assert_eq!(
        c.items,
        [
            Item::Message { author: Author::User, text: "fix it".into() },
            Item::Message { author: Author::Thought, text: "Looking".into() },
            Item::Message { author: Author::Agent, text: "Done now.".into() },
        ]
    );
    let before = c.version();
    assert!(!c.update(&json!({"sessionUpdate": "available_commands_update"})), "not shown");
    assert_eq!(c.version(), before);
    c.ended();
    assert!(!c.running);
}

/// A tool is updated in place: its status, its output, its changes as diffs, its files.
#[test]
fn a_tool_is_updated_in_place() {
    let mut c = Conversation::default();
    c.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit a.php",
        "kind": "edit", "status": "pending", "locations": [{"path": "/p/a.php", "line": 3}]}));
    c.update(&text("agent_message_chunk", "between"));
    c.update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed",
        "content": [
            {"type": "content", "content": {"type": "text", "text": "ok"}},
            {"type": "diff", "path": "/p/a.php", "oldText": "a", "newText": "b"},
            {"type": "diff", "path": "/p/new.php", "oldText": null, "newText": "n"}
        ]}),
    );
    let Item::Tool(tool) = &c.items[0] else { panic!("{:?}", c.items) };
    assert_eq!((tool.title.as_str(), tool.kind.as_str()), ("Edit a.php", "edit"));
    assert_eq!(tool.status, ToolStatus::Done);
    assert_eq!(tool.output, "ok");
    assert_eq!(tool.paths, ["/p/a.php"]);
    assert_eq!(
        tool.diffs[0],
        Diff {
            path: "/p/a.php".into(),
            old: Some("a".into()),
            new: "b".into(),
            added: 1,
            removed: 1
        }
    );
    assert_eq!(tool.diffs[1].old, None, "a new file");
    assert_eq!(c.items.len(), 2, "updated, not added");
}

/// One plan, kept current where it first showed.
#[test]
fn the_plan_is_kept_current() {
    let mut c = Conversation::default();
    let plan = |status: &str| {
        json!({"sessionUpdate": "plan", "entries": [
        {"content": "Read", "priority": "high", "status": "completed"},
        {"content": "Write", "priority": "high", "status": status}]})
    };
    c.update(&plan("pending"));
    c.update(&text("agent_message_chunk", "x"));
    c.update(&plan("in_progress"));
    assert_eq!(c.items.len(), 2);
    let Item::Plan(steps) = &c.items[0] else { panic!() };
    assert!(steps[0].done && steps[1].running && !steps[1].done);
}

/// A permission waits with its choices (its tool's title), answered with its request's id;
/// a turn's end lets it go.
#[test]
fn a_permission_waits_for_an_answer() {
    let mut c = Conversation::default();
    c.ask(
        json!(9),
        &json!({"sessionId": "s",
        "toolCall": {"toolCallId": "t2", "title": "Run phpunit", "kind": "execute"},
        "options": [
            {"optionId": "a", "name": "Allow once", "kind": "allow_once"},
            {"optionId": "r", "name": "Reject", "kind": "reject_once"}]}),
    );
    let p = c.permission.clone().unwrap();
    assert_eq!(p.title, "Run phpunit");
    assert_eq!(p.tool.as_deref(), Some("t2"));
    assert_eq!(
        p.choices.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
        ["allow_once", "reject_once"]
    );
    assert_eq!(c.answered(), Some(json!(9)));
    assert!(c.permission.is_none() && c.answered().is_none());
    c.ask(json!(10), &json!({"options": []}));
    c.ended();
    assert!(c.permission.is_none());
}

/// Each item keeps the version that last changed it: a chunk stamps only its message, a
/// tool's update only the tool, the plan only the plan, a permission its tool.
#[test]
fn each_item_is_stamped_by_its_last_change() {
    let mut c = Conversation::default();
    c.typed("go");
    c.update(&json!({"sessionUpdate": "tool_call", "toolCallId": "t1", "title": "Edit"}));
    c.update(&text("agent_message_chunk", "a"));
    let (user, tool, said) = (c.revision(0), c.revision(1), c.revision(2));
    assert!(user > 0 && user < tool && tool < said);
    c.update(&text("agent_message_chunk", "b"));
    assert_eq!((c.revision(0), c.revision(1)), (user, tool), "the others kept");
    assert!(c.revision(2) > said);
    let said = c.revision(2);
    c.update(
        &json!({"sessionUpdate": "tool_call_update", "toolCallId": "t1", "status": "completed"}),
    );
    assert!(c.revision(1) > said && c.revision(1) == c.version());
    assert_eq!(c.revision(2), said);
    c.update(&json!({"sessionUpdate": "plan", "entries": [{"content": "x", "status": "pending"}]}));
    let plan = c.revision(3);
    c.update(&text("agent_message_chunk", "c"));
    c.update(
        &json!({"sessionUpdate": "plan", "entries": [{"content": "x", "status": "completed"}]}),
    );
    assert!(c.revision(3) > plan && c.items.len() == 5);
    let tool = c.revision(1);
    c.ask(json!(1), &json!({"toolCall": {"toolCallId": "t1", "title": "Edit a"}, "options": []}));
    assert!(c.revision(1) > tool);
    let before = c.revision(4);
    c.ended();
    c.answered();
    assert_eq!(c.revision(4), before, "nothing shown changed");
    assert_eq!(c.revision(99), 0, "no such item");
}

/// The counts as they were first made: each side's lines searched in the other's.
fn counted_by_search(old: Option<&str>, new: &str) -> (usize, usize) {
    let old: Vec<&str> = old.map(|o| o.lines().collect()).unwrap_or_default();
    let new: Vec<&str> = new.lines().collect();
    let added = new.iter().filter(|l| !old.contains(l)).count();
    let removed = old.iter().filter(|l| !new.contains(l)).count();
    (added, removed)
}

/// A diff's lines are counted as it arrives, the same numbers as a search of each side gave
/// (repeated lines, empty ones, a new file, an emptied one, `\r\n`).
#[test]
fn a_diff_s_lines_are_counted_as_before() {
    let cases: [(Option<&str>, &str); 9] = [
        (Some("a\nb\nc"), "a\nB\nc\nd"),
        (None, "x\ny"),
        (Some("x\ny"), ""),
        (Some(""), ""),
        (Some("a\na\nb\n\n"), "a\nc\nc\n\n\n"),
        (Some("one\r\ntwo\r\n"), "one\ntwo\nthree"),
        (Some("same\nsame"), "same"),
        (Some("é\nü\n"), "ü\né\nö"),
        (Some("a\nb\na\nb"), "b\na\nb\na\nz"),
    ];
    for (old, new) in cases {
        assert_eq!(counts(old, new), counted_by_search(old, new), "{old:?} → {new:?}");
        let diff = Diff::new("p".into(), old.map(str::to_owned), new.to_owned());
        assert_eq!((diff.added, diff.removed), counted_by_search(old, new));
    }
    let big = |seed: usize| -> String {
        (0..500).map(|l| format!("line {}\n", (l * 7 + seed) % 120)).collect()
    };
    assert_eq!(counts(Some(&big(0)), &big(3)), counted_by_search(Some(&big(0)), &big(3)));
}
