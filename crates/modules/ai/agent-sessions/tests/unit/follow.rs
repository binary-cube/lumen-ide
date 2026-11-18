use super::*;
use std::path::Path;

fn session(id: &str, title: &str, started: i64) -> agents::AgentSession {
    agents::AgentSession {
        agent: "fake",
        id: id.into(),
        title: title.into(),
        folder: PathBuf::from("/p"),
        started: Some(started),
        active: started,
        file: None,
    }
}

fn followed(key: &str, id: Option<&str>, since: i64, title: &str) -> Followed {
    Followed {
        key: key.into(),
        agent: "fake",
        id: id.map(str::to_owned),
        folder: PathBuf::from("/p"),
        since,
        title: title.into(),
        said: id.map(|_| title.to_owned()),
    }
}

/// A session renamed in the agent: its tab renamed; nothing else changed: nothing asked.
#[test]
fn a_renamed_session_renames_its_tab() {
    let mut list = vec![followed("k1", Some("a"), 0, "old name")];
    let read = |_: &'static str, _: &Path| vec![session("a", "test-new-tab", 0)];
    assert_eq!(update(&mut list, &read, false), [("k1".to_owned(), "test-new-tab".to_owned())]);
    assert_eq!(list[0].title, "test-new-tab");
    assert!(update(&mut list, &read, false).is_empty(), "already so");
    assert_eq!(update(&mut list, &read, true).len(), 1, "named again on a check");
}

/// A new session takes the id of the session started since it was (not another tab's), its
/// title then.
#[test]
fn a_new_session_takes_the_id_that_appears_for_it() {
    let mut list = vec![
        followed("k-old", Some("old"), 0, "Old"),
        followed("k-new", None, 1_000, "Claude Code"),
    ];
    let read = |_: &'static str, _: &Path| {
        vec![session("newer", "Fix login", 1_002), session("old", "Old", 0)]
    };
    let renames = update(&mut list, &read, false);
    assert_eq!(list[1].id.as_deref(), Some("newer"));
    assert_eq!(renames, [("k-new".to_owned(), "Fix login".to_owned())]);
    // Nothing started since: still waiting.
    let mut waiting = vec![followed("k", None, 5_000, "Claude Code")];
    assert!(update(&mut waiting, &read, false).is_empty());
    assert_eq!(waiting[0].id, None);
}

/// A tab the user renamed keeps its name until the agent gives the session another title
/// (then that one).
#[test]
fn a_tab_renamed_by_the_user_is_not_named_back() {
    let mut list = vec![followed("k", Some("a"), 0, "Old")];
    let old = |_: &'static str, _: &Path| vec![session("a", "Old", 0)];
    list[0].title = "Mine".into();
    assert!(update(&mut list, &old, false).is_empty(), "the agent still says the old title");
    assert_eq!(list[0].title, "Mine");
    let new = |_: &'static str, _: &Path| vec![session("a", "Theirs", 0)];
    assert_eq!(update(&mut list, &new, false), [("k".to_owned(), "Theirs".to_owned())]);
}

/// Two new tabs whose sessions appear in the same read: the one started first takes the
/// session started first; each folder is listed once.
#[test]
fn new_tabs_take_their_sessions_in_order() {
    let mut list = vec![
        followed("k1", None, 1_000, "Claude Code"),
        followed("k2", None, 1_005, "Claude Code"),
    ];
    let reads = std::cell::Cell::new(0);
    let read = |_: &'static str, _: &Path| {
        reads.set(reads.get() + 1);
        vec![session("second", "Two", 1_008), session("first", "One", 1_001)]
    };
    update(&mut list, &read, false);
    assert_eq!(list[0].id.as_deref(), Some("first"));
    assert_eq!(list[1].id.as_deref(), Some("second"));
    assert_eq!(reads.get(), 1);
}

/// A tab renamed by the user: its session's name waits while its agent runs, and is due a
/// moment after it ended (its id as known by then); one never given an id is let go.
#[test]
fn a_rename_waits_for_its_agent_to_end() {
    let key = format!("pending-{}", kernel::id::uuid_text(kernel::id::new_uuid()));
    follow(key.clone(), "fake", Some("s-1".into()), PathBuf::from("/p"), "Old".into());
    assert_eq!(renamed(&key, "New"), Some("fake"));
    let mine = |due: Vec<(&'static str, PathBuf, String, String)>| {
        due.into_iter().filter(|(_, _, id, _)| id == "s-1").collect::<Vec<_>>()
    };
    assert!(mine(due(&mut |k| k == key)).is_empty(), "it runs: waits");
    assert!(mine(due(&mut |_| false)).is_empty(), "just ended: a moment more");
    lock(&PENDING).iter_mut().filter(|p| p.key == key).for_each(|p| {
        p.ended = Some(std::time::Instant::now() - GRACE);
    });
    let written = mine(due(&mut |_| false));
    assert_eq!(written, [("fake", PathBuf::from("/p"), "s-1".to_owned(), "New".to_owned())]);
    assert!(!lock(&PENDING).iter().any(|p| p.key == key), "written once");
    forget(&key);
}
