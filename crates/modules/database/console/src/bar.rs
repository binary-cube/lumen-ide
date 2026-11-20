//! The console's bar above its editor (an [`EditorToolbar`]): ▶ Run (green)
//! and its ▾ (Execute, Execute Script, Explain Plan, Explain Analyze), the history (🕘), the
//! parameters (`(p)`), `Tx: Auto ▾` (Commit and Rollback while a transaction is open), ■
//! Stop, `Playground ▾` / `Script ▾`, the settings (⚙) and the Services window's toggle; on
//! the right, the database and schema it runs on (`main ▾`), a read-only source's note and
//! an old driver's ("limited results"), and what the last run did. Its tab is
//! `console [source]` with the driver's icon and the source's color; its breadcrumb starts
//! `Database Consoles › source › console`. Its file renamed, it follows it (the same console
//! under the new name).
//!
//! On a project's SQL file attached to a source (39n, inside [`crate::file_bar`]) the same
//! bar keeps the file's own title, icon and breadcrumb, and shows the source it runs on
//! left of the schema: its menu attaches the file to another source or detaches it.
//!
//! Its buttons run the module's commands (the active file's text is theirs through the
//! host); its menus' rows come back as [`EditorToolbar::command`]. What it shows of the
//! runner is read when the runner's generation changes, the layout worked out once per
//! width and state.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The icon of a server without one of its own (the `.sql` file's as well).
const GENERIC_ICON: &str = "database";

use database::console::TxState;
use database::console::{
    ConsoleId, ConsoleSpec, Overrides, PlanInput, RunMode, RunPlan, SourceBrief, TxMode,
};
use database::events::DataSourcesChanged;
use database::{DataSourceId, Isolation, SchemaSwitch};
use kernel::{Memo, Point, Rect, Rgba, Subscription};
use render_api::{Layer, Quad};
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::editor_toolbar::{
    EditorState, EditorToolbar, StatementMark, ToolbarAction, ToolbarPaint,
};
use ui_components::{Palette, Tip, ViewMenuEntry, ViewerAction, ViewerCursor};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::consoles::Consoles;
use crate::layout::{self, BarLayout, BarSpec, Metrics, Tool};
use crate::runner::{Lease, Runner, Snapshot};

/// The bar's height (logical pixels).
pub const HEIGHT: f32 = 40.0;

/// What the layout depends on (besides the width): the transaction, the texts' widths.
type LayoutKey = (
    u32,
    u32,
    bool,
    u32,
    u32,
    u32,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
    Option<u32>,
);

/// A console's bar (see the module's doc).
pub struct ConsoleBar {
    consoles: Arc<Consoles>,
    console: ConsoleId,
    source: DataSourceId,
    runner: Runner,
    _lease: Lease,
    spec: ConsoleSpec,
    /// The source as last read; read again when the sources change.
    brief: Option<SourceBrief>,
    stale: Arc<AtomicBool>,
    _sources: Option<Subscription>,
    title: String,
    snapshot: Snapshot,
    seen: Option<u64>,
    layout: Memo<LayoutKey, BarLayout>,
    hits: Vec<(Rect, Tool)>,
    hover: Option<Tool>,
    /// The editor as last told (while Run is hovered, and at each press).
    editor: Option<EditorState>,
    highlight: Option<Range<usize>>,
    /// The statement the cursor is in (what Run runs): a bar beside its lines.
    current: Option<Range<usize>>,
    action: Option<ToolbarAction>,
    notifications: Vec<Notification>,
    /// The switcher's menu waits for its lists.
    place_menu: bool,
    /// What a profile's finding points at, lit while its statement is the one profiled.
    focused: Option<Range<usize>>,
}

impl Drop for ConsoleBar {
    /// Its file's findings go with it.
    fn drop(&mut self) {
        let path = self.spec.path(self.consoles.root());
        self.consoles.checks().forget(&self.consoles, &path);
    }
}

impl std::fmt::Debug for ConsoleBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConsoleBar").field("console", &self.console).finish_non_exhaustive()
    }
}

impl ConsoleBar {
    /// Console `spec`'s bar, its connection's lease taken (nothing connects yet).
    pub fn new(consoles: Arc<Consoles>, spec: ConsoleSpec, runner: Runner) -> Self {
        let stale = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&stale);
        let sources = Some(consoles.events().subscribe(move |_: &DataSourcesChanged| {
            flag.store(true, Ordering::Release);
        }));
        let lease = runner.lease();
        let mut bar = Self {
            console: spec.id,
            source: spec.source,
            _lease: lease,
            title: spec.name.clone(),
            spec,
            consoles,
            runner,
            brief: None,
            stale,
            _sources: sources,
            snapshot: Snapshot::default(),
            seen: None,
            layout: Memo::default(),
            hits: Vec::new(),
            hover: None,
            editor: None,
            highlight: None,
            current: None,
            action: None,
            notifications: Vec::new(),
            place_menu: false,
            focused: None,
        };
        bar.refresh();
        bar
    }

    pub fn console(&self) -> ConsoleId {
        self.console
    }

    /// Where control `tool` was last drawn.
    #[cfg(test)]
    pub(crate) fn hit_of(&self, tool: Tool) -> Option<Rect> {
        self.hits.iter().find(|(_, t)| *t == tool).map(|(r, _)| *r)
    }

    /// The source it runs on.
    pub fn source(&self) -> DataSourceId {
        self.source
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    /// The source and the console's settings read again when they changed; the runner's
    /// state when its generation did. `true` if something did.
    fn refresh(&mut self) -> bool {
        let mut changed = false;
        if self.stale.swap(false, Ordering::AcqRel) || self.brief.is_none() {
            let brief = self.consoles.access().and_then(|a| a.source(self.source));
            if brief.is_some() || self.brief.is_some() {
                changed |= brief != self.brief;
                self.brief = brief;
                if let Some(access) = self.consoles.access() {
                    self.runner.set_idle(access.idle_close());
                }
            }
        }
        if let Some(spec) = self.consoles.spec(self.console)
            && spec != self.spec
        {
            self.spec = spec;
            changed = true;
        }
        let title = match &self.brief {
            Some(brief) if !self.spec.is_attached() => {
                format!("{} [{}]", self.spec.name, brief.name)
            }
            _ => self.spec.name.clone(),
        };
        if title != self.title {
            self.title = title;
            changed = true;
        }
        let generation = self.runner.generation();
        if self.seen != Some(generation) {
            self.seen = Some(generation);
            self.snapshot = self.runner.snapshot();
            changed = true;
            self.focused = self.focus_range();
            // How long the last run took, kept with the console (its lists show it).
            let ms = self.snapshot.last.as_ref().map(|l| l.elapsed.as_millis() as u64);
            if !self.snapshot.busy
                && ms.is_some()
                && ms != self.spec.last_run_ms
                && let Some(spec) = self.consoles.update(self.console, |s| s.last_run_ms = ms)
            {
                self.spec = spec;
            }
            // The switcher's lists came: its menu opens.
            if self.place_menu && !self.snapshot.busy {
                self.place_menu = false;
                if self.snapshot.places.is_some() {
                    self.action = self.place_menu_action();
                }
            }
        }
        changed
    }

    /// Its source is production (its own mark or its folder's).
    fn production(&self) -> bool {
        self.brief.as_ref().is_some_and(|b| b.production)
    }

    /// `Tx: Auto`, with what waits for Commit while a transaction is open (`Tx: Manual · 2`),
    /// or that it failed.
    fn tx_label(&self) -> String {
        let mode = i18n::t(match self.spec.tx {
            TxMode::Auto => "console.tx_auto",
            TxMode::Manual => "console.tx_manual",
        });
        match self.snapshot.tx {
            TxState::Failed => i18n::t("console.tx_label_failed"),
            TxState::Open { writes, .. } if writes > 0 => {
                i18n::tr!("console.tx_label_pending", mode = mode, count = writes)
            }
            _ => i18n::tr!("console.tx_label", mode = mode),
        }
    }

    fn capabilities(&self) -> database::ConsoleCaps {
        self.brief
            .as_ref()
            .and_then(|b| b.capabilities.console)
            .unwrap_or(database::ConsoleCaps::BASIC)
    }

    /// The database and schema it runs on, as the switcher shows them.
    fn place_label(&self) -> String {
        let place = &self.snapshot.place;
        let database = self
            .spec
            .database
            .clone()
            .or_else(|| place.database.clone())
            .or_else(|| self.brief.as_ref().and_then(|b| b.database.clone()));
        let schema = self.spec.schema.clone().or_else(|| place.schema.clone());
        match (database, schema) {
            (Some(d), Some(s)) => format!("{d}.{s}"),
            (Some(one), None) | (None, Some(one)) => one,
            (None, None) => {
                if self.brief.as_ref().is_some_and(|b| b.file_based) {
                    "main".to_owned()
                } else {
                    i18n::t("console.default_schema")
                }
            }
        }
    }

    /// What the last run did, or what runs, and whether it's an error.
    fn status(&self) -> Option<(String, bool)> {
        if let Some(running) = &self.snapshot.running {
            return Some((
                if running.total > 1 {
                    i18n::tr!("console.running_n", index = running.index + 1, total = running.total)
                } else {
                    i18n::t("console.running")
                },
                false,
            ));
        }
        let last = self.snapshot.last.as_ref()?;
        let time = crate::runner::millis(last.elapsed);
        if let Some(error) = &last.error {
            return Some((kernel::text::one_line(error).into_owned(), true));
        }
        if last.cancelled {
            return Some((i18n::t("console.cancelled"), false));
        }
        let text = if let Some(steps) = last.plan_steps {
            i18n::tr!("console.status_plan", count = steps, time = time)
        } else if let Some(rows) = last.rows {
            let key =
                if last.truncated { "console.status_rows_more" } else { "console.status_rows" };
            i18n::tr!(key, count = rows, time = time)
        } else if let Some(n) = last.affected {
            i18n::tr!("console.status_affected", count = n, time = time)
        } else {
            i18n::tr!("console.status_done", time = time)
        };
        let text = if last.total > 1 {
            format!("{} · {}", i18n::tr!("console.status_statements", count = last.done), text)
        } else {
            text
        };
        Some((text, false))
    }

    /// Runs command `id` of the module (a button), as from its menu.
    /// Work only the host can start, handed over through the bar's action.
    fn with_host(
        &mut self,
        job: impl FnOnce(&mut dyn workbench_model::WorkbenchHost) + Send + 'static,
    ) {
        let work = ui_components::WithHost::once(job);
        self.action = Some(ToolbarAction::View(ViewerAction::WithHost(work)));
    }

    /// A setting changed from the ⚙ menu.
    fn save_setting(&mut self, change: crate::commands::SettingChange) {
        self.with_host(move |host| {
            crate::commands::save_setting(host, change);
        });
    }

    fn run_command(&mut self, id: &str) {
        self.action = Some(ToolbarAction::View(ViewerAction::RunCommand(id.to_owned())));
    }

    /// A menu under `tool`'s control.
    fn menu(&self, tool: Tool, entries: Vec<ViewMenuEntry>) -> Option<ToolbarAction> {
        let (rect, _) = self.hits.iter().find(|(_, t)| *t == tool)?;
        Some(ToolbarAction::Menu { anchor: *rect, entries })
    }

    fn checked(entry: ViewMenuEntry, on: bool) -> ViewMenuEntry {
        if on { entry.with_icon("check") } else { entry }
    }

    fn run_menu(&self) -> Vec<ViewMenuEntry> {
        let keys = |id: &str| ui_components::shortcuts::label(id);
        let item = |id: &str, key: &str, icon: &'static str, command: &str| {
            let entry = ViewMenuEntry::item(id, i18n::t(key)).with_icon(icon);
            match keys(command) {
                Some(k) => entry.with_shortcut(k),
                None => entry,
            }
        };
        let caps = self.capabilities();
        let mut entries = vec![
            item("run", "console.menu_execute", "play", crate::RUN.0),
            item("run-script", "console.menu_execute_script", "play", crate::RUN_SCRIPT.0),
        ];
        if caps.explain.plan || caps.explain.analyze {
            entries.push(ViewMenuEntry::Separator);
        }
        if caps.explain.plan {
            entries.push(item("explain", "console.menu_explain", "plan", crate::EXPLAIN.0));
        }
        if caps.explain.analyze {
            entries.push(item(
                "explain-analyze",
                "console.menu_explain_analyze",
                "plan",
                crate::EXPLAIN_ANALYZE.0,
            ));
        }
        entries
    }

    fn tx_menu(&self) -> Vec<ViewMenuEntry> {
        let mut entries = vec![
            Self::checked(
                ViewMenuEntry::item("tx:auto", i18n::t("console.tx_auto_long")),
                self.spec.tx == TxMode::Auto,
            ),
            Self::checked(
                ViewMenuEntry::item("tx:manual", i18n::t("console.tx_manual_long")),
                self.spec.tx == TxMode::Manual,
            ),
        ];
        let levels: Vec<Isolation> = Isolation::ALL
            .into_iter()
            .filter(|i| self.capabilities().isolation.contains(*i))
            .collect();
        if !levels.is_empty() {
            entries.push(ViewMenuEntry::Separator);
            let mut rows = vec![Self::checked(
                ViewMenuEntry::item("isolation:default", i18n::t("console.isolation_default")),
                self.spec.isolation.is_none(),
            )];
            rows.extend(levels.into_iter().map(|level| {
                Self::checked(
                    ViewMenuEntry::item(
                        format!("isolation:{}", level.id()),
                        i18n::t(&format!("console.isolation_{}", level.id())),
                    ),
                    self.spec.isolation == Some(level),
                )
            }));
            entries.push(ViewMenuEntry::Submenu {
                label: i18n::t("console.isolation"),
                icon: None,
                entries: rows,
            });
        }
        entries
    }

    fn mode_menu(&self) -> Vec<ViewMenuEntry> {
        vec![
            Self::checked(
                ViewMenuEntry::item("mode:playground", i18n::t("console.mode_playground_long")),
                self.spec.mode == RunMode::Playground,
            ),
            Self::checked(
                ViewMenuEntry::item("mode:script", i18n::t("console.mode_script_long")),
                self.spec.mode == RunMode::Script,
            ),
        ]
    }

    fn settings_menu(&self) -> Vec<ViewMenuEntry> {
        let limit = crate::settings::fetch_limit();
        let rows = crate::settings::FETCH_CHOICES
            .iter()
            .map(|&n| {
                Self::checked(
                    ViewMenuEntry::item(format!("fetch:{n}"), n.to_string()),
                    i64::from(limit) == n,
                )
            })
            .collect();
        vec![
            ViewMenuEntry::Submenu {
                label: i18n::t("console.menu_fetch_limit"),
                icon: Some("table"),
                entries: rows,
            },
            Self::checked(
                ViewMenuEntry::item("stop-on-error", i18n::t("console.menu_stop_on_error")),
                crate::settings::stop_on_error(),
            ),
            ViewMenuEntry::Submenu {
                label: i18n::t("console.menu_profiling"),
                icon: Some("plan"),
                entries: crate::settings::ProfileSwitch::ALL
                    .into_iter()
                    .map(|switch| {
                        Self::checked(
                            ViewMenuEntry::item(
                                format!("profile-option:{}", switch.id()),
                                i18n::t(&format!("console.menu_profile_{}", switch.id())),
                            ),
                            crate::settings::profile_switch(switch),
                        )
                    })
                    .collect(),
            },
        ]
    }

    /// An attached file's source pill: the sources speaking SQL (the one in use checked),
    /// then Detach.
    fn source_menu(&self) -> Vec<ViewMenuEntry> {
        let sources = self.consoles.access().map(|a| a.sources()).unwrap_or_default();
        let mut entries: Vec<ViewMenuEntry> = sources
            .iter()
            .filter(|s| s.dialect)
            .map(|s| {
                let entry = ViewMenuEntry::item(format!("source:{}", s.id), s.name.clone());
                if s.id == self.source { entry.with_icon("check") } else { entry.with_icon(s.icon) }
            })
            .collect();
        if !entries.is_empty() {
            entries.push(ViewMenuEntry::Separator);
        }
        entries.push(ViewMenuEntry::item("detach", i18n::t("console.detach")).with_icon("x"));
        entries
    }

    /// An attached file's source changed (`Some`: attached to it; `None`: detached), unless
    /// its transaction is open (said: it's committed or rolled back first). Its bar then
    /// follows ([`crate::file_bar`]).
    fn attach_to(&mut self, source: Option<DataSourceId>) -> bool {
        if !self.spec.is_attached() {
            return false;
        }
        if self.snapshot.in_transaction {
            self.notifications.push(Notification::warning(i18n::t("console.tx_open_attach")));
            return false;
        }
        let path = self.spec.path(self.consoles.root());
        match source {
            Some(source) if source == self.source => true,
            Some(source) => self.consoles.attach(&path, source).is_some(),
            None => self.consoles.detach(&path).is_some(),
        }
    }

    /// The source's name as the attached file's pill shows it.
    pub(crate) fn source_label(&self) -> String {
        self.brief.as_ref().map_or_else(|| i18n::t("console.source_gone_short"), |b| b.name.clone())
    }

    /// The switcher's menu: the schemas (the one in use checked), the databases beside.
    fn place_menu_action(&self) -> Option<ToolbarAction> {
        let places = self.snapshot.places.clone().unwrap_or_default();
        let current = self.spec.schema.clone().or_else(|| self.snapshot.place.schema.clone());
        let mut entries: Vec<ViewMenuEntry> = places
            .schemas
            .iter()
            .map(|s| {
                Self::checked(
                    ViewMenuEntry::item(format!("schema:{s}"), s.clone()),
                    current.as_deref() == Some(s.as_str()),
                )
            })
            .collect();
        let caps = self.capabilities();
        if caps.databases && places.databases.len() > 1 {
            let database =
                self.spec.database.clone().or_else(|| self.snapshot.place.database.clone());
            let rows = places
                .databases
                .iter()
                .map(|d| {
                    Self::checked(
                        ViewMenuEntry::item(format!("db:{d}"), d.clone()),
                        database.as_deref() == Some(d.as_str()),
                    )
                })
                .collect();
            if !entries.is_empty() {
                entries.push(ViewMenuEntry::Separator);
            }
            entries.push(ViewMenuEntry::Submenu {
                label: i18n::t("console.databases"),
                icon: Some("database"),
                entries: rows,
            });
        }
        if entries.is_empty() {
            entries.push(ViewMenuEntry::item("none", i18n::t("console.no_schemas")));
        }
        if caps.switch_schema == SchemaSwitch::NamesOnly {
            entries.push(ViewMenuEntry::Separator);
            entries.push(ViewMenuEntry::item("none", i18n::t("console.names_only")));
        }
        self.menu(Tool::Place, entries)
    }

    /// The switcher pressed: its menu, once its lists are read (connecting if needed).
    fn open_place_menu(&mut self) -> Option<ToolbarAction> {
        if self.snapshot.places.is_some() {
            return self.place_menu_action();
        }
        if self.snapshot.busy {
            self.notifications.push(Notification::info(i18n::t("console.busy")));
            return None;
        }
        let access = self.consoles.access()?;
        let overrides = Overrides { database: self.spec.database.clone() };
        match access.connector(self.source, &overrides) {
            Ok(connector) => {
                let name = self.brief.as_ref().map(|b| b.name.clone()).unwrap_or_default();
                if self.runner.list_places(connector, name) {
                    self.place_menu = true;
                }
            }
            Err(e) => self.notifications.push(Notification::error(crate::runner::message(&e))),
        }
        None
    }

    /// What a press on `tool` does.
    fn press(&mut self, tool: Tool) -> Option<ToolbarAction> {
        match tool {
            Tool::Run => {
                self.run_command(crate::RUN.0);
                self.action.take()
            }
            Tool::RunMenu => self.menu(Tool::RunMenu, self.run_menu()),
            Tool::History => {
                self.run_command(crate::HISTORY.0);
                self.action.take()
            }
            Tool::Params => {
                self.run_command(crate::PARAMS.0);
                self.action.take()
            }
            Tool::Tx => self.menu(Tool::Tx, self.tx_menu()),
            Tool::Commit => {
                self.run_command(crate::COMMIT.0);
                self.action.take()
            }
            Tool::Rollback => {
                self.run_command(crate::ROLLBACK.0);
                self.action.take()
            }
            Tool::Stop => {
                self.run_command(crate::STOP.0);
                self.action.take()
            }
            Tool::Mode => self.menu(Tool::Mode, self.mode_menu()),
            Tool::Settings => self.menu(Tool::Settings, self.settings_menu()),
            Tool::Services => {
                self.run_command(workbench_model::TOGGLE_SERVICES.0);
                self.action.take()
            }
            Tool::Place => self.open_place_menu(),
            Tool::Source => self.menu(Tool::Source, self.source_menu()),
            Tool::More => {
                let layout = self.layout.value().cloned().unwrap_or_default();
                let entries = layout
                    .collapsed
                    .iter()
                    .filter_map(|t| {
                        let (id, key, icon) = match t {
                            Tool::History => ("more:history", "console.tip_history", "clock"),
                            Tool::Mode => ("more:mode", "console.tip_mode", "play"),
                            Tool::Settings => ("more:settings", "console.tip_settings", "gear"),
                            Tool::Services => ("more:services", "console.tip_services", "services"),
                            _ => return None,
                        };
                        Some(ViewMenuEntry::item(id, i18n::t(key)).with_icon(icon))
                    })
                    .collect();
                self.menu(Tool::More, entries)
            }
            Tool::Status | Tool::Limited | Tool::ReadOnly | Tool::Production => None,
        }
    }

    fn enabled(&self, tool: Tool) -> bool {
        let busy = self.snapshot.busy;
        match tool {
            Tool::Run | Tool::RunMenu => !busy && self.brief.as_ref().is_none_or(|b| b.dialect),
            Tool::Stop => self.snapshot.running.is_some(),
            Tool::Commit | Tool::Rollback => self.snapshot.in_transaction && !busy,
            Tool::Params => !busy && self.brief.as_ref().is_none_or(|b| b.dialect),
            Tool::Place => !busy || self.snapshot.places.is_some(),
            _ => true,
        }
    }

    fn tooltip_of(&self, tool: Tool) -> Option<(String, Option<String>)> {
        let keys = |id: &str| ui_components::shortcuts::label(id);
        Some(match tool {
            Tool::Run => (i18n::t("console.tip_run"), keys(crate::RUN.0)),
            Tool::RunMenu => (i18n::t("console.tip_run_menu"), None),
            Tool::History => (i18n::t("console.tip_history"), keys(crate::HISTORY.0)),
            Tool::Params => (i18n::t("console.tip_params"), keys(crate::PARAMS.0)),
            Tool::Tx => (
                match self.snapshot.tx {
                    TxState::Failed => i18n::t("console.tip_tx_failed"),
                    TxState::Open { writes, .. } => {
                        i18n::tr!("console.tip_tx_open", count = writes)
                    }
                    TxState::Idle => i18n::t("console.tip_tx"),
                },
                None,
            ),
            Tool::Commit => (i18n::t("console.tip_commit"), keys(crate::COMMIT.0)),
            Tool::Rollback => (i18n::t("console.tip_rollback"), keys(crate::ROLLBACK.0)),
            Tool::Stop => (i18n::t("console.tip_stop"), keys(crate::STOP.0)),
            Tool::Mode => (i18n::t("console.tip_mode"), None),
            Tool::Settings => (i18n::t("console.tip_settings"), None),
            Tool::Services => {
                (i18n::t("console.tip_services"), keys(workbench_model::TOGGLE_SERVICES.0))
            }
            Tool::Place => (
                match self.capabilities().switch_schema {
                    SchemaSwitch::NamesOnly => i18n::t("console.names_only"),
                    _ => i18n::t("console.tip_place"),
                },
                None,
            ),
            Tool::Status => (self.status()?.0, None),
            Tool::Limited => (i18n::t("console.limited_detail"), None),
            Tool::ReadOnly => (i18n::t("console.read_only_detail"), None),
            Tool::Production => (i18n::t("console.production_detail"), None),
            Tool::Source => (i18n::t("console.tip_source"), None),
            Tool::More => (i18n::t("console.tip_more"), None),
        })
    }

    /// What a finding points at, while the editor's text there is the one profiled (`None`
    /// before the editor was told, or once it changed: the mark then let go).
    fn focus_range(&self) -> Option<Range<usize>> {
        let focus = self.snapshot.focus.as_ref()?;
        let editor = self.editor.as_ref()?;
        let now =
            editor.text.get(focus.statement.clone()).map(ui_components::editor_toolbar::text_hash);
        (now == Some(focus.hash) && focus.range.end <= editor.text.len())
            .then(|| focus.range.clone())
    }

    /// The statements Run would run now, as one range of the text (for the highlight).
    fn planned_range(&self) -> Option<Range<usize>> {
        let editor = self.editor.as_ref()?;
        let flavor = self.brief.as_ref().map(|b| b.flavor).unwrap_or_default();
        let plan = database::console::plan(&PlanInput {
            text: &editor.text,
            selection: editor.selection.clone(),
            cursor: editor.cursor,
            mode: self.spec.mode,
            flavor,
            options: Default::default(),
        });
        match plan {
            RunPlan::Run(statements) => {
                let start = statements.iter().map(|s| s.range.start).min()?;
                let end = statements.iter().map(|s| s.range.end).max()?;
                Some(start..end)
            }
            RunPlan::Choose(_) | RunPlan::Nothing => None,
        }
    }

    // ─── Drawing ───

    fn paint_bar(&mut self, ctx: ToolbarPaint<'_>) {
        let ToolbarPaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let mid = (b.min_y() + b.size.height / 2.0).round();
        // The line under it, between the bar and the text.
        ds::divider(layer, &ds, b.min_x(), b.max_x(), b.max_y() - ds.line());

        let font = ds.ui_font(-1.0);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let chevron = px(12.0);
        let pill_pad = px(10.0);
        let tx_label = self.tx_label();
        let mode_label = i18n::t(match self.spec.mode {
            RunMode::Playground => "console.mode_playground",
            RunMode::Script => "console.mode_script",
        });
        let place_label = self.place_label();
        let tx_shaped = text.shape_line(&tx_label, &font);
        let mode_shaped = text.shape_line(&mode_label, &font);
        let place_shaped = text.shape_line(&place_label, &font);
        let status = self.status();
        let status_w = status.as_ref().map(|(s, _)| text.shape_line(s, &font).width + px(22.0));
        let limited = self.snapshot.limited || self.capabilities().limited;
        let limited_label = i18n::t("console.limited");
        let small = ds.ui_font(-1.5);
        let limited_w = limited.then(|| text.shape_line(&limited_label, &small).width + px(30.0));
        let read_only = self.brief.as_ref().is_some_and(|s| s.read_only);
        let read_only_label = i18n::t("console.read_only");
        let read_only_w =
            read_only.then(|| text.shape_line(&read_only_label, &small).width + px(30.0));
        let production = self.production();
        let production_label = i18n::t("console.production_badge");
        let production_w = production.then(|| {
            let strong = small.clone().with_weight(Font::BOLD);
            text.shape_line(&production_label, &strong).width + px(30.0)
        });
        // Rounded up: rounded down, a cut label (the source's) would end in "…" for nothing.
        let pill = |w: f32| (w + pill_pad * 2.0 + chevron + px(4.0)).ceil();
        let source_label = self.spec.is_attached().then(|| self.source_label());
        let source_shaped = source_label.as_ref().map(|l| text.shape_line(l, &font));
        let spec = BarSpec {
            min_x: b.min_x(),
            max_x: b.max_x(),
            metrics: Metrics::new(px),
            transaction: self.snapshot.in_transaction,
            tx_w: pill(tx_shaped.width),
            mode_w: pill(mode_shaped.width),
            place_w: pill(place_shaped.width) + px(18.0),
            status_w,
            limited_w,
            read_only_w,
            production_w,
            source_w: source_shaped.as_ref().map(|s| pill(s.width) + px(18.0)),
        };
        let key: LayoutKey = (
            spec.min_x.to_bits(),
            spec.max_x.to_bits(),
            spec.transaction,
            spec.tx_w.to_bits(),
            spec.mode_w.to_bits(),
            spec.place_w.to_bits(),
            spec.status_w.map(f32::to_bits),
            spec.limited_w.map(f32::to_bits),
            spec.read_only_w.map(f32::to_bits),
            spec.production_w.map(f32::to_bits),
            spec.source_w.map(f32::to_bits),
        );
        let laid = self.layout.get_or_update(key, || layout::layout(&spec)).clone();
        self.hits.clear();
        for x in &laid.dividers {
            ds::divider_vertical(layer, &ds, *x, mid - px(9.0), mid + px(9.0));
        }
        let green = p.vcs_added;
        for &(tool, x, w) in &laid.placed {
            let enabled = self.enabled(tool);
            let hovered = self.hover == Some(tool) && enabled;
            let r = Rect::new(x, (mid - px(14.0)).round(), w, px(28.0));
            match tool {
                Tool::Run
                | Tool::History
                | Tool::Params
                | Tool::Commit
                | Tool::Rollback
                | Tool::Stop
                | Tool::Settings
                | Tool::Services
                | Tool::More => {
                    let (icon, tint) = match tool {
                        Tool::Run => ("play", Some(green)),
                        Tool::History => ("clock", None),
                        Tool::Params => ("parameter", None),
                        Tool::Commit => ("check", Some(green)),
                        Tool::Rollback => ("undo", None),
                        Tool::Stop => ("stop", Some(p.danger)),
                        Tool::Settings => ("gear", None),
                        Tool::Services => ("services", None),
                        _ => ("more-horizontal", None),
                    };
                    icon_button(
                        layer,
                        icons,
                        &ds,
                        r,
                        icon,
                        tint,
                        ui_components::ds::Press::new(enabled, hovered),
                    );
                }
                Tool::RunMenu => {
                    if hovered {
                        layer
                            .quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::SM)));
                    }
                    if let Some(id) = icons.id("chevron-down") {
                        let s = px(11.0);
                        let ir = Rect::new(
                            (r.min_x() + (w - s) / 2.0).round(),
                            (mid - s / 2.0).round(),
                            s,
                            s,
                        );
                        let color =
                            if enabled { p.text_muted } else { p.text_muted.with_alpha(0.4) };
                        draw_icon(layer, icons, id, ir, color);
                    }
                }
                Tool::Tx | Tool::Mode | Tool::Place | Tool::Source => {
                    let shaped = match tool {
                        Tool::Tx => &tx_shaped,
                        Tool::Mode => &mode_shaped,
                        Tool::Source => match &source_shaped {
                            Some(shaped) => shaped,
                            None => continue,
                        },
                        _ => &place_shaped,
                    };
                    let r = Rect::new(x, (mid - px(13.0)).round(), w, px(26.0));
                    let fill =
                        if hovered { p.text.with_alpha(0.08) } else { p.text.with_alpha(0.035) };
                    layer.quad(
                        Quad::filled(r, fill).rounded(px(radius::SM)).bordered(ds.line(), p.border),
                    );
                    let mut tx = r.min_x() + pill_pad;
                    if tool == Tool::Place || tool == Tool::Source {
                        // An attached file's source pill has the driver's icon.
                        let icon = match tool {
                            Tool::Place if self.spec.is_attached() => "database",
                            _ => self.brief.as_ref().map_or("database", |b| b.icon),
                        };
                        if let Some(id) = icons.id(icon) {
                            let s = px(13.0);
                            let ir = Rect::new(tx.round(), (mid - s / 2.0).round(), s, s);
                            draw_icon(layer, icons, id, ir, p.text_muted);
                        }
                        tx += px(18.0);
                    }
                    let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
                    let color = if enabled { p.text } else { p.text_muted };
                    // An attached file's source cut to its pill (a narrow bar).
                    let cut;
                    let shaped = match (&source_label, tool) {
                        (Some(label), Tool::Source)
                            if shaped.width > r.max_x() - pill_pad - chevron - px(4.0) - tx =>
                        {
                            let room = r.max_x() - pill_pad - chevron - px(4.0) - tx;
                            let line = ds::ellipsize(text, label, &font, room.max(0.0));
                            cut = text.shape_line(&line, &font);
                            &cut
                        }
                        _ => shaped,
                    };
                    // `Tx:` muted, its mode in full.
                    draw_text(layer, shaped, tx.round(), base, color);
                    if let Some(id) = icons.id("chevron-down") {
                        let ir = Rect::new(
                            (r.max_x() - pill_pad - chevron + px(3.0)).round(),
                            (mid - chevron / 2.0).round(),
                            chevron,
                            chevron,
                        );
                        draw_icon(layer, icons, id, ir, p.text_muted);
                    }
                    if self.snapshot.tx.is_open() && tool == Tool::Tx {
                        // An open transaction: a dot on the pill's corner (red: failed).
                        let d = px(7.0);
                        let dot = Rect::new(r.max_x() - d * 0.7, r.min_y() - d * 0.3, d, d);
                        let tint = if self.snapshot.tx.is_failed() { p.danger } else { p.warning };
                        layer.quad(Quad::filled(dot, tint).rounded(d / 2.0));
                    }
                    self.hits.push((r, tool));
                    continue;
                }
                Tool::Status => {
                    let Some((label, error)) = &status else { continue };
                    let running = self.snapshot.running.is_some();
                    let icon_x = x;
                    let s = px(13.0);
                    if running {
                        ds::loader(
                            layer,
                            text,
                            &ds,
                            icon_x,
                            mid,
                            &ds::Loader { size: 13.0, ..Default::default() },
                        );
                    } else {
                        let (icon, color) = if *error {
                            ("circle-alert", p.danger)
                        } else if self.snapshot.last.as_ref().is_some_and(|l| l.cancelled) {
                            ("square", p.text_muted)
                        } else {
                            ("check", green)
                        };
                        if let Some(id) = icons.id(icon) {
                            let ir = Rect::new(icon_x.round(), (mid - s / 2.0).round(), s, s);
                            draw_icon(layer, icons, id, ir, color);
                        }
                    }
                    let room = laid.status_room - px(22.0);
                    let line = ds::ellipsize(text, label, &font, room.max(0.0));
                    let shaped = text.shape_line(&line, &font);
                    let base = ds::optical_baseline(r.min_y(), r.size.height, &font);
                    let color = if *error { p.danger } else { p.text_muted };
                    draw_text(layer, &shaped, (x + px(20.0)).round(), base, color);
                    self.hits.push((r, tool));
                    continue;
                }
                Tool::Limited | Tool::ReadOnly | Tool::Production => {
                    let (label, icon, color) = match tool {
                        Tool::Limited => (&limited_label, "info", p.warning),
                        Tool::Production => (&production_label, "alert", p.danger),
                        _ => (&read_only_label, "lock", p.text_muted),
                    };
                    let r = Rect::new(x, (mid - px(11.0)).round(), w, px(22.0));
                    layer.quad(
                        Quad::filled(r, color.with_alpha(0.10))
                            .rounded(px(11.0))
                            .bordered(ds.line(), color.with_alpha(0.35)),
                    );
                    if let Some(id) = icons.id(icon) {
                        let s = px(12.0);
                        let ir =
                            Rect::new((r.min_x() + px(8.0)).round(), (mid - s / 2.0).round(), s, s);
                        draw_icon(layer, icons, id, ir, color);
                    }
                    let face = if tool == Tool::Production {
                        small.clone().with_weight(Font::BOLD)
                    } else {
                        small.clone()
                    };
                    let shaped = text.shape_line(label, &face);
                    let base = ds::optical_baseline(r.min_y(), r.size.height, &face);
                    draw_text(layer, &shaped, (r.min_x() + px(24.0)).round(), base, color);
                    self.hits.push((r, tool));
                    continue;
                }
            }
            if enabled || tool == Tool::Params {
                self.hits.push((r, tool));
            }
        }
        let _ = strong;
    }
}

/// An icon button: its background when hovered, its icon tinted (`tint`: Run's green, Stop's
/// red) or in the text's color, dimmed when disabled.
pub(crate) fn icon_button(
    layer: &mut Layer,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    r: Rect,
    icon: &str,
    tint: Option<Rgba>,
    press: ds::Press,
) {
    let ds::Press { enabled, hovered } = press;
    let p = ds.palette;
    if hovered {
        layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(ds.px(radius::SM)));
    }
    let Some(id) = icons.id(icon) else { return };
    let s = ds.px(15.0);
    let ir = Rect::new(
        (r.min_x() + (r.size.width - s) / 2.0).round(),
        (r.min_y() + (r.size.height - s) / 2.0).round(),
        s,
        s,
    );
    let color = match (enabled, tint) {
        (false, _) => p.text_muted.with_alpha(0.4),
        (true, Some(tint)) => tint,
        (true, None) => p.text.with_alpha(0.86),
    };
    draw_icon(layer, icons, id, ir, color);
}

impl EditorToolbar for ConsoleBar {
    fn height(&self) -> f32 {
        HEIGHT
    }

    fn paint(&mut self, ctx: ToolbarPaint<'_>) {
        self.refresh();
        self.paint_bar(ctx);
    }

    fn pointer_down(&mut self, p: Point) -> Option<ToolbarAction> {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)?;
        if !self.enabled(tool) {
            return None;
        }
        self.press(tool)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let tool = p.and_then(|p| self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t));
        let changed = tool != self.hover;
        self.hover = tool;
        // On Run: the statement it runs highlighted; leaving it: the highlight goes.
        if tool == Some(Tool::Run) {
            self.highlight = self.current.clone();
        } else if self.highlight.take().is_some() {
            return true;
        }
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t)?;
        (self.enabled(tool)
            && !matches!(tool, Tool::Status | Tool::Limited | Tool::ReadOnly | Tool::Production))
        .then_some(ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, tool) = self.hits.iter().find(|(r, _)| r.contains(p))?;
        let (label, keys) = self.tooltip_of(*tool)?;
        Some(Tip::new(*rect, label).with_shortcut(keys))
    }

    fn poll(&mut self) -> bool {
        self.refresh()
    }

    fn is_loading(&self) -> bool {
        self.snapshot.running.is_some()
    }

    fn title(&self) -> Option<String> {
        // An attached file keeps its own name.
        (!self.spec.is_attached()).then(|| self.title.clone())
    }

    /// Its server's icon; the generic database one is the `.sql` file's own (as the file
    /// icon set draws it), so the file's is kept then.
    fn icon(&self) -> Option<&'static str> {
        if self.spec.is_attached() {
            return None;
        }
        self.brief.as_ref().map(|b| b.icon).filter(|icon| *icon != GENERIC_ICON)
    }

    fn accent(&self, palette: &Palette) -> Option<Rgba> {
        let color = self.brief.as_ref()?.color?;
        ui_components::tag_colors::named(&color.name(), palette)
    }

    fn tab_badge(&self, palette: &Palette) -> Option<(String, Rgba)> {
        let tx = self.snapshot.tx;
        tx.is_open().then(|| {
            let color = if tx.is_failed() { palette.danger } else { palette.warning };
            (i18n::t("console.tab_tx"), color)
        })
    }

    fn crumbs(&self) -> Vec<String> {
        if self.spec.is_attached() {
            return Vec::new();
        }
        let source = self.brief.as_ref().map(|b| b.name.clone()).unwrap_or_default();
        vec![i18n::t("console.crumb_consoles"), source, self.spec.name.clone()]
    }

    fn may_close(&mut self) -> bool {
        self.refresh();
        // Its transaction answered for (committing, or rolled back): it closes.
        if self.consoles.take_allowed_close(self.console) || !self.snapshot.tx.is_open() {
            return true;
        }
        let console = self.console;
        self.with_host(move |host| {
            crate::commands::close(host, console);
        });
        false
    }

    fn moved(&mut self, path: &std::path::Path) -> bool {
        // Still this console's file, of the same source: the same bar under the new name.
        // Still a console (or still an attached file): another kind is another bar.
        match self.consoles.follow_file(self.console, path) {
            Some(spec) if spec.source == self.source && spec.kind == self.spec.kind => {
                let old = self.spec.path(self.consoles.root());
                self.consoles.checks().forget(&self.consoles, &old);
                self.spec = spec;
                self.layout = Memo::default();
                self.refresh();
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        let update = |bar: &mut Self, change: &dyn Fn(&mut ConsoleSpec)| {
            if let Some(spec) = bar.consoles.update(bar.console, |s| change(s)) {
                bar.spec = spec;
            }
        };
        match id {
            "run" => self.run_command(crate::RUN.0),
            "run-script" => self.run_command(crate::RUN_SCRIPT.0),
            "explain" => self.run_command(crate::EXPLAIN.0),
            "explain-analyze" => self.run_command(crate::EXPLAIN_ANALYZE.0),
            "tx:auto" => update(self, &|s| s.tx = TxMode::Auto),
            "tx:manual" => update(self, &|s| s.tx = TxMode::Manual),
            "isolation:default" => update(self, &|s| s.isolation = None),
            id if id.starts_with("isolation:") => {
                let level = Isolation::from_id(&id["isolation:".len()..]);
                update(self, &|s| s.isolation = level);
            }
            "mode:playground" => update(self, &|s| s.mode = RunMode::Playground),
            "mode:script" => update(self, &|s| s.mode = RunMode::Script),
            id if id.starts_with("fetch:") => {
                let Ok(n) = id["fetch:".len()..].parse::<i64>() else { return false };
                self.save_setting(crate::commands::SettingChange::FetchLimit(n));
            }
            id if id.starts_with("profile-option:") => {
                let Some(switch) =
                    crate::settings::ProfileSwitch::from_id(&id["profile-option:".len()..])
                else {
                    return false;
                };
                let on = !crate::settings::profile_switch(switch);
                self.save_setting(crate::commands::SettingChange::Profile(switch, on));
            }
            "stop-on-error" => {
                let on = !crate::settings::stop_on_error();
                self.save_setting(crate::commands::SettingChange::StopOnError(on));
            }
            id if id.starts_with("schema:") => {
                let schema = id["schema:".len()..].to_owned();
                update(self, &|s| s.schema = Some(schema.clone()));
            }
            id if id.starts_with("db:") => {
                if self.snapshot.in_transaction {
                    self.notifications
                        .push(Notification::warning(i18n::t("console.tx_open_switch")));
                    return false;
                }
                let database = id["db:".len()..].to_owned();
                update(self, &|s| {
                    s.database = Some(database.clone());
                    s.schema = None;
                });
                self.runner.disconnect();
            }
            id if id.starts_with("source:") => {
                let Some(source) = DataSourceId::parse(&id["source:".len()..]) else {
                    return false;
                };
                return self.attach_to(Some(source));
            }
            "detach" => return self.attach_to(None),
            "more:history" => self.run_command(crate::HISTORY.0),
            "more:mode" => self.action = self.menu(Tool::More, self.mode_menu()),
            "more:settings" => self.action = self.menu(Tool::More, self.settings_menu()),
            "more:services" => self.run_command(workbench_model::TOGGLE_SERVICES.0),
            "none" => {}
            _ => return false,
        }
        self.layout = Memo::default();
        true
    }

    fn take_action(&mut self) -> Option<ToolbarAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn follows_editor(&self) -> bool {
        // Always: the statement the cursor is in is marked beside its lines (Run hovered
        // highlights it too); an error underlined is gone once its statement is edited; the
        // text is checked against the schema as it changes.
        true
    }

    fn sync_editor(&mut self, state: &EditorState) {
        if self.brief.as_ref().is_some_and(|b| b.dialect) {
            let path = self.spec.path(self.consoles.root());
            self.consoles.checks().schedule(
                &self.consoles,
                &path,
                Arc::clone(&state.text),
                state.version,
            );
        }
        if let Some((range, hash)) = &self.snapshot.error_mark {
            let now = state.text.get(range.clone()).map(ui_components::editor_toolbar::text_hash);
            if now != Some(*hash) {
                self.snapshot.error_mark = None;
                self.runner.clear_diagnostics();
            }
        }
        self.editor = Some(state.clone());
        self.current = self.planned_range();
        self.highlight = if self.hover == Some(Tool::Run) { self.current.clone() } else { None };
        self.focused = self.focus_range();
        // The statement profiled was edited: what its finding pointed at isn't there.
        if self.snapshot.focus.is_some() && self.focused.is_none() {
            self.snapshot.focus = None;
            self.runner.set_focus(None);
        }
    }

    fn marks(&self) -> (&[StatementMark], u64) {
        (&self.snapshot.marks, self.snapshot.marks_generation)
    }

    fn highlight(&self) -> Option<Range<usize>> {
        self.highlight.clone()
    }

    fn current(&self) -> Option<Range<usize>> {
        self.current.clone()
    }

    fn focus(&self) -> Option<(Range<usize>, ui_components::Level)> {
        let level = self.snapshot.focus.as_ref()?.level;
        Some((self.focused.clone()?, level))
    }

    fn purge(&mut self) {
        self.layout = Memo::default();
        self.runner.purge();
    }

    fn grammar_hints(&self) -> bool {
        crate::settings::grammar_hints()
    }
}

#[cfg(test)]
#[path = "../tests/unit/bar.rs"]
mod tests;
