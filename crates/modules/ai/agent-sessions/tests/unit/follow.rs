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
        active: 0,
        state: None,
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

/// A new session taken up from the list before the follower saw its file: its new tab takes
/// it at once (no second tab); two new tabs that could be its: neither chosen here.
#[test]
fn a_session_resumed_early_finds_its_new_tab() {
    let folder = PathBuf::from(format!("/p-{}", kernel::id::new_uuid()));
    let new = |key: &str, since: i64| Followed {
        folder: folder.clone(),
        ..followed(key, None, since, "New")
    };
    let mut s = session("fresh", "Fresh", 2_000);
    s.folder = folder.clone();
    lock(&FOLLOWED).push(new("k-a", 1_999));
    assert_eq!(tab_of(&s).as_deref(), Some("k-a"));
    assert_eq!(key_of("fake", "fresh").as_deref(), Some("k-a"), "it took the id");
    // Two new tabs of the same agent, the same folder: not guessed.
    let mut other = session("second", "Second", 3_000);
    other.folder = folder.clone();
    lock(&FOLLOWED).push(new("k-b", 2_999));
    lock(&FOLLOWED).push(new("k-c", 2_999));
    assert_eq!(tab_of(&other), None);
    lock(&FOLLOWED).retain(|f| f.folder != folder);
}

/// An agent that resumes a session with `--resume <id>`.
#[derive(Debug)]
struct Resumer;

impl agents::AgentProvider for Resumer {
    fn id(&self) -> &'static str {
        "fake-resume"
    }
    fn name(&self) -> String {
        "Fake".into()
    }
    fn icon(&self) -> &'static str {
        "x"
    }
    fn detect(&self) -> Option<agents::AgentInstall> {
        None
    }
    fn sessions(&self, _: &Path, _: usize) -> Vec<agents::AgentSession> {
        Vec::new()
    }
    fn new_session(&self, root: &Path) -> agents::Launch {
        agents::Launch::new("agent", &[], Some(root))
    }
    fn resume(&self, session: &agents::AgentSession) -> agents::Launch {
        agents::Launch::new("agent", &["--resume", &session.id], Some(&session.folder))
    }
}

/// A followed session whose file has been still for the idle time resumes its agent's way;
/// one written lately (a turn may run), never read, without an id, or not followed: none.
#[test]
fn a_quiet_followed_session_says_how_it_resumes() {
    agents::register(std::sync::Arc::new(Resumer));
    let idle = Duration::from_secs(3_600);
    let key = format!("resume-{:x}", kernel::id::new_uuid());
    let mut f = followed(&key, Some("s-1"), 0, "Work");
    f.agent = "fake-resume";
    f.active = now() - 7_200;
    lock(&FOLLOWED).push(f);
    let launch = resume_launch(&key, idle).unwrap();
    assert_eq!(launch.args, ["--resume", "s-1"]);
    assert_eq!(launch.cwd.as_deref(), Some(Path::new("/p")));
    let set = |active: i64, id: Option<&str>| {
        let mut all = lock(&FOLLOWED);
        let f = all.iter_mut().find(|f| f.key == key).unwrap();
        f.active = active;
        f.id = id.map(str::to_owned);
    };
    set(now() - 60, Some("s-1"));
    assert_eq!(resume_launch(&key, idle), None, "written lately");
    set(0, Some("s-1"));
    assert_eq!(resume_launch(&key, idle), None, "never read");
    set(now() - 7_200, None);
    assert_eq!(resume_launch(&key, idle), None, "no id yet");
    assert_eq!(resume_launch("no-such-tab", idle), None);
    // What its terminal said, kept for its row.
    set(now() - 7_200, Some("s-1"));
    let said = contracts::sleep::SleepState {
        kind: "fake",
        memory: Some(42),
        running: false,
        asleep: true,
        can_sleep: contracts::sleep::CanSleep::No,
    };
    set_state(&key, Some(said));
    assert_eq!(state_of("fake-resume", "s-1"), Some(said));
    forget(&key);
    agents::unregister("fake-resume");
}

/// A session's "Put to Sleep" row: only with its tab followed and its terminal able to; greyed
/// out while something typed isn't sent.
#[test]
fn a_session_s_sleep_row_follows_its_terminal() {
    use contracts::sleep::{CanSleep, SleepState as TerminalState};
    let key = format!("sleep-row-{:x}", kernel::id::new_uuid());
    let id = format!("s-{:x}", kernel::id::new_uuid());
    assert!(crate::panel::sleep_row_for_tests("fake", &id).is_none(), "no tab");
    let mut f = followed(&key, Some(&id), 0, "Work");
    f.agent = "fake";
    lock(&FOLLOWED).push(f);
    assert!(crate::panel::sleep_row_for_tests("fake", &id).is_none(), "nothing said yet");
    let said = |can_sleep| TerminalState {
        kind: "fake",
        memory: None,
        running: true,
        asleep: false,
        can_sleep,
    };
    set_state(&key, Some(said(CanSleep::No)));
    assert!(crate::panel::sleep_row_for_tests("fake", &id).is_none(), "it can't");
    let typed = contracts::sleep::Veto {
        id: "typed",
        menu: "agents.sleep",
        refusal: "agents.sleep",
        by_hand: true,
        never: false,
    };
    set_state(&key, Some(said(CanSleep::NotNow(typed))));
    assert_eq!(crate::panel::sleep_row_for_tests("fake", &id), Some(true), "greyed out");
    set_state(&key, Some(said(CanSleep::Yes)));
    assert_eq!(crate::panel::sleep_row_for_tests("fake", &id), Some(false));
    // Asleep: "Wake Up" in its place.
    let asleep = TerminalState {
        kind: "fake",
        memory: None,
        running: false,
        asleep: true,
        can_sleep: CanSleep::No,
    };
    set_state(&key, Some(asleep));
    assert_eq!(crate::panel::wake_row_for_tests("fake", &id), Some(true));
    set_state(&key, Some(said(CanSleep::Yes)));
    assert_eq!(crate::panel::wake_row_for_tests("fake", &id), Some(false));
    // Asked to sleep: greyed out at once, asked again soon; asked to wake, likewise.
    expect(&key, CanSleep::Sleeping);
    assert_eq!(crate::panel::sleep_row_for_tests("fake", &id), Some(true), "going to sleep");
    // Its terminal tells it's asleep (its event): "Wake Up" at once, drawn again.
    let (project, other) = (kernel::ProjectId::new(), kernel::ProjectId::new());
    told(project, &key, Some(asleep));
    assert!(!take_told(Some(other)), "another project's panel isn't drawn for it");
    assert!(take_told(Some(project)) && !take_told(Some(project)));
    assert_eq!(crate::panel::wake_row_for_tests("fake", &id), Some(true));
    assert_eq!(crate::panel::sleep_row_for_tests("fake", &id), Some(false), "it can be woken");
    expect(&key, CanSleep::Waking);
    assert_eq!(crate::panel::wake_row_for_tests("fake", &id), Some(true), "still Wake Up");
    assert_eq!(crate::panel::sleep_row_for_tests("fake", &id), Some(true), "greyed out");
    // Its tab closed (told gone): no row left mid-way.
    told(project, &key, None);
    assert!(crate::panel::sleep_row_for_tests("fake", &id).is_none());
    forget(&key);
}

/// A session taken up again from the list (its tab followed anew) keeps what its terminal
/// told: asleep stays asleep on its row, never blank until the next ask.
#[test]
fn following_a_tab_again_keeps_its_state() {
    use contracts::sleep::{CanSleep, SleepState as TerminalState};
    let key = format!("again-{:x}", kernel::id::new_uuid());
    let id = format!("s-{:x}", kernel::id::new_uuid());
    follow(key.clone(), "fake", Some(id.clone()), PathBuf::from("/p"), "Work".into());
    let asleep = TerminalState {
        kind: "fake",
        memory: None,
        running: false,
        asleep: true,
        can_sleep: CanSleep::No,
    };
    told(kernel::ProjectId::new(), &key, Some(asleep));
    follow(key.clone(), "fake", Some(id.clone()), PathBuf::from("/p"), "Work".into());
    assert_eq!(state_of("fake", &id), Some(asleep));
    forget(&key);
}
