//! For the tests: a pool run by hand and a clock set by hand ([`Manual`]), a temporary
//! project with a SQLite file ([`Project`]), a port to its sources counting the connections
//! it makes ([`FakeAccess`]), and a host ([`FakeHost`]) holding one open file, remembering
//! what it was asked.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use database::console::{Connector, ConsoleAccess, Overrides, SourceBrief};
use database::sql::Flavor;
use database::{
    Cancel, Capabilities, Collect, DataSourceId, DbError, Driver, Secret, Session, Target, Value,
};
use kernel::{EventBus, ProjectId};
use workbench_model::{ActiveFile, ChoiceList, Notification, OnChoice, Question, WorkbenchHost};

use crate::consoles::Consoles;
use crate::runtime::{Job, Runtime};

/// A pool whose jobs run when the test says, and a clock that moves when it says.
#[derive(Clone)]
pub struct Manual {
    jobs: Arc<Mutex<VecDeque<Job>>>,
    now: Arc<Mutex<Instant>>,
}

impl Manual {
    pub fn new() -> Self {
        Self { jobs: Arc::default(), now: Arc::new(Mutex::new(Instant::now())) }
    }

    pub fn runtime(&self) -> Runtime {
        let (jobs, now) = (Arc::clone(&self.jobs), Arc::clone(&self.now));
        Runtime::new(
            move |job| jobs.lock().unwrap().push_back(job),
            move || *now.lock().unwrap(),
            || "10:00:00.000".to_owned(),
        )
    }

    pub fn run_all(&self) -> usize {
        let mut ran = 0;
        loop {
            let job = self.jobs.lock().unwrap().pop_front();
            let Some(job) = job else { return ran };
            job();
            ran += 1;
        }
    }

    pub fn jobs(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }

    /// The clock's time (a scripted session moves it as it works).
    pub fn clock(&self) -> Arc<Mutex<Instant>> {
        Arc::clone(&self.now)
    }
}

/// A temporary project folder with `shop.db` (users, orders) in it, gone when dropped.
pub struct Project {
    pub root: PathBuf,
}

impl Project {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "console-{name}-{}-{}",
            std::process::id(),
            kernel::id::new_uuid()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let mut session = sqlite(&root, true);
        for sql in [
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL)",
            "INSERT INTO users (name) VALUES ('ana'), ('bob'), ('cid')",
            "CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total REAL)",
            "INSERT INTO orders (user_id, total) VALUES (1, 10.5), (2, 20)",
        ] {
            session.execute(sql, &mut Collect::default(), &Cancel::default()).unwrap();
        }
        Self { root }
    }

    pub fn db(&self) -> PathBuf {
        self.root.join("shop.db")
    }

    /// The rows `sql` reads (another connection).
    pub fn look(&self, sql: &str) -> Vec<Vec<Value>> {
        let mut session = sqlite(&self.root, false);
        let mut rows = Collect::default();
        session.execute(sql, &mut rows, &Cancel::default()).unwrap();
        rows.rows
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn sqlite(root: &Path, create: bool) -> Box<dyn Session> {
    let mut target = Target {
        database: root.join("shop.db").to_string_lossy().into_owned(),
        ..Target::default()
    };
    if create {
        target.options.insert("create".into(), "true".into());
    }
    db_sqlite::SqliteDriver.connect(&target, &Secret::default()).unwrap()
}

/// The port to a project's sources: `shop` (the SQLite file) and `people` (no SQL), the
/// connections made counted.
pub struct FakeAccess {
    pub shop: DataSourceId,
    pub people: DataSourceId,
    pub db: PathBuf,
    pub connections: Arc<AtomicUsize>,
    pub read_only: bool,
    /// The shop marked as production.
    pub production: bool,
    pub selected: Mutex<Option<database::console::JumpRequest>>,
    /// The relations decided for the shop.
    pub notes: Mutex<database::relations::RelationNotes>,
    /// Its connections fail.
    pub broken: bool,
    /// The tables asked opened: the table, its structure, the column pointed at.
    pub opened: Mutex<Vec<(database::TableRef, bool, Option<String>)>>,
    /// The rows asked opened: the table, the filter.
    pub rows_opened: Mutex<Vec<(database::TableRef, String)>>,
    /// A second source speaking SQL (`archive`, on the same file), when asked for.
    pub archive: Option<DataSourceId>,
}

impl FakeAccess {
    pub fn new(project: &Project) -> Self {
        Self {
            shop: DataSourceId::new(),
            people: DataSourceId::new(),
            db: project.db(),
            connections: Arc::default(),
            read_only: false,
            production: false,
            selected: Mutex::default(),
            notes: Mutex::default(),
            broken: false,
            opened: Mutex::default(),
            rows_opened: Mutex::default(),
            archive: None,
        }
    }

    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

impl ConsoleAccess for FakeAccess {
    fn sources(&self) -> Vec<SourceBrief> {
        [Some(self.shop), Some(self.people), self.archive]
            .into_iter()
            .flatten()
            .filter_map(|id| self.source(id))
            .collect()
    }

    fn source(&self, id: DataSourceId) -> Option<SourceBrief> {
        let archive = self.archive == Some(id);
        let sql = id == self.shop || archive;
        (sql || id == self.people).then(|| SourceBrief {
            id,
            name: if archive {
                "archive".into()
            } else if sql {
                "shop".into()
            } else {
                "people".into()
            },
            driver: if sql { "sqlite".into() } else { "csv".into() },
            icon: "database",
            color: sql.then_some(if archive {
                database::SourceColor::Blue
            } else {
                database::SourceColor::Green
            }),
            production: id == self.shop && self.production,
            read_only: self.read_only,
            dialect: sql,
            file_based: true,
            database: None,
            flavor: if sql { Flavor::Sqlite } else { Flavor::Generic },
            capabilities: if sql {
                db_sqlite::SqliteDriver.capabilities()
            } else {
                Capabilities::default()
            },
        })
    }

    fn connector(&self, id: DataSourceId, _overrides: &Overrides) -> Result<Connector, DbError> {
        if id != self.shop && self.archive != Some(id) {
            return Err(DbError::Unsupported("no SQL".into()));
        }
        let (db, count, broken) = (self.db.clone(), Arc::clone(&self.connections), self.broken);
        Ok(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            if broken {
                return Err(DbError::Connect("refused".into()));
            }
            let target =
                Target { database: db.to_string_lossy().into_owned(), ..Target::default() };
            db_sqlite::SqliteDriver.connect(&target, &Secret::default())
        }))
    }

    fn open_table(
        &self,
        _id: DataSourceId,
        table: &database::TableRef,
    ) -> Option<database::diagram::Opening> {
        self.opened.lock().unwrap().push((table.clone(), false, None));
        Some(database::diagram::Opening::View(database::diagram::TableView {
            title: OPEN.into(),
            extension: OPEN,
            data: std::sync::Arc::new(()),
        }))
    }

    fn open_column(
        &self,
        _id: DataSourceId,
        table: &database::TableRef,
        column: &str,
    ) -> Option<database::diagram::Opening> {
        self.opened.lock().unwrap().push((table.clone(), true, Some(column.to_owned())));
        Some(database::diagram::Opening::File(
            self.db.clone(),
            database::diagram::FileTable {
                table: table.clone(),
                structure: false,
                column: None,
                filter: None,
            },
        ))
    }

    fn open_rows(
        &self,
        _id: DataSourceId,
        table: &database::TableRef,
        filter: &str,
    ) -> Option<database::diagram::Opening> {
        self.rows_opened.lock().unwrap().push((table.clone(), filter.to_owned()));
        Some(database::diagram::Opening::File(
            self.db.clone(),
            database::diagram::FileTable {
                table: table.clone(),
                structure: false,
                column: None,
                filter: None,
            },
        ))
    }

    fn selected(&self) -> Option<database::console::JumpRequest> {
        self.selected.lock().unwrap().clone()
    }

    fn notes(&self, id: DataSourceId) -> database::relations::RelationNotes {
        if id == self.shop { self.notes.lock().unwrap().clone() } else { Default::default() }
    }

    fn idle_close(&self) -> Option<Duration> {
        Some(Duration::from_secs(600))
    }
}

/// The command the fake's tables open with.
pub const OPEN: &str = "fake.open_table";

/// A project's consoles on `manual`'s pool, reaching `access`, their list in memory.
pub fn consoles(project: &Project, manual: &Manual, access: Arc<FakeAccess>) -> Arc<Consoles> {
    let consoles =
        Consoles::new(ProjectId::new(), &project.root, EventBus::new(), manual.runtime());
    consoles.set_access(access);
    // The schema's checks without the typing pause (nothing waits in the tests).
    consoles.checks().set_pause(Duration::ZERO);
    consoles
}

/// A host with one file open (its text, cursor, selection), what it was asked kept.
pub struct FakeHost {
    pub project: ProjectId,
    pub root: PathBuf,
    pub file: Option<(PathBuf, String)>,
    pub cursor: usize,
    pub selection: Option<std::ops::Range<usize>>,
    pub events: EventBus,
    pub told: Vec<Notification>,
    pub chooses: Vec<ChoiceList>,
    pub choice: Option<OnChoice>,
    pub asked: Vec<Question>,
    pub opened: Vec<PathBuf>,
    pub selected: Option<std::ops::Range<usize>>,
    pub commands: Vec<command_model::CommandId>,
    /// The tool windows asked to be shown.
    pub shown: Vec<String>,
    /// The console's confirmations opened (its modal).
    pub confirms: Vec<crate::confirm::ConfirmRequest>,
    /// The parameters' modals opened.
    pub params: Vec<crate::params::ParamsRequest>,
    /// The views opened in tabs (their title and extension).
    pub views: Vec<(String, String)>,
    /// The history windows opened.
    pub histories: Vec<crate::history_view::HistoryRequest>,
    /// The renames asked.
    pub renames: Vec<crate::rename::RenameRequest>,
    /// It shows the module's popovers (else `choose` lists them).
    pub overlays: bool,
    /// The consoles' popovers opened.
    pub jumps: Vec<crate::jump::JumpData>,
    /// The commands asked by their names.
    pub named: Vec<String>,
}

impl FakeHost {
    pub fn new(project: ProjectId, root: &Path) -> Self {
        Self {
            project,
            root: root.to_path_buf(),
            file: None,
            cursor: 0,
            selection: None,
            events: EventBus::new(),
            told: Vec::new(),
            chooses: Vec::new(),
            choice: None,
            asked: Vec::new(),
            opened: Vec::new(),
            selected: None,
            commands: Vec::new(),
            shown: Vec::new(),
            confirms: Vec::new(),
            params: Vec::new(),
            views: Vec::new(),
            histories: Vec::new(),
            renames: Vec::new(),
            overlays: false,
            jumps: Vec::new(),
            named: Vec::new(),
        }
    }

    /// The last parameters' modal given `entries` (as its main button does).
    pub fn give(&mut self, entries: Vec<crate::params::ParamEntry>) -> bool {
        let Some(request) = self.params.last().cloned() else { return false };
        request.then.take().is_some_and(|then| {
            then(self, entries);
            true
        })
    }

    /// The last parameters' modal given these texts (in its rows' order), as typed.
    pub fn give_texts(&mut self, texts: &[&str]) -> bool {
        let Some(request) = self.params.last().cloned() else { return false };
        let entries = request
            .entries
            .iter()
            .zip(texts)
            .map(|(e, t)| crate::params::ParamEntry { text: (*t).to_owned(), ..e.clone() })
            .collect();
        self.give(entries)
    }

    /// The last confirmation answered (`answer`), as its modal's button does.
    pub fn answer(&mut self, answer: crate::confirm::Answer) -> bool {
        let Some(request) = self.confirms.last().cloned() else { return false };
        let Some(then) = request.then.take() else { return false };
        then(self, answer);
        true
    }

    /// Picks row `i` of the list shown.
    pub fn pick(&mut self, i: usize) {
        if let Some(then) = self.choice.take() {
            then(i, self);
        }
    }
}

impl workbench_model::ViewHost for FakeHost {
    fn open_view(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        if extension == OPEN {
            self.views.push((title.to_owned(), extension.to_owned()));
            return true;
        }
        if extension != crate::profile::VIEW
            || data.downcast_ref::<crate::profile::ProfileTabData>().is_none()
        {
            return false;
        }
        self.views.push((title.to_owned(), extension.to_owned()));
        true
    }

    fn open_overlay(&mut self, extension: &str, data: &dyn std::any::Any, popover: bool) -> bool {
        match data.downcast_ref::<crate::jump::JumpData>() {
            Some(data) if self.overlays && popover && extension == crate::jump::EXTENSION => {
                self.jumps.push(data.clone());
                true
            }
            _ => false,
        }
    }

    fn open_modal(&mut self, _: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        if let Some(request) = data.downcast_ref::<crate::history_view::HistoryRequest>() {
            self.histories.push(request.clone());
            return extension == crate::history_view::EXTENSION;
        }
        if let Some(request) = data.downcast_ref::<crate::rename::RenameRequest>() {
            self.renames.push(request.clone());
            return extension == crate::rename::EXTENSION;
        }
        if let Some(request) = data.downcast_ref::<crate::params::ParamsRequest>() {
            if extension != crate::params::EXTENSION {
                return false;
            }
            self.params.push(request.clone());
            return true;
        }
        match data.downcast_ref::<crate::confirm::ConfirmRequest>() {
            Some(request) if extension == crate::confirm::EXTENSION => {
                self.confirms.push(request.clone());
                true
            }
            _ => false,
        }
    }

    fn show_side_panel(&mut self, id: &str) -> bool {
        self.shown.push(id.to_owned());
        true
    }
}

impl workbench_model::EditorHost for FakeHost {
    fn file_text(&self, path: &Path) -> Option<String> {
        self.file.as_ref().filter(|(p, _)| p == path).map(|(_, t)| t.clone())
    }

    fn edit_active(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        let Some((_, content)) = &mut self.file else { return false };
        content.replace_range(range, text);
        true
    }

    fn select_active(&mut self, range: std::ops::Range<usize>) -> bool {
        self.selected = Some(range);
        true
    }

    fn open_file(&mut self, path: &Path) -> bool {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        self.file = Some((path.to_path_buf(), text));
        self.opened.push(path.to_path_buf());
        true
    }
}

impl workbench_model::RefactoringHost for FakeHost {}
impl workbench_model::ServicesHost for FakeHost {}
impl workbench_model::SystemHost for FakeHost {}

impl WorkbenchHost for FakeHost {
    fn project_root(&self) -> Option<PathBuf> {
        Some(self.root.clone())
    }

    fn project_id(&self) -> Option<ProjectId> {
        Some(self.project)
    }

    fn active_file(&self) -> Option<ActiveFile> {
        let (path, text) = self.file.clone()?;
        Some(ActiveFile {
            path,
            text: text.into(),
            line: 0,
            cursor: self.cursor,
            selection: self.selection.clone(),
        })
    }

    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }

    fn project_storage(&self) -> Option<PathBuf> {
        None
    }

    fn events(&self) -> &EventBus {
        &self.events
    }

    fn confirm(&mut self, question: Question, then: workbench_model::OnYes) -> bool {
        self.asked.push(question);
        then(self);
        true
    }

    fn choose(&mut self, list: ChoiceList, then: OnChoice) -> bool {
        self.chooses.push(list);
        self.choice = Some(then);
        true
    }

    fn spawn(&mut self, _job: workbench_model::Job) {}

    fn notify(&mut self, notification: Notification) {
        self.told.push(notification);
    }

    fn request_command(&mut self, id: command_model::CommandId) {
        self.commands.push(id);
    }

    fn request_command_named(&mut self, id: &str) -> bool {
        self.named.push(id.to_owned());
        true
    }
}

/// What the user was told (a runner's toasts).
#[derive(Default)]
pub struct Told(pub Mutex<Vec<Notification>>);

impl workbench_model::Notifier for Told {
    fn show(&self, notification: Notification) {
        self.0.lock().unwrap().push(notification);
    }

    fn update(&self, _notification: Notification) {}

    fn dismiss(&self, _id: workbench_model::NotificationId) {}
}

/// A session playing a script: `SELECT …` gives `rows` rows (`id`, `name`), its execution
/// taking 43 ms and its fetching 368 ms on `clock`, the rest kept in a cursor when asked and
/// it `cursors`; `INSERT … RETURNING` gives them too, never a cursor; anything else changes
/// two rows. Every statement it ran is kept.
pub struct Scripted {
    pub clock: Arc<Mutex<Instant>>,
    pub rows: u64,
    pub cursors: bool,
    pub ran: Arc<Mutex<Vec<String>>>,
    /// The cursor's next row.
    at: u64,
    /// How long a statement takes past its fetching (a long run).
    pub extra: Duration,
}

impl Scripted {
    pub fn new(clock: Arc<Mutex<Instant>>, rows: u64, cursors: bool) -> Self {
        Self { clock, rows, cursors, ran: Arc::default(), at: 0, extra: Duration::ZERO }
    }

    /// A connector making it (each connection sharing what's ran).
    pub fn connector(
        clock: Arc<Mutex<Instant>>,
        rows: u64,
        cursors: bool,
        extra: Duration,
    ) -> (Connector, Arc<Mutex<Vec<String>>>) {
        let ran: Arc<Mutex<Vec<String>>> = Arc::default();
        let shared = Arc::clone(&ran);
        let connector: Connector = Arc::new(move || {
            let mut session = Scripted::new(Arc::clone(&clock), rows, cursors);
            session.ran = Arc::clone(&shared);
            session.extra = extra;
            Ok(Box::new(session) as Box<dyn Session>)
        });
        (connector, ran)
    }

    fn tick(&self, ms: u64) {
        *self.clock.lock().unwrap() += Duration::from_millis(ms);
    }

    fn send(&mut self, from: u64, count: u64, sink: &mut dyn database::RowSink) {
        sink.columns(&[
            database::Column { name: "id".into(), type_name: "INTEGER".into() },
            database::Column { name: "name".into(), type_name: "TEXT".into() },
        ]);
        for i in from..from + count {
            sink.row(vec![Value::Int(i as i64 + 1), Value::Text(format!("user {}", i + 1))]);
        }
    }
}

impl Session for Scripted {
    fn server(&self) -> String {
        "scripted".into()
    }

    fn dialect(&self) -> Option<Arc<dyn database::Dialect>> {
        Some(Arc::new(database::sqlite::Sqlite))
    }

    fn children(&mut self, _path: &[String]) -> Result<Vec<database::Node>, DbError> {
        Ok(Vec::new())
    }

    fn execute(
        &mut self,
        _query: &str,
        _sink: &mut dyn database::RowSink,
        _cancel: &Cancel,
    ) -> Result<database::Outcome, DbError> {
        Err(DbError::Unsupported("scripted".into()))
    }

    fn console(&self) -> Option<database::ConsoleCaps> {
        Some(database::ConsoleCaps { cursors: self.cursors, ..database::ConsoleCaps::BASIC })
    }

    fn current(&mut self) -> Result<database::Place, DbError> {
        Ok(database::Place { database: Some("shop".into()), schema: Some("public".into()) })
    }

    fn run(
        &mut self,
        statement: &str,
        options: &database::RunOptions,
        sink: &mut dyn database::RowSink,
        _cancel: &Cancel,
    ) -> Result<database::RunOutcome, DbError> {
        self.ran.lock().unwrap().push(statement.to_owned());
        let upper = statement.trim_start().to_ascii_uppercase();
        let returning = upper.starts_with("INSERT") && upper.contains("RETURNING");
        let mut outcome = database::RunOutcome::default();
        if upper.starts_with("SELECT") || returning {
            // A wrapped query reads from its OFFSET.
            let from = upper
                .rsplit_once("OFFSET ")
                .and_then(|(_, n)| n.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let left = self.rows.saturating_sub(from);
            let limit = u64::from(options.fetch_limit);
            self.tick(43);
            let count = left.min(limit);
            self.send(from, count, sink);
            self.tick(368);
            outcome.rows = count;
            outcome.truncated = left > limit;
            if outcome.truncated && options.keep_cursor && self.cursors && !returning {
                self.at = from + count;
                outcome.more = Some(database::run::CursorToken("c1".into()));
            }
            if returning {
                outcome.affected = Some(left);
            }
        } else {
            self.tick(6);
            outcome.affected = Some(2);
        }
        *self.clock.lock().unwrap() += self.extra;
        Ok(outcome)
    }

    fn fetch(
        &mut self,
        cursor: &database::run::CursorToken,
        rows: u32,
        sink: &mut dyn database::RowSink,
        _cancel: &Cancel,
    ) -> Result<database::RunOutcome, DbError> {
        self.ran.lock().unwrap().push(format!("FETCH {} {rows}", cursor.0));
        let left = self.rows.saturating_sub(self.at);
        let count = left.min(u64::from(rows));
        self.tick(5);
        self.send(self.at, count, sink);
        self.at += count;
        let more = self.at < self.rows;
        Ok(database::RunOutcome {
            rows: count,
            truncated: more,
            more: more.then(|| cursor.clone()),
            ..database::RunOutcome::default()
        })
    }
}
