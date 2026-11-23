//! A chart of any data (plan 94h): "Open as Chart" on a file (or its selection) — JSON,
//! YAML, XML, HTML, CSV, TSV — opens a tab whose tables are found by `data-tables` on a
//! worker (the text as the editor has it: nothing read from the disk), the first one drawn
//! with the same chart and series settings as a query's result, a choice of the others
//! above it when the document holds several.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, LazyLock};

use charts_model::Tabular;
use data_tables::{DataTable, Limits};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{Control, Press};
use ui_components::ds::{self, Ctx, DropdownSpec};
use ui_components::picker::{Picker, PickerEvent, PickerItem};
use ui_components::result_pane::{Held, PaneRequest, ResultPane};
use ui_components::{
    KeyReach, PressMods, Tip, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use workbench_model::{CommandHandler, WorkbenchHost};

use crate::pane::ChartPane;

/// The tab's virtual extension.
pub const EXTENSION: &str = "view-chart";
/// The command opening it.
pub const OPEN_COMMAND: &str = "charts.open_data";
/// The most of a document read for a chart (bytes): a bigger one is read up to here.
const MAX_TEXT: usize = 64 << 20;
/// The bar over the chart (logical pixels).
const BAR_H: f32 = 36.0;

/// The documents' reading, shared by all tabs.
static READ: LazyLock<kernel::Pool> =
    LazyLock::new(|| kernel::Pool::new(crate::PREPARE_THREAD, 2, kernel::work::IDLE));

/// What a chart tab is opened on: a document's name and its text.
#[derive(Debug, Clone)]
pub struct ChartOf {
    pub name: String,
    pub text: Arc<str>,
}

/// "Open as Chart": the active file (its selection, when there is one) in a chart tab.
#[derive(Debug)]
pub struct OpenAsChart;

impl CommandHandler for OpenAsChart {
    /// On a data file, or on any text with something selected (a JSON pasted in a note).
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some_and(|f| {
            f.selection.as_ref().is_some_and(|r| !r.is_empty()) || is_data(&f.path)
        })
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file() else { return false };
        let name = kernel::paths::file_name(&file.path);
        let text: Arc<str> = match file.selection.and_then(|r| file.text.get(r)) {
            Some(selected) if !selected.trim().is_empty() => Arc::from(selected),
            _ => Arc::clone(&file.text),
        };
        let title = i18n::tr!("charts.tab_title", name = name.as_str());
        host.open_view(&title, EXTENSION, &ChartOf { name, text })
    }
}

/// The extensions of the files data is read from as they are.
const DATA_EXTENSIONS: [&str; 11] =
    ["json", "jsonl", "geojson", "yaml", "yml", "xml", "html", "htm", "csv", "tsv", "tab"];

/// `path` is a data file (by its extension).
fn is_data(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| DATA_EXTENSIONS.iter().any(|d| d.eq_ignore_ascii_case(e)))
}

/// The chart tab's kind.
#[derive(Debug)]
pub struct ChartFileProvider;

impl ViewerProvider for ChartFileProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ChartFileView::default()))
    }
}

/// A chart tab (see the module's doc).
#[derive(Debug, Default)]
pub struct ChartFileView {
    reading: Option<Receiver<Vec<DataTable>>>,
    tables: Vec<Arc<DataTable>>,
    shown: usize,
    pane: ChartPane,
    /// The tables' choice, open.
    picker: Picker,
    choice: Rect,
    choice_hover: bool,
    pane_area: Rect,
    read: bool,
    /// What it asks of the app (the chart's "Save As"), what it tells the user.
    action: Option<ViewerAction>,
    told: Vec<workbench_model::Notification>,
}

impl ChartFileView {
    /// Table `i` drawn (its series settings as they were: a column it lacks says so).
    fn show(&mut self, i: usize) {
        let Some(table) = self.tables.get(i) else { return };
        self.shown = i;
        let held = Held { rows: table.rows(), more: table.left_out > 0, ..Held::default() };
        self.pane.set_rows(Arc::clone(table) as ui_components::result_pane::Lent, i as u64, held);
    }

    fn label(t: &DataTable) -> String {
        i18n::tr!("charts.table_choice", path = t.path.as_str(), count = t.rows())
    }
}

impl Viewer for ChartFileView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(of) = data.downcast_ref::<ChartOf>() else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let of = of.clone();
        READ.spawn(kernel::Priority::Interactive, None, move || {
            let end = of.text.floor_char_boundary(MAX_TEXT.min(of.text.len()));
            let _ = tx.send(data_tables::tables(&of.name, &of.text[..end], Limits::default()));
        });
        self.reading = Some(rx);
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let b = ctx.bounds;
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, ctx.palette.editor_background));
        if !self.read || self.tables.is_empty() {
            let message =
                if self.read { i18n::t("charts.no_tables") } else { i18n::t("charts.reading") };
            ds::empty_state(
                layer,
                ctx.text,
                &ds,
                b,
                &message,
                self.read.then(|| i18n::t("charts.no_tables_detail")).as_deref(),
            );
            return;
        }
        let bar = Rect::new(b.min_x(), b.min_y(), b.size.width, px(BAR_H));
        ds::divider(layer, &ds, bar.min_x(), bar.max_x(), bar.max_y() - ds.line());
        let label = self.tables.get(self.shown).map(|t| Self::label(t)).unwrap_or_default();
        let h = px(26.0);
        let w = (b.size.width * 0.5).min(px(420.0));
        self.choice =
            Rect::new(bar.min_x() + px(10.0), bar.min_y() + (bar.size.height - h) / 2.0, w, h);
        let many = self.tables.len() > 1;
        let spec = DropdownSpec {
            value: &label,
            open: self.picker.is_open(),
            hovered: self.choice_hover,
            enabled: many,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, ctx.text, ctx.icons, &ds, self.choice, spec);
        self.pane_area =
            Rect::new(b.min_x(), bar.max_y(), b.size.width, (b.max_y() - bar.max_y()).max(0.0));
        let (scene, text, icons, palette) =
            (&mut *ctx.scene, &mut *ctx.text, ctx.icons, ctx.palette);
        self.pane.paint(ViewerPaint {
            bounds: self.pane_area,
            scale: ctx.scale,
            text,
            scene,
            palette,
            icons,
            focused: ctx.focused,
            waker: ctx.waker.clone(),
        });
        self.picker.paint(scene, text, icons, &ds, (self.choice, b), "");
    }

    fn poll(&mut self) -> bool {
        let mut changed = self.pane.poll();
        match self.pane.take_request() {
            Some(PaneRequest::SaveAs { name }) => {
                self.action =
                    Some(ViewerAction::Request(ui_components::AppRequest::PickSavePath {
                        start: None,
                        name,
                    }));
                changed = true;
            }
            Some(PaneRequest::Tell { text, ok }) => {
                use workbench_model::Notification;
                self.told.push(if ok {
                    Notification::success(text)
                } else {
                    Notification::error(text)
                });
            }
            // Every row is here already.
            Some(PaneRequest::AllRows { .. } | PaneRequest::WholeRows) | None => {}
        }
        if let Some(rx) = &self.reading {
            match rx.try_recv() {
                Ok(tables) => {
                    self.tables = tables.into_iter().map(Arc::new).collect();
                    self.read = true;
                    self.reading = None;
                    self.show(0);
                    changed = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.read = true;
                    self.reading = None;
                    changed = true;
                }
            }
        }
        changed
    }

    fn is_loading(&self) -> bool {
        self.reading.is_some() || self.pane.busy()
    }

    fn is_animating(&self) -> bool {
        self.pane.animating()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.told)
    }

    /// The path the chart's "Save As" chose.
    fn command(&mut self, id: &str) -> bool {
        self.pane.command(id)
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Option<ViewerAction> {
        if self.picker.is_open() {
            if self.picker.contains(p) {
                if let Some(PickerEvent::Chosen(i)) = self.picker.press_with(p, clicks) {
                    self.show(i);
                }
                return None;
            }
            self.picker.close();
            if self.choice.contains(p) {
                return None;
            }
        }
        if self.choice.contains(p) && self.tables.len() > 1 {
            let items = self
                .tables
                .iter()
                .map(|t| PickerItem { label: Self::label(t), ..PickerItem::default() })
                .collect();
            self.picker.open(items, Some(self.shown));
            return None;
        }
        if self.pane_area.contains(p) {
            let _: Press = self.pane.press(p, clicks);
        }
        None
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.pointer_down(p, 1, PressMods::default())
    }

    fn drag(&mut self, p: Point) -> bool {
        if self.picker.dragging() {
            return self.picker.drag(p);
        }
        self.pane.drag(p)
    }

    fn release(&mut self) -> bool {
        if self.picker.dragging() {
            return self.picker.release();
        }
        self.pane.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        if self.picker.is_open() && self.picker.contains(p) {
            return self.picker.hover(p);
        }
        let over = self.choice.contains(p);
        let changed = over != self.choice_hover;
        self.choice_hover = over;
        changed | self.pane.hover(self.pane_area.contains(p).then_some(p))
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.picker.is_open() {
            self.picker.scroll(dy);
            return true;
        }
        self.pane.scroll(dx, dy)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        self.pane_area.contains(p).then(|| self.pane.tooltip(p)).flatten()
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.picker.is_open() && self.picker.contains(p) {
            return self.picker.cursor_at(p);
        }
        if self.choice.contains(p) && self.tables.len() > 1 {
            return Some(ViewerCursor::Pointer);
        }
        self.pane.cursor(p)
    }

    fn wants_text(&self) -> bool {
        self.picker.is_open() || self.pane.captures_keys()
    }

    fn text_input(&mut self, text: &str) -> bool {
        if self.picker.is_open() {
            return self.picker.text_input(text);
        }
        self.pane.text(text)
    }

    fn key(&mut self, c: char) -> bool {
        self.wants_text() && self.text_input(&c.to_string())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.picker.is_open() {
            if let Some(PickerEvent::Chosen(i)) = self.picker.key(key, reach) {
                self.show(i);
            }
            return true;
        }
        self.pane.key(key, reach)
    }
}

#[cfg(test)]
#[path = "../tests/unit/file_view.rs"]
mod tests;
