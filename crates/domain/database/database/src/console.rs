//! The query console, from the domain's side (plan 39e): what a console is ([`ConsoleSpec`]:
//! its source, its file, how it runs and commits), where its file is kept (under the
//! project's own folder, `consoles/<source>/<name>.sql`), what it runs for a cursor, a
//! selection or a whole script ([`plan`]), and the port it reaches the data sources through
//! ([`ConsoleAccess`], implemented by the explorer: the sources, a connection of the
//! console's own made with the source's password and startup script, a table opened).
//!
//! The port is registered per project ([`provide`] / [`access`]), as the drivers are
//! ([`crate::drivers`]): a console reopened with the project's session wasn't opened by the
//! explorer, so it finds it here rather than being handed it. What asks for a console (the
//! "Database" window, a data view) leaves a [`JumpRequest`] ([`ask_jump`]) and runs
//! [`JUMP_COMMAND`]; the console's module takes it ([`take_jump`]): neither knows the other.

use std::fmt;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use kernel::{ProjectId, Subscription};

use crate::danger::Danger;
use crate::diagram::Opening;
use crate::relations::RelationNotes;
use crate::sql::{self, Flavor, SplitOptions, StatementKind, TxControl};
use crate::{Capabilities, DataSourceId, DbError, Isolation, Session, SourceColor, TableRef};

/// A console's identity: a UUID v4, never its name (renamed, it stays itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConsoleId(u128);

impl ConsoleId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }

    pub const fn from_u128(id: u128) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    /// From its saved text; `None` if it isn't a UUID.
    pub fn parse(text: &str) -> Option<Self> {
        kernel::id::parse_uuid(text).map(Self)
    }
}

impl Default for ConsoleId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ConsoleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&kernel::id::uuid_text(self.0))
    }
}

/// How a console's statements are committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum TxMode {
    /// Each statement commits by itself (a `BEGIN` written by hand opens a transaction
    /// until its `COMMIT`).
    #[default]
    Auto,
    /// The first statement that writes opens a transaction, kept until Commit / Rollback.
    Manual,
}

impl TxMode {
    pub fn id(self) -> &'static str {
        match self {
            TxMode::Auto => "auto",
            TxMode::Manual => "manual",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "auto" => Some(TxMode::Auto),
            "manual" => Some(TxMode::Manual),
            _ => None,
        }
    }
}

impl TxMode {
    /// A new console's mode: [`TxMode::Manual`] on a production source (nothing is
    /// permanent before Commit), [`TxMode::Auto`] elsewhere.
    pub fn default_for(production: bool) -> Self {
        if production { TxMode::Manual } else { TxMode::Auto }
    }
}

/// Where a console's transaction is, followed statement by statement (pure: the runner
/// feeds it what ran and how it ended, then checks it against the engine's own word).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TxState {
    /// No transaction: each statement commits by itself.
    #[default]
    Idle,
    /// A transaction is open: since which statement of the console's session (counted from
    /// 0), and how many statements that may write ran in it (what Commit makes permanent).
    Open { since_statement: u64, writes: u64 },
    /// The transaction failed (PostgreSQL gives up a transaction at its first error): every
    /// statement is refused until it's rolled back (a `COMMIT` rolls it back too).
    Failed,
}

impl TxState {
    /// A transaction is open (failed or not): Commit / Rollback are offered, the connection
    /// is never closed for idleness, closing the console asks.
    pub fn is_open(self) -> bool {
        !matches!(self, TxState::Idle)
    }

    pub fn is_failed(self) -> bool {
        matches!(self, TxState::Failed)
    }

    /// The statements that may write waiting for Commit.
    pub fn pending(self) -> u64 {
        match self {
            TxState::Open { writes, .. } => writes,
            _ => 0,
        }
    }

    /// Statement number `index` of kind `kind` ran (`ok`: without an error) in `mode` on
    /// `flavor`'s engine.
    ///
    /// - A `BEGIN` / `COMMIT` / `ROLLBACK`… written by hand is followed ([`Self::on_control`]):
    ///   in [`TxMode::Auto`] a `BEGIN` keeps the console in a transaction until its `COMMIT`;
    ///   in [`TxMode::Manual`] it's taken as the transaction's start.
    /// - In [`TxMode::Manual`], the first statement that may write opens the transaction
    ///   (the runner began it before running it).
    /// - Inside a transaction a write is counted; a failure gives it up on PostgreSQL
    ///   ([`TxState::Failed`]), while the other engines keep it open.
    pub fn on_statement(
        self,
        index: u64,
        kind: StatementKind,
        ok: bool,
        mode: TxMode,
        flavor: Flavor,
    ) -> Self {
        if let StatementKind::Transaction(control) = kind {
            return self.on_control(index, control, ok);
        }
        match self {
            TxState::Failed => TxState::Failed,
            TxState::Open { .. } if !ok && matches!(flavor, Flavor::Postgres { .. }) => {
                TxState::Failed
            }
            TxState::Open { since_statement, writes } => {
                TxState::Open { since_statement, writes: writes + u64::from(ok && kind.writes()) }
            }
            TxState::Idle if mode == TxMode::Manual && kind.writes() && ok => {
                TxState::Open { since_statement: index, writes: 1 }
            }
            TxState::Idle => TxState::Idle,
        }
    }

    /// A transaction's control statement number `index` ran (`ok`: the engine took it).
    pub fn on_control(self, index: u64, control: TxControl, ok: bool) -> Self {
        if !ok {
            // A refused `COMMIT` of a failed transaction still ends it on PostgreSQL; the
            // rest leave things as they were (a statement failing inside one: see
            // `on_statement`).
            return self;
        }
        match control {
            TxControl::Begin => match self {
                TxState::Idle => TxState::Open { since_statement: index, writes: 0 },
                open => open,
            },
            // A failed transaction's `COMMIT` is a rollback: it ends either way.
            TxControl::Commit | TxControl::Rollback => TxState::Idle,
            // Back to a savepoint: a failed transaction may go on.
            TxControl::RollbackTo => match self {
                TxState::Failed => TxState::Open { since_statement: index, writes: 0 },
                other => other,
            },
            TxControl::Savepoint | TxControl::Release => self,
        }
    }

    /// Committed or rolled back from the bar (or the connection closed): no transaction.
    pub fn ended(self) -> Self {
        TxState::Idle
    }

    /// The state checked against the engine's own word after a statement number `index`
    /// (`open`: the connection is in a transaction): one the engine ended by itself (a DDL
    /// committing implicitly, a driver's rollback) is gone, one it opened (a statement the
    /// rules didn't see begin it) is followed from there.
    pub fn follow(self, index: u64, open: bool) -> Self {
        match (self, open) {
            (TxState::Idle, true) => TxState::Open { since_statement: index, writes: 0 },
            (TxState::Open { .. } | TxState::Failed, false) => TxState::Idle,
            (state, _) => state,
        }
    }
}

/// What a run asks to be confirmed before anything is sent: the dangers of its statements
/// (an `UPDATE` / `DELETE` without `WHERE`, a `DROP`, a `TRUNCATE`…) and, on a production
/// source in [`TxMode::Auto`], every statement that may write (it's permanent at once).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Safety {
    /// Each dangerous statement (its index in the run) and what it loses.
    pub dangers: Vec<(usize, Danger)>,
    /// The statements writing on production with nothing to undo them (their indices).
    pub production_writes: Vec<usize>,
    /// The source is production (the confirmation asks for "I understand" first).
    pub production: bool,
}

impl Safety {
    /// Nothing to confirm: the run goes at once.
    pub fn is_clear(&self) -> bool {
        self.dangers.is_empty() && self.production_writes.is_empty()
    }

    /// The statements the confirmation shows (each once, in order).
    pub fn statements(&self) -> Vec<usize> {
        let mut all: Vec<usize> = self
            .dangers
            .iter()
            .map(|(i, _)| *i)
            .chain(self.production_writes.iter().copied())
            .collect();
        all.sort_unstable();
        all.dedup();
        all
    }
}

/// What running `statements` on a source asks first (see [`Safety`]): `production` its
/// mark, `tx` the console's mode, `in_transaction` whether one is open already (a write
/// then waits for Commit even in [`TxMode::Auto`], after a `BEGIN` written by hand).
pub fn safety(
    statements: &[PlannedStatement],
    flavor: Flavor,
    production: bool,
    tx: TxMode,
    in_transaction: bool,
) -> Safety {
    let mut out = Safety { production, ..Safety::default() };
    // A `BEGIN` among them: what follows it waits for a Commit.
    let mut open = in_transaction || tx == TxMode::Manual;
    for (i, statement) in statements.iter().enumerate() {
        let tokens = sql::lex(&statement.text, flavor);
        for danger in crate::danger::statement_dangers(&statement.text, &tokens, flavor) {
            out.dangers.push((i, danger));
        }
        match statement.kind {
            StatementKind::Transaction(TxControl::Begin) => open = true,
            // Ended: in Manual the next write opens another one.
            StatementKind::Transaction(TxControl::Commit | TxControl::Rollback) => {
                open = tx == TxMode::Manual;
            }
            kind if production && kind.writes() && !open => out.production_writes.push(i),
            _ => {}
        }
    }
    out
}

/// What "Run" runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum RunMode {
    /// The statement under the cursor, or those selected (one after the other).
    #[default]
    Playground,
    /// The whole file (or the selection), as a script.
    Script,
}

impl RunMode {
    pub fn id(self) -> &'static str {
        match self {
            RunMode::Playground => "playground",
            RunMode::Script => "script",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "playground" => Some(RunMode::Playground),
            "script" => Some(RunMode::Script),
            _ => None,
        }
    }
}

/// What's kept about a console (never a password, never a parameter's value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleSpec {
    pub id: ConsoleId,
    pub source: DataSourceId,
    /// `console`, `console_1`… (its file's name, without `.sql`).
    pub name: String,
    /// Its file, relative to the project's own folder (`consoles/<source>/<name>.sql`).
    pub file: PathBuf,
    /// The database chosen (`None`: the connection's).
    pub database: Option<String>,
    /// The schema chosen (`None`: the connection's).
    pub schema: Option<String>,
    pub tx: TxMode,
    pub isolation: Option<Isolation>,
    pub mode: RunMode,
    /// How long its last run took (milliseconds), for the lists showing it.
    pub last_run_ms: Option<u64>,
    /// The names of the parameters it asked for (never their values).
    pub params: Vec<String>,
    /// A console's file, or a project's own SQL file attached to the source (39n).
    pub kind: ConsoleKind,
}

/// Where a console's text lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum ConsoleKind {
    /// A console's file, in the project's own folder (`consoles/<source>/<name>.sql`).
    #[default]
    Console,
    /// A `.sql` file of the project attached to a source ("No source — Attach…"): its
    /// [`ConsoleSpec::file`] relative to the project's root, its name the file's.
    Attached,
}

impl ConsoleSpec {
    /// A new console of `source` named `name`, its file where consoles are kept.
    pub fn new(source: DataSourceId, name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            id: ConsoleId::new(),
            source,
            file: relative_file(source, &name),
            name,
            database: None,
            schema: None,
            tx: TxMode::Auto,
            isolation: None,
            mode: RunMode::Playground,
            last_run_ms: None,
            params: Vec::new(),
            kind: ConsoleKind::Console,
        }
    }

    /// The project's SQL file `file` (relative to its root: [`attachable`]'s) attached to
    /// `source`, named as the file.
    pub fn attached(source: DataSourceId, file: PathBuf) -> Self {
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { file, kind: ConsoleKind::Attached, ..Self::new(source, name) }
    }

    /// A project's own SQL file attached to its source (not a console's file).
    pub fn is_attached(&self) -> bool {
        self.kind == ConsoleKind::Attached
    }

    /// Its file, absolute, for the project at `root`.
    pub fn path(&self, root: &Path) -> PathBuf {
        match self.kind {
            ConsoleKind::Console => root.join(kernel::brand::PROJECT_DIR).join(&self.file),
            ConsoleKind::Attached => root.join(&self.file),
        }
    }
}

/// The attached SQL files' list, in the project's own folder (39n).
pub const SQL_FILES: &str = "sql-files.toml";

/// The extensions of the SQL files a source can be attached to (the `sql` language's).
pub const SQL_EXTENSIONS: &[&str] = &["sql", "ddl", "pgsql", "psql"];

/// `path` as a SQL file of the project at `root` a source can be attached to: inside the
/// project (never in its own folder — a console's file is a console), with a SQL
/// extension. Its path relative to `root`, else `None`.
pub fn attachable(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = path.strip_prefix(root).ok()?;
    let mut parts = relative.components();
    let first = parts.next()?;
    if first.as_os_str() == kernel::brand::PROJECT_DIR
        || !relative.components().all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return None;
    }
    let sql = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SQL_EXTENSIONS.iter().any(|x| e.eq_ignore_ascii_case(x)));
    let named = path.file_stem().is_some_and(|s| !s.is_empty());
    (sql && named).then(|| relative.to_path_buf())
}

/// The folder consoles are kept in, inside the project's own folder.
pub const DIR: &str = "consoles";

/// The consoles' list's file, in [`DIR`].
pub const LIST_FILE: &str = "consoles.toml";

/// Where the consoles of the project at `root` are kept.
pub fn folder(root: &Path) -> PathBuf {
    root.join(kernel::brand::PROJECT_DIR).join(DIR)
}

/// A console's file relative to the project's own folder: `consoles/<source>/<name>.sql`
/// (no `[`, `:` or anything Windows refuses in a name: the name is the console's).
pub fn relative_file(source: DataSourceId, name: &str) -> PathBuf {
    Path::new(DIR).join(source.to_string()).join(format!("{name}.sql"))
}

/// Which source's console `path` is, and its name, when it's a console's file of the
/// project at `root` (`consoles/<source>/<name>.sql` in its own folder); `None` otherwise.
pub fn identify(root: &Path, path: &Path) -> Option<(DataSourceId, String)> {
    let rest = path.strip_prefix(folder(root)).ok()?;
    let mut parts = rest.components();
    let source = DataSourceId::parse(parts.next()?.as_os_str().to_str()?)?;
    let file = Path::new(parts.next()?.as_os_str());
    if parts.next().is_some() {
        return None;
    }
    let sql =
        file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("sql"));
    let name = file.file_stem()?.to_str()?;
    (sql && !name.is_empty()).then(|| (source, name.to_owned()))
}

/// A new console's name among `taken` (a source's): `console`, then `console_1`, `console_2`…
pub fn next_name<'a>(taken: impl IntoIterator<Item = &'a str>) -> String {
    let taken: Vec<String> = taken.into_iter().map(str::to_lowercase).collect();
    if !taken.iter().any(|t| t == "console") {
        return "console".to_owned();
    }
    (1..).map(|n| format!("console_{n}")).find(|n| !taken.contains(n)).unwrap_or_default()
}

/// Why a console can't be given a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameProblem {
    Empty,
    /// A character no file name may have on some system (`/ \ : * ? " < > |`, a control
    /// one), or a name ending in a dot or a space (Windows drops them).
    Invalid(char),
    /// One of Windows' reserved names (`CON`, `NUL`, `COM1`…).
    Reserved,
    TooLong,
    /// Another console of the source has it (any case: the same file on Windows / macOS).
    Taken,
}

/// The most characters a console's name has.
pub const NAME_MAX: usize = 80;

/// `name` (typed, trimmed) as a console of a source whose others are named `taken`: the name
/// its file is given (`<name>.sql`, `.sql` typed dropped), or why it can't be.
pub fn check_name<'a>(
    name: &str,
    taken: impl IntoIterator<Item = &'a str>,
) -> Result<String, NameProblem> {
    let mut name = name.trim();
    if name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".sql") {
        name = name[..name.len() - 4].trim_end();
    }
    if name.is_empty() {
        return Err(NameProblem::Empty);
    }
    if let Some(c) = name.chars().find(|c| c.is_control() || r#"/\:*?"<>|"#.contains(*c)) {
        return Err(NameProblem::Invalid(c));
    }
    // Neither hidden (`.x`) nor ending in a dot (Windows drops it).
    if let Some(c) =
        [name.chars().next(), name.chars().last()].into_iter().flatten().find(|c| *c == '.')
    {
        return Err(NameProblem::Invalid(c));
    }
    if name.chars().count() > NAME_MAX {
        return Err(NameProblem::TooLong);
    }
    let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        return Err(NameProblem::Reserved);
    }
    let lower = name.to_lowercase();
    if taken.into_iter().any(|t| t.to_lowercase() == lower) {
        return Err(NameProblem::Taken);
    }
    Ok(name.to_owned())
}

/// Where the consoles' list is kept (the project's `consoles/consoles.toml`, by
/// `storage::TomlConsoles`).
pub trait ConsoleStore: Send + Sync {
    /// The consoles kept; none when nothing was ever written. A list that can't be read is
    /// an error (and is never written over).
    fn load(&self) -> Result<Vec<ConsoleSpec>, String>;
    /// Keeps `consoles` (the whole list, written at once).
    fn save(&self, consoles: &[ConsoleSpec]) -> Result<(), String>;
}

/// A store keeping nothing (the tests, a window without a project).
#[derive(Debug, Default)]
pub struct NoConsoles;

impl ConsoleStore for NoConsoles {
    fn load(&self) -> Result<Vec<ConsoleSpec>, String> {
        Ok(Vec::new())
    }

    fn save(&self, _consoles: &[ConsoleSpec]) -> Result<(), String> {
        Ok(())
    }
}

/// A data source as a console sees it: cheap to ask (kept by the explorer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBrief {
    pub id: DataSourceId,
    pub name: String,
    /// Its driver's id (`sqlite`, `postgres`…).
    pub driver: String,
    /// Its driver's icon (a name of the app's set: `database`, `server`).
    pub icon: &'static str,
    /// The color it's shown in (its own, else its folder's).
    pub color: Option<SourceColor>,
    /// Marked as production (every write confirmed).
    pub production: bool,
    pub read_only: bool,
    /// Its driver writes SQL (it has a dialect): only those have consoles.
    pub dialect: bool,
    pub file_based: bool,
    /// The database a connection opens by default (a server's; `None`: a file's own).
    pub database: Option<String>,
    /// Whose SQL it speaks (its dialect's).
    pub flavor: Flavor,
    /// What its driver's console can do (`None`: no console).
    pub capabilities: Capabilities,
}

/// What a console's connection changes from its source's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// Another database of the same server (`None`: the source's).
    pub database: Option<String>,
}

/// Opens a console's own connection (on the calling thread: a background one), its
/// password read then and its startup script run, as the source's other sessions are.
pub type Connector = Arc<dyn Fn() -> Result<Box<dyn Session>, DbError> + Send + Sync>;

/// What a console asks of the data sources (implemented by the explorer). Cheap methods
/// (asked while drawing) read what's kept; [`ConsoleAccess::connector`] only makes the
/// function that connects, never connecting itself.
pub trait ConsoleAccess: Send + Sync {
    /// The sources that can have a console (with a dialect) and those that can't, as listed.
    fn sources(&self) -> Vec<SourceBrief>;
    /// Source `id` (`None`: removed, or not listed yet — asked again later).
    fn source(&self, id: DataSourceId) -> Option<SourceBrief>;
    /// The function opening a connection to `id` of the console's own, `overrides` taken.
    fn connector(&self, id: DataSourceId, overrides: &Overrides) -> Result<Connector, DbError>;
    /// The color source `id` is shown in (asked at each paint).
    fn color(&self, id: DataSourceId) -> Option<SourceColor> {
        self.source(id).and_then(|s| s.color)
    }
    /// Source `id` is marked as production.
    fn production(&self, id: DataSourceId) -> bool {
        self.source(id).is_some_and(|s| s.production)
    }
    /// Opens `table`'s data of source `id`; `None`: it can't be.
    fn open_table(&self, id: DataSourceId, table: &TableRef) -> Option<Opening> {
        let _ = (id, table);
        None
    }
    /// Opens `table`'s structure of source `id`; `None`: it can't be.
    fn open_structure(&self, id: DataSourceId, table: &TableRef) -> Option<Opening> {
        let _ = (id, table);
        None
    }
    /// Opens `table`'s structure of source `id` with its column `column` pointed at; by
    /// default, its structure.
    fn open_column(&self, id: DataSourceId, table: &TableRef, column: &str) -> Option<Opening> {
        let _ = column;
        self.open_structure(id, table)
    }
    /// Opens `table`'s data of source `id` filtered to the rows `filter` keeps (an SQL
    /// condition, `id = 7`: the row a key's cell points to); by default, its data.
    fn open_rows(&self, id: DataSourceId, table: &TableRef, filter: &str) -> Option<Opening> {
        let _ = filter;
        self.open_table(id, table)
    }
    /// The relations decided for source `id` (for `JOIN … ON`).
    fn notes(&self, id: DataSourceId) -> RelationNotes {
        let _ = id;
        RelationNotes::default()
    }
    /// What the "Database" window has selected (a source, a schema, a table), for a
    /// command run from the palette.
    fn selected(&self) -> Option<JumpRequest> {
        None
    }
    /// How long a console's unused connection stays open (`None`: never closed for that);
    /// one in a transaction or with a cursor open is never closed for it.
    fn idle_close(&self) -> Option<Duration> {
        Some(Duration::from_secs(10 * 60))
    }
}

/// The open projects' ports.
static ACCESS: Mutex<Vec<(ProjectId, Arc<dyn ConsoleAccess>)>> = Mutex::new(Vec::new());

/// Project `project`'s consoles reach its sources through `access` until the guard
/// returned is dropped (the project closes).
pub fn provide(project: ProjectId, access: Arc<dyn ConsoleAccess>) -> Subscription {
    {
        let mut all = ACCESS.lock().unwrap_or_else(PoisonError::into_inner);
        all.retain(|(p, _)| *p != project);
        all.push((project, access));
    }
    Subscription::on_end(move || {
        ACCESS.lock().unwrap_or_else(PoisonError::into_inner).retain(|(p, _)| *p != project);
    })
}

/// Project `project`'s port, while it's open (and its explorer on).
pub fn access(project: ProjectId) -> Option<Arc<dyn ConsoleAccess>> {
    ACCESS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(p, _)| *p == project)
        .map(|(_, a)| Arc::clone(a))
}

/// Tells the SQL dialect of a project's document (a console's: its source's) by its path.
pub type FlavorOf = Arc<dyn Fn(ProjectId, &Path) -> Option<Flavor> + Send + Sync>;

/// Who tells documents' dialects (the consoles' module, while it's on).
static FLAVORS: Mutex<Option<FlavorOf>> = Mutex::new(None);

/// Documents' dialects are told by `of` from now on (laying a console out by its source's
/// rules: `$$` bodies, `[x]` names).
pub fn tell_flavors(of: FlavorOf) {
    *FLAVORS.lock().unwrap_or_else(PoisonError::into_inner) = Some(of);
}

/// The dialect of `path` in `project`, if a module knows it (a console's); `None`: told by
/// the file's own name.
pub fn document_flavor(project: ProjectId, path: &Path) -> Option<Flavor> {
    let of = FLAVORS.lock().unwrap_or_else(PoisonError::into_inner).clone()?;
    of(project, path)
}

/// "Jump to Query Console…": the console's command, run by whatever asks for one.
pub const JUMP_COMMAND: &str = "database.console.jump";
/// "New Query Console": a new console of the source asked, whatever it already has.
pub const NEW_COMMAND: &str = "database.console.new";

/// Where a console was asked from: a source, its schema, its table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpRequest {
    pub project: ProjectId,
    pub source: DataSourceId,
    pub schema: Option<String>,
    /// The table it was asked on: an empty console gets `SELECT * FROM` it.
    pub table: Option<TableRef>,
    /// A new console, not one of those the source has.
    pub new: bool,
    /// The source's consoles listed even when it has none ("Database Consoles…": they're
    /// renamed and deleted there).
    pub list: bool,
    /// Another database of its server than the source's own (asked on one of its databases
    /// in the "Database" window): the console opened there.
    pub database: Option<String>,
}

/// The console asked for, waiting for [`JUMP_COMMAND`] (or [`NEW_COMMAND`]).
static PENDING: Mutex<Option<JumpRequest>> = Mutex::new(None);

/// Asks for a console (opened by the command the caller runs next).
pub fn ask_jump(request: JumpRequest) {
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(request);
}

/// The console asked for, taken.
pub fn take_jump() -> Option<JumpRequest> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner).take()
}

/// What an empty console opened on `table` starts with, its name written
/// by `quote` (the dialect's quoting when it needs one): the text and where the table's
/// name is (selected).
pub fn table_query(table: &TableRef, quote: impl Fn(&str) -> String) -> (String, Range<usize>) {
    let name = match &table.schema {
        Some(schema) => format!("{}.{}", quote(schema), quote(&table.name)),
        None => quote(&table.name),
    };
    let start = "SELECT * FROM ".len();
    (format!("SELECT * FROM {name};"), start..start + name.len())
}

/// A statement to run: its text, where it is in the document, what it does, and — its
/// parameters given values ([`bind_statements`]) — the text sent with them written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedStatement {
    /// As written in the document (what its marks and the history keep).
    pub text: String,
    /// Where it is in the document (bytes).
    pub range: Range<usize>,
    pub kind: StatementKind,
    /// Its parameters' values written in, and how the text sent maps back to the document
    /// (`None`: it's sent as written).
    pub bound: Option<sql::Bound>,
}

impl PlannedStatement {
    pub fn writes(&self) -> bool {
        self.kind.writes()
    }

    /// The text sent to the engine: its values written in, else as written.
    pub fn sent(&self) -> &str {
        self.bound.as_ref().map_or(&self.text, |b| &b.text)
    }

    /// How a position in [`PlannedStatement::sent`] goes back to the document (an engine's
    /// error placed in the user's text).
    pub fn map(&self) -> sql::BindMap {
        self.bound.as_ref().map_or_else(|| sql::BindMap::plain(self.range.start), |b| b.map.clone())
    }
}

/// A parameter a run asks a value for: its name, as written the first time (`:status`, `$1`,
/// `?`), and how many places use it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskedParam {
    pub name: sql::ParamName,
    pub label: String,
    pub uses: usize,
}

/// The parameters of `statements` (each once, in the order they first appear; a `?` is
/// numbered within its statement, so the first `?` of two statements is one parameter), as
/// `styles` reads them.
pub fn parameters(
    statements: &[PlannedStatement],
    flavor: Flavor,
    styles: &sql::ParamStyles,
) -> Vec<AskedParam> {
    let mut out: Vec<AskedParam> = Vec::new();
    for statement in statements {
        let tokens = sql::lex(&statement.text, flavor);
        for p in sql::params(&statement.text, &tokens, styles) {
            let label =
                p.places.first().map_or_else(String::new, |r| statement.text[r.clone()].to_owned());
            match out.iter_mut().find(|a| a.name == p.name) {
                Some(asked) => asked.uses += p.places.len(),
                None => out.push(AskedParam { name: p.name, label, uses: p.places.len() }),
            }
        }
    }
    out
}

/// `statements` (planned from `document`) with their parameters' `values` written in by
/// `dialect` (each statement's map back to the document kept); one without parameters is
/// sent as written. A parameter without a value is an error, never a silent `NULL`.
pub fn bind_statements(
    document: &str,
    statements: &[PlannedStatement],
    flavor: Flavor,
    styles: &sql::ParamStyles,
    values: &std::collections::HashMap<sql::ParamName, sql::ParamValue>,
    dialect: &dyn crate::Dialect,
) -> Result<Vec<PlannedStatement>, sql::BindError> {
    statements
        .iter()
        .map(|statement| {
            let tokens = sql::lex(&statement.text, flavor);
            let mut params = sql::params(&statement.text, &tokens, styles);
            if params.is_empty() {
                return Ok(PlannedStatement { bound: None, ..statement.clone() });
            }
            // Their places in the document (the statement's text is the document's there).
            let start = statement.range.start;
            for p in &mut params {
                for place in &mut p.places {
                    *place = place.start + start..place.end + start;
                }
            }
            let bound = if document.get(statement.range.clone()) == Some(statement.text.as_str()) {
                sql::bind(document, statement.range.clone(), &params, values, dialect)?
            } else {
                // The document changed meanwhile: bound over the statement's own text, its
                // places from its start.
                let mut own = params.clone();
                for p in &mut own {
                    for place in &mut p.places {
                        *place = place.start - start..place.end - start;
                    }
                }
                let mut bound =
                    sql::bind(&statement.text, 0..statement.text.len(), &own, values, dialect)?;
                bound.map.start = start;
                bound
            };
            Ok(PlannedStatement { bound: Some(bound), ..statement.clone() })
        })
        .collect()
}

/// The dialect of `flavor`'s engine, before a session gives its own (a value written in, a
/// query wrapped).
pub fn dialect_for(flavor: Flavor) -> Arc<dyn crate::Dialect> {
    match flavor {
        Flavor::Sqlite => Arc::new(crate::sqlite::Sqlite),
        Flavor::Postgres { version } => Arc::new(
            version.map_or_else(crate::postgres::Postgres::default, crate::postgres::Postgres::new),
        ),
        Flavor::MySql => Arc::new(crate::mysql::MySql::default()),
        Flavor::Generic => Arc::new(crate::GenericSql),
    }
}

/// What "Run" does for a document, its selection and its cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPlan {
    /// These statements, in order.
    Run(Vec<PlannedStatement>),
    /// None is under the cursor: which one (every statement of the document)?
    Choose(Vec<PlannedStatement>),
    /// Nothing to run (an empty document, a selection of comments).
    Nothing,
}

/// What [`plan`] reads a document by.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PlanInput<'a> {
    pub text: &'a str,
    /// The selection (bytes), if one isn't empty.
    pub selection: Option<Range<usize>>,
    pub cursor: usize,
    pub mode: RunMode,
    pub flavor: Flavor,
    pub options: SplitOptions,
}

/// What runs for `input`: in [`RunMode::Playground`], the statements of the selection, else
/// the one under the cursor (none there: the choice of every one); in [`RunMode::Script`],
/// every statement of the selection, else of the document.
pub fn plan(input: &PlanInput<'_>) -> RunPlan {
    let text = input.text;
    let selection = input
        .selection
        .clone()
        .filter(|r| r.start < r.end && r.end <= text.len())
        .filter(|r| text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
    // The selection's statements, their places back in the document.
    if let Some(range) = selection {
        let statements = statements_of(&text[range.clone()], input, range.start);
        return if statements.is_empty() { RunPlan::Nothing } else { RunPlan::Run(statements) };
    }
    let parsed = sql::parse(text, input.flavor, &input.options);
    let all = |parsed: &sql::Parsed| -> Vec<PlannedStatement> {
        parsed.statements.iter().map(|s| planned(text, parsed, s, 0, input.flavor)).collect()
    };
    if parsed.statements.is_empty() {
        return RunPlan::Nothing;
    }
    match input.mode {
        RunMode::Script => RunPlan::Run(all(&parsed)),
        RunMode::Playground => {
            let cursor = input.cursor.min(text.len());
            match sql::statement_at(text, &parsed.statements, cursor) {
                Some(s) => RunPlan::Run(vec![planned(text, &parsed, s, 0, input.flavor)]),
                None => RunPlan::Choose(all(&parsed)),
            }
        }
    }
}

/// The statements of `text` (a part of the document starting at `offset`).
fn statements_of(text: &str, input: &PlanInput<'_>, offset: usize) -> Vec<PlannedStatement> {
    let parsed = sql::parse(text, input.flavor, &input.options);
    parsed.statements.iter().map(|s| planned(text, &parsed, s, offset, input.flavor)).collect()
}

fn planned(
    text: &str,
    parsed: &sql::Parsed,
    s: &sql::StatementSpan,
    offset: usize,
    flavor: Flavor,
) -> PlannedStatement {
    let kind = sql::classify(text, parsed.tokens_of(s), flavor);
    PlannedStatement {
        text: s.text(text).to_owned(),
        range: s.range.start + offset..s.range.end + offset,
        kind,
        bound: None,
    }
}

/// The first statement of `statements` a read-only source refuses (one that may write: an
/// unknown one counts), if any.
pub fn refused(statements: &[PlannedStatement], read_only: bool) -> Option<usize> {
    if !read_only {
        return None;
    }
    statements.iter().position(PlannedStatement::writes)
}

/// The first line of `text` cut to `max` characters (with "…"), for lists and logs.
pub fn summary(text: &str, max: usize) -> String {
    let line = kernel::text::one_line(text);
    match line.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line.into_owned(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/console.rs"]
mod tests;
