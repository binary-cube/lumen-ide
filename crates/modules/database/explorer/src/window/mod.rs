//! The "Database" tool window: the data sources configured (the project's and every
//! project's) and the data files shown by themselves, as a tree ([`crate::tree`]) whose
//! nodes are read only when opened, through each source's worker (in the background,
//! never on the UI thread). It follows the project's events — the sources listed
//! ([`database::events::DataSourcesChanged`]), a structure changed by a commit
//! ([`database::events::DataChanged`]) — merged in a queue ([`kernel::queue::Coalescing`])
//! and taken at its next poll, never called.
//!
//! Its `+` adds a source: a driver among those registered (the recent ones first), or a
//! data file picked on disk; a double click on a table opens its data (the file's view for
//! a file, the source's own view otherwise); its rows' menus open the structure's modals,
//! copy names, refresh, remove a source. Hidden, it lets go of its reads (asked again when
//! shown); a session it opened closes when idle, as any.

mod paint;

use database::diagram::FileTable;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use database::events::{DataChanged, DataSourcesChanged};
use database::{DataSource, DataSourceId, Scope, TableKind, TableRef};
use kernel::queue::{Batch, Coalescing};
use kernel::{Point, ProjectId, Rect, Subscription};
use ui_components::{
    AppRequest, EdgeScrollbar, KeyReach, PanelButton, PanelContext, ToolPanel, ToolPanelAction,
    ViewMenuEntry, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};
use ui_kit::{ListFilter, TextInput};
use workbench_model::ProjectView;

use crate::catalog::Listed;
use crate::grid_source::message;
use crate::sessions::Sessions;
use crate::tree::{Group, Need, NodeKey, Part, Row, RowKind, SourceTree, TreeSource, Words};
use crate::worker::{Client, Request, Response, Ticket};

mod menus;
mod schema;
mod sources;
mod tool_panel;

/// The tool window's id (its button, its menu, its commands' view).
pub const TOOL: &str = "database";

/// Logical pixels: a row's height, the tree's indentation and inset, the filter field.
pub const ROW_HEIGHT: f32 = 28.0;
pub const INDENT: f32 = 16.0;
pub const PADDING: f32 = 14.0;
pub const FIELD_TOP: f32 = 4.0;
pub const FIELD_HEIGHT: f32 = 34.0;
/// Where the rows start, under the field.
pub const LIST_TOP: f32 = FIELD_TOP + FIELD_HEIGHT + 10.0;

/// The drivers shown first in `+` › Data Source.
const RECENT: usize = 3;

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the project said changed (merged until the next poll).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Change {
    Sources,
    Structure(DataSourceId),
}

/// A source's reads: its client, what each ticket was.
struct Loader {
    client: Client,
    tickets: HashMap<Ticket, Need>,
}

/// What a picked file becomes.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Adding {
    /// A source of this driver.
    Driver(String),
    /// A source of whichever driver opens it.
    Any,
}

/// Where the mouse is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hover {
    Field,
    Clear,
    Row(usize),
}

/// Opens a source's tree where the caller says (the source, its table, its columns), the
/// table selected: given to the window ([`ToolPanel::set_data`]), e.g. by a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reveal {
    pub source: DataSourceId,
    pub table: Option<TableRef>,
    pub columns: bool,
}

/// The window (see the module's doc).
pub struct DatabaseWindow {
    project: Option<ProjectId>,
    sessions: Option<Arc<Sessions>>,
    tree: SourceTree,
    rows: Vec<Row>,
    /// The rows must be made again (something changed).
    stale: bool,
    filter: TextInput,
    /// The filter has the keys (else the rows).
    typing: bool,
    selected: Option<NodeKey>,
    hover: Option<Hover>,
    scroll: f32,
    bar: EdgeScrollbar,
    loaders: HashMap<DataSourceId, Loader>,
    waker: Arc<Mutex<Option<Waker>>>,
    changes: Arc<Mutex<Coalescing<Change>>>,
    subscriptions: Vec<Subscription>,
    action: Option<ToolPanelAction>,
    adding: Option<Adding>,
    /// A structure's modal waiting for its table's structure.
    waiting_schema: Option<(DataSourceId, TableRef, crate::schema::Kind)>,
    schema_answer: Option<crate::schema::Answer>,
    /// Where it was last drawn.
    bounds: Rect,
    field: Rect,
    clear: Option<Rect>,
    list: Rect,
    scale: f32,
}

impl std::fmt::Debug for DatabaseWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseWindow").field("project", &self.project).finish_non_exhaustive()
    }
}

impl Default for DatabaseWindow {
    fn default() -> Self {
        Self::new()
    }
}

/// The rows' words, translated.
pub fn words() -> Words {
    Words {
        loading: i18n::t("datasources.loading"),
        default_database: i18n::t("datasources.default_database"),
        no_databases: i18n::t("database.no_databases"),
        not_configured: i18n::t("datasources.not_configured"),
        no_tables: i18n::t("database.no_tables"),
        tables: i18n::t("datasources.tables"),
        views: i18n::t("datasources.views"),
        columns: i18n::t("datasources.columns"),
        indexes: i18n::t("datasources.indexes"),
        keys: i18n::t("datasources.keys"),
        unique: i18n::t("datasources.unique"),
        global: i18n::t("datasources.global"),
        read_only: i18n::t("database.read_only"),
        open_file: i18n::t("datasources.open_file"),
        rows: |n| i18n::tr!("datasources.rows", count = n),
    }
}

impl DatabaseWindow {
    pub fn new() -> Self {
        Self {
            project: None,
            sessions: None,
            tree: SourceTree::default(),
            rows: Vec::new(),
            stale: true,
            filter: TextInput::default(),
            typing: false,
            selected: None,
            hover: None,
            scroll: 0.0,
            bar: EdgeScrollbar::default(),
            loaders: HashMap::new(),
            waker: Arc::default(),
            changes: Arc::new(Mutex::new(Coalescing::new(
                Duration::ZERO,
                Duration::ZERO,
                Duration::from_secs(1),
            ))),
            subscriptions: Vec::new(),
            action: None,
            adding: None,
            waiting_schema: None,
            schema_answer: None,
            bounds: Rect::default(),
            field: Rect::default(),
            clear: None,
            list: Rect::default(),
            scale: 1.0,
        }
    }

    /// The tree (what the tests read).
    pub fn tree(&self) -> &SourceTree {
        &self.tree
    }

    /// The rows shown now.
    pub fn rows(&mut self) -> &[Row] {
        self.refresh_rows();
        &self.rows
    }

    pub fn selected(&self) -> Option<&NodeKey> {
        self.selected.as_ref()
    }

    /// Tells the user `notification` (a toast of the project's: what the window did, or
    /// why it couldn't).
    fn notify(&self, notification: workbench_model::Notification) {
        if let Some(sessions) = &self.sessions {
            sessions.notify(notification);
        }
    }

    fn wake(&self) -> Waker {
        let cell = Arc::clone(&self.waker);
        Arc::new(move || {
            let waker = cell.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(waker) = waker {
                waker();
            }
        })
    }

    /// The project shown: its sources (read in the background the first time), its events
    /// followed. The same project keeps what's open.
    pub fn set_project(&mut self, project: Option<ProjectId>) {
        if project == self.project && (project.is_none() || self.sessions.is_some()) {
            return;
        }
        self.project = project;
        self.subscriptions.clear();
        self.loaders.clear();
        self.tree = SourceTree::default();
        self.selected = None;
        self.sessions = project.and_then(crate::sessions::sessions);
        let Some(sessions) = self.sessions.clone() else {
            self.stale = true;
            return;
        };
        let (changes, wake) = (Arc::clone(&self.changes), self.wake());
        self.subscriptions.push(sessions.events().subscribe(move |_: &DataSourcesChanged| {
            push(&changes, Change::Sources);
            wake();
        }));
        // A console's transaction opened or ended: its source's mark.
        let (changes, wake) = (Arc::clone(&self.changes), self.wake());
        self.subscriptions.push(sessions.events().subscribe(
            move |_: &database::events::TransactionChanged| {
                push(&changes, Change::Sources);
                wake();
            },
        ));
        let (changes, wake) = (Arc::clone(&self.changes), self.wake());
        self.subscriptions.push(sessions.events().subscribe(move |e: &DataChanged| {
            if e.structure {
                push(&changes, Change::Structure(e.source));
                wake();
            }
        }));
        sessions.catalog().ensure_loaded();
        push(&self.changes, Change::Sources);
    }

    /// The sources as the tree shows them: the configured ones (the project's, then the
    /// global ones), then the files shown by themselves that aren't.
    fn compose(&mut self) {
        let Some(sessions) = self.sessions.clone() else {
            self.tree.set_sources(Vec::new());
            self.tree.set_notices(Vec::new());
            return;
        };
        let catalog = sessions.catalog();
        let mut notices = Vec::new();
        for scope in [Scope::Project, Scope::Global] {
            if let Listed::Unreadable(why) = catalog.list(scope) {
                notices.push((scope, i18n::tr!("datasources.unreadable", why = why.as_str())));
            } else if let Some(why) = catalog.write_error(scope) {
                notices.push((scope, i18n::tr!("datasources.unwritten", why = why.as_str())));
            }
        }
        let root = sessions.root().to_path_buf();
        let folders = catalog.folders();
        let mut sources: Vec<TreeSource> = catalog
            .all()
            .iter()
            .map(|s| {
                let mut source = tree_source(&sessions, s, Some(s.scope), s.file_in(&root));
                source.color = database::folders::effective_color(s, &folders);
                source.production = database::folders::effective_production(s, &folders);
                source.transaction = sessions.console_transaction(s.id);
                source
            })
            .collect();
        let identities = sessions.identities();
        for (path, id, driver) in sessions.open_files() {
            // A file configured as a source is listed once, by whatever way it was opened.
            if sources
                .iter()
                .any(|s| s.id == id || s.file.as_deref().is_some_and(|f| identities.same(f, &path)))
            {
                continue;
            }
            let name = kernel::paths::file_name(&path);
            let mut source = DataSource::new(name, driver, Scope::Project);
            source.id = id;
            source.file = Some(path.clone());
            sources.push(tree_source(&sessions, &source, None, Some(path)));
        }
        // The readers of sources gone go too (a server's databases' with their source).
        let tree = &self.tree;
        self.loaders.retain(|id, _| {
            let base = tree.base_of(*id);
            sources.iter().any(|s| s.id == *id || s.id == base)
        });
        self.tree.set_sources(sources);
        self.tree.set_folders(folders);
        self.tree.set_notices(notices);
        let needs = self.tree.open_needs();
        self.ask(needs);
    }

    /// The rows made again if something changed; the selection kept (or moved to the
    /// nearest row shown).
    fn refresh_rows(&mut self) {
        if !self.stale {
            return;
        }
        self.stale = false;
        self.rows = self.tree.rows(&ListFilter::new(self.filter.text()), &words());
    }

    /// Reads what the tree needs, each through its source's worker (made if needed).
    fn ask(&mut self, needs: Vec<Need>) {
        for need in needs {
            let id = match &need {
                Need::Databases(id) | Need::Tables(id) | Need::Structure(id, _) => *id,
            };
            let Some(loader) = self.loader(id) else { continue };
            let request = match &need {
                Need::Databases(_) => Request::Databases,
                Need::Tables(_) => Request::Tables(Vec::new()),
                Need::Structure(_, table) => Request::Structure(table.clone()),
            };
            let ticket = loader.client.send(request);
            loader.tickets.insert(ticket, need);
        }
        self.stale = true;
    }

    /// Source `id`'s reader: its worker's client (the file's own when it's shown by
    /// itself); `None` (said in the tree) when it can't be had.
    fn loader(&mut self, id: DataSourceId) -> Option<&mut Loader> {
        if !self.loaders.contains_key(&id) {
            let sessions = self.sessions.clone()?;
            let worker =
                match sessions.get(id).map(Ok).unwrap_or_else(|| sessions.source_worker(id)) {
                    Ok(worker) => worker,
                    Err(e) => {
                        if self.tree.source(id).is_some_and(|s| s.databases) {
                            self.tree.set_databases(id, Err(message(&e)));
                        } else {
                            self.tree.set_tables(id, Err(message(&e)));
                        }
                        return None;
                    }
                };
            let client = worker.client(self.wake());
            self.loaders.insert(id, Loader { client, tickets: HashMap::new() });
        }
        self.loaders.get_mut(&id)
    }

    /// Source `id`'s databases as the tree lists them: the one its session is on read by
    /// the source itself (its own identity: its consoles, grids and completion as before),
    /// each other one by an identity of its own (kept in the catalog, so a session opens
    /// it there), connected only when opened.
    fn database_list(
        &self,
        id: DataSourceId,
        names: Vec<String>,
        current: Option<String>,
    ) -> Vec<crate::tree::TreeDatabase> {
        let Some(sessions) = &self.sessions else { return Vec::new() };
        let configured = sessions.catalog().get(id).map(|s| s.target.database).unwrap_or_default();
        let own = current.or_else(|| (!configured.is_empty()).then_some(configured));
        let mut names = names;
        if let Some(own) = &own
            && !names.contains(own)
        {
            names.push(own.clone());
        }
        names
            .into_iter()
            .map(|name| {
                let default = own.as_deref() == Some(name.as_str());
                let id = if default { id } else { sessions.catalog().database_id(id, &name) };
                crate::tree::TreeDatabase { name, id, default }
            })
            .collect()
    }

    /// The readers' replies, put in the tree. `true` if any came.
    fn take_replies(&mut self) -> bool {
        let mut came = false;
        let mut done = Vec::new();
        for loader in self.loaders.values_mut() {
            for reply in loader.client.poll() {
                if let Some(need) = loader.tickets.remove(&reply.ticket) {
                    done.push((need, reply.result));
                }
            }
        }
        for (need, result) in done {
            came = true;
            match (need, result) {
                (Need::Tables(id), Ok(Response::Tables(tables))) => {
                    // A server's version, shown beside its name.
                    if self.source(id).is_some_and(|s| s.file.is_none())
                        && let Some(worker) = self.sessions.as_ref().and_then(|s| s.get(id))
                    {
                        self.tree.set_server(id, worker.server());
                    }
                    self.tree.set_tables(id, Ok(tables));
                    let needs = self.tree.open_needs();
                    self.ask(needs);
                }
                (Need::Tables(id), Err(e)) => self.tree.set_tables(id, Err(message(&e))),
                (Need::Databases(id), Ok(Response::Databases { names, current })) => {
                    if let Some(worker) = self.sessions.as_ref().and_then(|s| s.get(id)) {
                        self.tree.set_server(id, worker.server());
                    }
                    let list = self.database_list(id, names, current);
                    self.tree.set_databases(id, Ok(list));
                    let needs = self.tree.open_needs();
                    self.ask(needs);
                }
                (Need::Databases(id), Err(e)) => self.tree.set_databases(id, Err(message(&e))),
                (Need::Structure(id, table), Ok(Response::Structure(s))) => {
                    self.tree.set_structure(id, table, Ok(s));
                }
                (Need::Structure(id, table), Err(e)) => {
                    self.tree.set_structure(id, table, Err(message(&e)));
                }
                _ => {}
            }
        }
        if came {
            self.stale = true;
            self.open_waiting_schema();
        }
        came
    }

    /// What the project said changed, merged: the sources listed again, a source read
    /// again.
    fn take_changes(&mut self) -> bool {
        let batch =
            self.changes.lock().unwrap_or_else(PoisonError::into_inner).take(Instant::now());
        let Some(batch) = batch else { return false };
        let keys = match batch {
            Batch::All => vec![Change::Sources],
            Batch::Keys(keys) => keys,
        };
        for change in keys {
            match change {
                Change::Sources => self.compose(),
                Change::Structure(id) => self.refresh_source(id),
            }
        }
        self.stale = true;
        true
    }

    /// Source `id` read again (what's open in it).
    fn refresh_source(&mut self, id: DataSourceId) {
        if self.tree.source(id).is_none() {
            return;
        }
        if let Some(loader) = self.loaders.get_mut(&id) {
            for ticket in loader.tickets.keys() {
                loader.client.cancel(*ticket);
            }
            loader.tickets.clear();
        }
        let needs = self.tree.refresh(id);
        self.ask(needs);
    }

    /// Refresh asked by the user (of `source`, else of everything): read again here, and
    /// what else knows the schemas (a console's completion) told they're stale.
    fn refresh_asked(&mut self, source: Option<DataSourceId>) {
        let ids: Vec<DataSourceId> = match source {
            Some(id) => vec![id],
            None => self.tree.sources().iter().map(|s| s.id).collect(),
        };
        if let Some(sessions) = &self.sessions {
            // A server's database: its source's (the consoles' indexes are its source's,
            // one per database).
            let mut told = Vec::new();
            for id in &ids {
                let source = self.tree.base_of(*id);
                if !told.contains(&source) {
                    told.push(source);
                    sessions.events().publish(&database::events::SchemaInvalidated { source });
                }
            }
        }
        match source {
            Some(id) => self.refresh_source(id),
            None => self.refresh_all(),
        }
    }

    /// Everything read again: the lists, then every open source.
    pub fn refresh_all(&mut self) {
        self.compose();
        let ids: Vec<DataSourceId> = self.tree.sources().iter().map(|s| s.id).collect();
        for id in ids {
            self.refresh_source(id);
        }
    }

    /// Opens `key` (what it needs read is asked).
    pub fn expand(&mut self, key: &NodeKey) {
        if let Some(need) = self.tree.expand(key) {
            self.ask(vec![need]);
        }
        self.stale = true;
    }

    fn toggle(&mut self, key: &NodeKey) {
        if let Some(need) = self.tree.toggle(key) {
            self.ask(vec![need]);
        }
        self.stale = true;
    }

    pub fn collapse_all(&mut self) {
        self.tree.collapse_all();
        self.stale = true;
        self.scroll = 0.0;
    }

    /// Selects `key`.
    pub fn select(&mut self, key: NodeKey) {
        self.selected = Some(key);
    }

    /// The selected row's place among the rows.
    fn selected_index(&mut self) -> Option<usize> {
        self.refresh_rows();
        // Its own row, else the nearest shown (a node closed or filtered out over it).
        crate::tree::nearest(&self.rows, self.selected.as_ref()?)
    }

    fn row(&self, key: &NodeKey) -> Option<&Row> {
        self.rows.iter().find(|r| r.key == *key)
    }

    /// The tree's source `id`.
    fn source(&self, id: DataSourceId) -> Option<&TreeSource> {
        self.tree.source(id)
    }

    /// The selected row acted on (Enter, a double click): a table's data opened, a node
    /// opened or closed.
    fn activate(&mut self, key: &NodeKey) {
        match key {
            NodeKey::Table(..) | NodeKey::Column(..) => self.open_table(key, false),
            // A source not configured yet, or refused: its settings.
            NodeKey::Status(parent)
                if matches!(**parent, NodeKey::Source(_))
                    && key
                        .source()
                        .and_then(|id| self.source(id))
                        .is_some_and(|s| s.scope.is_some()) =>
            {
                let refused = self.row(key).is_some_and(|r| r.kind == RowKind::Error);
                let focus = refused.then_some(crate::sources::Field::Password);
                self.open_sources(key.source(), None, focus);
            }
            _ if self.row(key).is_some_and(|r| r.expandable) => self.toggle(key),
            _ => {}
        }
    }

    /// Opens the table of `key` in a data view: the file's own view for a file (shown by
    /// itself), the source's otherwise; its structure if `structure`.
    pub fn open_table(&mut self, key: &NodeKey, structure: bool) {
        let (Some(id), Some(table)) = (key.source(), key.table().cloned()) else { return };
        let Some(source) = self.source(id).cloned() else { return };
        if let Some(file) = &source.file {
            let path = file.clone();
            let asked = FileTable { table, structure, column: None, filter: None };
            let open = ui_components::WithHost::once(move |host| {
                host.open_file_with(&path, &asked);
            });
            self.action = Some(ToolPanelAction::View(ViewerAction::WithHost(open)));
            return;
        }
        let Some(project) = self.project else { return };
        let open = crate::view::OpenSource {
            project,
            source: id,
            name: source.name,
            table: Some(table),
            structure,
            column: None,
            filter: None,
        };
        let title = open.name.clone();
        self.action = Some(ToolPanelAction::View(ViewerAction::OpenView(
            ui_components::OpenView::new(title, crate::provider::SOURCE_EXTENSION, open),
        )));
    }

    /// What's selected, told to the sessions (the consoles' commands run from the palette
    /// open the selected source's): only when it changed.
    fn share_selection(&self) {
        let Some(sessions) = &self.sessions else { return };
        let selected = self.selected.as_ref().and_then(|key| {
            let source = key.source()?;
            let (schema, table) = match key {
                NodeKey::Schema(_, schema) => (Some(schema.clone()), None),
                NodeKey::Group(_, schema, _) => (schema.clone(), None),
                other => (other.table().and_then(|t| t.schema.clone()), other.table().cloned()),
            };
            Some(crate::sessions::Selected { source, schema, table })
        });
        if sessions.selected() != selected {
            sessions.set_selected(selected);
        }
    }

    /// A query console of `key`'s source (its schema, its table), opened by the consoles'
    /// command (`new`: a new one); no source selected: the command asks which. A source
    /// without SQL, or a file not kept as a source, is told why not.
    pub fn console(&mut self, key: Option<&NodeKey>, new: bool) {
        self.console_listed(key, new, false);
    }

    /// [`Self::console`], the source's consoles listed even when it has none ("Database
    /// Consoles…": renamed and deleted there).
    pub fn console_listed(&mut self, key: Option<&NodeKey>, new: bool, list: bool) {
        let command = |id: &str| ToolPanelAction::View(ViewerAction::RunCommand(id.to_owned()));
        let Some((id, sessions)) = key.and_then(NodeKey::source).zip(self.sessions.clone()) else {
            let id =
                if new { database::console::NEW_COMMAND } else { database::console::JUMP_COMMAND };
            self.action = Some(command(id));
            return;
        };
        let Some(source) = self.source(id).cloned() else { return };
        if !source.configured || source.scope.is_none() {
            self.notify(workbench_model::Notification::info(i18n::tr!(
                "datasources.console_keep_first",
                name = source.name.as_str()
            )));
            return;
        }
        let has_sql = sessions.driver(&source.driver).is_some_and(|d| d.dialect().is_some());
        if !has_sql {
            self.notify(workbench_model::Notification::info(i18n::tr!(
                "datasources.console_no_sql_named",
                name = source.name.as_str()
            )));
            return;
        }
        let (schema, table) = match key {
            Some(NodeKey::Schema(_, schema)) => (Some(schema.clone()), None),
            Some(NodeKey::Group(_, schema, _)) => (schema.clone(), None),
            Some(other) => (other.table().and_then(|t| t.schema.clone()), other.table().cloned()),
            None => (None, None),
        };
        // Another database of a source's server: the source's console, on that database.
        let (id, database) = match sessions.catalog().database_of(id) {
            Some((base, name)) => (base, Some(name)),
            None => (id, None),
        };
        let id = crate::console::ask_in(&sessions, id, database, schema, table, new, list);
        self.action = Some(command(id));
    }

    /// The relations diagram of `key`'s source (its schema; around its table), opened by the
    /// diagram's command; why not, told, if its session can't be had.
    pub fn show_diagram(&mut self, key: &NodeKey) {
        let (Some(id), Some(sessions)) = (key.source(), self.sessions.clone()) else { return };
        let Some(source) = self.source(id).cloned() else { return };
        let (schema, focus) = match key {
            NodeKey::Schema(_, schema) => (Some(schema.clone()), None),
            NodeKey::Group(_, schema, _) => (schema.clone(), None),
            other => (None, other.table().cloned()),
        };
        let access = crate::diagram::SourceSchema::open(
            &sessions,
            id,
            &source.name,
            &source.driver,
            source.file.as_deref(),
        );
        match access {
            Ok(access) => {
                let (title, request) = crate::diagram::request(&sessions, access, schema, focus);
                self.action = Some(ToolPanelAction::View(ViewerAction::OpenView(
                    crate::diagram::view(title, request),
                )));
            }
            Err(e) => self.notify(workbench_model::Notification::error(message(&e))),
        }
    }

    /// Opens a source's tree at `reveal` (its schema, tables, the table and its columns),
    /// the table selected.
    pub fn reveal(&mut self, reveal: &Reveal) {
        let id = reveal.source;
        // A server's database (known to the catalog before the tree lists it): its source.
        let base = match &self.sessions {
            Some(sessions) => sessions.catalog().base_id(id),
            None => self.tree.base_of(id),
        };
        self.expand(&NodeKey::Source(base));
        // A server's databases between: the one shown (the source's own by its identity).
        if self.source(base).is_some_and(|s| s.databases) {
            self.expand(&NodeKey::Database(id));
        }
        let Some(table) = &reveal.table else {
            self.selected = Some(NodeKey::Source(base));
            return;
        };
        if let Some(schema) = &table.schema {
            self.expand(&NodeKey::Schema(id, schema.clone()));
        }
        self.expand(&NodeKey::Group(id, table.schema.clone(), Group::Tables));
        self.expand(&NodeKey::Table(id, table.clone()));
        if reveal.columns {
            self.expand(&NodeKey::Part(id, table.clone(), Part::Columns));
        }
        self.selected = Some(NodeKey::Table(id, table.clone()));
    }

    // ─── Adding and removing ───

    // ─── Menus ───

    // ─── Keys ───

    /// The selection moved to row `i` (kept in view).
    fn select_index(&mut self, i: usize) {
        let Some(row) = self.rows.get(i) else { return };
        self.selected = Some(row.key.clone());
        let h = (ROW_HEIGHT * self.scale).round();
        let top = i as f32 * h;
        let view = self.list.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if view > 0.0 && top + h > self.scroll + view {
            self.scroll = top + h - view;
        }
    }

    /// A key on the rows.
    fn row_key(&mut self, key: ViewerKey) -> bool {
        self.refresh_rows();
        let len = self.rows.len();
        if len == 0 {
            return false;
        }
        let at = self.selected_index();
        let page = ((self.list.size.height / (ROW_HEIGHT * self.scale)).floor() as usize).max(1);
        match key {
            ViewerKey::Down => self.select_index(at.map_or(0, |i| (i + 1).min(len - 1))),
            ViewerKey::Up => self.select_index(at.map_or(0, |i| i.saturating_sub(1))),
            ViewerKey::Home => self.select_index(0),
            ViewerKey::End => self.select_index(len - 1),
            ViewerKey::PageDown => self.select_index(at.map_or(0, |i| (i + page).min(len - 1))),
            ViewerKey::PageUp => self.select_index(at.map_or(0, |i| i.saturating_sub(page))),
            ViewerKey::Right => {
                let Some(i) = at else { return false };
                let row = self.rows[i].clone();
                if row.expandable && !row.expanded {
                    self.expand(&row.key);
                } else if row.expanded && i + 1 < len {
                    self.select_index(i + 1);
                }
            }
            ViewerKey::Left => {
                let Some(i) = at else { return false };
                let row = self.rows[i].clone();
                if row.expanded && self.filter.text().trim().is_empty() {
                    self.tree.collapse(&row.key);
                    self.stale = true;
                } else if let Some(parent) = (0..i).rev().find(|&j| self.rows[j].depth < row.depth)
                {
                    self.select_index(parent);
                }
            }
            ViewerKey::Enter => {
                let Some(i) = at else { return false };
                let key = self.rows[i].key.clone();
                self.activate(&key);
            }
            ViewerKey::Escape if !self.filter.text().is_empty() => {
                self.filter.set_text("");
                self.stale = true;
            }
            _ => return false,
        }
        true
    }

    /// The filter changed: the rows made again from the top.
    fn filtered(&mut self) {
        self.stale = true;
        self.scroll = 0.0;
    }

    /// The row at `p`, if any.
    fn row_at(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) {
            return None;
        }
        let h = (ROW_HEIGHT * self.scale).round();
        let list = ui_components::TreeList::new(h, self.rows.len());
        list.row_at(p.y - self.list.min_y() + self.scroll - (6.0 * self.scale).round())
    }

    fn hit(&self, p: Point) -> Option<Hover> {
        if self.clear.is_some_and(|r| r.contains(p)) {
            return Some(Hover::Clear);
        }
        if self.field.contains(p) {
            return Some(Hover::Field);
        }
        self.row_at(p).map(Hover::Row)
    }

    /// Whether `p` is on row `i`'s chevron.
    fn on_chevron(&self, i: usize, p: Point) -> bool {
        let Some(row) = self.rows.get(i) else { return false };
        let x =
            self.list.min_x() + ((PADDING + f32::from(row.depth) * INDENT) * self.scale).round();
        row.expandable && p.x >= x - 4.0 * self.scale && p.x <= x + 16.0 * self.scale
    }
}

/// Pushes `change` into the merging queue.
fn push(changes: &Mutex<Coalescing<Change>>, change: Change) {
    changes.lock().unwrap_or_else(PoisonError::into_inner).push(change, Instant::now());
}

/// A driver's icon: a server's, a database file's, a text file's.
pub(crate) fn driver_icon(sessions: &Sessions, driver: &str) -> &'static str {
    let file_based = sessions.driver(driver).is_some_and(|d| d.capabilities().file_based);
    if !file_based {
        return "server";
    }
    let text = database::drivers::files().iter().any(|f| f.driver == driver && f.text);
    if text { "file-text" } else { "database" }
}

/// `source` as the tree shows it.
fn tree_source(
    sessions: &Sessions,
    source: &DataSource,
    scope: Option<Scope>,
    file: Option<PathBuf>,
) -> TreeSource {
    let driver = sessions.driver(&source.driver);
    let capabilities = driver.as_ref().map(|d| d.capabilities()).unwrap_or_default();
    let driver_name = database::drivers::find(&source.driver)
        .map_or_else(|| source.driver.clone(), |f| f.name.into_owned());
    TreeSource {
        id: source.id,
        name: source.name.clone(),
        driver: source.driver.clone(),
        driver_name,
        icon: driver_icon(sessions, &source.driver),
        scope,
        file: file.filter(|_| capabilities.file_based),
        configured: driver.is_some() && source.is_configured(capabilities.file_based),
        read_only: source.read_only,
        schema_changes: if source.read_only {
            database::SchemaChanges::default()
        } else {
            capabilities.schema_changes
        },
        schemas: source.schemas.clone(),
        // A server with several databases (its console's capabilities say).
        databases: !capabilities.file_based && capabilities.console.is_some_and(|c| c.databases),
        shown_databases: source.databases.clone(),
        // A server's version once it's known (a file's engine isn't shown: its driver is).
        server: (!capabilities.file_based)
            .then(|| sessions.get(source.id).and_then(|w| w.server()))
            .flatten(),
        folder: source.folder,
        color: source.color,
        production: source.production,
        transaction: sessions.console_transaction(source.id),
    }
}

/// Reads the window's sources again (F5 while it has the keys).
pub const REFRESH: command_model::CommandId = command_model::CommandId("database.refresh_sources");

/// Shows / hides the window.
pub const TOGGLE: command_model::CommandId = command_model::CommandId("database.window");

#[cfg(test)]
#[path = "../../tests/unit/window.rs"]
mod tests;
