use super::*;

#[test]
fn round_trip() {
    let dir = std::env::temp_dir().join(format!("session-{}", std::process::id()));
    let session = Session {
        groups: vec![
            SavedGroup {
                tabs: vec![
                    SavedTab {
                        path: "src/a.php".into(),
                        pinned: true,
                        split: Some(0.625),
                        locked: true,
                    },
                    SavedTab { path: "b ă.php".into(), pinned: false, split: None, locked: false },
                ],
                active: 1,
            },
            SavedGroup {
                tabs: vec![SavedTab {
                    path: "c.php".into(),
                    pinned: false,
                    split: None,
                    locked: false,
                }],
                active: 0,
            },
        ],
        group: 1,
        split: "R(0.5000 L0,0.5000 L1)".into(),
        tool_windows: vec![SavedToolWindow {
            id: "terminal".into(),
            views: vec![
                SavedView {
                    title: "build".into(),
                    extension: "view-terminal".into(),
                    pinned: true,
                    group: 0,
                    key: Some("Local".into()),
                    memento: None,
                },
                SavedView {
                    title: "Local (2)".into(),
                    extension: "view-terminal".into(),
                    pinned: false,
                    group: 1,
                    key: None,
                    memento: Some("src/a.php".into()),
                },
            ],
            // Split in two: the second group current, each one's active view.
            active: 0,
            split: "R(0.4000 L0,0.6000 L1)".into(),
            group: 1,
            actives: vec![0, 0],
        }],
        bottom: Some("terminal".into()),
        zoom: 1.25,
        theme: Some("custom-a-mea".into()),
        // What the panels keep of themselves (the Services window's tree width).
        panels: vec![("services".into(), "tree=420".into()), ("structure".into(), "x".into())],
        // The sizes dragged (the docks', the views' splitters), sorted by key.
        sizes: vec![
            ("database.tables-list".into(), 312.0),
            ("dock.bottom".into(), 360.0),
            ("task-manager.detail-right".into(), 0.375),
        ],
    };
    let store = TomlSessionStore;
    assert!(store.save(&dir, &session).is_ok());
    let loaded = store.load(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(loaded, Some(session));
}

/// A session saved before the terminal was a tool window like any: its shells read as that
/// window's views (names, pins, shown).
#[test]
fn the_old_terminal_section_is_read_as_its_window() {
    let dir = std::env::temp_dir().join(format!("session-old-{}", std::process::id()));
    let file = file(&dir);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        &file,
        "group = 0\nsplit = \"L0\"\nzoom = 1.0\ngroups = []\n\n[terminal]\nactive = 1\nvisible = true\npinned = [false, true]\ntabs = [\"\", \"build\"]\n",
    )
    .unwrap();
    let loaded = TomlSessionStore.load(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(loaded.bottom.as_deref(), Some("terminal"));
    let w = &loaded.tool_windows[0];
    assert_eq!((w.id.as_str(), w.active), ("terminal", 1));
    let views: Vec<_> =
        w.views.iter().map(|v| (v.title.as_str(), v.extension.as_str(), v.pinned)).collect();
    assert_eq!(views, [("Local", "view-terminal", false), ("build", "view-terminal", true)]);
}

/// A session saved before the sizes were kept: none read (the defaults apply); an integer
/// size is read as a number.
#[test]
fn sizes_are_optional_and_read_from_integers() {
    let dir = std::env::temp_dir().join(format!("session-sizes-{}", std::process::id()));
    let file = file(&dir);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "group = 0\nsplit = \"L0\"\nzoom = 1.0\ngroups = []\n").unwrap();
    let old = TomlSessionStore.load(&dir).unwrap();
    std::fs::write(
        &file,
        "group = 0\nsplit = \"L0\"\nzoom = 1.0\ngroups = []\n\n[sizes]\n\"dock.left\" = 300\nbad = \"x\"\n",
    )
    .unwrap();
    let new = TomlSessionStore.load(&dir).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(old.sizes.is_empty());
    assert_eq!(new.sizes, [("dock.left".to_owned(), 300.0)]);
}
