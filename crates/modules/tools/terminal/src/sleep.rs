//! The terminal's own part of the sleeping (plan 101): programs other modules run in a
//! terminal (an agent's session) are a kind of view that can sleep ([`SESSIONS`]); when and
//! how often is the sleeping module's, never this one's. What is the terminal's:
//!
//! - what keeps a session awake ([`veto`]): its program ended, nobody can resume it (or not
//!   yet: it's starting), something typed isn't sent, its owner's record was written lately
//!   (a turn under way: told by `ResumeLaunch` asked with the idle time);
//! - how it resumes, learned from its owner ([`learn_resumes`], `ResumeLaunch`) at each
//!   window's poll on its project's clock, and every second while a session starts (a
//!   thread of its own, `terminal-start`, wakes those windows);
//! - what each holds (its process with everything it started), measured on a thread
//!   (`terminal-measure`) only while someone asked lately.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use contracts::terminal::ResumeLaunch;
use ui_components::sleep::{self as views, SleepKind, SleepTexts, Veto};

use crate::view::{Asked, NameSlot, asked, keyed};

/// Programs other modules run in a terminal (an agent's session), as a kind of view that can
/// sleep: their hint, their words.
pub(crate) static SESSIONS: SleepKind = SleepKind {
    id: "agent-session",
    viewer: crate::VIEWER,
    hint: Some(crate::SLEEP_HINT),
    texts: &TEXTS,
};

static TEXTS: SleepTexts = SleepTexts {
    asleep_title: "terminal.asleep_title",
    resuming: "terminal.resuming",
    by_hand: "terminal.asleep_by_hand",
    idle: "terminal.asleep_idle",
    freed: "terminal.asleep_freed",
    wake_up: "terminal.wake_up",
    how: "terminal.asleep_how",
    sleep_now: "terminal.sleep_now",
    going_to_sleep: "terminal.going_to_sleep",
    waking_up: "terminal.waking_up",
    cant: "terminal.cant_sleep",
    cant_sleeping: "terminal.cant_sleep_sleeping",
    cant_waking: "terminal.cant_sleep_waking",
};

/// Its program ended (not asleep): nothing to put to sleep.
pub(crate) const ENDED: Veto = Veto {
    id: "not running",
    menu: "terminal.sleep_now",
    refusal: "terminal.cant_sleep",
    by_hand: true,
    never: true,
};
/// Nobody says how it would come back: it can't sleep.
pub(crate) const NO_RESUME: Veto = Veto { id: "nobody can resume it", ..ENDED };
/// Just started: its owner learns how it resumes within a second or two.
pub(crate) const STARTING: Veto = Veto {
    id: "starting",
    menu: "terminal.sleep_now_starting",
    refusal: "terminal.cant_sleep_starting",
    by_hand: true,
    never: false,
};
/// Something typed isn't sent yet (it would be lost).
pub(crate) const TYPED: Veto = Veto {
    id: "text typed, not sent",
    menu: "terminal.sleep_now_typed",
    refusal: "terminal.cant_sleep_typed",
    by_hand: true,
    never: false,
};
/// Its owner's record was written lately (a turn may run): only the idle sleep waits.
pub(crate) const OWNER_BUSY: Veto =
    Veto { id: "its owner is at work", by_hand: false, never: false, ..TYPED };

/// How long a session counts as starting (its owner learning how it resumes, within a second
/// or two): past that, one still not known can't sleep, and isn't asked about often.
pub(crate) const STARTING_WITHIN: Duration = Duration::from_secs(10);

/// What of its own keeps the terminal described by `asked` awake now (`None`: nothing).
pub(crate) fn veto(asked: &Asked, now: Instant) -> Option<Veto> {
    if !asked.running || asked.pid.is_none() {
        return Some(ENDED);
    }
    if asked.resume.is_none() {
        let answered = workbench_model::presence::request::<ResumeLaunch>();
        let fresh =
            asked.started.is_some_and(|at| now.saturating_duration_since(at) < STARTING_WITHIN);
        return Some(if answered && fresh { STARTING } else { NO_RESUME });
    }
    if asked.typed {
        return Some(TYPED);
    }
    asked.owner_busy.then_some(OWNER_BUSY)
}

/// The terminal in `slot` tells its sleeping slot what keeps it awake now (its lock let go
/// first: the slot announces).
pub(crate) fn tell_veto(slot: &NameSlot) {
    let (sleeping, veto) = {
        let asked = asked(slot);
        (asked.sleeping.clone(), veto(&asked, Instant::now()))
    };
    if let Some(sleeping) = sleeping {
        sleeping.set_veto(veto);
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

// ---------------------------------------------------------------------------------------
// The windows to wake while a session starts.

/// What wakes each open project's window (the newest asked; a closed one's let go).
type Wake = Arc<dyn Fn() + Send + Sync>;
static WAKES: Mutex<Vec<(kernel::ProjectId, Wake)>> = Mutex::new(Vec::new());
/// What wakes each open window, a project in it or not (by a number of its own): a terminal
/// of no project is followed by any window's poll.
static WINDOW_WAKES: Mutex<Vec<(u64, Wake)>> = Mutex::new(Vec::new());

/// Window wake `wake` kept until the subscription returned ends (the window closed).
pub(crate) fn add_window_wake(wake: Wake) -> kernel::Subscription {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&WINDOW_WAKES).push((id, wake));
    kernel::Subscription::on_end(move || lock(&WINDOW_WAKES).retain(|(w, _)| *w != id))
}

/// What wakes project `project`'s window (as it attaches; until [`unset_wake`]).
pub(crate) fn set_wake(project: kernel::ProjectId, wake: Wake) {
    let mut wakes = lock(&WAKES);
    wakes.retain(|(p, _)| *p != project);
    wakes.push((project, wake));
}

/// Project `project`'s window closed: not woken any more.
pub(crate) fn unset_wake(project: kernel::ProjectId) {
    lock(&WAKES).retain(|(p, _)| *p != project);
}

/// Project `project` closed: when it last asked how its terminals resume, let go.
pub(crate) fn forget_project(project: kernel::ProjectId) {
    lock(&RESUMES_ASKED).retain(|(p, _)| *p != Some(project));
}

/// The windows of `projects` woken (`None`: a terminal of no project — every window, a
/// project in it or not). Called with no lock held.
pub(crate) fn wake_projects(projects: &[Option<kernel::ProjectId>]) {
    let mut wakes: Vec<Wake> = lock(&WAKES)
        .iter()
        .filter(|(p, _)| projects.iter().any(|q| q.is_none_or(|q| q == *p)))
        .map(|(_, w)| Arc::clone(w))
        .collect();
    if projects.contains(&None) {
        wakes.extend(lock(&WINDOW_WAKES).iter().map(|(_, w)| Arc::clone(w)));
    }
    for wake in wakes {
        wake();
    }
}

// ---------------------------------------------------------------------------------------
// What each session holds: measured while someone asked lately.

/// How often what they hold is measured, and for how long after someone asked.
const MEASURE_EVERY: Duration = Duration::from_secs(10);
const ASKED_WITHIN: Duration = Duration::from_secs(30);
/// The measuring thread runs.
static MEASURING: AtomicBool = AtomicBool::new(false);

/// Whether measuring has work: some keyed terminal lives and someone asked lately.
fn measure_needed() -> bool {
    crate::view::any_keyed() && views::memory_wanted(ASKED_WITHIN)
}

/// The measuring thread started, if it's needed and not running.
pub(crate) fn ensure_measuring() {
    if !measure_needed() || MEASURING.swap(true, Ordering::AcqRel) {
        return;
    }
    // A thread of its own: it waits between measures for as long as it's needed.
    #[allow(clippy::disallowed_methods)]
    let spawned = std::thread::Builder::new().name("terminal-measure".into()).spawn(measuring);
    if let Err(e) = spawned {
        MEASURING.store(false, Ordering::Release);
        tracing::warn!(error = %e, "can't start the terminals' measuring thread");
    }
}

/// The measuring loop: at once, then every [`MEASURE_EVERY`] while it's needed.
fn measuring() {
    let mut trees = process_stats::ProcessTrees::default();
    loop {
        measure(&mut trees);
        std::thread::sleep(MEASURE_EVERY);
        if !measure_needed() {
            MEASURING.store(false, Ordering::Release);
            // Needed again meanwhile: go on (else a new thread is started by it).
            if !measure_needed() || MEASURING.swap(true, Ordering::AcqRel) {
                return;
            }
        }
    }
}

/// Each keyed terminal's program measured with everything it started, told to its slot.
fn measure(trees: &mut process_stats::ProcessTrees) {
    let slots = keyed();
    let pids: Vec<u32> = slots.iter().filter_map(|(_, s)| asked(s).pid).collect();
    let used = trees.sample(&pids);
    for (_, slot) in &slots {
        let (sleeping, memory) = {
            let asked = asked(slot);
            (asked.sleeping.clone(), asked.pid.and_then(|pid| used.get(&pid)).map(|u| u.memory))
        };
        if let Some(sleeping) = sleeping {
            sleeping.set_memory(memory);
        }
    }
}

// ---------------------------------------------------------------------------------------
// How each session resumes, learned from its owner.

/// How often the keyed terminals' owners are asked how they resume (what the menus offer),
/// and how often while one runs not knowing it yet (a session just started: its owner learns
/// its id within a second or two, the menus follow then).
const RESUMES_EVERY: Duration = Duration::from_secs(5);
const RESUMES_SOON: Duration = Duration::from_secs(1);
/// How much earlier than [`RESUMES_SOON`] a window may ask again: its poll comes a moment after
/// the wake that called it, so a second's wakes give polls a little under or over a second
/// apart (without it, every other one would be skipped).
const RESUMES_SLACK: Duration = Duration::from_millis(200);
/// When each project's window last asked (`None`: terminals of no project).
type ResumesAsked = Vec<(Option<kernel::ProjectId>, Instant)>;
static RESUMES_ASKED: Mutex<ResumesAsked> = Mutex::new(Vec::new());

/// Whether a window should ask again now, `last` asked when it did (`None`: never): every
/// [`RESUMES_EVERY`], or every [`RESUMES_SOON`] while `unknown` (one of its terminals is
/// starting, not knowing how it resumes; looked at only then).
pub(crate) fn resumes_due(
    last: Option<Instant>,
    now: Instant,
    unknown: impl FnOnce() -> bool,
) -> bool {
    let Some(last) = last else { return true };
    let since = now.saturating_duration_since(last);
    since >= RESUMES_EVERY || (since + RESUMES_SLACK >= RESUMES_SOON && unknown())
}

/// When project `project`'s window last asked (each project on its own clock: another
/// window's asking never holds this one back).
pub(crate) fn resumes_asked_at(
    asked: &ResumesAsked,
    project: Option<kernel::ProjectId>,
) -> Option<Instant> {
    asked.iter().find(|(p, _)| *p == project).map(|(_, at)| *at)
}

/// Project `project`'s window asked at `now`.
pub(crate) fn note_resumes_asked(
    asked: &mut ResumesAsked,
    project: Option<kernel::ProjectId>,
    now: Instant,
) {
    match asked.iter_mut().find(|(p, _)| *p == project) {
        Some((_, at)) => *at = now,
        None => asked.push((project, now)),
    }
}

/// The keyed terminals just started, their owner not having said yet how they resume: each
/// with its project and until when it's followed ([`STARTING_WITHIN`] after its start).
type Starting = Vec<(std::sync::Weak<NameSlot>, Option<kernel::ProjectId>, Instant)>;
static STARTING_LIST: Mutex<Starting> = Mutex::new(Vec::new());
/// The thread waking their windows runs.
static STARTING_RUNS: AtomicBool = AtomicBool::new(false);

/// A keyed terminal of `project` just started: its window woken every [`RESUMES_SOON`] while
/// it's starting (its poll asks how it resumes, the menus follow), even a quiet one that
/// nothing else wakes — for [`STARTING_WITHIN`] at most (and once more after, so its menus
/// stop saying "starting"), by a thread that ends with the last.
pub(crate) fn follow_start(slot: &Arc<NameSlot>, project: Option<kernel::ProjectId>) {
    let until = Instant::now() + STARTING_WITHIN;
    lock(&STARTING_LIST).push((Arc::downgrade(slot), project, until));
    if STARTING_RUNS.swap(true, Ordering::AcqRel) {
        return;
    }
    // A thread of its own: it waits between wakes (a pool's would be held all that while).
    #[allow(clippy::disallowed_methods)]
    let spawned = std::thread::Builder::new().name("terminal-start".into()).spawn(wake_starting);
    if let Err(e) = spawned {
        STARTING_RUNS.store(false, Ordering::Release);
        tracing::warn!(error = %e, "can't start the thread following started terminals");
    }
}

/// Whether the terminal described by `asked` is starting: running, its owner not having said
/// yet how it resumes, within [`STARTING_WITHIN`] of its start.
fn starting(asked: &Asked, now: Instant) -> bool {
    veto(asked, now) == Some(STARTING)
}

/// The terminals still starting kept (those known, gone or past their time let go: one past
/// its time told what keeps it awake now, no longer "starting"), and their projects.
pub(crate) fn still_starting(list: &mut Starting, now: Instant) -> Vec<Option<kernel::ProjectId>> {
    let mut projects: Vec<Option<kernel::ProjectId>> = Vec::new();
    list.retain(|(slot, project, until)| {
        let Some(slot) = slot.upgrade() else { return false };
        if now >= *until {
            tell_veto(&slot);
            return false;
        }
        let keep = starting(&asked(&slot), now);
        if keep && !projects.contains(project) {
            projects.push(*project);
        }
        keep
    });
    projects
}

/// The thread's loop: the windows of the terminals starting woken every [`RESUMES_SOON`];
/// none left, it ends (started again by the next one).
fn wake_starting() {
    loop {
        std::thread::sleep(RESUMES_SOON);
        // Taken out and looked at without the list's lock (each slot has its own).
        let mut list = std::mem::take(&mut *lock(&STARTING_LIST));
        let projects = still_starting(&mut list, Instant::now());
        let left = {
            let mut kept = lock(&STARTING_LIST);
            // Started meanwhile: kept with them.
            list.append(&mut kept);
            *kept = list;
            !kept.is_empty()
        };
        if !projects.is_empty() {
            wake_projects(&projects);
        }
        if left {
            continue;
        }
        STARTING_RUNS.store(false, Ordering::Release);
        // One started meanwhile: go on (else a new thread is started by it).
        if lock(&STARTING_LIST).is_empty() || STARTING_RUNS.swap(true, Ordering::AcqRel) {
            return;
        }
    }
}

/// Each running keyed terminal of the window's project told how it resumes, as its owner says
/// now (asked off the menus, which read only this; nobody answers: it can't sleep), and
/// whether its owner is at work (asked again with the idle time, while sleeping is on).
fn learn_resumes(host: &mut dyn workbench_model::WorkbenchHost) {
    if !crate::view::any_keyed() {
        return;
    }
    let project = host.project_id();
    let now = Instant::now();
    // Some terminal of its project is starting (memory only; a short while, then it's asked
    // at the usual pace).
    let unknown = || {
        keyed().iter().any(|(_, slot)| {
            let asked = asked(slot);
            asked.project.is_none_or(|p| Some(p) == project) && starting(&asked, now)
        })
    };
    // When it last asked, read and let go: deciding looks at the terminals (their locks),
    // never under this one.
    let last = resumes_asked_at(&lock(&RESUMES_ASKED), project);
    if !resumes_due(last, now, unknown) {
        return;
    }
    note_resumes_asked(&mut lock(&RESUMES_ASKED), project, now);
    let idle = views::idle_for(SESSIONS.id);
    for (key, slot) in keyed() {
        let ours = {
            let asked = asked(&slot);
            asked.pid.is_some() && asked.project.is_none_or(|p| Some(p) == project)
        };
        if !ours {
            continue;
        }
        // Asked outside the slot's lock (it reaches another module).
        let resume = host.ask(ResumeLaunch { key: key.clone(), idle: Duration::ZERO }).flatten();
        let owner_busy = match idle {
            Some(idle) if resume.is_some() => {
                host.ask(ResumeLaunch { key: key.clone(), idle }).flatten().is_none()
            }
            _ => false,
        };
        {
            let mut asked = asked(&slot);
            asked.resume = resume;
            asked.owner_busy = owner_busy;
        }
        tell_veto(&slot);
    }
}

/// The terminal's part of each window's poll: how its sessions resume, learned; their
/// memory measured while someone shows it.
#[derive(Debug)]
pub(crate) struct Sleeper;

impl workbench_model::ProjectService for Sleeper {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        set_wake(project.id, Arc::clone(&project.wake));
        let (id, root) = (project.id, project.root.to_path_buf());
        // Closed: not woken any more, its terminals' history store let go.
        vec![kernel::Subscription::on_end(move || {
            unset_wake(id);
            forget_project(id);
            crate::spill::release(&root);
        })]
    }

    /// A window, a project in it or not: woken for a terminal of no project.
    fn window_opened(&self, wake: &Arc<dyn Fn() + Send + Sync>) -> Vec<kernel::Subscription> {
        vec![add_window_wake(Arc::clone(wake))]
    }

    fn poll(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        learn_resumes(host);
        ensure_measuring();
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/sleep.rs"]
mod tests;
