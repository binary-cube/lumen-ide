//! The programs the app starts, known after a crash (plan 83): this instance's registry
//! (`process_registry`, told by `process::launched`) and, once at start on a worker, what an
//! earlier run that ended without stopping its programs left running — its helpers stopped
//! there, its terminals, agents and tasks kept here to ask about: in the window of their
//! project once it's open, those of no project in the first window. What's answered is
//! forgotten (never asked again); stopping runs on a worker. While they run, they're also
//! the Task Manager's rows under „Left from the last session” (with Stop): kept in memory,
//! their end seen by a look at only their pids every few seconds, on a thread that lives
//! only while there are some.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use process_registry::{Found, Registry};

/// This instance's registry (none in tests or snapshots).
static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();

/// What the earlier run left, still to ask about.
static FOUND: Mutex<Vec<Found>> = Mutex::new(Vec::new());

/// What the earlier run left that still runs and isn't stopped: the Task Manager's rows
/// (asked about or not, left running or not).
static KEPT: Mutex<Vec<Found>> = Mutex::new(Vec::new());

/// How often the rows' programs are looked at (their end).
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// Something in [`FOUND`] (the windows look only when set).
static PENDING: AtomicBool = AtomicBool::new(false);

/// The event loop's waker: the look's result wakes it.
static WAKER: Mutex<Option<Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);

/// The registry started (no disk touched here) and the programs started from now on told
/// to it; what an earlier run left looked at on a worker.
pub fn start() {
    let Some(data) = storage::data_dir() else { return };
    let dir = process_registry::dir(&data);
    let Some(registry) = Registry::open(&dir) else { return };
    let registry = Arc::new(registry);
    process::launched::set_recorder(Arc::clone(&registry) as Arc<dyn process::launched::Recorder>);
    let _ = REGISTRY.set(Arc::clone(&registry));
    let _ = std::thread::Builder::new().name("left-behind".into()).spawn(move || {
        let mut survey = process_registry::survey(&dir, registry.file());
        if survey.stopped > 0 {
            tracing::info!(stopped = survey.stopped, "helpers left by an earlier run stopped");
        }
        let records = survey.asked.iter().map(|f| f.record.clone()).collect::<Vec<_>>();
        if !records.is_empty() {
            tracing::info!(count = records.len(), "programs left by an earlier run: asked");
            // Kept in this instance's file until answered (a crash before then asks again).
            registry.adopt(records);
        }
        survey.done();
        if survey.asked.is_empty() {
            return;
        }
        if let Ok(mut kept) = KEPT.lock() {
            kept.extend(survey.asked.iter().cloned());
        }
        watch();
        if let Ok(mut found) = FOUND.lock() {
            found.append(&mut survey.asked);
        }
        PENDING.store(true, Ordering::Release);
        let waker = WAKER.lock().ok().and_then(|w| w.clone());
        if let Some(wake) = waker {
            wake();
        }
    });
}

/// A normal exit: this instance's file removed (kept with only what's still to ask about).
pub fn finish() {
    if let Some(registry) = REGISTRY.get() {
        registry.finish();
    }
}

/// The event loop's waker (the first window's).
pub fn set_waker(waker: Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut slot) = WAKER.lock() {
        *slot = Some(waker);
    }
    // Found before the window was there.
    if PENDING.load(Ordering::Acquire) {
        let waker = WAKER.lock().ok().and_then(|w| w.clone());
        if let Some(wake) = waker {
            wake();
        }
    }
}

/// Whether something is still to ask about (an atomic: cheap on every event).
pub fn pending() -> bool {
    PENDING.load(Ordering::Acquire)
}

/// What's to ask about in a window with project `root` (`first`: the first window, which
/// also takes those of no project), taken out.
pub fn take(root: Option<&Path>, first: bool) -> Vec<Found> {
    if !pending() {
        return Vec::new();
    }
    let Ok(mut found) = FOUND.lock() else { return Vec::new() };
    let (here, rest): (Vec<Found>, Vec<Found>) =
        std::mem::take(&mut *found).into_iter().partition(|f| belongs_here(&f.record, root, first));
    *found = rest;
    PENDING.store(!found.is_empty(), Ordering::Release);
    here
}

/// Whether `record` is asked about in a window with project `root` (`first`: the first
/// window, which takes those of no project).
pub fn belongs_here(record: &process_registry::Record, root: Option<&Path>, first: bool) -> bool {
    match record.project_path() {
        Some(_) => root.is_some_and(|r| record.belongs_to(r)),
        None => first,
    }
}

/// The user's answer about `found`: stopped (on a worker: asked, then made to, with what
/// they started) or left running; either way forgotten.
pub fn answer(found: Vec<Found>, stop: bool) {
    if found.is_empty() {
        return;
    }
    if let Some(registry) = REGISTRY.get() {
        registry.forget(found.iter().map(|f| (f.record.pid, f.record.started)).collect());
    }
    if stop && let Ok(mut kept) = KEPT.lock() {
        kept.retain(|k| !found.iter().any(|f| same(f, k)));
    }
    if !stop {
        tracing::info!(count = found.len(), "programs left by an earlier run: left running");
        return;
    }
    tracing::info!(count = found.len(), "programs left by an earlier run: stopped");
    let trees: Vec<(u32, u64)> = found.iter().flat_map(|f| f.tree.iter().copied()).collect();
    let _ = std::thread::Builder::new().name("left-behind-stop".into()).spawn(move || {
        process_registry::stop::stop(&trees, process_registry::stop::GRACE);
    });
}

/// Whether `a` and `b` are the same program (its pid and start time).
fn same(a: &Found, b: &Found) -> bool {
    a.record.pid == b.record.pid && a.record.started == b.record.started
}

/// The rows' programs looked at every [`WATCH_EVERY`] on a thread of its own (only their
/// pids): those ended leave the rows; the thread ends with the last one.
fn watch() {
    let _ = std::thread::Builder::new().name("left-behind-watch".into()).spawn(|| {
        let mut system = sysinfo::System::new();
        loop {
            std::thread::sleep(WATCH_EVERY);
            let kept = KEPT.lock().map(|k| k.clone()).unwrap_or_default();
            if kept.is_empty() {
                return;
            }
            let running = process_registry::survey::still_running(&mut system, &kept);
            let ended: Vec<&Found> =
                kept.iter().zip(running).filter(|(_, r)| !r).map(|(f, _)| f).collect();
            if ended.is_empty() {
                continue;
            }
            if let Ok(mut list) = KEPT.lock() {
                list.retain(|k| !ended.iter().any(|f| same(f, k)));
            }
            if let Some(registry) = REGISTRY.get() {
                registry.forget(ended.iter().map(|f| (f.record.pid, f.record.started)).collect());
            }
        }
    });
}

/// The Task Manager's action stopping a program left running.
const STOP: &str = "left-behind-stop";

/// Whose the rows are.
const OWNER: &str = "app.left_behind";

/// What an earlier run left running, in the Task Manager: a row each (its program and
/// pid; what it is, its project, how long it has run), its processes measured there, with
/// Stop. Read from memory only (asked on the UI thread too).
#[derive(Debug, Default)]
pub struct Reporter;

impl Reporter {
    /// The rows of `kept` at `now` (seconds since the epoch).
    pub fn parts(kept: &[Found], now: u64) -> Vec<workbench_model::ResourceUse> {
        use workbench_model::resources::{ActionKind, ActionTarget, PartAction};
        kept.iter()
            .map(|found| {
                let row = process_registry::row(found, now);
                let name = format!("{} ({})", row.program, found.record.pid);
                let mut part = workbench_model::ResourceUse::new(name, OWNER);
                let (what, since) = (kind_words(&row), since_words(&row));
                part.detail = match &row.project {
                    Some(project) => i18n::tr!(
                        "left_behind.part_project",
                        what = what,
                        project = project.as_str(),
                        since = since
                    ),
                    None => i18n::tr!("left_behind.part", what = what, since = since),
                };
                part.group = workbench_model::resources::LEFT_BEHIND;
                part.processes = found.tree.iter().map(|(pid, _)| *pid).collect();
                part.actions =
                    vec![PartAction::new(STOP, ActionKind::Stop, ActionTarget::Part).confirmed()];
                part
            })
            .collect()
    }
}

impl workbench_model::ResourceReporter for Reporter {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let kept = KEPT.lock().map(|k| k.clone()).unwrap_or_default();
        if kept.is_empty() {
            return Vec::new();
        }
        Self::parts(&kept, process_registry::row::now())
    }

    /// Nothing a closing stops: they already outlived a run.
    fn running(&self) -> Vec<workbench_model::running::RunningWork> {
        Vec::new()
    }

    fn act(
        &self,
        part: &workbench_model::resources::PartRef,
        action: &str,
        _: workbench_model::resources::ActionTarget,
    ) -> workbench_model::ActOutcome {
        if action != STOP || part.owner != OWNER {
            return workbench_model::ActOutcome::Unsupported;
        }
        let kept = KEPT.lock().map(|k| k.clone()).unwrap_or_default();
        let now = process_registry::row::now();
        let Some(found) = kept.into_iter().find(|f| {
            Self::parts(std::slice::from_ref(f), now).first().is_some_and(|p| part.is(p))
        }) else {
            return workbench_model::ActOutcome::Unsupported;
        };
        // Not asked about again in a window either.
        if let Ok(mut waiting) = FOUND.lock() {
            waiting.retain(|f| !same(f, &found));
            PENDING.store(!waiting.is_empty(), Ordering::Release);
        }
        answer(vec![found], true);
        workbench_model::ActOutcome::Done
    }
}

/// What `row` is, in words ("an agent").
fn kind_words(row: &process_registry::Row) -> String {
    use process::launched::Kind;
    i18n::t(match row.kind {
        Kind::Terminal => "left_behind.kind_terminal",
        Kind::Agent => "left_behind.kind_agent",
        Kind::Task => "left_behind.kind_task",
        Kind::Server => "left_behind.kind_server",
        Kind::Git | Kind::Other => "left_behind.kind_other",
    })
}

/// How long `row` has run, in words ("for 3 hours").
fn since_words(row: &process_registry::Row) -> String {
    let secs = row.running_for.as_secs();
    if secs >= 2 * 86_400 {
        i18n::tr!("left_behind.for_days", n = secs / 86_400)
    } else if secs >= 3_600 {
        i18n::tr!("left_behind.for_hours", n = secs / 3_600)
    } else if secs >= 60 {
        i18n::tr!("left_behind.for_minutes", n = secs / 60)
    } else {
        i18n::tr!("left_behind.for_seconds", n = secs)
    }
}

/// The question's text about `row`: what it is, its project, how long it has run, whether
/// it works now; then that it can only be stopped or left.
pub fn message(row: &process_registry::Row) -> String {
    let (what, since) = (kind_words(row), since_words(row));
    let state = if row.working {
        i18n::tr!("left_behind.working", cpu = format!("{:.0}", row.cpu))
    } else {
        i18n::t("left_behind.idle")
    };
    let about = match &row.project {
        Some(project) => i18n::tr!(
            "left_behind.about_project",
            program = format!("**{}**", row.program),
            what = what,
            project = format!("**{project}**"),
            since = since,
            state = state,
        ),
        None => i18n::tr!(
            "left_behind.about",
            program = format!("**{}**", row.program),
            what = what,
            since = since,
            state = state,
        ),
    };
    format!("{about}\n\n{}", i18n::t("left_behind.explain"))
}

#[cfg(test)]
#[path = "../tests/unit/left_behind.rs"]
mod tests;
