//! The data view: a file a driver opens by itself, shown as data — a SQLite database
//! instead of its bytes (the tables listed on the left, the chosen one's rows or structure
//! on the right), a CSV file beside its text (one table: the file). Its rows are read
//! through the file's own source (one per file and project, shared by the views showing
//! it, closed with the last of them), in the background; the grid is
//! `ui_components::data_grid`'s, over a [`TableSource`].
//!
//! Nothing starts until it's first drawn (it needs the app's waker); everything goes with
//! the view (its grid, its blocks of rows, its use of the source). A file changed on disk
//! by another program is read again; our own commit isn't taken for one. While a CSV's
//! text has unsaved changes the grid shows what's on disk and says the text must be saved
//! first (the two would write over each other).

mod counts;
mod paint;
mod structure;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use database::diagram::FileTable;
use database::{DataFiles, DbError, RepositoryEvent, TableInfo, TableKind};
use kernel::{EventBus, Point, ProjectId, Subscription};
use ui_components::data_grid::{DataGrid, ExportFormat, GridAction, GridSource, MENU_PREFIX, Tool};
use ui_components::{
    AppRequest, KeyReach, PressMods, Restored, Splitter, ViewMemory, ViewMenuEntry, Viewer,
    ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};

use crate::grid_source::{Abilities, TableSource, message};
use crate::repository::SessionRepository;
use crate::runtime::Runtime;
use crate::sessions::{FileLease, Sessions};
use crate::worker::{Client, Request, Response, SessionWorker, Ticket};
use workbench_model::Notification;

mod export;
mod schema;
mod viewer;

/// The view's kind (its viewer's id: where its keys work).
pub const VIEW: &str = "data";

/// What the right side shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Data,
    Structure,
}

/// A part of the view's own chrome (the grid has its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Table(usize),
    Tab(Tab),
    /// The export's Cancel.
    ExportCancel,
    /// A structure section's button adding one.
    Add(structure::Part),
    /// A row of a structure section.
    Row(structure::Part, usize),
    /// "Jump to Query Console…" (a configured source writing SQL).
    Console,
}

/// The file's size and time when last seen: a change reported by the disk that leaves
/// them as our commit did isn't another program's.
type Stamp = Option<(u64, SystemTime)>;

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

/// Wakes the app from a background thread.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// What the view runs on once started.
struct Open {
    /// The project's sessions, or the view's own (a file outside any project).
    sessions: Arc<Sessions>,
    worker: SessionWorker,
    /// A file's: its source closes with the last view of it.
    _lease: Option<FileLease>,
    /// The tables' list and their counts.
    client: Client,
    tables_ticket: Option<Ticket>,
    /// The disk said the file changed (another program, or our commit).
    changed: Arc<AtomicBool>,
    _watch: Option<Subscription>,
    /// A commit changed the source's structure (here, or in another view of it).
    restructured: Arc<AtomicBool>,
    _structure_watch: Subscription,
}

/// What the "Database" window asks a view to open: configured source `source` of project
/// `project` (its name the tab's title), table `table` shown if given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSource {
    pub project: ProjectId,
    pub source: database::DataSourceId,
    pub name: String,
    pub table: Option<database::TableRef>,
    /// Its structure shown (not its rows).
    pub structure: bool,
    /// The column pointed at in its structure (a console's "Go to Definition" on it).
    pub column: Option<String>,
    /// Its rows filtered by this condition (a console's "Go to Referenced Row").
    pub filter: Option<String>,
}

impl OpenSource {
    /// As a table's opening, for what isn't a view of the explorer (a diagram, a console).
    pub fn into_opening(self) -> database::diagram::Opening {
        let title = self.name.clone();
        database::diagram::Opening::View(database::diagram::TableView {
            title,
            extension: crate::provider::SOURCE_EXTENSION,
            data: std::sync::Arc::new(self),
        })
    }
}

/// A file's view shows `table`'s rows, filtered by `filter` if any.
pub fn rows_of(table: database::TableRef, filter: Option<String>) -> FileTable {
    FileTable { table, structure: false, column: None, filter }
}

/// The key of the tables list's width in the project's sizes
/// ([`workbench_model::LayoutSizes`]), and its bounds (logical pixels).
pub const LIST_SIZE: &str = "database.tables-list";
const LIST_MIN: f32 = 160.0;
const LIST_MAX: f32 = 520.0;

/// The view (see the module's doc).
pub struct DataView {
    path: PathBuf,
    files: DataFiles,
    /// A configured source's view (from the "Database" window), not a file's: its id and
    /// name (the title).
    source: Option<(database::DataSourceId, String)>,
    /// The table-asking generation last looked at.
    project: Option<ProjectId>,
    /// The app's waker, as last given to a paint (the background's replies wake it).
    waker: Arc<Mutex<Option<Waker>>>,
    open: Option<Open>,
    /// Why it couldn't be opened.
    error: Option<String>,
    pub(crate) tables: Vec<TableInfo>,
    /// The tables' counts: estimated, then counted (see [`counts`]).
    pub(crate) counts: counts::ListCounts,
    pub(crate) tables_loading: bool,
    pub(crate) selected: Option<usize>,
    pub(crate) grid: Option<DataGrid<TableSource>>,
    pub(crate) tab: Tab,
    /// The editor's text of the file has unsaved changes (a CSV).
    pub(crate) unsaved: bool,
    /// The list's width (logical pixels), its scroll, the structure's scroll.
    pub(crate) list_width: f32,
    pub(crate) list_scroll: f32,
    pub(crate) structure_scroll: f32,
    /// The tables' and the structure's scrollbars, hidden at rest.
    pub(crate) list_bar: ui_components::EdgeScrollbar,
    pub(crate) structure_bar: ui_components::EdgeScrollbar,
    /// The column pointed at in the structure (a console's "Go to Definition").
    pub(crate) structure_focus: Option<String>,
    /// It's still to be brought into view.
    pub(crate) structure_reveal: bool,
    pub(crate) splitter: Splitter,
    /// The project's dragged sizes: the list's width is kept there ([`LIST_SIZE`]).
    sizes: Option<workbench_model::LayoutSizes>,
    hits: Vec<(kernel::Rect, Hit)>,
    /// Tooltips of what isn't clicked (a name cut, a key's mark, a button drawn as its
    /// icon only), from the last paint.
    tips: Vec<(kernel::Rect, String)>,
    hover: Option<Hit>,
    /// The page size taken from the settings (their generation).
    page_generation: u64,
    /// The grid's layout version last kept.
    kept_version: u64,
    /// The file as our last commit left it.
    own: Stamp,
    /// Its file's size as last seen (when its tables came, after our own writes): what a
    /// click on a table needs of it, never read from the disk then.
    file_bytes: Option<u64>,
    action: Option<ViewerAction>,
    /// Changes shown in the preview, waiting for its answer.
    asked: Option<(database::ChangeSet, Arc<Mutex<Option<bool>>>)>,
    /// The structure's modal open, its outcome to come here.
    schema_answer: Option<crate::schema::Answer>,
    /// The table to show once the tables are listed again (one created or renamed).
    reselect: Option<database::TableRef>,
    /// The filter a key's cell led to, set once its table's grid knows its columns.
    pending_where: Option<String>,
    /// What it told the user (how a submit or an export went, why something couldn't be),
    /// waiting for the app to show it as notifications.
    pub(crate) told: Vec<Notification>,
    /// The export running (its progress shown over the grid).
    pub(crate) export: Option<crate::export::Export>,
    /// Its rows when last drawn.
    export_seen: u64,
    /// The format of an export waiting for its file to be chosen.
    export_format: Option<ExportFormat>,
    /// Where the grid is (the right side, under its header).
    pub(crate) grid_area: kernel::Rect,
    pub(crate) list_area: kernel::Rect,
    pub(crate) structure_area: kernel::Rect,
    /// The last paint's scale, and where the mouse last was.
    pub(crate) last_scale: f32,
    last_hover: Point,
    /// Its source's color (its tab's), as last worked out.
    color: crate::catalog::ColorCache,
    /// Whether its source has query consoles, for the catalog's generation (its header's
    /// button, worked out again only when the sources change).
    pub(crate) console_cache: Option<(u64, bool)>,
}

impl std::fmt::Debug for DataView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataView").field("path", &self.path).finish_non_exhaustive()
    }
}

impl DataView {
    pub fn new(path: PathBuf, files: DataFiles) -> Self {
        Self {
            path,
            files,
            source: None,
            // Looked at once first: the table may have been asked before the view was made.
            project: None,
            waker: Arc::new(Mutex::new(None)),
            open: None,
            error: None,
            tables: Vec::new(),
            counts: counts::ListCounts::default(),
            tables_loading: true,
            selected: None,
            grid: None,
            tab: Tab::Data,
            unsaved: false,
            list_width: 240.0,
            list_scroll: 0.0,
            structure_scroll: 0.0,
            list_bar: ui_components::EdgeScrollbar::auto_hiding(),
            structure_bar: ui_components::EdgeScrollbar::auto_hiding(),
            structure_focus: None,
            structure_reveal: false,
            splitter: Splitter::new(Default::default(), ui_components::Orientation::Vertical),
            sizes: None,
            hits: Vec::new(),
            tips: Vec::new(),
            hover: None,
            page_generation: crate::settings::page_generation(),
            kept_version: 0,
            own: None,
            file_bytes: None,
            action: None,
            asked: None,
            schema_answer: None,
            reselect: None,
            pending_where: None,
            told: Vec::new(),
            export: None,
            export_seen: 0,
            export_format: None,
            grid_area: kernel::Rect::default(),
            list_area: kernel::Rect::default(),
            structure_area: kernel::Rect::default(),
            last_scale: 1.0,
            last_hover: Point::new(0.0, 0.0),
            color: crate::catalog::ColorCache::default(),
            console_cache: None,
        }
    }

    /// A view of a configured source (its id given by [`OpenSource`] before it's drawn).
    pub fn for_source() -> Self {
        // A configured source's view has no file of its own.
        Self::new(PathBuf::new(), DataFiles::none())
    }

    /// The configured source shown, if it's one (not a file).
    pub fn source_id(&self) -> Option<database::DataSourceId> {
        self.source.as_ref().map(|(id, _)| *id)
    }

    /// Its title: the file's name, or the source's.
    pub(crate) fn title(&self) -> String {
        match &self.source {
            Some((_, name)) => name.clone(),
            None => {
                self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            }
        }
    }

    /// `table` shown: now if it's listed, else once the tables are.
    pub fn show_table(&mut self, table: database::TableRef) {
        self.tab = Tab::Data;
        self.structure_focus = None;
        match self.tables.iter().position(|t| t.table == table) {
            Some(i) if !self.tables_loading => self.select(i),
            _ => self.reselect = Some(table),
        }
    }

    /// A table asked for this file's view.
    fn show_asked(&mut self, asked: &FileTable) {
        self.show_table(asked.table.clone());
        if asked.structure {
            self.tab = Tab::Structure;
            self.point_at(asked.column.clone());
        }
        self.filter_to(asked.filter.clone());
    }

    /// Column `column` pointed at in the structure (lit, brought into view once drawn).
    pub fn point_at(&mut self, column: Option<String>) {
        self.structure_reveal = column.is_some();
        self.structure_focus = column;
    }

    /// The column pointed at in the structure.
    pub fn pointed_column(&self) -> Option<&str> {
        self.structure_focus.as_deref()
    }

    /// A text file (a CSV): one table, shown beside the text.
    pub fn is_text(&self) -> bool {
        self.files.text
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Its file (none for a configured source's view).
    fn file(&self) -> Option<&Path> {
        (self.source.is_none() && !self.path.as_os_str().is_empty()).then_some(self.path.as_path())
    }

    /// The table shown.
    pub fn table(&self) -> Option<&TableInfo> {
        self.selected.and_then(|i| self.tables.get(i))
    }

    /// The configured source it shows (a file kept as one too), when it writes SQL: only
    /// those have query consoles.
    pub(crate) fn console_source(&self) -> Option<database::DataSource> {
        let open = self.open.as_ref()?;
        let source = match &self.source {
            Some((id, _)) => open.sessions.catalog().get(*id)?,
            None => crate::diagram::configured(
                &open.sessions,
                open.worker.source().id,
                Some(&self.path),
            )?,
        };
        crate::console::has_sql(&open.sessions, &source).then_some(source)
    }

    /// Its source has query consoles (kept for the catalog's generation).
    pub(crate) fn has_console(&mut self) -> bool {
        let Some(generation) = self.open.as_ref().map(|o| o.sessions.catalog().generation()) else {
            return false;
        };
        match self.console_cache {
            Some((g, has)) if g == generation => has,
            _ => {
                let has = self.console_source().is_some();
                self.console_cache = Some((generation, has));
                has
            }
        }
    }

    /// A query console of its source, on table `row` (none: the one shown), opened by the
    /// consoles' command (`new`: a new one).
    fn console(&mut self, row: Option<usize>, new: bool) -> bool {
        let Some(source) = self.console_source() else { return false };
        let Some(open) = &self.open else { return false };
        let table = row.or(self.selected).and_then(|i| self.tables.get(i)).map(|t| t.table.clone());
        let schema = table.as_ref().and_then(|t| t.schema.clone());
        let id = crate::console::ask(&open.sessions, source.id, schema, table, new);
        self.action = Some(ViewerAction::RunCommand(id.to_owned()));
        true
    }

    fn wake(&self) -> Arc<dyn Fn() + Send + Sync> {
        let cell = Arc::clone(&self.waker);
        Arc::new(move || {
            let waker = cell.lock().unwrap_or_else(PoisonError::into_inner).clone();
            if let Some(waker) = waker {
                waker();
            }
        })
    }

    /// Opens a configured source (the first paint): through its project's sessions.
    fn start_source(&mut self, id: database::DataSourceId) {
        let Some(sessions) = self.project.and_then(crate::sessions::sessions) else {
            self.error = Some(i18n::t("datasources.project_closed"));
            return;
        };
        let worker = match sessions.source_worker(id) {
            Ok(worker) => worker,
            Err(e) => {
                self.error = Some(message(&e));
                return;
            }
        };
        let wake = self.wake();
        let client = worker.client(Arc::clone(&wake));
        let tables_ticket = Some(client.send(Request::Tables(Vec::new())));
        let restructured = Arc::new(AtomicBool::new(false));
        let structure_watch = {
            let (flag, wake) = (Arc::clone(&restructured), Arc::clone(&wake));
            sessions.events().subscribe(move |e: &database::events::DataChanged| {
                if e.source == id && e.structure {
                    flag.store(true, Ordering::Release);
                    wake();
                }
            })
        };
        self.tables_loading = true;
        self.open = Some(Open {
            sessions,
            worker,
            _lease: None,
            client,
            tables_ticket,
            changed: Arc::default(),
            _watch: None,
            restructured,
            _structure_watch: structure_watch,
        });
    }

    /// Opens the file's source (the first paint): the project's sessions, else its own.
    fn start(&mut self) {
        if self.open.is_some() || self.error.is_some() {
            return;
        }
        if let Some((id, _)) = &self.source {
            let id = *id;
            self.start_source(id);
            return;
        }
        if self.path.as_os_str().is_empty() {
            // A source's view not told which yet.
            return;
        }
        let sessions = self.project.and_then(crate::sessions::sessions).unwrap_or_else(|| {
            let root = self.path.parent().unwrap_or(Path::new("")).to_path_buf();
            Sessions::new(ProjectId::new(), &root, EventBus::new(), Runtime::pool(), None)
        });
        let mut options = Vec::new();
        if self.files.text {
            options.push((db_option_sample(), crate::settings::csv_sample().to_string()));
        }
        let (worker, lease) = match sessions.open_file(&self.path, &self.files.driver, &options) {
            Ok(opened) => opened,
            Err(e) => {
                self.error = Some(message(&e));
                return;
            }
        };
        let wake = self.wake();
        let client = worker.client(Arc::clone(&wake));
        let tables_ticket = Some(client.send(Request::Tables(Vec::new())));
        // The disk's changes of the file (or of a SQLite file's journal).
        let changed = Arc::new(AtomicBool::new(false));
        let watch = watch_file(&sessions, &changed, &wake, &self.path);
        // The source's structure changed (its tables, a table's columns): read again.
        let restructured = Arc::new(AtomicBool::new(false));
        let structure_watch = {
            let (flag, wake, id) =
                (Arc::clone(&restructured), Arc::clone(&wake), worker.source().id);
            sessions.events().subscribe(move |e: &database::events::DataChanged| {
                if e.source == id && e.structure {
                    flag.store(true, Ordering::Release);
                    wake();
                }
            })
        };
        self.tables_loading = true;
        self.open = Some(Open {
            sessions,
            worker,
            _lease: Some(lease),
            client,
            tables_ticket,
            changed,
            _watch: Some(watch),
            restructured,
            _structure_watch: structure_watch,
        });
    }

    /// Table `i` shown (its grid made, its layout as it was left).
    pub(crate) fn select(&mut self, i: usize) {
        if self.selected == Some(i) && self.grid.is_some() {
            return;
        }
        self.keep_layout();
        let Some(info) = self.tables.get(i).cloned() else { return };
        let Some(open) = &self.open else { return };
        self.selected = Some(i);
        self.structure_scroll = 0.0;
        let wake = self.wake();
        let repo = SessionRepository::new(
            &open.worker,
            info.table.clone(),
            wake,
            Some(open.sessions.events()),
            crate::cache::DEFAULT_LIMIT,
        );
        let capabilities = open.worker.source().capabilities;
        let abilities = Abilities {
            sorting: capabilities.sorting,
            edit_rows: capabilities.edit_rows,
            read_only: open.worker.source().read_only,
            view: info.kind == TableKind::View,
            bytes: self.file_bytes,
            cancel: capabilities.cancel,
        };
        let saved = match &self.source {
            Some((id, _)) => open.sessions.source_layout(*id, &info.table),
            None => open.sessions.layout(&self.path, &info.table),
        };
        let saved = saved.map(crate::layout::to_grid);
        // A page size chosen in this table's grid before, else the setting's.
        let page = saved.as_ref().and_then(|s| s.page).unwrap_or_else(crate::settings::page_size);
        let source =
            TableSource::new(Box::new(repo), info.table.clone(), abilities, page, info.rows)
                .with_dialect(open.worker.source().dialect.clone());
        let mut grid = DataGrid::new(source, page);
        grid.set_commands(tool_command);
        grid.set_project(self.project);
        if let Some(layout) = saved {
            grid.restore_layout(layout);
        }
        self.kept_version = grid.layout_version();
        self.grid = Some(grid);
    }

    /// The shown table's layout kept (it comes back when the table is shown again, also
    /// after the project is opened again).
    fn keep_layout(&mut self) {
        if let (Some(grid), Some(open), Some(info)) = (&self.grid, &self.open, self.table()) {
            let layout = crate::layout::from_grid(&grid.saved_layout());
            match &self.source {
                Some((id, _)) => open.sessions.keep_source_layout(*id, &info.table, layout),
                None => open.sessions.keep_layout(&self.path, &info.table, layout),
            }
            self.kept_version = grid.layout_version();
        }
    }

    /// The layout kept again if it changed (a column resized, moved, hidden; another
    /// order, filter, page size).
    fn follow_layout(&mut self) {
        if self.grid.as_ref().is_some_and(|g| g.layout_version() != self.kept_version) {
            self.keep_layout();
        }
    }

    /// The worker's replies (the tables, their counts).
    fn take_replies(&mut self) -> bool {
        let Some(open) = &mut self.open else { return false };
        let mut changed = false;
        let mut listed = None;
        for reply in open.client.poll() {
            if Some(reply.ticket) == open.tables_ticket {
                open.tables_ticket = None;
                listed = Some(reply.result);
                changed = true;
            } else if let Some(counted) = self.counts.take(&reply, Some(&open.client), &self.tables)
            {
                changed |= counted;
            } else {
                changed = true;
            }
        }
        match listed {
            Some(Ok(Response::Tables(tables))) => self.take_tables(tables),
            Some(Ok(_)) => {}
            Some(Err(DbError::Cancelled)) => self.tables_loading = false,
            Some(Err(e)) => {
                self.tables_loading = false;
                self.error = Some(message(&e));
            }
            None => {}
        }
        changed
    }

    /// The tables listed (tables first, then views, each by name); their estimates shown,
    /// every one counted when that's cheap (see [`counts`]).
    fn take_tables(&mut self, mut tables: Vec<TableInfo>) {
        self.tables_loading = false;
        // Each name lowercased once (thousands of tables).
        tables.sort_by_cached_key(|t| (t.kind == TableKind::View, t.table.name.to_lowercase()));
        // The one asked for (created, renamed), else the one shown stays shown (a
        // refresh), else the first.
        let shown = self.reselect.take().or_else(|| self.table().map(|t| t.table.clone()));
        self.tables = tables;
        self.file_bytes = self.file().and_then(|p| std::fs::metadata(p).ok()).map(|m| m.len());
        let all = self.cheap_counts();
        self.counts.reset(self.open.as_ref().map(|o| &o.client), &self.tables, all);
        let at = shown.and_then(|t| self.tables.iter().position(|i| i.table == t)).unwrap_or(0);
        if !self.tables.is_empty() {
            let same =
                self.grid.as_ref().is_some_and(|g| g.source().table() == &self.tables[at].table);
            if self.selected != Some(at) || !same {
                self.keep_layout();
                self.grid = None;
            }
            self.select(at);
        } else {
            self.selected = None;
            self.grid = None;
        }
    }

    /// Our own write noted (a commit, an export, a schema change): the file as it left it.
    pub(crate) fn note_own(&mut self) {
        self.own = stamp(&self.path);
        if self.file().is_some() {
            self.file_bytes = self.own.map(|(len, _)| len);
        }
    }

    /// The file changed on disk: unless our commit left it so, everything is read again.
    fn file_changed(&mut self) -> bool {
        let Some(open) = &self.open else { return false };
        if !open.changed.swap(false, Ordering::AcqRel) {
            return false;
        }
        let now = stamp(&self.path);
        if now.is_some() && now == self.own {
            return false;
        }
        self.reload();
        true
    }

    /// Every table counted exactly costs little: an SQL file small enough (a SQLite one;
    /// a CSV file's one table is counted by its grid).
    fn cheap_counts(&self) -> bool {
        let Some(open) = &self.open else { return false };
        let source = open.worker.source();
        if self.files.text || !source.capabilities.file_based || source.dialect.is_none() {
            return false;
        }
        let file = match &self.source {
            Some((id, _)) => open.sessions.file_of(*id),
            None => self.file().map(Path::to_path_buf),
        };
        file.and_then(|f| std::fs::metadata(f).ok()).is_some_and(|m| m.len() <= counts::CHEAP_FILE)
    }

    /// The shown table's exact rows, once its grid counted them (no filter): the list's
    /// too.
    fn follow_grid_total(&mut self) -> bool {
        let (Some(i), Some(grid)) = (self.selected, &self.grid) else { return false };
        let source = grid.source();
        if self.tables.get(i).is_none_or(|t| t.table != *source.table())
            || source.query().1.is_some()
        {
            return false;
        }
        match source.total().known() {
            Some(n) => self.counts.set_exact(i, n),
            None => false,
        }
    }

    /// The tables listed again and the grid read again.
    pub fn reload(&mut self) {
        self.counts.cancel(self.open.as_ref().map(|o| &o.client));
        if let Some(open) = &mut self.open {
            open.tables_ticket = Some(open.client.send(Request::Tables(Vec::new())));
            self.tables_loading = true;
        }
        if let Some(grid) = &mut self.grid {
            grid.refresh();
        }
    }

    /// What the grid's source said happened (a commit: the file as it left it noted).
    fn take_source_events(&mut self) {
        let Some(grid) = &mut self.grid else { return };
        for event in grid.source_mut().take_events() {
            if let RepositoryEvent::Applied(Ok(_)) = event {
                self.note_own();
            }
        }
    }

    /// The settings' page size, when it changed.
    fn follow_settings(&mut self) -> bool {
        let generation = crate::settings::page_generation();
        if generation == self.page_generation {
            return false;
        }
        self.page_generation = generation;
        let page = crate::settings::page_size();
        if let Some(grid) = &mut self.grid {
            grid.source_mut().set_page_size(page);
            grid.set_page_size(page);
        }
        true
    }

    fn take_grid_action(&mut self) {
        match self.grid.as_mut().and_then(DataGrid::take_action) {
            Some(GridAction::Copy(text)) => {
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            Some(GridAction::Submit) => self.ask_submit(),
            Some(GridAction::Export { format, clipboard }) => {
                let Some(format) = ExportFormat::find(self.project, &format) else { return };
                if clipboard {
                    self.start_export(format, crate::export::Target::Clipboard);
                } else {
                    self.ask_export(format);
                }
            }
            Some(GridAction::Command(command)) => {
                self.action = Some(ViewerAction::RunCommand(command));
            }
            None => {}
        }
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// Tells the user `notification` (the app shows it as a toast).
    fn notify(&mut self, notification: Notification) {
        self.told.push(notification);
    }

    fn in_grid(&self, p: Point) -> bool {
        self.tab == Tab::Data && self.grid_area.contains(p)
    }
}

/// Where the snapshot's mouse rests (`snapshot-point:<tool>`).
#[cfg(feature = "snapshot")]
pub static SNAPSHOT_POINT: Mutex<Option<Point>> = Mutex::new(None);

/// The command behind a grid toolbar's action (its keys shown in its tooltip and the "…"
/// menu): the module's own (crate::ACTIONS), and the app's Find, which a grid answers.
pub(crate) fn tool_command(tool: Tool) -> Option<&'static str> {
    Some(match tool {
        Tool::Refresh => "database.refresh",
        Tool::Transpose => "database.transpose",
        Tool::Submit => "database.submit",
        Tool::Add => "database.add_row",
        Tool::Delete => "database.delete_rows",
        Tool::Revert => "database.revert",
        // The app's Find (⌘F), sent to the view with the keys as `find`.
        Tool::Find => "find.open",
        _ => return None,
    })
}

/// The CSV driver's sample option's name (the explorer never names the driver's crate).
fn db_option_sample() -> &'static str {
    "sample_rows"
}

/// A new row's key as the engine gave it back (`id = 41`, `rowid 41`).
fn key_text(key: &database::RowKey) -> String {
    match key {
        database::RowKey::Primary(columns) => columns
            .iter()
            .map(|(c, v)| format!("{c} = {}", v.display()))
            .collect::<Vec<_>>()
            .join(", "),
        database::RowKey::RowId(id) => format!("rowid {id}"),
        database::RowKey::Line { line, .. } => i18n::tr!("database.summary_line", line = line + 1),
    }
}

/// What the disk says of file `path` (written, or its SQLite journal): `changed` set,
/// the view woken.
fn watch_file(
    sessions: &Sessions,
    changed: &Arc<AtomicBool>,
    wake: &Arc<dyn Fn() + Send + Sync>,
    path: &Path,
) -> Subscription {
    let (changed, wake, path) = (Arc::clone(changed), Arc::clone(wake), path.to_path_buf());
    sessions.events().subscribe(move |e: &workspace::events::FilesWritten| {
        if e.paths.iter().any(|p| touches(p, &path)) {
            changed.store(true, Ordering::Release);
            wake();
        }
    })
}

/// A path the disk reported is the file, or its SQLite journal beside it.
fn touches(reported: &Path, file: &Path) -> bool {
    if reported == file {
        return true;
    }
    let (Some(name), Some(file_name)) = (reported.file_name(), file.file_name()) else {
        return false;
    };
    let (name, file_name) = (name.to_string_lossy(), file_name.to_string_lossy());
    reported.parent() == file.parent()
        && ["-wal", "-journal"].iter().any(|s| name == format!("{file_name}{s}"))
}

impl Drop for DataView {
    fn drop(&mut self) {
        self.keep_layout();
    }
}

impl DataView {
    /// The scale of the last paint.
    fn scale(&self) -> f32 {
        self.last_scale
    }

    /// The mouse's last place (scrolling goes to what's under it).
    fn hover_point(&self) -> Point {
        self.last_hover
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
