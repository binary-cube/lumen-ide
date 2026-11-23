//! A result's tab in the Services window: the grid (`ui_components::data_grid`) over a
//! [`ResultSource`] — its pages, sorting, filter, find, transposing, copying and, for a
//! result of one table's rows, editing the grid's own (submitted through [`crate::edit`]; its
//! menu adds "Open Table" and, on a key's cell, "Go to Referenced Row") — under a thin bar of
//! its own: Commit and Rollback while the console's
//! transaction is open, how long the statement took (execution and fetching apart in its
//! tooltip), what to know of its rows (sorted by the engine, more left that a write won't
//! bring, rows let go for memory, big cells cut) and the export (every row, through the
//! project's data extractor or the one asked, or an Excel workbook:
//! [`ui_components::data_grid::export`]).
//!
//! An export of rows that are all here is written from them in the background (in the order
//! shown); one of a pure query whose rest isn't here runs it again, the rows written as they
//! come (the runner's); a write's result exports only what it brought, as said.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, PoisonError};

use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::Font;
use ui_components::control::{Control, Controls};
use ui_components::data_grid::export::{
    CLIPBOARD_ROWS, ExportFormat, ExportOutcome, ExportSink, ExportWriter, Target,
};
use ui_components::data_grid::{
    DataGrid, GridAction, GridColumn, GridPaint, GridSource, MENU_PREFIX,
};
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, Tone, button_size};
use ui_components::{
    AppRequest, KeyReach, PressMods, Tip, ViewMemory, ViewMenuEntry, Viewer, ViewerAction,
    ViewerCursor, ViewerKey, ViewerPaint,
};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::result_source::{ResultSource, rows_in_order};
use crate::results::cell_ref;
use crate::runner::{ExportRequest, ExportState, Runner, millis};
use crate::runtime::Runtime;

/// The bar's height over the grid (logical pixels).
pub const BAR_H: f32 = 30.0;

/// The grid's place in the view's parts.
const GRID: usize = 0;

/// A part of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Commit,
    Rollback,
    Export,
    /// The run's time (its tooltip says execution and fetching).
    Time,
    /// What to know of the rows (its tooltip says it all).
    Note,
    /// A pane offered over the rows (by its place among the offered).
    Pane(usize),
}

/// A result's tab (see the module's doc).
pub struct ResultView {
    grid: DataGrid<ResultSource>,
    runner: Runner,
    runtime: Runtime,
    /// The console's file's folder (where an export is first offered).
    folder: Option<PathBuf>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    told: Vec<Notification>,
    export: Option<Arc<ExportState>>,
    export_format: Option<ExportFormat>,
    /// The runner's generation last drawn (Commit / Rollback shown or not).
    seen: u64,
    /// What its source allows was read once (again with each change of the runner).
    rights_read: bool,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
    /// The panes offered over its rows, made once its result is known.
    panes: Option<crate::result_panes::Panes>,
    /// A pane's "Save As" is open (its path goes to the pane, not to an export).
    pane_saving: bool,
    /// The room under its bar, drawn last.
    body: Rect,
}

impl std::fmt::Debug for ResultView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultView").field("result", &self.grid.source().id()).finish()
    }
}

impl ResultView {
    /// Result `source`'s tab, `page` rows a page; an export first offered in `folder`.
    pub fn new(source: ResultSource, page: Option<u32>, folder: Option<PathBuf>) -> Self {
        let runner = source.runner().clone();
        let runtime = runner_runtime(&runner);
        let mut grid = DataGrid::new(source, page);
        grid.set_project(runner.project());
        Self {
            grid,
            runner,
            runtime,
            folder,
            hits: Vec::new(),
            hover: None,
            action: None,
            told: Vec::new(),
            export: None,
            export_format: None,
            seen: 0,
            rights_read: false,
            // A press on the bar leaves the keys with the grid.
            controls: Controls::sticky(),
            panes: None,
            pane_saving: false,
            body: Rect::default(),
        }
        .with_grid_focused()
    }

    /// The grid has the keys, the text and the wheel without a press (the tab is the grid):
    /// it's focused from the start, and a press elsewhere keeps it so.
    fn with_grid_focused(mut self) -> Self {
        self.with_controls(|c, parts| c.focus(parts, Some(GRID)));
        self
    }

    /// Its part at `i`, read-only (the copy's selection asked from `&self`).
    fn part(&self, i: usize) -> Option<&dyn Control> {
        (i == GRID).then_some(&self.grid as &dyn Control)
    }

    /// Its parts (the grid, under the bar), with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut parts: [&mut dyn Control; 1] = [&mut self.grid];
        f(&mut self.controls, &mut parts)
    }

    pub fn grid(&self) -> &DataGrid<ResultSource> {
        &self.grid
    }

    pub fn grid_mut(&mut self) -> &mut DataGrid<ResultSource> {
        &mut self.grid
    }

    /// Whether an export runs.
    pub fn exporting(&self) -> bool {
        self.export.is_some()
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    fn take_grid_action(&mut self) {
        match self.grid.take_action() {
            Some(GridAction::Copy(text)) => {
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            Some(GridAction::Export { format, clipboard }) => {
                let Some(format) = ExportFormat::find(self.grid.project(), &format) else {
                    return;
                };
                if clipboard {
                    self.start_export(format, Target::Clipboard);
                } else {
                    self.ask_export(format);
                }
            }
            Some(GridAction::Command(command)) => {
                self.action = Some(ViewerAction::RunCommand(command));
            }
            Some(GridAction::Submit) => self.ask_submit(),
            None => {}
        }
    }

    /// The snapshots' edits of the sample's customers: a name changed, a row added, the
    /// third marked for deletion, a cell chosen.
    #[cfg(feature = "snapshot")]
    fn snapshot_edit(&mut self) -> bool {
        use ui_components::data_grid::CellValue;
        let source = self.grid.source_mut();
        if source.editable().is_none() {
            return false;
        }
        let _ = source.set(0, 1, CellValue::Text("Ana-Maria Ionescu".into()));
        let _ = source.set(1, 3, CellValue::Text("MD".into()));
        if let Some(new) = source.insert(None) {
            let _ = source.set(new, 1, CellValue::Text("Dan Rusu".into()));
            let _ = source.set(new, 2, CellValue::Text("dan@example.com".into()));
            let _ = source.set(new, 3, CellValue::Text("RO".into()));
        }
        source.delete(&[2]);
        self.grid.command("select:0,1")
    }

    /// The pending changes shown first and, confirmed, submitted ([`crate::edit`]).
    fn ask_submit(&mut self) {
        let source = self.grid.source();
        let (Some(set), Some(editable)) = (source.change_set(), source.editable().cloned()) else {
            return;
        };
        let asked = crate::edit::SubmitAsk {
            runner: self.runner.clone(),
            result: source.id(),
            set,
            editable,
        };
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            crate::edit::submit(host, asked);
        })));
    }

    /// How a submit ended, told (refused: every change kept).
    fn take_submitted(&mut self) -> bool {
        let Some(outcome) = self.grid.source_mut().take_submitted() else { return false };
        let told = match outcome {
            Ok(applied) => {
                let key = if self.runner.in_transaction() {
                    "console.edits_pending"
                } else {
                    "console.edits_committed"
                };
                Notification::success(i18n::tr!(key, count = applied.affected))
            }
            Err(database::DbError::Conflict(why)) => {
                Notification::error(i18n::tr!("database.conflict", why = why.as_str()))
            }
            Err(e) => Notification::error(i18n::tr!(
                "database.submit_failed",
                why = crate::runner::message(&e)
            )),
        };
        self.told.push(told);
        true
    }

    /// What the result's source allows now (read-only, its driver editing rows).
    fn follow_rights(&mut self) {
        let brief = self
            .runner
            .project()
            .and_then(crate::consoles::consoles)
            .and_then(|c| c.access())
            .and_then(|a| a.source(self.runner.source()));
        if let Some(brief) = brief {
            self.grid.source_mut().set_rights(crate::result_source::Rights {
                read_only: brief.read_only,
                edit_rows: brief.capabilities.edit_rows,
            });
        }
    }

    /// The result's table opened (its data), in the Database window's view.
    fn open_table(&mut self, filter: Option<String>) -> bool {
        let source = self.grid.source();
        let Some(table) = source.editable().map(|e| e.table.clone()) else { return false };
        self.open_rows(table, filter)
    }

    /// `table` of the console's source opened (`filter`: its rows filtered).
    fn open_rows(&mut self, table: database::TableRef, filter: Option<String>) -> bool {
        let Some(access) =
            self.runner.project().and_then(crate::consoles::consoles).and_then(|c| c.access())
        else {
            return false;
        };
        let id = self.runner.source();
        let opening = match &filter {
            Some(filter) => access.open_rows(id, &table, filter),
            None => access.open_table(id, &table),
        };
        self.action = match opening {
            Some(database::diagram::Opening::File(path, asked)) => {
                Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
                    host.open_file_with(&path, &asked);
                })))
            }
            Some(database::diagram::Opening::View(v)) => Some(ViewerAction::OpenView(
                ui_components::OpenView::shared(v.title, v.extension, v.data),
            )),
            None => return false,
        };
        true
    }

    /// The row the cell at the grid's cursor points to by a foreign key: its table shown,
    /// filtered to it.
    fn go_to_reference(&mut self) -> bool {
        let Some((table, column, filter)) = self.reference() else { return false };
        let _ = column;
        self.open_rows(table, Some(filter))
    }

    /// The table, the column and the condition of the row the cell at the grid's cursor
    /// points to by a foreign key of the result's table (`None`: not a key's cell).
    pub fn reference(&self) -> Option<(database::TableRef, String, String)> {
        let cell = self.grid.selection().cursor()?;
        let column = self.grid.source_column(cell.col)?;
        let row = self.grid.row_of(cell.row);
        let (table, target, value) = self.grid.source().reference(row, column)?;
        let literal = match value {
            database::Value::Int(i) => i.to_string(),
            database::Value::Float(f) => f.to_string(),
            database::Value::Bool(b) => i64::from(b).to_string(),
            database::Value::Text(t) | database::Value::Other(t) => {
                format!("'{}'", t.replace('\'', "''"))
            }
            _ => return None,
        };
        let simple = target.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && target.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let name =
            if simple { target.clone() } else { format!("\"{}\"", target.replace('"', "\"\"")) };
        Some((table, target, format!("{name} = {literal}")))
    }

    /// Where every row goes: the system's "Save As", at the console's folder, the result's
    /// name with the format's extension filled in.
    fn ask_export(&mut self, format: ExportFormat) {
        let name = format!("{}.{}", self.grid.source().name(), format.extension());
        self.export_format = Some(format);
        // The path picked now is the export's (a pane's "Save As" cancelled before it isn't
        // waiting any more: a cancel sends nothing back).
        self.pane_saving = false;
        self.action = Some(ViewerAction::Request(AppRequest::PickSavePath {
            start: self.folder.clone(),
            name,
        }));
    }

    /// Every row of the result (in the order and filter shown, the columns as shown)
    /// written to `target` in the background (one export at a time).
    pub fn start_export(&mut self, format: ExportFormat, target: Target) {
        if self.export.is_some() {
            return;
        }
        let source = self.grid.source();
        let Some(view) = source.result().cloned() else { return };
        // The columns as shown (all of them, in their order, before it's first drawn).
        let mut shown_at = self.grid.shown_columns();
        if shown_at.is_empty() {
            shown_at = (0..source.columns().len()).collect();
        }
        let shown: Vec<GridColumn> =
            shown_at.into_iter().filter_map(|c| source.columns().get(c)).cloned().collect();
        let (sql, options) = (source.sql(), extractors::catalog().options());
        // Its rest isn't here: a pure query is run again for it (the runner's).
        if !source.complete() && view.rerunnable {
            if target == Target::Clipboard {
                self.told.push(Notification::warning(i18n::tr!(
                    "database.export_clipboard_limit",
                    count = CLIPBOARD_ROWS
                )));
                return;
            }
            let request = ExportRequest {
                result: view.id,
                columns: shown,
                view: if source.by_server() {
                    ResultSource::server_view(self.grid.query())
                } else {
                    view.view
                },
                format,
                sql,
                options,
                target,
            };
            match self.runner.export(request) {
                Some(export) => self.export = Some(export),
                None => self.told.push(Notification::info(i18n::t("console.busy"))),
            }
            return;
        }
        let order = source.local();
        let count = order.as_ref().map_or(view.len, |o| o.len() as u64);
        if target == Target::Clipboard && count > CLIPBOARD_ROWS {
            self.told.push(Notification::warning(i18n::tr!(
                "database.export_clipboard_limit",
                count = CLIPBOARD_ROWS
            )));
            return;
        }
        if !source.complete() {
            // A write's result: what it brought is all there is.
            self.told.push(Notification::info(i18n::tr!("console.export_partial", count = count)));
        }
        let export = Arc::new(ExportState::default());
        let state = Arc::clone(&export);
        let table = source.name();
        let runner = self.runner.clone();
        self.runtime.spawn(move || {
            let outcome = write_rows(
                &view,
                order.as_deref().map(Vec::as_slice),
                &shown,
                &table,
                &Writing { format: &format, sql, options },
                &target,
                &state,
            );
            *state.outcome.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
            runner.wake_up();
        });
        self.export = Some(export);
    }

    /// How the export is going; its end told (copied rows put on the clipboard).
    fn follow_export(&mut self) -> bool {
        let Some(export) = &self.export else { return false };
        let Some(outcome) = export.take_outcome() else { return false };
        self.export = None;
        let (told, copied) = outcome.told();
        if let Some(text) = copied {
            self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
        }
        self.told.push(told);
        true
    }

    /// A pane over its rows takes the keys and the text now (its list open).
    fn pane_captures(&self) -> bool {
        self.panes.as_ref().is_some_and(|panes| panes.host.captures())
    }

    /// The panes over its rows: made once its result is known (their states read for its
    /// statement), then followed (rows lent as they come, all of them fetched when asked).
    fn follow_panes(&mut self) -> bool {
        let source = self.grid.source();
        let Some(view) = source.result() else { return false };
        if self.panes.is_none() {
            let root = self
                .runner
                .project()
                .and_then(crate::consoles::consoles)
                .map(|c| c.root().to_path_buf());
            self.panes =
                Some(crate::result_panes::Panes::new(root, &view.statement, &self.runtime));
            return true;
        }
        let Some(panes) = &mut self.panes else { return false };
        let (changed, request) = panes.poll(view, source.columns(), &self.runner, &self.runtime);
        match request {
            Some(ui_components::result_pane::PaneRequest::SaveAs { name }) => {
                self.pane_saving = true;
                self.action = Some(ViewerAction::Request(AppRequest::PickSavePath {
                    start: self.folder.clone(),
                    name,
                }));
            }
            Some(ui_components::result_pane::PaneRequest::Tell { text, ok }) => {
                self.told.push(if ok {
                    Notification::success(text)
                } else {
                    Notification::error(text)
                });
            }
            _ => {}
        }
        changed
    }

    /// What the bar says of the rows (`None`: nothing to say), and its tooltip.
    fn note(&self) -> Option<(String, String)> {
        let source = self.grid.source();
        let view = source.result()?;
        if self.export.is_some() {
            let rows = self.export.as_ref().map_or(0, |e| e.rows.load(Ordering::Relaxed));
            let text = i18n::tr!("console.exporting", count = rows);
            return Some((text.clone(), text));
        }
        if view.dropped {
            return Some((i18n::t("console.note_dropped"), i18n::t("console.note_dropped_detail")));
        }
        if matches!(view.more, crate::results::More::Lost) {
            return Some((i18n::t("console.note_lost"), i18n::t("console.rows_left")));
        }
        if source.by_server() {
            return Some((i18n::t("console.note_server"), i18n::t("console.note_server_detail")));
        }
        if view.cut > 0 {
            return Some((
                i18n::tr!("console.note_cut", count = view.cut),
                i18n::t("console.note_cut_detail"),
            ));
        }
        None
    }

    fn paint_bar(&mut self, ctx: &mut ViewerPaint<'_>, bar: Rect) {
        let p = ctx.palette;
        let ds = Ctx::new(p, ctx.scale);
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(bar));
        layer.quad(Quad::filled(bar, p.editor_background));
        ds::divider(layer, &ds, bar.min_x(), bar.max_x(), bar.max_y() - ds.line());
        let mid = (bar.min_y() + bar.size.height / 2.0).round();
        let font = ds.ui_font(-1.0);
        let mut x = bar.min_x() + px(8.0);
        let side = px(button_size::MEDIUM);
        let button = |x: f32| Rect::new(x, (mid - side / 2.0).round(), side, side);
        // Commit and Rollback while the console's transaction is open.
        if self.runner.in_transaction() {
            for (hit, icon, tone) in
                [(Hit::Commit, "check", Some(Tone::Success)), (Hit::Rollback, "undo", None)]
            {
                let r = button(x);
                let mut spec =
                    ButtonSpec::new(ButtonKind::Ghost).icon(icon).hovered(self.hover == Some(hit));
                if let Some(tone) = tone {
                    spec = spec.tone(tone);
                }
                ds::button(layer, ctx.text, ctx.icons, &ds, r, spec);
                self.hits.push((r, hit));
                x += side + px(2.0);
            }
            ds::divider_vertical(layer, &ds, (x + px(4.0)).round(), mid - px(8.0), mid + px(8.0));
            x += px(10.0);
        }
        // How long it took.
        if let Some(view) = self.grid.source().result() {
            let total = view.execution + view.fetching;
            let label = millis(total);
            let shaped = ctx.text.shape_line(&label, &font);
            let s = px(13.0);
            if let Some(id) = ctx.icons.id("clock") {
                let ir = Rect::new((x + px(4.0)).round(), (mid - s / 2.0).round(), s, s);
                draw_icon(layer, ctx.icons, id, ir, p.text_muted);
            }
            let base = ds::optical_baseline(bar.min_y(), bar.size.height, &font);
            draw_text(layer, &shaped, (x + px(22.0)).round(), base, p.text_muted);
            let w = px(22.0) + shaped.width + px(6.0);
            self.hits.push((Rect::new(x, bar.min_y(), w, bar.size.height), Hit::Time));
            x += w + px(10.0);
        }
        // The export, at the right; the panes offered left of it.
        let export = button(bar.max_x() - px(8.0) - side);
        let mut right = export.min_x() - px(2.0);
        if let Some(panes) = &self.panes {
            let hovered = match self.hover {
                Some(Hit::Pane(i)) => Some(i),
                _ => None,
            };
            let mut hits = Vec::new();
            right = panes.host.paint_buttons(
                (layer, ctx.text, ctx.icons, &ds),
                (right, mid, side),
                hovered,
                &mut hits,
            );
            self.hits.extend(hits.into_iter().map(|(r, i)| (r, Hit::Pane(i))));
        }
        let spec = ButtonSpec::new(ButtonKind::Ghost)
            .icon("download")
            .enabled(self.export.is_none())
            .hovered(self.hover == Some(Hit::Export));
        ds::button(layer, ctx.text, ctx.icons, &ds, export, spec);
        if self.export.is_none() {
            self.hits.push((export, Hit::Export));
        }
        // What to know of the rows, between.
        if let Some((note, _)) = self.note() {
            let room = (right - px(10.0) - x).max(0.0);
            let h = px(button_size::SMALL);
            let note_spec = |label| {
                ButtonSpec::new(ButtonKind::Soft)
                    .round()
                    .icon("info")
                    .label(label)
                    .tone(Tone::Neutral)
                    .hovered(self.hover == Some(Hit::Note))
            };
            // What the pill takes besides its label: its icon, the gap, the padding.
            let chrome = ds::button_width(ctx.text, &ds, &note_spec(""), h);
            let font = ds::button_label_font(&ds);
            let line = ds::ellipsize(ctx.text, &note, &font, (room - chrome).max(0.0));
            if room > px(NOTE_MIN_ROOM) {
                let w = ds::button_width(ctx.text, &ds, &note_spec(&line), h);
                let r = Rect::new(x, (mid - h / 2.0).round(), w, h);
                ds::button(layer, ctx.text, ctx.icons, &ds, r, note_spec(&line));
                self.hits.push((r, Hit::Note));
            }
        }
        let _ = Font::SEMIBOLD;
    }
}

/// The room under which the rows' note isn't shown at all (logical pixels): less would
/// leave only its icon and an ellipsis.
const NOTE_MIN_ROOM: f32 = 60.0;

/// The runner's pool (an export of rows here is written there too).
fn runner_runtime(runner: &Runner) -> Runtime {
    runner.runtime()
}

/// What an export of rows here writes with: the format, the source's SQL, the columns its
/// SQL skips.
#[derive(Debug)]
pub struct Writing<'a> {
    pub format: &'a ExportFormat,
    pub sql: Option<Arc<dyn extractors::SqlStyle>>,
    pub options: extractors::Options,
}

/// The rows of `view` (in `order`, if given), the `columns` asked, written as `writing` says
/// to `target`: never more than a block of text at a time in memory.
pub fn write_rows(
    view: &crate::runner::ResultView,
    order: Option<&[u32]>,
    columns: &[GridColumn],
    table: &str,
    writing: &Writing<'_>,
    target: &Target,
    state: &ExportState,
) -> ExportOutcome {
    let mut sink = match ExportSink::open(target) {
        Ok(sink) => sink,
        Err(e) => return ExportOutcome::Failed(e.to_string()),
    };
    let mut stream =
        ExportWriter::new(writing.format, table, columns, writing.sql.clone(), writing.options);
    let at: Vec<Option<usize>> =
        columns.iter().map(|w| view.columns.iter().position(|c| c.name == w.name)).collect();
    let write = |sink: &mut ExportSink, bytes: Result<Vec<u8>, String>| {
        bytes.and_then(|b| sink.write_bytes(&b).map_err(|e| e.to_string()))
    };
    if let Err(why) = write(&mut sink, stream.begin()) {
        sink.discard();
        return ExportOutcome::Failed(why);
    }
    let mut block = Vec::with_capacity(1000);
    let flush = |sink: &mut ExportSink, stream: &mut ExportWriter, block: &mut Vec<Vec<_>>| {
        if block.is_empty() {
            return Ok(());
        }
        let bytes = stream.rows(block);
        state.rows.fetch_add(block.len() as u64, Ordering::Relaxed);
        block.clear();
        bytes.and_then(|b| sink.write_bytes(&b).map_err(|e| e.to_string()))
    };
    for row in rows_in_order(view, order) {
        if state.cancel.is_cancelled() {
            sink.discard();
            return ExportOutcome::Cancelled;
        }
        block.push(
            at.iter()
                .map(|i| {
                    i.and_then(|i| row.get(i))
                        .map_or(ui_components::data_grid::CellRef::Null, cell_ref)
                })
                .collect(),
        );
        if block.len() >= 1000
            && let Err(why) = flush(&mut sink, &mut stream, &mut block)
        {
            sink.discard();
            return ExportOutcome::Failed(why);
        }
    }
    if let Err(why) = flush(&mut sink, &mut stream, &mut block) {
        sink.discard();
        return ExportOutcome::Failed(why);
    }
    let written = stream.written();
    let text = stream.is_text();
    match stream.finish() {
        Ok(finish) => sink.complete(&finish, written, text),
        Err(why) => {
            sink.discard();
            ExportOutcome::Failed(why)
        }
    }
}

impl Viewer for ResultView {
    fn memory(&self) -> ViewMemory {
        // The rows are the runner's (counted there).
        ViewMemory::default()
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        if let Some(waker) = &ctx.waker {
            self.runner.add_wake("view", Arc::clone(waker));
        }
        self.hits.clear();
        let b = ctx.bounds;
        let px = |v: f32| (v * ctx.scale).round();
        let bar = Rect::new(b.min_x(), b.min_y(), b.size.width, px(BAR_H));
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, ctx.palette.editor_background));
        self.paint_bar(&mut ctx, bar);
        let area =
            Rect::new(b.min_x(), bar.max_y(), b.size.width, (b.max_y() - bar.max_y()).max(0.0));
        self.body = area;
        let (grid, pane) = match &mut self.panes {
            Some(panes) => panes.host.split(area, ctx.scale),
            None => (area, None),
        };
        self.grid.paint(GridPaint {
            bounds: grid,
            scale: ctx.scale,
            text: ctx.text,
            scene: ctx.scene,
            palette: ctx.palette,
            icons: ctx.icons,
        });
        if let (Some(rect), Some(pane)) = (pane, self.panes.as_mut().and_then(|p| p.host.pane())) {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let layer = ctx.scene.push_layer(Some(rect));
            layer.quad(Quad::filled(rect, ctx.palette.editor_background));
            ds::divider(layer, &ds, rect.min_x(), rect.max_x(), rect.min_y());
            pane.paint(ViewerPaint {
                bounds: rect,
                scale: ctx.scale,
                text: ctx.text,
                scene: ctx.scene,
                palette: ctx.palette,
                icons: ctx.icons,
                focused: ctx.focused,
                waker: ctx.waker.clone(),
            });
        }
    }

    fn poll(&mut self) -> bool {
        let generation = self.runner.generation();
        let seen = std::mem::replace(&mut self.seen, generation) != generation;
        if seen || !self.rights_read {
            self.rights_read = true;
            self.follow_rights();
        }
        let mut changed = self.grid.poll();
        if let Some(notice) = self.grid.source_mut().take_notice() {
            self.told.push(Notification::info(notice));
        }
        changed |= self.take_submitted();
        changed |= self.follow_export();
        changed |= self.follow_panes();
        changed || seen
    }

    fn actions_view(&self) -> Option<&'static str> {
        Some(crate::edit::VIEW)
    }

    fn is_loading(&self) -> bool {
        self.grid.is_loading() || self.panes.as_ref().is_some_and(crate::result_panes::Panes::busy)
    }

    fn working(&self) -> bool {
        self.is_loading() || self.export.is_some()
    }

    /// A pane under the grid moves (a chart's transition): frames while it does.
    fn is_animating(&self) -> bool {
        self.panes.as_ref().is_some_and(|panes| panes.host.animating())
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        match self.hit(p) {
            Some(Hit::Commit) => {
                self.runner.finish_transaction(true);
                return None;
            }
            Some(Hit::Rollback) => {
                self.runner.finish_transaction(false);
                return None;
            }
            Some(Hit::Export) => {
                // Every row to a file, in the project's data extractor (its format otherwise by
                // the name's extension); the formats one by one in its menu (right click), as in
                // the grid's.
                self.ask_export(ExportFormat::chosen(self.grid.project()));
                return self.action.take();
            }
            Some(Hit::Pane(i)) => {
                if let Some(panes) = &mut self.panes {
                    panes.host.toggle(i);
                }
                return None;
            }
            Some(_) => return None,
            None => {}
        }
        if let Some(panes) = &mut self.panes
            && panes.host.over(p)
        {
            panes.host.press(p, clicks);
            return None;
        }
        // The grid (⇧ / ⌘ extend its choice).
        self.with_controls(|c, parts| c.press_with(parts, p, clicks, mods));
        self.take_grid_action();
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.pointer_down(p, 1, PressMods::default())
    }

    fn drag(&mut self, p: Point) -> bool {
        let body = self.body;
        if let Some(moved) = self.panes.as_mut().and_then(|panes| panes.host.drag(p, body)) {
            return moved;
        }
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        if let Some(changed) = self.panes.as_mut().and_then(|panes| panes.host.release()) {
            return changed;
        }
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        let pane = self.panes.as_mut().is_some_and(|panes| panes.host.hover(p));
        changed | pane | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        if let Some((r, hit)) = self.hits.iter().rev().find(|(r, _)| r.contains(p)) {
            let label = match hit {
                Hit::Commit => i18n::t("console.tip_commit"),
                Hit::Rollback => i18n::t("console.tip_rollback"),
                Hit::Export => i18n::t("console.tip_export"),
                Hit::Time => {
                    let view = self.grid.source().result()?;
                    i18n::tr!(
                        "console.tip_time",
                        total = millis(view.execution + view.fetching),
                        exec = millis(view.execution),
                        fetch = millis(view.fetching)
                    )
                }
                Hit::Note => self.note()?.1,
                Hit::Pane(i) => return self.panes.as_ref()?.host.button_tip(*i, *r),
            };
            return Some(Tip::new(*r, label));
        }
        if let Some(panes) = self.panes.as_ref().filter(|panes| panes.host.over(p)) {
            return panes.host.tooltip(p);
        }
        self.controls.tooltip_at(p)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if matches!(self.hit(p), Some(Hit::Commit | Hit::Rollback | Hit::Export | Hit::Pane(_))) {
            return Some(ViewerCursor::Pointer);
        }
        if let Some(panes) = self.panes.as_ref().filter(|panes| panes.host.over(p)) {
            return panes.host.cursor(p);
        }
        self.controls.cursor_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.panes.as_mut().is_some_and(|panes| panes.host.scroll(dx, dy)) {
            return true;
        }
        self.with_controls(|c, parts| c.scroll_focused(parts, dx, dy)).is_some()
    }

    fn wants_text(&self) -> bool {
        self.pane_captures() || self.grid.wants_text()
    }

    fn text_input(&mut self, text: &str) -> bool {
        if self.pane_captures() {
            return self.panes.as_mut().is_some_and(|panes| panes.host.text(text));
        }
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn key(&mut self, c: char) -> bool {
        // A pane's open list takes what's typed (its search), as text.
        if self.pane_captures() {
            return self.text_input(&c.to_string());
        }
        // The grid's control has no `char_key`: its typing starts here.
        self.grid.type_char(c)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.pane_captures() {
            return self.panes.as_mut().is_some_and(|panes| panes.host.key(key, reach));
        }
        let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        self.take_grid_action();
        used
    }

    fn select_all(&mut self) -> bool {
        self.grid.select_all()
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| self.part(i))
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if self.hit(p) == Some(Hit::Export) {
            return Some(ui_components::data_grid::export_menu(self.grid.project()));
        }
        let (_, mut entries) = self.with_controls(|c, parts| c.menu(parts, p))?;
        // On the rows: a pane drawn from the selection, from all of them again, closed.
        if let Some(panes) = &self.panes
            && !panes.host.over(p)
        {
            entries.extend(panes.host.menu(self.grid.has_picked()));
        }
        // Its table's data, and the row a key's cell points to (as the data view's).
        if self.grid.source().editable().is_some() {
            entries.push(ViewMenuEntry::Separator);
            entries.push(
                ViewMenuEntry::item("open-table", i18n::t("console.open_table")).with_icon("table"),
            );
            if self.reference().is_some() {
                entries.push(
                    ViewMenuEntry::item("goto-ref", i18n::t("database.goto_reference"))
                        .with_icon("link"),
                );
            }
        }
        Some(entries)
    }

    /// The menu its grid's toolbar asked for ("…", the page sizes): the app shows it.
    fn take_menu(&mut self) -> Option<(kernel::Rect, Vec<ViewMenuEntry>)> {
        self.grid.take_menu()
    }

    fn command(&mut self, id: &str) -> bool {
        // A pane's row of the right-click menu (the selection taken only when it's asked).
        if id.starts_with(ui_components::result_pane::PANE_MENU)
            && let Some(panes) = &mut self.panes
        {
            let grid = &self.grid;
            let picked = || {
                grid.picked().map(|p| std::sync::Arc::new(p) as ui_components::result_pane::Lent)
            };
            return panes.host.menu_command(id, picked);
        }
        let used = match id {
            "find" => {
                self.grid.open_find();
                true
            }
            "refresh" => {
                self.grid.refresh();
                true
            }
            "select_all" => self.grid.select_all(),
            "submit" => self.grid.submit(),
            // A snapshot: the sample's rows edited (nothing submitted).
            #[cfg(feature = "snapshot")]
            "snapshot-edit" => self.snapshot_edit(),
            "open-table" => self.open_table(None),
            "goto-ref" => self.go_to_reference(),
            "export-cancel" => self.export.as_ref().is_some_and(|e| {
                e.cancel.cancel();
                true
            }),
            id if id.starts_with("picked-save:") && self.pane_saving => {
                self.pane_saving = false;
                if matches!(
                    self.action,
                    Some(ViewerAction::Request(AppRequest::PickSavePath { .. }))
                ) {
                    self.action = None;
                }
                self.panes.as_mut().is_some_and(|panes| panes.host.command(id))
            }
            id if id.starts_with("picked-save:") => match self.export_format.take() {
                Some(chosen) => {
                    if matches!(
                        self.action,
                        Some(ViewerAction::Request(AppRequest::PickSavePath { .. }))
                    ) {
                        self.action = None;
                    }
                    let path = PathBuf::from(&id["picked-save:".len()..]);
                    let format = ui_components::data_grid::export::format_of(
                        &path,
                        self.grid.project(),
                        &chosen,
                    )
                    .unwrap_or(chosen);
                    self.start_export(format, Target::File(path));
                    true
                }
                None => false,
            },
            id if id.starts_with(MENU_PREFIX) || id == "transpose" => self.grid.command(id),
            _ => false,
        };
        self.take_grid_action();
        used
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.told)
    }

    fn purge(&mut self) {
        self.grid.refresh();
    }

    fn status(&self) -> Option<String> {
        let source = self.grid.source();
        let title = source.result()?.title.clone();
        Some(match source.total().known() {
            Some(n) => i18n::tr!("database.status", table = title, count = n),
            None => title,
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/result_view.rs"]
mod tests;
