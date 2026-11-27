//! Programs other modules run in a terminal (an agent's session), watched while they live:
//! what each holds (its process with everything it started, measured only while someone asks)
//! and, when the user chose it, put to sleep once untouched and idle — the program stopped, its
//! tab kept with its last screen, started again the way its owner says once the user wakes it.
//!
//! A thread of its own (`terminal-watch`) runs while some terminal is known by a key: every
//! [`TICK`] it measures (if asked within [`ASKED_WITHIN`]), every [`SLEEP_TICKS`] ticks it looks
//! for one to put to sleep and wakes its project's window. The window's poll ([`Sleeper`])
//! asks the owner how the program resumes (`ResumeLaunch`: nobody answers, it stays awake)
//! and leaves the order in the terminal's slot; its view stops the program at its own poll.
//!
//! Never asleep (each checked, else it stays awake): touched within the idle time (a key, a
//! click, the wheel, a paste, its tab chosen: on screen or not), its program wrote within it,
//! its owner's session was written within it (its agent at work), text typed and not sent,
//! its program ended, already asleep. Why it stays awake is in the log (`debug`), as it
//! changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use contracts::terminal::ResumeLaunch;

use crate::view::{Asked, asked, keyed};

/// How often the terminals are looked at.
const TICK: Duration = Duration::from_secs(10);
/// Every so many ticks, whether one should sleep (a minute: idle times are minutes long).
const SLEEP_TICKS: u32 = 6;
/// Memory is measured only while someone asked for it this recently (a panel showing it).
const ASKED_WITHIN: Duration = Duration::from_secs(30);

/// What wakes each open project's window (the newest asked; a closed one's let go).
type Wake = Arc<dyn Fn() + Send + Sync>;
static WAKES: Mutex<Vec<(kernel::ProjectId, Wake)>> = Mutex::new(Vec::new());
/// What wakes each open window, a project in it or not (by a number of its own): a
/// terminal of no project is put to sleep by any window's poll.
static WINDOW_WAKES: Mutex<Vec<(u64, Wake)>> = Mutex::new(Vec::new());

/// Each open project's bus: where its terminals say how they stand.
/// Each kept under a number of its own: a project attached again before its last
/// attachment ended keeps its new bus when the old one goes.
static BUSES: Mutex<Vec<(u64, kernel::ProjectId, kernel::EventBus)>> = Mutex::new(Vec::new());

/// `bus` heard as project `project`'s until the guard returned is dropped.
pub(crate) fn hear(project: kernel::ProjectId, bus: kernel::EventBus) -> kernel::Subscription {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&BUSES).push((id, project, bus));
    kernel::Subscription::on_end(move || lock(&BUSES).retain(|(b, ..)| *b != id))
}

/// `event` published on project `project`'s bus (`None`: a terminal of no project — on every
/// open project's). The buses are taken out of the lock first: a listener may publish too.
pub(crate) fn publish<E: kernel::DomainEvent>(project: Option<kernel::ProjectId>, event: &E) {
    let buses: Vec<kernel::EventBus> = lock(&BUSES)
        .iter()
        .filter(|(_, p, _)| project.is_none_or(|q| q == *p))
        .map(|(.., bus)| bus.clone())
        .collect();
    for bus in buses {
        bus.publish(event);
    }
}

/// Window wake `wake` kept until the subscription returned ends (the window closed).
pub(crate) fn add_window_wake(wake: Wake) -> kernel::Subscription {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    lock(&WINDOW_WAKES).push((id, wake));
    kernel::Subscription::on_end(move || lock(&WINDOW_WAKES).retain(|(w, _)| *w != id))
}
/// The thread runs.
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The projects with a terminal to put to sleep (`None`: one of no project's), found by the
/// thread: each window's poll takes its own.
static DUE: Mutex<Vec<Option<kernel::ProjectId>>> = Mutex::new(Vec::new());
/// When a terminal's memory was last asked for.
static ASKED: Mutex<Option<Instant>> = Mutex::new(None);

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether the terminal described by `asked` should be put to sleep now, after `idle`
/// untouched (no key, click, wheel, paste, its tab not chosen: on screen or not) and quiet:
/// conservative, any doubt keeps it awake.
pub(crate) fn sleepy(asked: &Asked, idle: Duration, now: Instant) -> bool {
    awake_because(asked, idle, now).is_none()
}

/// What keeps the terminal described by `asked` awake now (`None`: nothing, it may sleep):
/// the first of its reasons, as the log names it.
pub(crate) fn awake_because(asked: &Asked, idle: Duration, now: Instant) -> Option<&'static str> {
    let Some(touched) = asked.touched else { return Some("never started") };
    let untouched = now.saturating_duration_since(touched) >= idle;
    let quiet = asked.watch.as_ref().and_then(|w| w.quiet_for()).is_some_and(|q| q >= idle);
    let reason = if asked.asleep {
        "asleep"
    } else if asked.sleep.is_some() {
        "ordered to sleep"
    } else if !asked.running {
        "not running"
    } else if asked.pid.is_none() {
        "no process"
    } else if asked.typed {
        "text typed, not sent"
    } else if !untouched {
        "used lately"
    } else if !quiet {
        "output lately"
    } else {
        return None;
    };
    Some(reason)
}

/// Someone wants to know what the terminals hold: measured for a while from now on.
pub(crate) fn memory_asked() {
    *lock(&ASKED) = Some(Instant::now());
    ensure_running();
}

/// What wakes project `project`'s window (as it attaches; until [`unset_wake`]).
pub(crate) fn set_wake(project: kernel::ProjectId, wake: Wake) {
    let mut wakes = lock(&WAKES);
    wakes.retain(|(p, _)| *p != project);
    wakes.push((project, wake));
}

/// Project `project` closed: what's kept for it (sleeps due, when it last asked how its
/// terminals resume) let go.
pub(crate) fn forget_project(project: kernel::ProjectId) {
    lock(&DUE).retain(|p| *p != Some(project));
    lock(&RESUMES_ASKED).retain(|(p, _)| *p != Some(project));
}

/// Project `project`'s window closed: not woken any more.
pub(crate) fn unset_wake(project: kernel::ProjectId) {
    lock(&WAKES).retain(|(p, _)| *p != project);
}

/// The watching thread started, if it isn't running and some terminal is known by a key.
pub(crate) fn ensure_running() {
    if !needed() || RUNNING.swap(true, Ordering::AcqRel) {
        return;
    }
    // A thread of its own: it waits between looks for as long as it's needed (a pool's
    // would be held all that while), and ends when it isn't.
    #[allow(clippy::disallowed_methods)]
    let spawned = std::thread::Builder::new().name("terminal-watch".into()).spawn(run);
    if let Err(e) = spawned {
        RUNNING.store(false, Ordering::Release);
        tracing::warn!(error = %e, "can't start the terminals' watching thread");
    }
}

/// Whether the watching thread has work: some keyed terminal lives, and sleeping is on or
/// someone asked lately what they hold (else nothing wakes it every tick).
pub(crate) fn watch_needed(
    keyed: bool,
    sleeping: bool,
    asked: Option<Instant>,
    now: Instant,
) -> bool {
    let asked_lately = asked.is_some_and(|at| now.saturating_duration_since(at) < ASKED_WITHIN);
    keyed && (sleeping || asked_lately)
}

/// [`watch_needed`] now.
fn needed() -> bool {
    let sleeping = crate::config::current().sleep_after.is_some();
    watch_needed(crate::view::any_keyed(), sleeping, *lock(&ASKED), Instant::now())
}

/// The thread's loop: while it's needed ([`watch_needed`]); started again when it is
/// (a keyed terminal opened, sleeping turned on, its memory asked).
fn run() {
    let mut trees = process_stats::ProcessTrees::default();
    let mut ticks = 0u32;
    loop {
        std::thread::sleep(TICK);
        if !needed() {
            RUNNING.store(false, Ordering::Release);
            // Needed again meanwhile: go on (else a new thread is started by it).
            if !needed() || RUNNING.swap(true, Ordering::AcqRel) {
                return;
            }
        }
        ticks = ticks.wrapping_add(1);
        let asked_lately = lock(&ASKED).is_some_and(|at| at.elapsed() < ASKED_WITHIN);
        if asked_lately {
            measure(&mut trees);
        }
        if ticks.is_multiple_of(SLEEP_TICKS) {
            look_for_sleepers();
        }
    }
}

/// Each keyed terminal's program measured with everything it started (its slot keeps it).
fn measure(trees: &mut process_stats::ProcessTrees) {
    let slots = keyed();
    let pids: Vec<u32> = slots.iter().filter_map(|(_, s)| asked(s).pid).collect();
    let used = trees.sample(&pids);
    for (key, slot) in &slots {
        {
            let mut asked = asked(slot);
            asked.memory = asked.pid.and_then(|pid| used.get(&pid)).map(|u| u.memory);
        }
        // Said on its bus from this thread (its listeners only keep it and wake a window).
        crate::view::announce(slot, key);
    }
}

/// The windows of the projects with a terminal to put to sleep woken (their poll asks how
/// each resumes).
fn look_for_sleepers() {
    // Orders never taken (no view polled them in time): let go, the slot free again.
    for (key, slot) in keyed() {
        {
            let mut asked = asked(&slot);
            if !asked
                .sleep
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() >= crate::view::SLEEP_ORDER_FRESH)
            {
                continue;
            }
            asked.sleep = None;
            asked.by_hand = false;
        }
        crate::view::announce(&slot, &key);
    }
    let Some(idle) = crate::config::current().sleep_after else { return };
    let now = Instant::now();
    let projects: Vec<Option<kernel::ProjectId>> = keyed()
        .iter()
        .filter_map(|(key, slot)| {
            let (awake, project) = {
                let asked = asked(slot);
                (awake_because(&asked, idle, now), asked.project)
            };
            tell_awake(slot, key, awake);
            awake.is_none().then_some(project)
        })
        .collect();
    if projects.is_empty() {
        return;
    }
    {
        let mut due = lock(&DUE);
        for project in &projects {
            if !due.contains(project) {
                due.push(*project);
            }
        }
    }
    wake_projects(&projects);
}

/// Why the terminal in `slot`, known by `key`, stays awake (`None`: it may sleep), written to
/// the log only when that changed since it last was (the lock let go first).
fn tell_awake(slot: &crate::view::NameSlot, key: &str, awake: Option<&'static str>) {
    let changed = {
        let mut asked = asked(slot);
        let changed = asked.awake != Some(awake);
        asked.awake = Some(awake);
        changed
    };
    if changed {
        tracing::debug!(key, awake = awake.unwrap_or("no: may sleep"), "a session's sleep");
    }
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

/// The terminals of the window's project (one without a project: any window's) to put to
/// sleep, each asked of its owner (how it resumes) and, answered, given the order.
pub(crate) fn put_to_sleep(host: &mut dyn workbench_model::WorkbenchHost) -> bool {
    let project = host.project_id();
    {
        let mut due = lock(&DUE);
        if due.is_empty() {
            return false;
        }
        let before = due.len();
        due.retain(|p| p.is_some_and(|p| Some(p) != project));
        if due.len() == before {
            return false;
        }
    }
    let Some(idle) = crate::config::current().sleep_after else { return false };
    let now = Instant::now();
    let due: Vec<(String, Arc<crate::view::NameSlot>)> = keyed()
        .into_iter()
        .filter(|(_, slot)| {
            let asked = asked(slot);
            asked.project.is_none_or(|p| Some(p) == project) && sleepy(&asked, idle, now)
        })
        .collect();
    let mut any = false;
    for (key, slot) in due {
        // Asked outside the slot's lock (it reaches another module).
        let Some(Some(launch)) = host.ask(ResumeLaunch { key: key.clone(), idle }) else {
            tell_awake(&slot, &key, Some("its owner can't resume it now"));
            continue;
        };
        let ordered = {
            let mut asked = asked(&slot);
            // Looked at again: touched or typed into meanwhile, it stays awake.
            let now = Instant::now();
            let sleepy = sleepy(&asked, idle, now);
            if sleepy {
                asked.sleep = Some((launch, now));
            }
            sleepy
        };
        if ordered {
            crate::view::announce(&slot, &key);
            any = true;
        }
    }
    any
}

/// How often the keyed terminals' owners are asked how they resume (what the menus offer),
/// and how often while one runs not knowing it yet (a session just started: its owner
/// learns its id within a second or two, the menus follow then).
const RESUMES_EVERY: Duration = Duration::from_secs(5);
const RESUMES_SOON: Duration = Duration::from_secs(1);
/// How much earlier than [`RESUMES_SOON`] a window may ask again: its poll comes a moment
/// after the wake that called it, so a second's wakes give polls a little under or over a
/// second apart (without it, every other one would be skipped).
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
type Starting = Vec<(std::sync::Weak<crate::view::NameSlot>, Option<kernel::ProjectId>, Instant)>;
static STARTING: Mutex<Starting> = Mutex::new(Vec::new());
/// The thread waking their windows runs.
static STARTING_RUNS: AtomicBool = AtomicBool::new(false);

/// A keyed terminal of `project` just started: its window woken every [`RESUMES_SOON`] while
/// it's starting (its poll asks how it resumes, the menus follow), even a quiet one that
/// nothing else wakes — for [`STARTING_WITHIN`] at most, by a thread that ends with the last.
pub(crate) fn follow_start(slot: &Arc<crate::view::NameSlot>, project: Option<kernel::ProjectId>) {
    let until = Instant::now() + crate::view::STARTING_WITHIN;
    lock(&STARTING).push((Arc::downgrade(slot), project, until));
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

/// The terminals still starting kept (those known, gone or past their time let go), and
/// the projects they're in.
pub(crate) fn still_starting(list: &mut Starting, now: Instant) -> Vec<Option<kernel::ProjectId>> {
    use contracts::terminal::CanSleep;
    list.retain(|(slot, _, until)| {
        now < *until && slot.upgrade().is_some_and(|s| asked(&s).can_sleep() == CanSleep::Starting)
    });
    let mut projects: Vec<Option<kernel::ProjectId>> = Vec::new();
    for (_, project, _) in list.iter() {
        if !projects.contains(project) {
            projects.push(*project);
        }
    }
    projects
}

/// The thread's loop: the windows of the terminals starting woken every [`RESUMES_SOON`];
/// none left, it ends (started again by the next one).
fn wake_starting() {
    loop {
        std::thread::sleep(RESUMES_SOON);
        // Taken out and looked at without the list's lock (each slot has its own).
        let mut list = std::mem::take(&mut *lock(&STARTING));
        let projects = still_starting(&mut list, Instant::now());
        {
            let mut kept = lock(&STARTING);
            // Started meanwhile: kept with them.
            list.append(&mut kept);
            *kept = list;
        }
        if !projects.is_empty() {
            wake_projects(&projects);
            continue;
        }
        STARTING_RUNS.store(false, Ordering::Release);
        // One started meanwhile: go on (else a new thread is started by it).
        if lock(&STARTING).is_empty() || STARTING_RUNS.swap(true, Ordering::AcqRel) {
            return;
        }
    }
}

/// Each running keyed terminal of the window's project told how it resumes, as its owner
/// says now (asked off the menus, which read only this; nobody answers: it can't sleep).
fn learn_resumes(host: &mut dyn workbench_model::WorkbenchHost) {
    let project = host.project_id();
    // Some terminal of its project is starting, not knowing how it resumes yet (memory
    // only; a short while, then it's asked at the usual pace).
    let unknown = || {
        keyed().iter().any(|(_, slot)| {
            let asked = asked(slot);
            asked.can_sleep() == contracts::terminal::CanSleep::Starting
                && asked.project.is_none_or(|p| Some(p) == project)
        })
    };
    if !crate::view::any_keyed() {
        return;
    }
    // When it last asked, read and let go: deciding looks at the terminals (their locks),
    // never under this one.
    let now = Instant::now();
    let last = resumes_asked_at(&lock(&RESUMES_ASKED), project);
    if !resumes_due(last, now, unknown) {
        return;
    }
    note_resumes_asked(&mut lock(&RESUMES_ASKED), project, now);
    for (key, slot) in keyed() {
        let running = {
            let asked = asked(&slot);
            asked.pid.is_some() && asked.project.is_none_or(|p| Some(p) == project)
        };
        if !running {
            continue;
        }
        // Asked outside the slot's lock (it reaches another module).
        let resume = host.ask(ResumeLaunch { key: key.clone(), idle: Duration::ZERO }).flatten();
        asked(&slot).resume = resume;
        crate::view::announce(&slot, &key);
    }
}

/// The terminal's part of each window's poll: put to sleep what's due (see the module).
#[derive(Debug)]
pub(crate) struct Sleeper;

impl workbench_model::ProjectService for Sleeper {
    fn attach(&self, project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        set_wake(project.id, Arc::clone(&project.wake));
        let bus = hear(project.id, project.events.clone());
        let (id, root) = (project.id, project.root.to_path_buf());
        // Closed: not woken any more, its bus and its terminals' history store let go.
        vec![
            bus,
            kernel::Subscription::on_end(move || {
                unset_wake(id);
                forget_project(id);
                crate::spill::release(&root);
            }),
        ]
    }

    /// A window, a project in it or not: woken for a terminal of no project.
    fn window_opened(&self, wake: &Arc<dyn Fn() + Send + Sync>) -> Vec<kernel::Subscription> {
        vec![add_window_wake(Arc::clone(wake))]
    }

    fn poll(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        learn_resumes(host);
        put_to_sleep(host)
    }
}

#[cfg(test)]
#[path = "../tests/unit/sleep.rs"]
mod tests;
