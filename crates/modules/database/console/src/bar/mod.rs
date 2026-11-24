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
use text_layout::Font;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, Tone, button_size};
use ui_components::editor_toolbar::{
    EditorState, EditorToolbar, StatementMark, ToolbarAction, ToolbarPaint,
};
use ui_components::{Palette, Tip, ViewMenuEntry, ViewerAction, ViewerCursor};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::consoles::Consoles;
use crate::layout::{self, BarLayout, BarSpec, Metrics, Tool};
use crate::runner::{Lease, Runner, Snapshot};

mod menus;
mod paint;
mod toolbar;

/// The bar's height (logical pixels).
pub const HEIGHT: f32 = 40.0;

/// A pill's field (Tx, Mode) showing `value`; its menu is the bar's, so never shown open.
fn pill_spec(value: &str, hovered: bool, enabled: bool) -> ds::DropdownSpec<'_> {
    ds::DropdownSpec { value, open: false, hovered, enabled, note: None, placeholder: None }
}

/// The height of its pills: Tx, Mode, the place, the source (logical pixels).
const PILL_H: f32 = 26.0;

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
}

#[cfg(test)]
#[path = "../../tests/unit/bar.rs"]
mod tests;
