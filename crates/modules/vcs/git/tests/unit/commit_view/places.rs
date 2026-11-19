use super::*;

fn r(name: &str, kind: RefKind) -> RefLabel {
    RefLabel { name: name.into(), kind }
}

fn places() -> Places {
    Places {
        refs: vec![
            r("feature/chat", RefKind::Branch),
            r("feature/embeddings", RefKind::Branch),
            r("main", RefKind::Branch),
            r("origin/main", RefKind::Remote),
            r("v1.0", RefKind::Tag),
        ],
        stashes: vec![Stash {
            name: "stash@{0}".into(),
            message: "On main: wip".into(),
            id: "1".into(),
        }],
        submodules: vec!["libs/x".into()],
        states: vec![SyncState { branch: "main".into(), upstream: None, ahead: 1, behind: 0 }],
    }
}

fn shown(rows: &[PlaceRow]) -> Vec<String> {
    rows.iter()
        .map(|row| {
            let what = match &row.place {
                Place::Section { key, .. } => format!("[{key}]"),
                Place::Work(work) => format!("{work:?}"),
                Place::Folder { label, .. } => format!("{label}/"),
                Place::Ref { label, current: true, ahead, .. } => format!("{label}* {ahead}↑"),
                Place::Ref { label, .. } => label.clone(),
                Place::Stash(s) => s.message.clone(),
                Place::Submodule(path) => path.clone(),
            };
            format!("{}{what}", "  ".repeat(row.depth))
        })
        .collect()
}

/// Every section in its order, the branches in folders (the current one marked with what it
/// has to push), the remotes' branches by remote; a section folded shows only its head.
#[test]
fn the_places_are_the_repository_s_by_section() {
    assert_eq!(
        shown(&rows(&places(), Some("main"), &HashSet::new(), "")),
        [
            "[workspace]",
            "  FileStatus",
            "  History",
            "  Search",
            "[branches]",
            "  feature/",
            "    chat",
            "    embeddings",
            "  main* 1↑",
            "[tags]",
            "  v1.0",
            "[remotes]",
            "  origin/",
            "    main",
            "[stashes]",
            "  On main: wip",
            "[submodules]",
            "  libs/x",
        ]
    );
    let folded: HashSet<String> = ["branches".to_owned(), "remote/origin".to_owned()].into();
    let rows = shown(&rows(&places(), Some("main"), &folded, ""));
    assert!(rows.contains(&"[branches]".to_owned()) && !rows.iter().any(|r| r.contains("chat")));
    assert!(rows.contains(&"  origin/".to_owned()) && !rows.contains(&"    main".to_owned()));
}

/// Filtered, only what contains the text shows (regardless of case), its folders open even
/// when folded; the workspace and the sections finding nothing are left out.
#[test]
fn a_filter_keeps_only_what_contains_it() {
    let folded: HashSet<String> = ["branches".to_owned(), "feature".to_owned()].into();
    assert_eq!(
        shown(&rows(&places(), Some("main"), &folded, "EMB")),
        ["[branches]", "  feature/", "    embeddings"]
    );
    assert_eq!(
        shown(&rows(&places(), Some("main"), &folded, "main")),
        [
            "[branches]",
            "  main* 1↑",
            "[remotes]",
            "  origin/",
            "    main",
            "[stashes]",
            "  On main: wip"
        ]
    );
    assert_eq!(
        shown(&rows(&places(), Some("main"), &folded, "libs")),
        ["[submodules]", "  libs/x"]
    );
    assert!(rows(&places(), None, &folded, "nothing like it").is_empty());
}
