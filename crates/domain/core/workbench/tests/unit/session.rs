use std::cell::RefCell;

use super::*;

/// Sessions kept in memory; counts the writes.
#[derive(Debug, Default)]
struct Memory(RefCell<Vec<Session>>);

impl SessionStore for Memory {
    fn load(&self, _: &Path) -> Option<Session> {
        self.0.borrow().last().cloned()
    }
    fn save(&self, _: &Path, session: &Session) -> std::io::Result<()> {
        self.0.borrow_mut().push(session.clone());
        Ok(())
    }
}

fn tab(path: &Path) -> Option<OpenTab<'_>> {
    Some(OpenTab { path, pinned: false, split: None, locked: false })
}

/// The files' tabs only, relative to the project (absolute outside it), the active one
/// counted among them.
#[test]
fn a_session_keeps_the_files_tabs_relative_to_the_project() {
    let (a, b, outside) = (Path::new("/p/src/a.php"), Path::new("/p/b.php"), Path::new("/x/c.txt"));
    let group = OpenGroup {
        tabs: vec![
            tab(a),
            None,
            Some(OpenTab { path: b, pinned: true, split: Some(0.3), locked: true }),
            tab(outside),
        ],
        active: 2,
    };
    let session = Session::capture(
        Path::new("/p"),
        Current { groups: vec![group], split: "L0".into(), zoom: 1.0, ..Current::default() },
    );
    let saved = &session.groups[0];
    let paths: Vec<&Path> = saved.tabs.iter().map(|t| t.path.as_path()).collect();
    assert_eq!(paths, [Path::new("src/a.php"), Path::new("b.php"), outside]);
    assert_eq!(saved.active, 1, "the view before it isn't saved");
    assert!(saved.tabs[1].pinned);
    assert_eq!(saved.tabs[1].split, Some(0.3), "its preview's split ratio");
    assert!(saved.tabs[1].locked && !saved.tabs[0].locked, "its lock");
}

/// A group left without a file: its layout can't come back — the tabs in one group.
#[test]
fn a_group_without_files_merges_the_tabs_into_one() {
    let a = Path::new("/p/a.php");
    let groups = vec![
        OpenGroup { tabs: vec![tab(a)], active: 0 },
        OpenGroup { tabs: vec![None], active: 0 },
    ];
    let session = Session::capture(
        Path::new("/p"),
        Current { groups, group: 1, split: "R(0.5000 L0,0.5000 L1)".into(), ..Current::default() },
    );
    assert_eq!(session.groups.len(), 1);
    assert_eq!((session.group, session.split.as_str()), (0, "L0"));
}

/// Reopened: the files still there, resolved from the project, the active one marked.
#[test]
fn the_files_still_there_are_reopened() {
    let session = Session {
        groups: vec![SavedGroup {
            tabs: vec![
                SavedTab { path: "a.php".into(), pinned: true, split: Some(0.7), locked: true },
                SavedTab { path: "gone.php".into(), pinned: false, split: None, locked: false },
                SavedTab { path: "/x/c.txt".into(), pinned: false, split: None, locked: false },
            ],
            active: 2,
        }],
        ..Session::default()
    };
    let exists = |p: &Path| !p.ends_with("gone.php");
    let reopen = session.reopen(Path::new("/p"), &exists);
    assert_eq!(
        reopen,
        [vec![
            Reopen {
                path: "/p/a.php".into(),
                pinned: true,
                active: false,
                split: Some(0.7),
                locked: true,
            },
            Reopen {
                path: "/x/c.txt".into(),
                pinned: false,
                active: true,
                split: None,
                locked: false,
            },
        ]]
    );
}

/// Saved only when it changed, at most every half second, never once frozen.
#[test]
#[allow(clippy::disallowed_methods)] // The test's own starting point in time.
fn the_session_is_saved_when_it_changes_until_frozen() {
    let store = Memory::default();
    let root = Path::new("/p");
    let mut keeper = SessionKeeper::default();
    let start = Instant::now();
    assert!(keeper.due(start));
    assert!(!keeper.due(start + Duration::from_millis(100)), "too soon");
    assert!(keeper.due(start + SessionKeeper::EVERY));
    let one = Session { zoom: 1.0, ..Session::default() };
    keeper.save_if_changed(&store, root, Some(one.clone())).unwrap();
    keeper.save_if_changed(&store, root, Some(one.clone())).unwrap();
    assert_eq!(store.0.borrow().len(), 1, "unchanged: not written again");
    keeper.freeze();
    keeper.save_if_changed(&store, root, Some(Session { zoom: 2.0, ..one })).unwrap();
    assert_eq!(store.0.borrow().len(), 1, "frozen");
    keeper.reset();
    keeper.save_if_changed(&store, root, Some(Session::default())).unwrap();
    assert_eq!(store.0.borrow().len(), 2, "another project: saved again");
}
