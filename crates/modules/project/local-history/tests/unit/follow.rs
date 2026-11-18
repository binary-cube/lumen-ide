use super::*;

/// The active file is asked only while the fixed tab is seen; a change counted once.
#[test]
fn the_active_file_is_followed_only_while_seen() {
    let project = kernel::ProjectId::new();
    let asked = std::cell::Cell::new(0);
    let active = || {
        asked.set(asked.get() + 1);
        Some(PathBuf::from("/p/a.php"))
    };
    assert!(!poll(project, active), "never drawn: not asked");
    assert_eq!(asked.get(), 0);
    seen(project);
    assert!(poll(project, active));
    assert_eq!(file(project), (1, Some(PathBuf::from("/p/a.php"))));
    assert!(!poll(project, active), "the same file: no change");
    with(project, |w| w.seen = Some(Instant::now() - SEEN_FOR));
    let before = asked.get();
    assert!(!poll(project, active), "hidden a while: not asked");
    assert_eq!(asked.get(), before);
    forget(project);
    assert_eq!(file(project), (0, None));
}

/// A tab that isn't a file in front (a version opened to compare) keeps the file followed:
/// its history stays shown; another file takes its place.
#[test]
fn a_view_in_front_keeps_the_file_followed() {
    let project = kernel::ProjectId::new();
    seen(project);
    assert!(poll(project, || Some(PathBuf::from("/p/.env"))));
    assert!(!poll(project, || None), "a comparison's tab in front: nothing changes");
    assert_eq!(file(project), (1, Some(PathBuf::from("/p/.env"))));
    assert!(poll(project, || Some(PathBuf::from("/p/b.php"))));
    assert_eq!(file(project).1, Some(PathBuf::from("/p/b.php")));
    forget(project);
}
