use super::*;

/// The file read as hostile input: names tidied to one line, what isn't a folder skipped,
/// an id seen twice kept once; written as read.
#[test]
fn folders_are_read_with_care() {
    let text = r#"[{"id":"a","name":"  Bugs\n","open":false}, 3, {"id":"b"},
        {"id":"a","name":"Again"}, {"id":"","name":"No id"}, {"id":"c","name":"Work"}]"#;
    let folders = parse(text);
    assert_eq!(folders.len(), 2, "{folders:?}");
    assert_eq!(folders[0], Folder { id: "a".into(), name: "Bugs".into(), open: false });
    assert!(folders[1].open, "open unless said");
    assert_eq!(parse(&written(&folders)), folders);
    assert!(parse("[").is_empty());
}

/// Made, renamed, closed; a session moved in and out; deleted with a session in it: the
/// session back in the list, its other marks kept.
#[test]
fn a_folder_holds_sessions_until_it_s_deleted() {
    let project = kernel::ProjectId::new();
    let id = create(project, "Bugs").unwrap();
    rename(project, &id, "Payment bugs");
    rename(project, &id, "   ");
    toggle_open(project, &id);
    let folders = list(project);
    assert_eq!(folders.len(), 1);
    assert_eq!((folders[0].name.as_str(), folders[0].open), ("Payment bugs", false));
    move_to(project, "claude", "s1", Some(&id));
    move_to(project, "claude", "s2", Some(&id));
    move_to(project, "claude", "s3", Some("not-a-folder"));
    crate::favorites::mark(project, "claude", "s2", |m| m.pinned = true);
    let in_it = |p| {
        crate::favorites::marks(p)
            .into_iter()
            .filter(|m| m.folder.as_deref() == Some(id.as_str()))
            .count()
    };
    assert_eq!(in_it(project), 2, "a folder that isn't there takes nothing");
    move_to(project, "claude", "s1", None);
    assert_eq!(in_it(project), 1);
    remove(project, &id);
    assert!(list(project).is_empty());
    let marks = crate::favorites::marks(project);
    assert_eq!(marks.len(), 1, "s2 keeps its pin; s1 had nothing left: {marks:?}");
    assert!(marks[0].pinned && marks[0].folder.is_none());
}
