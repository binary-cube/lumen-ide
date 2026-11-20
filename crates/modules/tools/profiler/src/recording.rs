//! The recording under way, one for the whole app: started from the consent window, stopped
//! by its button, its command or its own time; its file taken once written (by the module's
//! poll, back on the interface's thread). Whose each thread is comes from the app's resources
//! (each part's owner — a module's id or `app` — and its threads' names: never a part's
//! name, which may say a project's), read at the start and at the end only; the GPU's memory
//! from them at most once a second.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use profiling::Owners;
use workbench_model::ResourceSource;

/// The lengths offered (seconds), the default's place, and their labels' keys.
pub const LENGTHS: [(u64, &str); 5] = [
    (60, "profiler.length_1m"),
    (300, "profiler.length_5m"),
    (900, "profiler.length_15m"),
    (1800, "profiler.length_30m"),
    (3600, "profiler.length_1h"),
];
pub const DEFAULT_LENGTH: usize = 1;

/// How often it samples.
pub const INTERVAL: Duration = Duration::from_millis(100);

/// The resources (the GPU's memory, what the parts moved) and the modules' own figures read
/// at most this often.
const READ_EVERY: Duration = Duration::from_secs(1);

/// A window's waker, by a number of its own.
type Wake = (u64, Arc<dyn Fn() + Send + Sync>);

#[derive(Default)]
struct State {
    recorder: Option<profiling_collect::Recorder>,
    /// What wakes each open window (its time left, the file written), until it closes.
    wakes: Vec<Wake>,
    next_wake: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = STATE.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(State::default))
}

/// Counts the recordings written and deleted (a list follows it to read its folder again).
static WRITTEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many recordings were written or deleted so far.
pub fn written() -> u64 {
    WRITTEN.load(std::sync::atomic::Ordering::Relaxed)
}

/// A recording was written or deleted.
pub fn touched() {
    WRITTEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

/// Where the recordings are kept: the app's data folder (never a project's).
pub fn folder() -> Option<PathBuf> {
    storage::data_dir().map(|d| d.join("profiles"))
}

/// A window's waker kept while it's open (the subscription returned lets it go).
pub fn add_wake(wake: Arc<dyn Fn() + Send + Sync>) -> kernel::Subscription {
    let id = with(|s| {
        s.next_wake += 1;
        s.wakes.push((s.next_wake, wake));
        s.next_wake
    });
    kernel::Subscription::on_end(move || with(|s| s.wakes.retain(|(w, _)| *w != id)))
}

/// Every open window woken (each looks at the recording at its next poll).
fn wake_all() {
    let wakes: Vec<Arc<dyn Fn() + Send + Sync>> =
        with(|s| s.wakes.iter().map(|(_, w)| Arc::clone(w)).collect());
    for wake in wakes {
        wake();
    }
}

/// A recording is under way.
pub fn recording() -> bool {
    with(|s| s.recorder.is_some())
}

/// How long it has run and has left (`None`: none runs).
pub fn progress() -> Option<(Duration, Duration)> {
    with(|s| s.recorder.as_ref().map(|r| (r.elapsed(), r.remaining())))
}

/// A recording of `length` started, whose threads are named by `resources` (when given).
pub fn start(length: Duration, resources: Option<Arc<dyn ResourceSource>>) -> Result<(), String> {
    let dir = folder().ok_or_else(|| i18n::t("profiler.no_folder"))?;
    // One at a time: refused before anything (the sources of the one running stay told).
    if recording() {
        return Err(i18n::t("profiler.already"));
    }
    let owned = resources.clone();
    let owners = Arc::new(move || owners_of(owned.as_deref()));
    // What draws the window and at what scale (the app knows them), read once.
    let looked = resources.as_deref().map(ResourceSource::snapshot_all).unwrap_or_default();
    let (adapter, driver) = adapter_and_driver(looked.renderer.as_deref().unwrap_or(""));
    let scale = looked.scale_percent.map_or(0.0, |p| p as f32 / 100.0);
    let (gpu, io) = readers(resources);
    // The modules' own figures: told it begins, read once a second, told it ended.
    let sources = contracts::profiling::sources();
    for source in &sources {
        source.started();
    }
    let ended = sources.clone();
    let stopped_if_refused = sources.clone();
    let plan = profiling_collect::Plan {
        dir,
        length,
        interval: INTERVAL,
        gpu: adapter,
        gpu_driver: driver,
        scale,
        app_version: kernel::brand::version().to_owned(),
        build: if cfg!(debug_assertions) { "debug" } else { "release" }.to_owned(),
        owners,
        gpu_memory: gpu,
        threads: Arc::new(|| {
            process_stats::threads().into_iter().map(|t| (t.id, t.name, t.cpu)).collect()
        }),
        memory: Arc::new(process_stats::footprint),
        io,
        disk: Arc::new(process_stats::disk_io),
        readings: module_readings(sources),
        tick: Arc::new(wake_all),
        done: Arc::new(move || {
            for source in &ended {
                source.stopped();
            }
            wake_all();
        }),
    };
    let recorder = match profiling_collect::start(plan) {
        Ok(recorder) => recorder,
        Err(why) => {
            // Told it began: told it's over.
            for source in &stopped_if_refused {
                source.stopped();
            }
            return Err(why);
        }
    };
    tracing::info!(seconds = length.as_secs(), "a profile recording started");
    with(|s| s.recorder = Some(recorder));
    Ok(())
}

/// The recording ends now (its file written on its thread).
pub fn stop() {
    with(|s| {
        if let Some(r) = &s.recorder {
            r.stop();
        }
    });
}

/// Its file, once written (or why not): the recording is over then.
pub fn take_finished() -> Option<Result<PathBuf, String>> {
    with(|s| {
        let done = s.recorder.as_ref()?.finished()?;
        s.recorder = None;
        touched();
        Some(done)
    })
}

/// The modules' own figures (`contracts::profiling`), read at most once a second: counts,
/// bytes and time as counters in their units, what they hold as levels. Their names are
/// cleaned like every name kept.
#[allow(clippy::type_complexity)]
pub(crate) fn module_readings(
    sources: Vec<Arc<dyn contracts::profiling::ProfileSource>>,
) -> Arc<dyn Fn() -> (Vec<profiling::Counter>, Vec<profiling::Level>) + Send + Sync> {
    use contracts::profiling::ReadingKind;
    let last: Mutex<Option<Instant>> = Mutex::new(None);
    Arc::new(move || {
        let mut at = last.lock().unwrap_or_else(PoisonError::into_inner);
        if at.is_some_and(|t| t.elapsed() < READ_EVERY) {
            return (Vec::new(), Vec::new());
        }
        *at = Some(Instant::now());
        drop(at);
        let (mut counters, mut levels) = (Vec::new(), Vec::new());
        for source in &sources {
            let owner = profiling::name(source.owner());
            for r in source.read() {
                let what = profiling::name(r.what);
                match r.kind {
                    ReadingKind::Level => levels.push(profiling::Level {
                        owner: owner.clone(),
                        what,
                        peak: r.value,
                        last: r.value,
                    }),
                    kind => counters.push(profiling::Counter {
                        unit: kind.unit().to_owned(),
                        ..profiling::Counter::new(owner.clone(), what, r.value)
                    }),
                }
            }
        }
        (counters, levels)
    })
}

/// The graphics adapter and the driver out of the renderer's description (`wgpu / Metal /
/// Apple M3`: `Apple M3`, `wgpu / Metal`).
pub(crate) fn adapter_and_driver(description: &str) -> (String, String) {
    match description.rsplit_once(" / ") {
        Some((driver, adapter)) => (adapter.trim().to_owned(), driver.trim().to_owned()),
        None => (description.trim().to_owned(), String::new()),
    }
}

/// Whose each thread is: each part's owner for its threads' names (a part's name is never
/// read: it may say a project's).
pub(crate) fn owners_of(resources: Option<&dyn ResourceSource>) -> Owners {
    let Some(resources) = resources else { return Owners::default() };
    let mut owners: Vec<(String, String)> = Vec::new();
    for part in resources.snapshot_all().parts {
        for thread in &part.threads {
            if !owners.iter().any(|(t, _)| t == thread) {
                owners.push(((*thread).to_owned(), profiling::name(&part.owner)));
            }
        }
    }
    Owners(owners)
}

/// What a look at the resources gives the recording: the GPU's memory, and what each
/// part moved so far (by its owner only).
#[derive(Clone, Default)]
struct Look {
    gpu: Option<u64>,
    io: Vec<profiling::Io>,
}

/// The resources looked at, at most once a second (shared by the GPU's reader and the
/// network's and disk's).
type Looks = Arc<Mutex<Option<(Instant, Look)>>>;

fn look(resources: &dyn ResourceSource, kept: &Looks) -> Look {
    // The lock only moves data: the resources are read with it let go.
    if let Some((at, look)) = kept.lock().unwrap_or_else(PoisonError::into_inner).as_ref()
        && at.elapsed() < READ_EVERY
    {
        return look.clone();
    }
    let snapshot = resources.snapshot_all();
    let mut io: Vec<profiling::Io> = Vec::new();
    for part in &snapshot.parts {
        let moved = profiling::Io {
            owner: profiling::name(&part.owner),
            sent: part.sent.unwrap_or(0),
            received: part.received.unwrap_or(0),
            read: part.read.unwrap_or(0),
            written: part.written.unwrap_or(0),
        };
        if moved.is_empty() {
            continue;
        }
        match io.iter_mut().find(|i| i.owner == moved.owner) {
            Some(i) => {
                i.sent = i.sent.saturating_add(moved.sent);
                i.received = i.received.saturating_add(moved.received);
                i.read = i.read.saturating_add(moved.read);
                i.written = i.written.saturating_add(moved.written);
            }
            None => io.push(moved),
        }
    }
    let look = Look { gpu: snapshot.gpu, io };
    *kept.lock().unwrap_or_else(PoisonError::into_inner) = Some((Instant::now(), look.clone()));
    look
}

/// The GPU's reader and the network's and disk's, from `resources` (none without them).
#[allow(clippy::type_complexity)]
fn readers(
    resources: Option<Arc<dyn ResourceSource>>,
) -> (Arc<dyn Fn() -> Option<u64> + Send + Sync>, Arc<dyn Fn() -> Vec<profiling::Io> + Send + Sync>)
{
    let kept: Looks = Arc::new(Mutex::new(None));
    let (r, k) = (resources.clone(), Arc::clone(&kept));
    let gpu = Arc::new(move || r.as_deref().and_then(|r| look(r, &k).gpu));
    let io = Arc::new(move || resources.as_deref().map(|r| look(r, &kept).io).unwrap_or_default());
    (gpu, io)
}
