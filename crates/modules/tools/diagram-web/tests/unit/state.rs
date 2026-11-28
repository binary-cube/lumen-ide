use super::*;

fn message(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => panic!("an object"),
    }
}

fn told() -> Value {
    json!({"event": "hostState", "page": 1, "scale": 1.5, "tx": 776, "ty": 386, "left": 983,
           "top": 579, "selection": ["b", "c"], "busy": false})
}

/// What the page says of where the user is is read back as it was, and given back the same.
#[test]
fn where_the_user_is_is_read_and_given_back() {
    let view = ViewState::read(&message(told())).unwrap();
    assert_eq!(view.page, 1);
    assert_eq!((view.scale, view.translate, view.scroll), (1.5, (776.0, 386.0), (983.0, 579.0)));
    assert_eq!(view.selection, ["b", "c"]);
    assert!(!view.busy);
    let back = view.to_json();
    assert_eq!(back["page"], 1);
    assert_eq!(back["scale"], 1.5);
    assert_eq!(back["selection"], json!(["b", "c"]));
    assert!(back.get("busy").is_none(), "only where it was, never whether it was busy");
}

/// What the page says is another program's: out of range, not numbers, too many or too long,
/// it's refused or cut.
#[test]
fn what_the_page_says_is_checked() {
    let with = |key: &str, value: Value| {
        let mut m = message(told());
        m.insert(key.into(), value);
        ViewState::read(&m)
    };
    assert!(with("scale", json!(0)).is_none(), "no zoom of nothing");
    assert!(with("scale", json!(1e9)).is_none());
    assert!(with("tx", json!(1e12)).is_none(), "farther than anything");
    assert!(with("page", json!(-1)).is_none());
    assert!(with("page", json!(1_000_000)).is_none());
    assert!(with("left", json!("far")).is_none());
    let many: Vec<String> = (0..5_000).map(|i| format!("c{i}")).collect();
    assert_eq!(with("selection", json!(many)).unwrap().selection.len(), 1_000);
    let odd = json!(["ok", "", "x".repeat(100), 7]);
    assert_eq!(with("selection", odd).unwrap().selection, ["ok"], "empty, too long, not text");
    assert!(with("busy", json!(true)).unwrap().busy);
}

fn text(s: &str) -> Arc<str> {
    Arc::from(s)
}

/// Undo here takes back only what came before the page was made (draw.io's own history has
/// the changes since); redo goes forward again; a new change lets the redo go.
#[test]
fn undo_here_goes_on_where_the_page_s_own_history_ends() {
    let mut history = History::default();
    assert_eq!(history.can(), (false, false));
    history.changed(text("a"));
    history.changed(text("b"));
    assert_eq!(history.can(), (false, false), "the page's own history has them");
    history.page_made();
    assert_eq!(history.can(), (true, false));
    // Changes on the new page (its own), undone by it, then one step here.
    history.changed(text("c"));
    assert_eq!(history.undo(text("c")).as_deref(), Some("b"), "before the page was made");
    assert_eq!(history.can(), (true, true));
    assert_eq!(history.undo(text("b")).as_deref(), Some("a"));
    assert_eq!(history.can(), (false, true));
    assert_eq!(history.undo(text("a")), None, "nothing more");
    assert_eq!(history.redo(text("a")).as_deref(), Some("b"));
    assert_eq!(history.redo(text("b")).as_deref(), Some("c"));
    assert_eq!(history.redo(text("c")), None);
    history.undo(text("c"));
    history.changed(text("b2"));
    assert!(!history.can().1, "a new change: no redo");
}

/// What's kept is bounded: the oldest let go first, by count and by bytes.
#[test]
fn what_s_kept_to_undo_is_bounded() {
    let mut history = History::default();
    for i in 0..(MOST_PAST + 20) {
        history.changed(text(&format!("{i}")));
    }
    history.page_made();
    let mut steps = 0;
    let mut now = text("now");
    while let Some(back) = history.undo(Arc::clone(&now)) {
        now = back;
        steps += 1;
    }
    assert_eq!(steps, MOST_PAST);
    assert_eq!(&*now, "20", "the oldest let go");
    let mut history = History::default();
    let big = text(&"x".repeat(PAST_BYTES / 2 + 1));
    for _ in 0..3 {
        history.changed(Arc::clone(&big));
    }
    history.page_made();
    assert!(history.undo(text("now")).is_some());
    assert!(history.undo(text("now")).is_none(), "only what fits in its bytes");
}
