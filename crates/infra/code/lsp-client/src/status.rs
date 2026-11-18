//! What a running server is doing and what it costs, kept as it goes (by its driver) and
//! read by whoever shows it (the Task Manager, the settings): its state, its process, its
//! documents, its requests per minute and their latency, its last error, its log.

use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// The log lines kept (stderr, the server's messages, the traffic when it's followed).
pub const LOG_LINES: usize = 2000;
/// A log file's size before it's moved aside (`<name>.1.log`, the one before replaced) and
/// started again.
pub const LOG_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// The latencies kept for the percentiles.
const LATENCIES: usize = 256;

/// Where a server is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum State {
    /// Started, not ready yet (its `initialize` not answered).
    #[default]
    Starting,
    /// Ready (maybe working on something: [`Snapshot::progress`]).
    Ready,
    /// Ready, without any document open (it stops after a while).
    Idle,
    /// It crashed and starts again at that time.
    Restarting,
    /// Stopped: asked to, unused for long, or after crashing too often (`crashed`).
    Stopped { crashed: bool },
    /// It couldn't be started (why).
    Failed(String),
}

/// What a server can answer (from its `initialize` answer).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// It answers `textDocument/diagnostic` (it may push none).
    pub pull_diagnostics: bool,
    pub completion: bool,
    pub resolve: bool,
    pub hover: bool,
    pub signature_help: bool,
    pub definition: bool,
    pub references: bool,
    pub rename: bool,
    pub prepare_rename: bool,
    pub highlights: bool,
    pub symbols: bool,
    pub formatting: bool,
    pub range_formatting: bool,
    pub code_actions: bool,
    pub code_action_resolve: bool,
    pub implementation: bool,
    pub type_definition: bool,
    pub type_hierarchy: bool,
    pub inlay_hints: bool,
    pub code_lens: bool,
    pub code_lens_resolve: bool,
    pub document_links: bool,
    /// The characters after which it suggests, besides a name's.
    pub completion_triggers: String,
    pub signature_triggers: String,
}

/// A feature asked of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    Completion,
    Hover,
    SignatureHelp,
    Definition,
    References,
    Rename,
    Highlights,
    Symbols,
    Formatting,
    CodeActions,
    Implementation,
    TypeDefinition,
    TypeHierarchy,
    InlayHints,
    CodeLens,
    DocumentLinks,
}

impl Capabilities {
    pub fn supports(&self, feature: Feature) -> bool {
        match feature {
            Feature::Completion => self.completion,
            Feature::Hover => self.hover,
            Feature::SignatureHelp => self.signature_help,
            Feature::Definition => self.definition,
            Feature::References => self.references,
            Feature::Rename => self.rename,
            Feature::Highlights => self.highlights,
            Feature::Symbols => self.symbols,
            Feature::Formatting => self.formatting || self.range_formatting,
            Feature::CodeActions => self.code_actions,
            Feature::Implementation => self.implementation,
            Feature::TypeDefinition => self.type_definition,
            Feature::TypeHierarchy => self.type_hierarchy,
            Feature::InlayHints => self.inlay_hints,
            Feature::CodeLens => self.code_lens,
            Feature::DocumentLinks => self.document_links,
        }
    }
}

/// A server's figures at a moment.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub state: State,
    pub pid: Option<u32>,
    /// How long its process has run.
    pub uptime: Option<Duration>,
    pub documents: usize,
    /// The requests sent in the last minute.
    pub per_minute: usize,
    /// The requests' latency, the median and the 95th percentile.
    pub p50: Option<Duration>,
    pub p95: Option<Duration>,
    pub last_error: Option<String>,
    /// What it's working on (`Indexing`, its percentage), if anything.
    pub progress: Option<(String, Option<u32>)>,
    /// How many times it crashed.
    pub crashes: usize,
    pub capabilities: Capabilities,
}

#[derive(Debug, Default)]
struct Inner {
    state: State,
    pid: Option<u32>,
    started: Option<Instant>,
    documents: usize,
    requests: VecDeque<Instant>,
    latencies: VecDeque<Duration>,
    last_error: Option<String>,
    progress: HashMap<String, (String, Option<u32>)>,
    crashes: usize,
    capabilities: Capabilities,
    log: VecDeque<String>,
    /// Bumped on each change of the log (whoever shows it reads it again).
    log_generation: u64,
    /// The log kept on disk too, if it is.
    file: Option<LogFile>,
    /// Secrets it was given (a licence key): never written in its log, in memory or on disk.
    redacted: Vec<kernel::Secret>,
}

/// What stands for a secret in a log.
pub const REDACTED: &str = "‹redacted›";

/// `line` with every one of `secrets` replaced by [`REDACTED`].
fn redact(line: &str, secrets: &[kernel::Secret]) -> String {
    secrets
        .iter()
        .filter(|s| !s.expose().is_empty())
        .fold(line.to_owned(), |line, s| line.replace(s.expose(), REDACTED))
}

/// A log on disk: its lines with their time (UTC), appended.
#[derive(Debug)]
struct LogFile {
    path: PathBuf,
    /// `None` once it couldn't be written (not tried again: the log in memory stays).
    file: Option<std::fs::File>,
    size: u64,
}

impl LogFile {
    fn open(path: &Path) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        let size = file.metadata().map_or(0, |m| m.len());
        Ok(Self { path: path.to_owned(), file: Some(file), size })
    }

    fn write(&mut self, stamp: &str, line: &str) {
        if self.size >= LOG_FILE_BYTES {
            self.file = None;
            let aside = self.path.with_extension("1.log");
            let _ = std::fs::rename(&self.path, aside);
            match Self::open(&self.path) {
                Ok(fresh) => *self = fresh,
                Err(e) => tracing::warn!("a language server's log {}: {e}", self.path.display()),
            }
        }
        let Some(file) = self.file.as_mut() else { return };
        let entry = format!("{stamp} {line}\n");
        match file.write_all(entry.as_bytes()) {
            Ok(()) => self.size += entry.len() as u64,
            Err(e) => {
                tracing::warn!("a language server's log {}: {e}", self.path.display());
                self.file = None;
            }
        }
    }
}

/// `now` as `2026-11-02 09:30:37.485` (UTC).
fn stamp(now: SystemTime) -> String {
    let since = now.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Days since the epoch to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}.{:03}",
        rest / 3600,
        rest / 60 % 60,
        rest % 60,
        since.subsec_millis()
    )
}

/// A server's figures, written by its driver.
#[derive(Debug, Default)]
pub struct Status(Mutex<Inner>);

impl Status {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        let mut inner = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&mut inner)
    }

    pub fn set_state(&self, state: State) {
        self.with(|i| {
            if !matches!(state, State::Ready | State::Idle) {
                i.progress.clear();
            }
            i.state = state;
        });
    }

    pub fn state(&self) -> State {
        self.with(|i| i.state.clone())
    }

    pub fn set_process(&self, pid: Option<u32>, started: Option<Instant>) {
        self.with(|i| {
            i.pid = pid;
            i.started = started;
        });
    }

    pub fn set_documents(&self, count: usize) {
        self.with(|i| i.documents = count);
    }

    pub fn set_capabilities(&self, capabilities: Capabilities) {
        self.with(|i| i.capabilities = capabilities);
    }

    pub fn capabilities(&self) -> Capabilities {
        self.with(|i| i.capabilities.clone())
    }

    /// A feature it answers now (ready, and its capabilities say so).
    pub fn answers(&self, feature: Feature) -> bool {
        self.with(|i| {
            matches!(i.state, State::Ready | State::Idle) && i.capabilities.supports(feature)
        })
    }

    pub fn crashed(&self) {
        self.with(|i| i.crashes += 1);
    }

    /// A request sent at `at`.
    pub fn requested(&self, at: Instant) {
        self.with(|i| {
            i.requests.push_back(at);
            while i
                .requests
                .front()
                .is_some_and(|t| at.duration_since(*t) > Duration::from_secs(60))
            {
                i.requests.pop_front();
            }
        });
    }

    /// A request answered after `took`.
    pub fn answered(&self, took: Duration) {
        self.with(|i| {
            i.latencies.push_back(took);
            if i.latencies.len() > LATENCIES {
                i.latencies.pop_front();
            }
        });
    }

    pub fn error(&self, message: impl Into<String>) {
        let message = message.into();
        self.log(format!("error: {message}"));
        self.with(|i| i.last_error = Some(redact(&message, &i.redacted)));
    }

    /// `text` with the secrets it was given replaced by [`REDACTED`].
    pub fn redacted(&self, text: &str) -> String {
        self.with(
            |i| if i.redacted.is_empty() { text.to_owned() } else { redact(text, &i.redacted) },
        )
    }

    /// `secrets` never appear in its log from now on (they're replaced by [`REDACTED`]).
    pub fn redact(&self, secrets: Vec<kernel::Secret>) {
        self.with(|i| i.redacted = secrets);
    }

    /// Work `token` going on (`None`: ended).
    pub fn progress(&self, token: &str, work: Option<(String, Option<u32>)>) {
        self.with(|i| match work {
            Some(work) => {
                i.progress.insert(token.to_owned(), work);
            }
            None => {
                i.progress.remove(token);
            }
        });
    }

    /// The log is kept in `path` too from now on (what it holds already written first).
    pub fn keep_log_in(&self, path: &Path) {
        let mut file = match LogFile::open(path) {
            Ok(file) => file,
            Err(e) => {
                tracing::warn!("a language server's log {}: {e}", path.display());
                return;
            }
        };
        let now = stamp(SystemTime::now());
        self.with(|i| {
            for line in &i.log {
                file.write(&now, line);
            }
            i.file = Some(file);
        });
    }

    pub fn log(&self, line: impl Into<String>) {
        let line = line.into();
        let now = stamp(SystemTime::now());
        self.with(|i| {
            let line = if i.redacted.is_empty() { line } else { redact(&line, &i.redacted) };
            for line in line.lines() {
                if let Some(file) = i.file.as_mut() {
                    file.write(&now, line);
                }
                i.log.push_back(line.to_owned());
            }
            while i.log.len() > LOG_LINES {
                i.log.pop_front();
            }
            i.log_generation += 1;
        });
    }

    /// The log's lines and its generation.
    pub fn log_lines(&self) -> (Vec<String>, u64) {
        self.with(|i| (i.log.iter().cloned().collect(), i.log_generation))
    }

    /// The log's lines from `window(len)`'s first, as many as it says (what a view shows of
    /// it: the others aren't copied), how many there are and its generation.
    pub fn log_window(
        &self,
        window: impl FnOnce(usize) -> (usize, usize),
    ) -> (Vec<String>, usize, u64) {
        self.with(|i| {
            let len = i.log.len();
            let (first, count) = window(len);
            let lines = i.log.iter().skip(first).take(count).cloned().collect();
            (lines, len, i.log_generation)
        })
    }

    pub fn log_generation(&self) -> u64 {
        self.with(|i| i.log_generation)
    }

    /// Its figures now.
    pub fn snapshot(&self, now: Instant) -> Snapshot {
        self.with(|i| {
            let mut sorted: Vec<Duration> = i.latencies.iter().copied().collect();
            sorted.sort();
            let percentile =
                |p: usize| (!sorted.is_empty()).then(|| sorted[(sorted.len() - 1) * p / 100]);
            let per_minute = i
                .requests
                .iter()
                .filter(|t| now.duration_since(**t) <= Duration::from_secs(60))
                .count();
            let progress = i.progress.values().max_by_key(|(_, p)| p.unwrap_or(0)).cloned();
            Snapshot {
                state: i.state.clone(),
                pid: i.pid,
                uptime: i.started.map(|s| now.saturating_duration_since(s)),
                documents: i.documents,
                per_minute,
                p50: percentile(50),
                p95: percentile(95),
                last_error: i.last_error.clone(),
                progress,
                crashes: i.crashes,
                capabilities: i.capabilities.clone(),
            }
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/status.rs"]
mod tests;
