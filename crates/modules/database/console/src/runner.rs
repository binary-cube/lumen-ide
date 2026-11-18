//! A console's runner: its own connection to its source (a transaction of its own, never
//! the grid's), made at the first run and nothing before, used by one job at a time on the
//! pool `db-console`: the statements of a run one after the other (stopping at the first
//! error when asked), a plan explained, the databases and schemas listed, a commit or a
//! rollback, a result's next page or its rows read again sorted and filtered, a result
//! exported. Its state ([`Snapshot`]) is what the bar draws: what runs, how the last run
//! ended, the gutter's marks, whether a transaction is open; each change bumps its
//! generation and wakes the app.
//!
//! What a run brings is kept here ([`crate::results`]): each result set as it comes (a tab
//! of the Services window), its rows in chunks; the next run replaces the unpinned ones. The
//! rest of a result is read from the session's cursor where it keeps one, else by running
//! a pure query again from the next row — a statement that wrote is never run again. The
//! Output says what happened, in the user's language, its errors linked to their place in
//! the console (and underlined there, through the project's diagnostics); the project's bus
//! hears `DataChanged` for the tables a statement changed (their open
//! grids read again). A long run ending while the Services window is hidden is told in a
//! toast.
//!
//! The connection is the bars' ([`Lease`]): the last one gone (the console's tabs closed), it
//! closes, cancelling what runs (a transaction left open is rolled back by the engine as the
//! connection goes); unused for the source's idle time it closes too — never while a
//! transaction is open — and the next run opens it again.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant};

use database::console::{Connector, ConsoleId, PlannedStatement, TxMode, TxState};
use database::profile::{ProfileId, ProfileNote, ProfileOptions, ProfileReport};
use database::run::CursorToken;
use database::sql::{self, StatementKind, TextPosition};
use database::transaction::{self, Script};
use database::{
    Applied, Cancel, Column, DataSourceId, DbError, Dialect, Flow, Isolation, Place, PlanNode,
    RowKey, RowSink, RunOptions, Session, TableRef, TxOptions, Value,
};
use kernel::{EventBus, ProjectId};
use ui_components::data_grid::GridColumn;
use ui_components::data_grid::export::{
    ExportFormat, ExportOutcome, ExportSink, ExportWriter, Target,
};
use ui_components::editor_toolbar::{MarkState, StatementMark, text_hash};
use ui_components::{Log, LogKind, LogLine, ServiceId, ViewerAction};
use workbench_model::{Notification, Notifier};

use crate::results::{
    Editability, More, PlanSet, ResultSet, Results, ServerView, WholeValue, cell_ref,
};
use crate::runtime::Runtime;

use self::sinks::*;
pub use self::text::*;

mod profile_work;
mod profiles;
mod result_work;
mod results;
mod sinks;
mod text;
mod worker;

/// The diagnostics' source (the console's errors, underlined in its text).
pub const DIAGNOSTICS: &str = "sql-console";

/// A run lasting this long, ended while the Services window is hidden, is told in a toast.
pub const LONG_RUN: Duration = Duration::from_secs(10);

/// A transaction open this long is told once (it may hold locks others wait for).
pub const LONG_TRANSACTION: Duration = Duration::from_secs(60 * 60);

/// Rows an export writes at a time.
const EXPORT_BLOCK: usize = 2000;

/// How a run goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    /// Rows of each result brought at first.
    pub fetch_limit: u32,
    /// Stop at the first statement that fails.
    pub stop_on_error: bool,
    pub tx: TxMode,
    pub isolation: Option<Isolation>,
    /// The schema unqualified names are looked for in (`None`: the connection's).
    pub schema: Option<String>,
    /// Whose SQL it is (the passwords hidden in the Output by its rules).
    pub flavor: sql::Flavor,
    /// The most bytes the console's results hold together.
    pub result_memory: usize,
}

impl Default for RunSettings {
    fn default() -> Self {
        Self {
            fetch_limit: database::run::FETCH_LIMIT,
            stop_on_error: true,
            tx: TxMode::Auto,
            isolation: None,
            schema: None,
            flavor: sql::Flavor::Generic,
            result_memory: 64 << 20,
        }
    }
}

/// What a run is given: its statements (their places in `document`), how it goes.
#[derive(Clone)]
pub struct RunRequest {
    pub statements: Vec<PlannedStatement>,
    /// The document's text as it was when they were planned (errors placed in it).
    pub document: Arc<str>,
    pub settings: RunSettings,
    /// Connects, when it isn't.
    pub connector: Connector,
    /// The source's name (`Connected to …`).
    pub source_name: String,
    /// Its statements aren't the console's text (a statement of the history run again): no
    /// marks in the gutter, no underline, no link to a place in the console.
    pub detached: bool,
}

impl std::fmt::Debug for RunRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunRequest").field("statements", &self.statements).finish_non_exhaustive()
    }
}

/// Where the runner tells what it did: the project (whose Services window may be hidden), its
/// bus, the console's file (its errors' links and underlines) and the user.
#[derive(Clone)]
pub struct RunnerContext {
    pub project: Option<ProjectId>,
    pub events: Option<EventBus>,
    /// The console's file (absolute), and relative to the project's root.
    pub file: Option<PathBuf>,
    pub relative: Option<PathBuf>,
    /// The console's name (a toast says it).
    pub name: String,
    pub notifier: Arc<dyn Notifier>,
    /// Where each statement run is recorded (the project's histories).
    pub history: Option<Arc<crate::history::Histories>>,
}

impl Default for RunnerContext {
    fn default() -> Self {
        Self {
            project: None,
            events: None,
            file: None,
            relative: None,
            name: String::new(),
            notifier: Arc::new(workbench_model::NoNotifier),
            history: None,
        }
    }
}

impl std::fmt::Debug for RunnerContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunnerContext").field("file", &self.file).finish_non_exhaustive()
    }
}

/// How the last run (or explain) ended, as the bar says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastRun {
    pub ok: bool,
    /// Statements run, of how many asked.
    pub done: usize,
    pub total: usize,
    /// The last statement's rows brought (`None`: it brought no result).
    pub rows: Option<u64>,
    /// More rows were left than the first page.
    pub truncated: bool,
    /// Rows the last statement changed, when the engine said.
    pub affected: Option<u64>,
    pub elapsed: Duration,
    /// Why it failed, as told (with its place: line and column, from 1).
    pub error: Option<String>,
    pub cancelled: bool,
    /// An explain's steps (a plan was read).
    pub plan_steps: Option<usize>,
}

/// What's running now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// The statement running (from 0), of how many.
    pub index: usize,
    pub total: usize,
    pub since: Instant,
}

/// The databases and schemas the connection can switch to (the bar's switcher).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Places {
    pub databases: Vec<String>,
    pub schemas: Vec<String>,
}

/// A runner's state, as the bar draws it.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub running: Option<Progress>,
    /// A job is queued or working (a run, a list, a commit).
    pub busy: bool,
    pub connected: bool,
    pub last: Option<LastRun>,
    pub in_transaction: bool,
    /// Where the transaction is (open since which statement, how many writes wait for
    /// Commit, failed); [`Snapshot::in_transaction`] is its being open.
    pub tx: TxState,
    /// Since when it's open.
    pub tx_since: Option<Instant>,
    /// The driver gives whole results, without positions (an old external driver).
    pub limited: bool,
    /// Where the connection is (its database and schema), once connected.
    pub place: Place,
    pub places: Option<Places>,
    pub marks: Vec<StatementMark>,
    /// Bumped whenever [`Snapshot::marks`] change.
    pub marks_generation: u64,
    /// The statement whose error is underlined in the text (its range and its text's hash),
    /// until its text changes.
    pub error_mark: Option<(Range<usize>, u64)>,
    /// What a profile's finding points at, lit in the console while its statement's text is
    /// the one profiled.
    pub focus: Option<FocusMark>,
    /// The rules whose findings this console's session doesn't show ("Ignore in this
    /// console", in memory only).
    pub ignored: Vec<String>,
}

/// A place of the console's text a profile's finding points at ([`Runner::set_focus`]):
/// lit while the statement profiled (its range, its text's hash) is still the same.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusMark {
    pub statement: Range<usize>,
    pub hash: u64,
    pub range: Range<usize>,
    /// The finding's severity (the color it's lit in).
    pub level: ui_components::Level,
}

/// A profile to show (its tab, its line in the Output): the report, the statement as sent
/// (its secrets masked) and its place in the console's text, how it was made.
#[derive(Debug, Clone)]
pub struct ProfileShown {
    pub report: Arc<ProfileReport>,
    pub title: String,
    /// The one table it queries.
    pub table: Option<String>,
    pub statement: String,
    pub range: Range<usize>,
    pub analyze: bool,
    pub rolled_back: bool,
    pub elapsed: Duration,
    /// The hash of the statement's text in the console when it was profiled.
    pub document_hash: Option<u64>,
    /// What was known of its tables (their structures, their statistics).
    pub facts: Option<Arc<database::profile::rules::Facts>>,
    /// The statement's fingerprint ([`sql::fingerprint`]).
    pub fingerprint: u64,
}

/// A result's tab, as the Services window lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultTab {
    pub id: ServiceId,
    pub title: String,
    pub pinned: bool,
    pub loading: bool,
    /// A plan (Explain), not rows.
    pub plan: bool,
}

/// What a view of a result reads: its rows' chunks (shared, never copied) and what's known
/// of the rest.
#[derive(Debug, Clone)]
pub struct ResultView {
    pub id: ServiceId,
    pub title: String,
    pub statement: String,
    pub columns: Vec<Column>,
    pub chunks: Vec<Arc<[crate::results::Row]>>,
    pub starts: Vec<u64>,
    /// The rows in the chunks.
    pub len: u64,
    pub more: More,
    pub rerunnable: bool,
    pub view: ServerView,
    pub execution: Duration,
    pub fetching: Duration,
    pub pinned: bool,
    pub dropped: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub cut: u64,
    /// Where its cut cells are (row, column).
    pub cut_at: Vec<(u64, u32)>,
    /// Changes when its rows are let go (read again: their places changed).
    pub epoch: u64,
    /// Why its query's rows can't be edited (`None`: they can, as far as the query goes).
    pub refused: Option<database::edits::NotEditable>,
    pub editing: Editability,
    pub submitting: bool,
    /// The whole values of its big cells read so far.
    pub wholes: Vec<WholeValue>,
    pub generation: u64,
}

/// What waits for the runner to be free (asked while it worked).
#[derive(Debug, Clone, PartialEq)]
enum Queued {
    Fetch(ServiceId),
    Requery(ServiceId, ServerView),
    /// A profile's tables' statistics read (its Tables view opened without them).
    TableStats(ProfileId),
    /// A result's table's structure read: whether its rows can be edited.
    Describe(ServiceId),
    /// A big cell's whole value read (its row's key, its column).
    Whole(ServiceId, RowKey, usize),
}

/// A result's changes to submit ([`Runner::submit`]): the statements shown in the preview,
/// exactly as they run, and how they go.
#[derive(Debug, Clone)]
pub struct SubmitJob {
    pub result: ServiceId,
    /// The transaction applying them, as shown.
    pub script: Script,
    /// Each step's kind (the console's transaction counts its writes).
    pub kinds: Vec<StatementKind>,
    /// The table they change, as the grids name it (its open grids told).
    pub table: TableRef,
    /// `Manual`: they open the console's transaction (or join the one open), never
    /// committed here.
    pub tx: TxMode,
    pub isolation: Option<Isolation>,
}

/// The savepoint a result's changes run under inside the console's open transaction.
pub const EDIT_SAVEPOINT: &str = "console_edit";

/// An export of a result running: its rows written so far, how it ended.
#[derive(Debug, Default)]
pub struct ExportState {
    pub rows: AtomicU64,
    pub outcome: Mutex<Option<ExportOutcome>>,
    pub cancel: Cancel,
}

impl ExportState {
    pub fn take_outcome(&self) -> Option<ExportOutcome> {
        self.outcome.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// What an export writes: which result, its columns (as shown), the format, where.
#[derive(Debug, Clone)]
pub struct ExportRequest {
    pub result: ServiceId,
    /// The columns, in the order shown (by name among the result's).
    pub columns: Vec<GridColumn>,
    pub view: ServerView,
    pub format: ExportFormat,
    /// How the source writes SQL (the SQL extractors'), and the columns they skip.
    pub sql: Option<Arc<dyn extractors::SqlStyle>>,
    pub options: extractors::Options,
    pub target: Target,
}

/// The queries the consoles run (every project's), for the Task Manager and the question
/// before closing: each run an operation, its "Stop" the console's own.
pub static QUERIES: workbench_model::Activities = workbench_model::Activities::new(Instant::now);

/// The most of a query's text kept on its operation (its tooltip).
const QUERY_TEXT: usize = 8 * 1024;
/// The most of its first line kept as the operation's subject.
const QUERY_SUBJECT: usize = 200;

/// A query's operation: its first line (cut), its whole text (cut too).
fn query_texts(statements: &[&str]) -> (String, String) {
    let cut = |s: &str, at: usize| match s.char_indices().nth(at) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_owned(),
    };
    let text = statements.join("\n");
    let first = text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default();
    (cut(first, QUERY_SUBJECT), cut(text.trim(), QUERY_TEXT))
}

struct State {
    snapshot: Snapshot,
    last_used: Instant,
    /// The connection (with what it was made for: the database chosen).
    connector: Option<Connector>,
    source_name: String,
    /// The leases are gone: the connection closes.
    closed: bool,
    /// How long it may stay unused before it's closed (the source's).
    idle: Option<Duration>,
    /// The schema the connection was last set to.
    schema: Option<String>,
    /// How the last run went (the next pages are read the same way).
    settings: RunSettings,
    /// The session's dialect (a query wrapped for its next page), once connected.
    dialect: Option<Arc<dyn Dialect>>,
    /// The tables written inside the transaction open, told once it's committed.
    uncommitted: Vec<(TableRef, bool)>,
    /// The console's diagnostics aren't empty.
    diagnosed: bool,
    /// The node and tab the Services window shows next (a run's last result, or its Output).
    reveal: Option<Option<ServiceId>>,
    /// The statements run on the connection so far (the transaction's start counted by it).
    executed: u64,
    /// What the user confirmed once for this console's session (a confirmation asked once,
    /// by its key).
    acknowledged: Vec<&'static str>,
    /// The open transaction's age was told.
    long_told: bool,
    /// What may have written inside the open transaction (masked), oldest first.
    tx_statements: Vec<String>,
    /// The parameters' values kept for this console's session (asked again with them; in
    /// memory only, never written anywhere: they may be personal data).
    params: Vec<crate::params::Remembered>,
    /// What the analysis of a profile knows of the source (its accepted keys, production).
    analysis: crate::profile::Analysis,
    /// The statements this session ran, by fingerprint (the analysis' N+1).
    recent: database::profile::rules::RecentRuns,
    queued: VecDeque<Queued>,
    context: RunnerContext,
    /// The tables its statements made (for the schema's diagnostics: its temporary ones).
    created: Vec<TableRef>,
    /// It attached another database (SQLite): an unqualified name may be that one's.
    attached: bool,
    /// The run's operation (a query running), ended with its job.
    activity: Option<u128>,
}

struct Inner {
    console: ConsoleId,
    source: DataSourceId,
    runtime: Runtime,
    state: Mutex<State>,
    /// The connection, while open; taken by the job using it.
    session: Mutex<Option<Box<dyn Session>>>,
    cancel: Mutex<Cancel>,
    generation: AtomicU64,
    log: Log,
    results: Mutex<Results>,
    /// Its profiles (Explain's, kept while the project is open).
    profiles: Mutex<crate::profile::Profiles>,
    /// Who it wakes when something changes (the bar's, the Services window's, a view's).
    wakes: Mutex<Vec<(&'static str, Waker)>>,
    leases: AtomicUsize,
    /// When it was made (the recent runs' times count from it).
    started: Instant,
}

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// A console's runner (cloning shares it).
#[derive(Clone)]
pub struct Runner {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Runner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runner").field("console", &self.inner.console).finish_non_exhaustive()
    }
}

/// The most statements a transaction's list keeps.
const HISTORY: usize = 50;

impl Runner {
    pub fn new(console: ConsoleId, source: DataSourceId, runtime: Runtime) -> Self {
        let now = runtime.now();
        Self {
            inner: Arc::new(Inner {
                console,
                source,
                runtime,
                state: Mutex::new(State {
                    snapshot: Snapshot::default(),
                    last_used: now,
                    connector: None,
                    source_name: String::new(),
                    closed: false,
                    idle: None,
                    schema: None,
                    settings: RunSettings::default(),
                    dialect: None,
                    uncommitted: Vec::new(),
                    diagnosed: false,
                    reveal: None,
                    executed: 0,
                    acknowledged: Vec::new(),
                    long_told: false,
                    tx_statements: Vec::new(),
                    params: Vec::new(),
                    analysis: crate::profile::Analysis::default(),
                    recent: database::profile::rules::RecentRuns::default(),
                    queued: VecDeque::new(),
                    created: Vec::new(),
                    attached: false,
                    context: RunnerContext::default(),
                    activity: None,
                }),
                session: Mutex::new(None),
                cancel: Mutex::new(Cancel::default()),
                generation: AtomicU64::new(0),
                log: Log::new(),
                results: Mutex::new(Results::default()),
                profiles: Mutex::default(),
                wakes: Mutex::default(),
                leases: AtomicUsize::new(0),
                started: now,
            }),
        }
    }

    pub fn console(&self) -> ConsoleId {
        self.inner.console
    }

    pub fn source(&self) -> DataSourceId {
        self.inner.source
    }

    /// The console's Output (what the Services window shows).
    pub fn log(&self) -> &Log {
        &self.inner.log
    }

    /// The app is woken by `wake` when something changes (the bar's).
    pub fn set_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        self.add_wake("bar", wake);
    }

    /// `wake` is called when something changes, as `who`'s (one per who: given again, it
    /// replaces the one before).
    pub fn add_wake(&self, who: &'static str, wake: Arc<dyn Fn() + Send + Sync>) {
        let mut wakes = self.inner.wakes.lock().unwrap_or_else(PoisonError::into_inner);
        wakes.retain(|(w, _)| *w != who);
        wakes.push((who, wake));
    }

    /// Where it tells what it did (the project's bus, the console's file, the user).
    pub fn set_context(&self, context: RunnerContext) {
        self.inner.state().context = context;
    }

    /// Its console's file renamed or moved (its name the file's): told from now with
    /// `context`, and drawn again (the bar's title, the Services window's node).
    pub fn renamed(&self, context: RunnerContext) {
        self.set_context(context);
        self.inner.changed();
    }

    /// The console's file (absolute), once told.
    pub fn file(&self) -> Option<PathBuf> {
        self.inner.state().context.file.clone()
    }

    /// The console's name, and its project, as told.
    pub fn name(&self) -> String {
        self.inner.state().context.name.clone()
    }

    pub fn project(&self) -> Option<ProjectId> {
        self.inner.state().context.project
    }

    /// The source's name it last connected with (empty before).
    pub fn source_name(&self) -> String {
        self.inner.state().source_name.clone()
    }

    /// How long it may stay unused before its connection closes (`None`: never for that).
    pub fn set_idle(&self, idle: Option<Duration>) {
        self.inner.state().idle = idle;
    }

    /// Where its work runs (a result's export of the rows here too).
    pub fn runtime(&self) -> Runtime {
        self.inner.runtime.clone()
    }

    /// Wakes the app (something of its results came: a local order worked out).
    pub fn wake_up(&self) {
        self.inner.wake();
    }

    /// Changes with each change of its state.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    pub fn snapshot(&self) -> Snapshot {
        self.inner.state().snapshot.clone()
    }

    /// A bar's use of the connection: the last one gone, it closes.
    pub fn lease(&self) -> Lease {
        self.inner.leases.fetch_add(1, Ordering::AcqRel);
        self.inner.state().closed = false;
        Lease { runner: Arc::downgrade(&self.inner) }
    }

    pub fn leases(&self) -> usize {
        self.inner.leases.load(Ordering::Acquire)
    }

    pub fn busy(&self) -> bool {
        self.inner.state().snapshot.busy
    }

    pub fn connected(&self) -> bool {
        self.inner.state().snapshot.connected
    }

    pub fn in_transaction(&self) -> bool {
        self.inner.state().snapshot.in_transaction
    }

    /// Where its transaction is.
    pub fn tx(&self) -> TxState {
        self.inner.state().snapshot.tx
    }

    /// The statements that may have written in the open transaction (what Commit makes
    /// permanent), oldest first.
    pub fn transaction_statements(&self) -> Vec<String> {
        self.inner.state().tx_statements.clone()
    }

    /// The user confirmed `what` already in this console's session (asked once).
    pub fn acknowledged(&self, what: &str) -> bool {
        self.inner.state().acknowledged.contains(&what)
    }

    /// `what` confirmed: not asked again while the project is open.
    pub fn acknowledge(&self, what: &'static str) {
        let mut state = self.inner.state();
        if !state.acknowledged.contains(&what) {
            state.acknowledged.push(what);
        }
    }

    /// A statement refused before it was sent (a read-only source's write): why, in the
    /// Output.
    pub fn refused(&self, why: String) {
        self.inner.log.push(self.inner.line(LogKind::Warning, why));
        self.inner.state().reveal = Some(None);
        self.inner.changed();
    }

    /// The parameters' values kept for this console (the last ones given, while the project
    /// is open).
    pub fn remembered_params(&self) -> Vec<crate::params::Remembered> {
        self.inner.state().params.clone()
    }

    /// The values given for a run kept (those asked to be) or forgotten (the others).
    pub fn remember_params(&self, entries: &[crate::params::ParamEntry]) {
        let mut state = self.inner.state();
        for entry in entries {
            state.params.retain(|r| r.name != entry.name);
            if entry.remember {
                state.params.push(crate::params::Remembered {
                    name: entry.name.clone(),
                    kind: entry.kind,
                    text: entry.text.clone(),
                });
            }
        }
    }

    /// What the next profiles' analysis knows of the source: the relations decided for it
    /// (its accepted virtual keys) and whether it's production.
    pub fn set_analysis(&self, analysis: crate::profile::Analysis) {
        self.inner.state().analysis = analysis;
    }

    /// How many times a statement of `text`'s fingerprint ran in this session lately (within
    /// `window`).
    pub fn recent_runs(&self, text: &str, flavor: sql::Flavor, window: Duration) -> usize {
        let hash = sql::fingerprint(text, flavor).hash;
        self.inner.state().recent.count_within(hash, window)
    }

    /// The session's own dialect, once connected (a value written in by its rules).
    pub fn dialect(&self) -> Option<Arc<dyn Dialect>> {
        self.inner.state().dialect.clone()
    }

    /// Runs `request`'s statements in the background. `false`: something is running (one
    /// thing at a time), or nothing was asked. The unpinned results go (this run's replace
    /// them).
    pub fn run(&self, request: RunRequest) -> bool {
        if request.statements.is_empty() || !self.start() {
            return false;
        }
        {
            let mut state = self.inner.state();
            state.connector = Some(Arc::clone(&request.connector));
            state.source_name = request.source_name.clone();
            state.settings = request.settings.clone();
            let total = request.statements.len();
            state.snapshot.running =
                Some(Progress { index: 0, total, since: self.inner.runtime.now() });
            state.snapshot.marks.clear();
            state.snapshot.marks_generation += 1;
            // What a result's tab asked of its table is kept (the rest is this run's).
            state.queued.retain(|q| matches!(q, Queued::Describe(_) | Queued::Whole(..)));
            let at = self.inner.runtime.now().saturating_duration_since(self.inner.started);
            for s in &request.statements {
                let hash = sql::fingerprint(s.sent(), request.settings.flavor).hash;
                state.recent.record(hash, at);
            }
        }
        let sent: Vec<&str> = request.statements.iter().map(PlannedStatement::sent).collect();
        self.track(&sent, &request.source_name);
        // Replaced by this run's (a cursor of theirs is closed by its first statement).
        self.inner.results().replace_unpinned();
        self.inner.clear_diagnostics();
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || inner.run(&request));
        true
    }

    /// Explains `statement` (its plan; `analyze`: as it runs) in the background: its profile
    /// (as the settings ask: [`crate::settings::profile_options`]) kept, its plan in a Plan
    /// tab of the Services window (an unpinned one replaced), its summary and tree in the
    /// Output. `document` is the console's text it was planned from (an error placed in it).
    pub fn explain(
        &self,
        statement: PlannedStatement,
        analyze: bool,
        connector: Connector,
        source_name: String,
        flavor: sql::Flavor,
        document: Arc<str>,
    ) -> bool {
        let options = crate::settings::profile_options(analyze);
        self.profile(statement, options, connector, source_name, flavor, document)
    }

    /// Lists the databases and schemas it can switch to (connecting if it isn't).
    pub fn list_places(&self, connector: Connector, source_name: String) -> bool {
        if !self.start() {
            return false;
        }
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || inner.list_places(&connector, &source_name));
        true
    }

    /// Commits (`true`) or rolls back the transaction open.
    pub fn finish_transaction(&self, commit: bool) -> bool {
        if !self.inner.state().snapshot.in_transaction || !self.start() {
            return false;
        }
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || inner.finish_transaction(commit));
        true
    }

    /// What starts running listed as an operation ([`QUERIES`]): its text, where it runs
    /// (the source, the database, the console), stopped as the console's Stop does.
    fn track(&self, statements: &[&str], source: &str) {
        let (project, console, database) = {
            let state = self.inner.state();
            (
                state.context.project,
                state.context.name.clone(),
                state.snapshot.place.database.clone(),
            )
        };
        let (subject, text) = query_texts(statements);
        let id = QUERIES.start(
            project,
            workbench_model::ActivityKind::Query,
            subject,
            workbench_model::ActivityUnit::Rows,
            None,
        );
        let detail: Vec<&str> = [Some(source), database.as_deref(), Some(console.as_str())]
            .into_iter()
            .flatten()
            .filter(|s| !s.is_empty())
            .collect();
        QUERIES.describe(id, detail.join(" · "), Some(text));
        let inner = Arc::downgrade(&self.inner);
        QUERIES.stoppable_with(id, move || {
            if let Some(inner) = inner.upgrade() {
                Runner { inner }.stop();
            }
        });
        self.inner.state().activity = Some(id);
    }

    /// Stops what runs (the engine asked to). `false`: nothing runs.
    pub fn stop(&self) -> bool {
        let state = self.inner.state();
        if state.snapshot.running.is_none() && !state.snapshot.busy {
            return false;
        }
        drop(state);
        self.inner.cancel.lock().unwrap_or_else(PoisonError::into_inner).cancel();
        true
    }

    /// Closes the connection (another database chosen, idle): in the background, after
    /// what runs is stopped. The next run opens it again.
    pub fn disconnect(&self) {
        self.inner.disconnect();
    }

    /// A transaction left open is given up: the connection closes (the engine rolls it
    /// back), its state forgotten at once (the console may close).
    pub fn abandon(&self) {
        self.inner.state().uncommitted.clear();
        self.inner.set_tx(TxState::Idle);
        self.inner.disconnect();
    }

    /// Unused for its idle time at `now` (connected, nothing running, no transaction open).
    /// A transaction or a cursor open, it never is.
    pub fn idle_at(&self, now: Instant) -> bool {
        let cursor = self.inner.results().has_cursor();
        let state = self.inner.state();
        let s = &state.snapshot;
        state.idle.is_some_and(|idle| {
            s.connected
                && !s.busy
                && !s.tx.is_open()
                && !cursor
                && now.saturating_duration_since(state.last_used) >= idle
        })
    }

    /// When it will be idle (`None`: not connected, a transaction or a cursor open, or
    /// never closed for that).
    pub fn idle_due(&self) -> Option<Instant> {
        let cursor = self.inner.results().has_cursor();
        let state = self.inner.state();
        let s = &state.snapshot;
        (s.connected && !s.busy && !s.tx.is_open() && !cursor)
            .then_some(())
            .and(state.idle)
            .map(|i| state.last_used + i)
    }

    /// When its open transaction gets old enough to be told (`None`: none open, or told).
    pub fn long_due(&self) -> Option<Instant> {
        let state = self.inner.state();
        (!state.long_told).then_some(())?;
        state
            .snapshot
            .tx_since
            .filter(|_| state.snapshot.tx.is_open())
            .map(|t| t + LONG_TRANSACTION)
    }

    /// Its transaction is open for [`LONG_TRANSACTION`] at `now`: told once (it may hold
    /// locks). `true` if it was told now.
    pub fn tell_long_transaction(&self, now: Instant) -> bool {
        if self.long_due().is_none_or(|due| now < due) {
            return false;
        }
        let context = {
            let mut state = self.inner.state();
            state.long_told = true;
            state.context.clone()
        };
        context.notifier.show(
            Notification::warning(i18n::tr!(
                "console.long_transaction",
                name = context.name.as_str()
            ))
            .with_detail(i18n::t("console.long_transaction_detail")),
        );
        true
    }

    /// What it holds: the Output's lines, the results' rows, the profiles.
    pub fn memory(&self) -> usize {
        let results = self.inner.results();
        self.inner.log.memory()
            + results.bytes()
            + results.plan_bytes()
            + self.inner.profiles().bytes()
    }

    /// Clears what can be made again (the Output's lines, the switcher's lists, the rows of
    /// the results that can be read again, the unpinned profiles).
    pub fn purge(&self) {
        self.inner.log.clear();
        self.inner.profiles().purge();
        self.inner.state().snapshot.places = None;
        {
            let mut results = self.inner.results();
            for set in &mut results.sets {
                if set.rerunnable && !set.loading {
                    set.rows.clear();
                    set.dropped = true;
                    set.touch();
                }
            }
        }
        self.inner.changed();
    }

    // ─── Results ───

    /// The tables its statements made, and whether it attached another database.
    pub fn made(&self) -> (Vec<TableRef>, bool) {
        let state = self.inner.state();
        (state.created.clone(), state.attached)
    }

    /// The error underlined in the text is let go (its statement was edited).
    pub fn clear_diagnostics(&self) {
        self.inner.clear_diagnostics();
        self.inner.changed();
    }

    /// `queued` now if the runner is free, else once it is.
    fn queue(&self, queued: Queued) -> bool {
        if !self.start() {
            let mut state = self.inner.state();
            if !state.queued.contains(&queued) {
                state.queued.push_back(queued);
            }
            drop(state);
            self.inner.changed();
            return true;
        }
        self.inner.changed();
        let inner = Arc::clone(&self.inner);
        self.inner.runtime.spawn(move || inner.run_queued(queued));
        true
    }

    /// Marks it busy, if it wasn't.
    fn start(&self) -> bool {
        let mut state = self.inner.state();
        if state.snapshot.busy {
            return false;
        }
        state.snapshot.busy = true;
        *self.inner.cancel.lock().unwrap_or_else(PoisonError::into_inner) = Cancel::default();
        true
    }
}

/// A bar's use of a runner's connection ([`Runner::lease`]).
#[derive(Debug)]
pub struct Lease {
    runner: Weak<Inner>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        let Some(inner) = self.runner.upgrade() else { return };
        if inner.leases.fetch_sub(1, Ordering::AcqRel) == 1 {
            inner.state().closed = true;
            inner.disconnect();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/runner.rs"]
mod tests;
