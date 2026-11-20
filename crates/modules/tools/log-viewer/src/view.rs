//! A log file's view, in its editor tab: a bar (the file, how many lines, live or paused,
//! Pause / Resume, the file before and after) over the app's grid of its lines — the newest
//! first, filtered by the grid's own WHERE (`level = 'ERROR'`, `msg LIKE '%save%'`). A row
//! chosen (a click, the arrows) shows its details under the grid: its message whole, what
//! it is, each field on its own, the spans it ran in — selectable, scrolled, closed by ×. The
//! file is read by its own thread (`source`), made at the first paint (when the view's
//! waker is known) and let go with the view.

use std::path::{Path, PathBuf};

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::data_grid::{DataGrid, GridPaint};
use ui_components::ds::{self, Ctx};
use ui_components::tool_panel::KeyReach;
use ui_components::{PressMods, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};
use ui_kit::draw_text;

use crate::source::LogSource;

/// The bar's height (logical pixels).
const BAR_H: f32 = 44.0;
/// The details' header (logical pixels), and their share of the height under the bar.
const DETAILS_HEAD: f32 = 34.0;
const DETAILS_SHARE: f32 = 0.4;

/// What the bar's buttons do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Pause,
    Older,
    Newer,
}

/// The view of one log file.
pub struct LogFileView {
    path: PathBuf,
    grid: Option<DataGrid<LogSource>>,
    /// The thread couldn't start (said in the view).
    failed: bool,
    buttons: Vec<(Rect, Button)>,
    hover: Option<Button>,
    grid_area: Rect,
    action: Option<ViewerAction>,
    /// The details of the row chosen (the source's row), and their text.
    details: Option<(u64, ui_components::ReadText)>,
    /// Closed by ×: none shown until another row is chosen.
    details_closed: Option<u64>,
    details_area: Rect,
    details_close: Rect,
    close_hover: bool,
    /// The mouse is over the details (the wheel scrolls them).
    hover_in_details: bool,
    /// The details' line as written (what "Copy the Line" copies).
    details_raw: String,
}

/// The details' right-click menu: what's selected, the message, the line as written.
const COPY_SELECTION: &str = "log.copy_selection";
const COPY_MESSAGE: &str = "log.copy_message";
const COPY_LINE: &str = "log.copy_line";

impl std::fmt::Debug for LogFileView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogFileView").field("path", &self.path).finish_non_exhaustive()
    }
}

impl LogFileView {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            grid: None,
            failed: false,
            buttons: Vec::new(),
            hover: None,
            grid_area: Rect::default(),
            action: None,
            details: None,
            details_closed: None,
            details_area: Rect::default(),
            details_close: Rect::default(),
            close_hover: false,
            hover_in_details: false,
            details_raw: String::new(),
        }
    }

    /// The row chosen in the grid (the source's), if any.
    fn chosen(&self) -> Option<u64> {
        let grid = self.grid.as_ref()?;
        let cursor = grid.selection().cursor()?;
        Some(grid.row_of(cursor.row))
    }

    /// The details follow the row chosen: made when it changes (its line here), gone when
    /// none is; closed by ×, none until another row is chosen.
    fn sync_details(&mut self) -> bool {
        let chosen = self.chosen();
        if chosen.is_none() || chosen == self.details_closed {
            let had = self.details.take().is_some();
            if chosen.is_none() {
                self.details_closed = None;
            }
            return had;
        }
        let row = chosen.unwrap_or_default();
        if self.details.as_ref().is_some_and(|(r, _)| *r == row) {
            return false;
        }
        self.details_closed = None;
        let raw = self.grid.as_ref().and_then(|g| g.source().entry_at(row)).map(|e| e.raw.clone());
        match raw {
            Some(raw) => {
                self.details =
                    Some((row, ui_components::ReadText::new(crate::entry::details(&raw))));
                self.details_raw = raw;
                true
            }
            // Its block still coming: made at a later poll.
            None => false,
        }
    }

    fn close_details(&mut self) {
        if let Some((row, _)) = self.details.take() {
            self.details_closed = Some(row);
        }
    }

    fn in_details(&self, p: Point) -> bool {
        self.details.is_some() && self.details_area.contains(p)
    }

    /// The details' panel under the grid: a line, "Details" and ×, their text.
    fn paint_details(&mut self, ctx: &mut ViewerPaint<'_>, area: Rect) {
        let Some((_, read)) = &mut self.details else { return };
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let px = |v: f32| ds.px(v);
        let p = ctx.palette;
        let head = Rect::new(area.min_x(), area.min_y(), area.size.width, px(DETAILS_HEAD));
        {
            let layer = ctx.scene.push_layer(Some(area));
            layer.quad(Quad::filled(area, p.editor_background));
            layer.quad(Quad::filled(
                Rect::new(area.min_x(), area.min_y(), area.size.width, ds.line()),
                p.border,
            ));
            let font = ds.ui_font(-1.0).with_weight(text_layout::Font::SEMIBOLD);
            let title = ctx.text.shape_line(&t("log_viewer.details"), &font);
            draw_text(
                layer,
                &title,
                area.min_x() + px(16.0),
                ds::optical_baseline(head.min_y(), head.size.height, &font),
                p.text_muted,
            );
            let size = px(14.0);
            self.details_close = Rect::new(
                area.max_x() - px(16.0) - size,
                (head.min_y() + (head.size.height - size) / 2.0).round(),
                size,
                size,
            );
            if let Some(id) = ctx.icons.id("x") {
                let color = if self.close_hover { p.text } else { p.text_muted };
                ui_kit::draw_icon(layer, ctx.icons, id, self.details_close, color);
            }
        }
        let body = Rect::new(
            area.min_x() + px(8.0),
            head.max_y(),
            area.size.width - px(16.0),
            (area.max_y() - head.max_y()).max(0.0),
        );
        read.paint(ctx.scene, ctx.text, &ds, body);
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }

    fn press_button(&mut self, button: Button) {
        let Some(grid) = &mut self.grid else { return };
        let source = grid.source_mut();
        match button {
            Button::Pause => source.pause(!source.paused()),
            Button::Older | Button::Newer => {
                let (older, newer) = source.neighbors().clone();
                let target = if button == Button::Older { older } else { newer };
                if let Some(path) = target {
                    self.action = Some(ViewerAction::OpenFile { path, line: None, column: None });
                }
            }
        }
    }

    /// The bar: the file's name and what's shown, then the buttons on the right.
    fn paint_bar(&mut self, ctx: &mut ViewerPaint<'_>, bar: Rect) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let px = |v: f32| ds.px(v);
        let p = ctx.palette;
        let layer = ctx.scene.push_layer(Some(bar));
        layer.quad(Quad::filled(
            Rect::new(bar.min_x(), bar.max_y() - ds.line(), bar.size.width, ds.line()),
            p.border,
        ));
        let (paused, shown, latest, older, newer) = match &self.grid {
            Some(grid) => {
                let source = grid.source();
                let (shown, latest) = source.counts();
                let (older, newer) = source.neighbors();
                (source.paused(), shown, latest, older.is_some(), newer.is_some())
            }
            None => (false, 0, 0, false, false),
        };
        // The buttons, right to left.
        self.buttons.clear();
        let mut right = bar.max_x() - px(12.0);
        let mid = bar.min_y() + bar.size.height / 2.0;
        let h = px(ds::TOOLBAR_BUTTON_H);
        let pause_label = t(if paused { "log_viewer.resume" } else { "log_viewer.pause" });
        let buttons = [
            (Button::Newer, t("log_viewer.newer"), "chevron-right", newer, false),
            (Button::Older, t("log_viewer.older"), "chevron-left", older, false),
            (Button::Pause, pause_label, if paused { "play" } else { "pause" }, true, paused),
        ];
        for (button, label, icon, enabled, accent) in buttons {
            if !enabled {
                continue;
            }
            let w = ds::toolbar_button_width(ctx.text, &ds, &label);
            let rect = Rect::new(right - w, (mid - h / 2.0).round(), w, h);
            let look = ds::ToolbarButton {
                label: &label,
                icon,
                accent,
                hovered: self.hover == Some(button),
            };
            ds::toolbar_button(layer, ctx.text, ctx.icons, &ds, rect, look);
            self.buttons.push((rect, button));
            right = rect.min_x() - px(8.0);
        }
        // The file, then how many lines (and how many came while paused).
        let left = bar.min_x() + px(16.0);
        let name =
            self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let font = ds.ui_font(0.0).with_weight(text_layout::Font::SEMIBOLD);
        let shaped = ctx.text.shape_line(&name, &font);
        draw_text(
            layer,
            &shaped,
            left,
            ds::optical_baseline(bar.min_y(), bar.size.height, &font),
            p.text,
        );
        let status = if paused && latest > shown {
            tr!("log_viewer.paused_count", shown = shown, new = latest - shown)
        } else if paused {
            tr!("log_viewer.paused_lines", count = shown)
        } else {
            tr!("log_viewer.live_lines", count = shown)
        };
        let small = ds.ui_font(-1.0);
        let x = left + shaped.width + px(14.0);
        // A dot: live (green) or paused (the warning color).
        let dot = px(7.0);
        let color = if paused { p.warning } else { ds::Tone::Success.color(p) };
        layer.quad(
            Quad::filled(Rect::new(x, (mid - dot / 2.0).round(), dot, dot), color)
                .rounded(dot / 2.0),
        );
        let room = (right - x - dot - px(8.0)).max(0.0);
        ds::draw_ellipsized(
            layer,
            ctx.text,
            &status,
            &small,
            (x + dot + px(8.0), ds::optical_baseline(bar.min_y(), bar.size.height, &small)),
            room,
            p.text_muted,
        );
    }
}

impl Viewer for LogFileView {
    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        if self.grid.is_none() && !self.failed {
            // Without a waker (a snapshot), polled at each frame all the same.
            let wake: std::sync::Arc<dyn Fn() + Send + Sync> =
                ctx.waker.clone().unwrap_or_else(|| std::sync::Arc::new(|| {}));
            match LogSource::open(self.path.clone(), wake) {
                Some(source) => self.grid = Some(DataGrid::new(source, None)),
                None => self.failed = true,
            }
        }
        let b = ctx.bounds;
        let bar_h = (BAR_H * ctx.scale).round();
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, ctx.palette.editor_background));
        self.paint_bar(&mut ctx, Rect::new(b.min_x(), b.min_y(), b.size.width, bar_h));
        let below =
            Rect::new(b.min_x(), b.min_y() + bar_h, b.size.width, (b.size.height - bar_h).max(0.0));
        // The details, when a row is chosen: under the grid, their share of the height.
        let details_h =
            if self.details.is_some() { (below.size.height * DETAILS_SHARE).round() } else { 0.0 };
        self.grid_area = Rect::new(
            below.min_x(),
            below.min_y(),
            below.size.width,
            below.size.height - details_h,
        );
        self.details_area =
            Rect::new(below.min_x(), self.grid_area.max_y(), below.size.width, details_h);
        if let Some(grid) = &mut self.grid {
            grid.paint(GridPaint {
                bounds: self.grid_area,
                scale: ctx.scale,
                text: ctx.text,
                scene: ctx.scene,
                palette: ctx.palette,
                icons: ctx.icons,
            });
        }
        let area = self.details_area;
        if self.details.is_some() {
            self.paint_details(&mut ctx, area);
        }
    }

    fn poll(&mut self) -> bool {
        let polled = self.grid.as_mut().is_some_and(DataGrid::poll);
        polled | self.sync_details()
    }

    fn is_loading(&self) -> bool {
        self.grid.as_ref().is_some_and(DataGrid::is_loading)
    }

    /// Its reader still walks the file or reads rows asked for.
    fn working(&self) -> bool {
        self.is_loading()
    }

    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        if let Some(button) = self.button_at(p) {
            self.press_button(button);
            return self.action.take();
        }
        if self.in_details(p) {
            if self.details_close.contains(p) {
                self.close_details();
            } else if let Some((_, read)) = &mut self.details {
                read.press(p, clicks, mods.shift);
            }
            return self.action.take();
        }
        if let Some(grid) = &mut self.grid
            && self.grid_area.contains(p)
        {
            grid.press(p, clicks, mods);
        }
        self.sync_details();
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.pointer_down(p, 1, PressMods::default())
    }

    fn drag(&mut self, p: Point) -> bool {
        let text = self.details.as_mut().is_some_and(|(_, r)| r.drag(p));
        text | self.grid.as_mut().is_some_and(|g| g.drag(p))
    }

    fn release(&mut self) -> bool {
        let text = self.details.as_mut().is_some_and(|(_, r)| r.release());
        text | self.grid.as_mut().is_some_and(DataGrid::release)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.button_at(p);
        let close = self.in_details(p) && self.details_close.contains(p);
        let changed = hover != self.hover || close != self.close_hover;
        self.hover = hover;
        self.close_hover = close;
        self.hover_in_details = self.in_details(p);
        let text = self.details.as_mut().is_some_and(|(_, r)| r.hover(p));
        changed | text | self.grid.as_mut().is_some_and(|g| g.hover(p))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.button_at(p).is_some() || (self.in_details(p) && self.details_close.contains(p)) {
            return Some(ViewerCursor::Pointer);
        }
        if self.in_details(p) {
            let over = self.details.as_ref().is_some_and(|(_, r)| r.over_text(p));
            return Some(if over { ViewerCursor::Text } else { ViewerCursor::Arrow });
        }
        self.grid.as_ref()?.cursor_at(p)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.grid.as_ref()?.tooltip_at(p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        if self.hover_in_details
            && let Some((_, read)) = &mut self.details
        {
            return read.scroll_by(dy);
        }
        self.grid.as_mut().is_some_and(|g| g.scroll(dx, dy))
    }

    fn wants_text(&self) -> bool {
        self.grid.as_ref().is_some_and(DataGrid::wants_text)
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.grid.as_mut().is_some_and(|g| g.text_input(typed))
    }

    fn key(&mut self, c: char) -> bool {
        self.grid.as_mut().is_some_and(|g| g.type_char(c))
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = self.grid.as_mut().is_some_and(|g| g.edit_key(key, reach));
        // Esc the grid didn't take: the details close.
        if !used && key == ViewerKey::Escape && self.details.is_some() {
            self.close_details();
            return true;
        }
        self.sync_details();
        used
    }

    fn select_all(&mut self) -> bool {
        self.grid.as_mut().is_some_and(DataGrid::select_all)
    }

    /// The details' text selected first (what was chosen last), else the grid's cells.
    fn selected_text(&self) -> Option<String> {
        if let Some(text) = self.details.as_ref().and_then(|(_, r)| r.selected_text()) {
            return Some(text);
        }
        self.grid.as_ref()?.selected_text()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        use ui_components::ViewMenuEntry;
        if self.in_details(p) {
            let selected = self.details.as_ref().is_some_and(|(_, r)| r.has_selection());
            return Some(vec![
                ViewMenuEntry::item(COPY_SELECTION, t("log_viewer.copy_selection"))
                    .with_icon("copy")
                    .disabled_if(!selected),
                ViewMenuEntry::item(COPY_MESSAGE, t("log_viewer.copy_message")),
                ViewMenuEntry::item(COPY_LINE, t("log_viewer.copy_line")),
            ]);
        }
        self.grid.as_mut()?.context_menu(p)
    }

    fn command(&mut self, id: &str) -> bool {
        // The details' menu: what it copies.
        let copied = match id {
            COPY_SELECTION => self.details.as_ref().and_then(|(_, r)| r.selected_text()),
            COPY_MESSAGE => self
                .details
                .is_some()
                .then(|| crate::entry::parse(&self.details_raw).cell(crate::entry::MSG).to_owned()),
            COPY_LINE => self.details.is_some().then(|| self.details_raw.clone()),
            _ => None,
        };
        if let Some(text) = copied {
            self.action = Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text)));
            return true;
        }
        let used = self.grid.as_mut().is_some_and(|g| g.command(id));
        // A row chosen by a command (select, find): its details follow.
        self.sync_details();
        used
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
