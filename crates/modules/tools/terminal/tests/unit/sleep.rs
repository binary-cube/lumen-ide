use super::*;

/// A running session, its owner knowing how it resumes.
fn running() -> Asked {
    Asked {
        running: true,
        pid: Some(4_000_001),
        resume: Some(agents::Launch::default()),
        started: Some(Instant::now()),
        ..Asked::default()
    }
}

/// What keeps a session awake, the terminal's own reasons, the first that holds: its program
/// ended, nobody can resume it (or not yet: it's starting), something typed isn't sent, its
/// owner at work; none, nothing.
#[test]
fn a_session_says_what_of_its_own_keeps_it_awake() {
    let now = Instant::now();
    assert_eq!(veto(&running(), now), None);
    assert_eq!(veto(&Asked { running: false, ..running() }, now), Some(ENDED));
    assert_eq!(veto(&Asked { pid: None, ..running() }, now), Some(ENDED));
    let unknown = Asked { resume: None, started: Some(now), ..running() };
    let starting = veto(&unknown, now);
    assert!(starting == Some(STARTING) || starting == Some(NO_RESUME), "as someone answers");
    let later = now + STARTING_WITHIN;
    assert_eq!(veto(&unknown, later), Some(NO_RESUME), "never starting for good");
    assert_eq!(veto(&Asked { typed: true, ..running() }, now), Some(TYPED));
    assert_eq!(veto(&Asked { owner_busy: true, ..running() }, now), Some(OWNER_BUSY));
    let holds = |veto: Veto| (veto.by_hand, veto.never);
    assert_eq!(holds(TYPED), (true, false), "the user's own order waits for it, greyed out");
    assert_eq!(holds(STARTING), (true, false));
    assert_eq!(holds(OWNER_BUSY), (false, false), "not the user's: they chose");
    assert_eq!(holds(ENDED), (true, true), "its row hidden");
    assert_eq!(holds(NO_RESUME), (true, true));
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

/// A project closed: nothing is kept for it (its asking clock).
#[test]
fn a_closed_project_leaves_nothing_behind() {
    let project = kernel::ProjectId::new();
    note_resumes_asked(&mut lock(&RESUMES_ASKED), Some(project), Instant::now());
    forget_project(project);
    assert!(lock(&RESUMES_ASKED).iter().all(|(p, _)| *p != Some(project)));
}
