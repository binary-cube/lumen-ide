use super::*;

/// A program quiet for a given time.
struct Quiet(Duration);

impl terminal_session::SessionWatch for Quiet {
    fn foreground(&self) -> terminal_session::history::Foreground {
        terminal_session::history::Foreground::Unknown
    }
    fn program(&self) -> Option<String> {
        None
    }
    fn title(&self) -> Option<String> {
        None
    }
    fn quiet_for(&self) -> Option<Duration> {
        Some(self.0)
    }
}

const IDLE: Duration = Duration::from_secs(3600);

/// A keyed terminal unseen and quiet for `unseen` and `quiet`.
fn state(unseen: Duration, quiet: Duration) -> (Asked, Instant) {
    let now = Instant::now() + IDLE * 3;
    let asked = Asked {
        shown: Some(now - unseen),
        watch: Some(Arc::new(Quiet(quiet))),
        pid: Some(4_000_001),
        ..Asked::default()
    };
    (asked, now)
}

/// Only a program unseen and quiet for the whole idle time sleeps; anything else keeps it
/// awake: shown or writing lately, text typed and not sent, ended, already asleep or about
/// to be, never shown, its quiet not known.
#[test]
fn only_an_unseen_quiet_program_sleeps() {
    let (asked, now) = state(IDLE, IDLE);
    assert!(sleepy(&asked, IDLE, now));
    let (asked, now) = state(IDLE / 2, IDLE);
    assert!(!sleepy(&asked, IDLE, now), "shown lately");
    let (asked, now) = state(IDLE, IDLE / 2);
    assert!(!sleepy(&asked, IDLE, now), "at work: it wrote lately");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.typed = true;
    assert!(!sleepy(&asked, IDLE, now), "something typed, not sent");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.running = false;
    assert!(!sleepy(&asked, IDLE, now), "its program ended");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.asleep = true;
    assert!(!sleepy(&asked, IDLE, now), "asleep already");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.sleep = Some((agents::Launch::default(), Instant::now()));
    assert!(!sleepy(&asked, IDLE, now), "about to sleep");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.shown = None;
    assert!(!sleepy(&asked, IDLE, now), "never shown: never started");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.watch = None;
    assert!(!sleepy(&asked, IDLE, now), "its quiet not known");
    let (mut asked, now) = state(IDLE * 2, IDLE * 2);
    asked.visible = true;
    assert!(!sleepy(&asked, IDLE, now), "on screen (no frame drawn for long: still seen)");
    let (mut asked, now) = state(IDLE, IDLE);
    asked.pid = None;
    assert!(!sleepy(&asked, IDLE, now), "no process");
}

/// Each project's window asks how its terminals resume on its own clock: one asking never
/// holds another back.
#[test]
fn each_project_asks_on_its_own_clock() {
    let (a, b) = (Some(kernel::ProjectId::new()), Some(kernel::ProjectId::new()));
    let mut asked = Vec::new();
    let now = Instant::now();
    let ask = |asked: &mut ResumesAsked, p, at, unknown: bool| {
        let due = resumes_due(resumes_asked_at(asked, p), at, || unknown);
        if due {
            note_resumes_asked(asked, p, at);
        }
        due
    };
    assert!(ask(&mut asked, a, now, false));
    assert!(ask(&mut asked, b, now, false), "another project: not held back by the first");
    assert!(!ask(&mut asked, a, now + RESUMES_EVERY / 2, false));
    assert!(ask(&mut asked, a, now + RESUMES_EVERY, false));
    assert!(ask(&mut asked, None, now, false), "terminals of no project too");
    // One of its terminals not knowing how it resumes yet: asked again sooner.
    let later = now + RESUMES_EVERY;
    assert!(!ask(&mut asked, a, later + RESUMES_SOON / 2, true));
    // A poll a moment early for its second (it came after the wake that called it): taken.
    assert!(ask(&mut asked, a, later + RESUMES_SOON - RESUMES_SLACK / 2, true));
}

/// A terminal of no project wakes every window (one without a project too); a project's,
/// only that project's. A window closed isn't woken any more.
#[test]
fn a_terminal_of_no_project_wakes_any_window() {
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    let window = add_window_wake(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    wake_projects(&[Some(kernel::ProjectId::new())]);
    assert_eq!(woken.load(Ordering::SeqCst), 0, "another project's: not this window");
    wake_projects(&[None]);
    assert_eq!(woken.load(Ordering::SeqCst), 1, "no project: every window");
    drop(window);
    wake_projects(&[None]);
    assert_eq!(woken.load(Ordering::SeqCst), 1, "closed: not woken");
}

/// The watching thread runs only while there's work: a keyed terminal, and sleeping on or
/// its memory asked lately — not every tick for nothing.
#[test]
fn the_watch_runs_only_while_needed() {
    let now = Instant::now();
    assert!(!watch_needed(true, false, None, now), "sleeping off, nobody asking");
    assert!(watch_needed(true, true, None, now), "sleeping on");
    assert!(watch_needed(true, false, Some(now), now), "asked lately");
    assert!(!watch_needed(true, false, Some(now - ASKED_WITHIN * 2), now), "asked long ago");
    assert!(!watch_needed(false, true, Some(now), now), "no keyed terminal");
}

/// A project closed: nothing is kept for it (its sleeps due, its asking clock).
#[test]
fn a_closed_project_leaves_nothing_behind() {
    let project = kernel::ProjectId::new();
    lock(&DUE).push(Some(project));
    note_resumes_asked(&mut lock(&RESUMES_ASKED), Some(project), Instant::now());
    forget_project(project);
    assert!(!lock(&DUE).contains(&Some(project)));
    assert!(lock(&RESUMES_ASKED).iter().all(|(p, _)| *p != Some(project)));
}
