//! The app's log (plan 89): every event one line of JSON (`ts`, `level`, `target`,
//! `thread`, `run`, `msg`, `fields`, `span`), written by a thread of its own into a file a
//! day in the log folder, turned over past a size and let go after some days or past a
//! total (`files`). Made on the thread that logs (`layer`), handed over on a bounded
//! channel: nobody waits for the disk. A panic is logged (its message, place, thread and
//! stack) and written before the process goes on to end. The console, when there is one,
//! gets the same lines.

pub mod files;
pub mod layer;
pub mod line;
pub mod redact;
pub mod writer;

use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

pub use files::Limits;
pub use layer::JsonLayer;

/// Lines waiting for the writer at most (more: dropped and counted).
pub const QUEUE: usize = 8192;

/// How long [`flush`] waits for the writer at most.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// Where and how the log is written.
#[derive(Debug, Clone)]
pub struct Options {
    /// The folder of the files (`None`: no file — the log is off on disk).
    pub dir: Option<PathBuf>,
    /// The lines on the console too (the app started from a terminal).
    pub console: bool,
    pub limits: Limits,
}

/// The folder the lines go to now (`None`: none on disk), for whoever reads them.
static FOLDER: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The folder the log is written to now (`None`: the log is off on disk).
pub fn folder() -> Option<PathBuf> {
    FOLDER.lock().ok().and_then(|f| f.clone())
}

fn set_folder(dir: Option<PathBuf>) {
    if let Ok(mut folder) = FOLDER.lock() {
        *folder = dir;
    }
}

/// The writer's channel, for [`flush`].
static WRITER: OnceLock<Mutex<SyncSender<writer::Msg>>> = OnceLock::new();

/// This run's id (a UUID v4's first 12 hex digits): every line says it.
pub fn run_id() -> &'static str {
    static RUN: OnceLock<String> = OnceLock::new();
    RUN.get_or_init(|| uuid::Uuid::new_v4().simple().to_string()[..12].to_owned())
}

/// The writer started (`log-writer`) as `options` say: the layer that feeds it, for the
/// app's subscriber. `None` if the thread couldn't start.
pub fn start(options: &Options) -> Option<JsonLayer> {
    let (tx, rx) = mpsc::sync_channel(QUEUE);
    let sinks = writer::Sinks {
        dir: options.dir.clone(),
        console: options.console,
        limits: options.limits,
        run: run_id().to_owned(),
    };
    std::thread::Builder::new()
        .name("log-writer".into())
        .spawn(move || writer::run(&rx, &sinks))
        .ok()?;
    let _ = WRITER.set(Mutex::new(tx.clone()));
    set_folder(options.dir.clone());
    Some(JsonLayer::new(tx, run_id().to_owned()))
}

/// The app's subscriber installed: `filter` (as `RUST_LOG` writes it: `info,database=debug`)
/// in front of the JSON layer, both as `options` say. The filter can be changed later
/// ([`set_filter`]), the folder too ([`set_dir`]). `Err`: the filter can't be read (the
/// default `info` used), or a subscriber was already there.
pub fn install(options: &Options, filter: &str) -> Result<(), String> {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    let (parsed, error) = match tracing_subscriber::EnvFilter::try_new(filter) {
        Ok(parsed) => (parsed, None),
        Err(e) => (tracing_subscriber::EnvFilter::new("info"), Some(format!("{filter}: {e}"))),
    };
    let (filter_layer, handle) = tracing_subscriber::reload::Layer::new(parsed);
    let layer = start(options);
    tracing_subscriber::registry()
        .with(filter_layer)
        .with(layer)
        .try_init()
        .map_err(|e| e.to_string())?;
    let _ = FILTER.set(Box::new(move |spec: &str| {
        let parsed = tracing_subscriber::EnvFilter::try_new(spec).map_err(|e| e.to_string())?;
        handle.reload(parsed).map_err(|e| e.to_string())
    }));
    error.map_or(Ok(()), Err)
}

/// Changes the filter: what's logged from now (`spec` as `RUST_LOG` writes it). `Err`: it
/// can't be read (the one before kept), or nothing was installed.
pub fn set_filter(spec: &str) -> Result<(), String> {
    match FILTER.get() {
        Some(set) => set(spec),
        None => Err("no log installed".into()),
    }
}

/// Whether `spec` reads as a filter (`info`, `database=debug,warn`…): why not, when not.
pub fn check_filter(spec: &str) -> Result<(), String> {
    tracing_subscriber::EnvFilter::try_new(spec).map(|_| ()).map_err(|e| e.to_string())
}

/// The lines go into `dir` from now (`None`: none on disk).
pub fn set_dir(dir: Option<PathBuf>) {
    set_folder(dir.clone());
    if let Some(writer) = WRITER.get()
        && let Ok(tx) = writer.lock()
    {
        let _ = tx.send(writer::Msg::Dir(dir));
    }
}

/// What changes the installed filter ([`install`]).
type SetFilter = Box<dyn Fn(&str) -> Result<(), String> + Send + Sync>;
static FILTER: OnceLock<SetFilter> = OnceLock::new();

/// Everything logged so far written to disk (at most a second): before the process ends.
pub fn flush() {
    let Some(writer) = WRITER.get() else { return };
    let (done, wait) = mpsc::sync_channel(1);
    let sent = writer.lock().is_ok_and(|tx| tx.send(writer::Msg::Flush(done)).is_ok());
    if sent {
        let _ = wait.recv_timeout(FLUSH_WAIT);
    }
}

/// A panic logged — its message, place, thread and stack — and written before the hook
/// that was there runs (the app's own, then the default one).
pub fn log_panics() {
    let before = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        let place =
            info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let stack = std::backtrace::Backtrace::force_capture().to_string();
        tracing::error!(target: "panic", %message, %place, %stack, "the app panicked");
        flush();
        before(info);
    }));
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
