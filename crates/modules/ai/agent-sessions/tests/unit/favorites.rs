use super::*;

/// The file read as hostile input: what isn't a favorite skipped, titles bounded.
#[test]
fn the_file_is_read_with_care() {
    let text = format!(
        r#"[{{"agent":"claude","id":"a","title":"One"}}, 7, {{"agent":"claude"}},
           {{"agent":"claude","id":"b","title":"{}"}}]"#,
        "x".repeat(500)
    );
    let list = parse(&text);
    assert_eq!(list.len(), 2);
    assert_eq!(list[0], Favorite { agent: "claude".into(), id: "a".into(), title: "One".into() });
    assert_eq!(list[1].title.len(), 200);
    assert!(parse("not json").is_empty());
    assert_eq!(parse(&written(&list)), list, "written as read");
}

/// Added and taken off; one whose session is gone marked missing and told once; the
/// missing ones taken off the list on asking.
#[test]
fn favorites_are_kept_and_missing_ones_told_once() {
    let project = kernel::ProjectId::new();
    assert!(toggle(project, "claude", "a", "One"));
    assert!(toggle(project, "claude", "b", "Two"));
    let (_, list, _) = list(project);
    assert_eq!(list.len(), 2);
    // "b" gone: told once.
    let told = seen(project, &[("claude", "a")]);
    assert_eq!(told.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["b"]);
    assert_eq!(seen(project, &[("claude", "a")]).len(), 1, "not told yet: asked again");
    super::told(project, &told);
    assert!(seen(project, &[("claude", "a")]).is_empty(), "told once");
    assert_eq!(super::list(project).2, [("claude".to_owned(), "b".to_owned())]);
    forget_missing(project);
    let (_, list, missing) = super::list(project);
    assert_eq!(list.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["a"]);
    assert!(missing.is_empty());
    assert!(!toggle(project, "claude", "a", "One"), "taken off");
}

/// Many changes in a row: the file ends with the latest list, whole (one write at a time,
/// through a temporary file); the project let go, read back from it.
#[test]
fn saves_end_with_the_latest_list() {
    let dir = std::env::temp_dir()
        .join(format!("favorites-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    let project = kernel::ProjectId::new();
    set_storage(project, &dir);
    with(project, |p| p.loaded = true);
    for i in 0..20 {
        toggle(project, "claude", &format!("s{i}"), "T");
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while with(project, |p| p.saving) && std::time::Instant::now() < deadline {
        std::thread::yield_now();
    }
    let file = dir.join("agent-sessions").join("favorites.json");
    let read = parse(&std::fs::read_to_string(&file).unwrap_or_default());
    assert_eq!(read.len(), 20);
    assert!(!file.with_extension("json.tmp").exists());
    release(project);
    set_storage(project, &dir);
    load(project);
    assert_eq!(list(project).1.len(), 20);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The marks file read as hostile input: unknown labels dropped, nothing-marked entries
/// skipped; written as read.
#[test]
fn marks_are_read_with_care_and_written_as_read() {
    let text = r#"[{"agent":"claude","id":"a","done":true,"pinned":true,"label":"green"},
        {"agent":"claude","id":"b","label":"plaid"}, 3, {"id":"c","unread":true},
        {"agent":"claude","id":"d","unread":true}]"#;
    let marks = parse_marks(text);
    assert_eq!(marks.len(), 2, "{marks:?}");
    assert!(marks[0].done && marks[0].pinned && marks[0].label == Some(Label::Green));
    assert!(marks[1].unread && marks[1].id == "d");
    assert_eq!(parse_marks(&written_marks(&marks)), marks);
    assert!(parse_marks("{").is_empty());
}

/// A mark set, changed and taken off: a session with nothing marked isn't kept.
#[test]
fn a_session_is_marked_and_unmarked() {
    let project = kernel::ProjectId::new();
    let before = generation(project);
    mark(project, "claude", "a", |m| m.label = Some(Label::Red));
    mark(project, "claude", "a", |m| m.done = true);
    let kept = marks(project);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].done && kept[0].label == Some(Label::Red));
    assert!(generation(project) > before, "the panel shows it again");
    let at = generation(project);
    mark(project, "claude", "a", |m| m.done = true);
    assert_eq!(generation(project), at, "unchanged: nothing done");
    mark(project, "claude", "a", |m| {
        m.done = false;
        m.label = None;
    });
    assert!(marks(project).is_empty(), "nothing marked: let go");
    for l in Label::ALL {
        assert_eq!(Label::from_key(l.key()), Some(l));
    }
}
