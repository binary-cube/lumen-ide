//! What submitting a table's changes will run, shown before it does: a modal of the
//! module's own (`host.open_modal`: closed only by its buttons, × or Esc — never by a click
//! outside), listing the SQL statements exactly as the domain wrote them (they're what runs)
//! or, for a source that writes none (a CSV file), a line per change. Deleting rows asks
//! for a tick first ([`crate::settings::CONFIRM_DELETE`]). The answer goes back to the data
//! view that asked through a shared slot (the view takes it at its next poll); a modal
//! closed any other way is a "no".
//!
//! The view can't open a modal itself: it leaves its request in [`PENDING`] and asks the app
//! to run [`PREVIEW`], whose handler opens it.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    EdgeScrollbar, Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-data-preview";

/// What the modal shows and where its answer goes.
#[derive(Clone)]
pub struct PreviewRequest {
    pub table: String,
    /// The statements (SQL), or the summary's lines (`sql` false).
    pub lines: Vec<String>,
    pub sql: bool,
    pub changes: usize,
    /// Rows deleted (a tick asked first when the setting says so).
    pub deletes: usize,
    pub confirm: bool,
    /// `Some(true)`: submit; `Some(false)`: given up.
    pub answer: Arc<Mutex<Option<bool>>>,
    /// Wakes the view that asked.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for PreviewRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewRequest").field("table", &self.table).finish_non_exhaustive()
    }
}

impl PreviewRequest {
    fn answer(&self, yes: bool) {
        let mut slot = self.answer.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_none() {
            *slot = Some(yes);
            drop(slot);
            (self.wake)();
        }
    }
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct PreviewProvider;

impl ViewerProvider for PreviewProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PreviewView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Confirm,
}

/// The footer's buttons (the window draws them): their commands.
const SUBMIT: &str = "database.preview.submit";
const CANCEL: &str = "database.preview.cancel";

/// The modal's view.
#[derive(Default)]
pub struct PreviewView {
    request: Option<PreviewRequest>,
    /// The deletion ticked (when asked).
    confirmed: bool,
    scroll: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for PreviewView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl PreviewView {
    /// Submitting is allowed (the deletion ticked, when asked).
    pub fn can_submit(&self) -> bool {
        self.request.as_ref().is_some_and(|r| !r.confirm || r.deletes == 0 || self.confirmed)
    }

    /// Submit (when it can): the answer given, the modal closed.
    pub fn submit(&mut self) -> bool {
        if !self.can_submit() {
            return false;
        }
        if let Some(r) = &self.request {
            r.answer(true);
        }
        self.action = Some(ViewerAction::Close);
        true
    }

    pub fn cancel(&mut self) {
        if let Some(r) = &self.request {
            r.answer(false);
        }
        self.action = Some(ViewerAction::Close);
    }

    /// Its parts (the statements' scrollbar), the topmost first, with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut statements = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 1] = [&mut statements];
        f(&mut self.controls, &mut parts)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }
}

impl Drop for PreviewView {
    /// Closed without an answer (×, Esc): given up.
    fn drop(&mut self) {
        if let Some(r) = &self.request {
            r.answer(false);
        }
    }
}

/// SQL's words drawn in the accent color.
const KEYWORDS: [&str; 22] = [
    "SELECT",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "FROM",
    "WHERE",
    "AND",
    "OR",
    "IS",
    "NULL",
    "NOT",
    "RETURNING",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "CREATE",
    "TABLE",
    "ALTER",
    "DROP",
];

impl Viewer for PreviewView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<PreviewRequest>() {
            self.request = Some(request.clone());
        }
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(i18n::tr!("database.preview_subtitle", table = r.table.as_str(), count = r.changes))
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("table")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![(vec!["↵"], t("database.preview_submit"))]
    }

    /// Submit (once the deletion is ticked, when asked), Cancel.
    fn modal_buttons(&self) -> Vec<ui_components::ModalButton> {
        use ui_components::ModalButton;
        vec![
            ModalButton::main(SUBMIT, t("database.preview_submit"), self.can_submit()),
            ModalButton::other(CANCEL, t("database.preview_cancel")),
        ]
    }

    /// What it is: SQL run on the server, or a file written.
    fn modal_status(&self) -> Option<ui_components::ModalStatus> {
        let r = self.request.as_ref()?;
        let note = if r.sql { "database.preview_sql_note" } else { "database.preview_file_note" };
        Some(ui_components::ModalStatus::Note(t(note)))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let Some(request) = self.request.clone() else { return };
        let pad = px(20.0);
        // The footer (its buttons, what it is) is the window's.
        let footer_h = px(12.0);
        let warn = request.deletes > 0;
        let warn_h = if warn { px(46.0) } else { 0.0 };
        let layer = scene.push_layer(Some(b));
        // What runs, in a code card that scrolls.
        let card = Rect::new(
            b.min_x() + pad,
            b.min_y() + px(8.0),
            b.size.width - 2.0 * pad,
            b.size.height - px(8.0) - footer_h - warn_h,
        );
        layer.quad(
            Quad::filled(card, p.editor_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        let font = if request.sql { ds.code_font(-1.0) } else { ds.ui_font(-0.5) };
        let line_h = px(22.0);
        let inner = Rect::new(
            card.min_x() + px(14.0),
            card.min_y() + px(10.0),
            card.size.width - px(28.0),
            card.size.height - px(20.0),
        );
        let content = request.lines.len() as f32 * line_h;
        self.scroll = self.scroll.clamp(0.0, (content - inner.size.height).max(0.0));
        let code = scene.push_layer(Some(inner));
        let metrics = text.metrics(&font);
        let space = text.shape_line(" ", &font).width;
        for (i, line) in request.lines.iter().enumerate() {
            let top = inner.min_y() + i as f32 * line_h - self.scroll;
            if top + line_h < inner.min_y() || top > inner.max_y() {
                continue;
            }
            let base = ui_kit::centered_baseline(top, line_h, &metrics);
            let mut x = inner.min_x();
            if !request.sql {
                let shaped = text.shape_line(&format!("• {line}"), &font);
                draw_text(code, &shaped, x, base, p.text);
                continue;
            }
            // Words, their keywords lit (quoted texts as they are).
            for (j, word) in line.split(' ').enumerate() {
                if j > 0 {
                    x += space;
                }
                let bare = word.trim_matches(|c: char| !c.is_ascii_alphabetic());
                let color = if KEYWORDS.contains(&bare) && !word.starts_with('\'') {
                    p.accent
                } else if word.starts_with('\'') || word.starts_with('"') {
                    p.syntax.as_ref().map_or(p.text, |s| s.string)
                } else {
                    p.text
                };
                let shaped = text.shape_line(word, &font);
                draw_text(code, &shaped, x, base, color);
                x += shaped.width;
            }
        }
        self.bar.paint(scene, p, scale, card, content + px(20.0), self.scroll);
        let layer = scene.push_layer(Some(b));
        // Rows deleted: said, and ticked first when the setting asks.
        if warn {
            let r = Rect::new(card.min_x(), card.max_y() + px(10.0), card.size.width, px(36.0));
            layer.quad(Quad::filled(r, p.danger.with_alpha(0.10)).rounded(px(radius::MD)));
            let label = i18n::tr!("database.preview_deletes", count = request.deletes);
            let small = ds.ui_font(-0.5);
            let mid = r.min_y() + r.size.height / 2.0;
            let mut x = r.min_x() + px(12.0);
            if request.confirm {
                let state = if self.confirmed { ds::Check::On } else { ds::Check::Off };
                let check = ds::checkbox(layer, text, &ds, x, mid, state, true);
                self.hits.push((r, Hit::Confirm));
                x = check.max_x() + px(10.0);
            }
            let shaped = text.shape_line(&label, &small);
            draw_text(
                layer,
                &shaped,
                x,
                ds::optical_baseline(r.min_y(), r.size.height, &small),
                p.danger,
            );
        }
    }

    /// A press on the statements' scrollbar takes its thumb (a drag follows); elsewhere the
    /// tick.
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        if self.hit(p) == Some(Hit::Confirm) {
            self.confirmed = !self.confirmed;
        }
        self.action.take()
    }

    /// Its parts' (the statements' scrollbar), else a hand over what can be pressed.
    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.controls.cursor_or(p, || self.hit(p).map(|_| ui_components::ViewerCursor::Pointer))
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.scroll = (self.scroll + dy).max(0.0);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter => self.submit(),
            ViewerKey::Escape => {
                self.cancel();
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "submit" | SUBMIT => self.submit(),
            CANCEL => {
                self.cancel();
                true
            }
            _ => false,
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/preview.rs"]
mod tests;
