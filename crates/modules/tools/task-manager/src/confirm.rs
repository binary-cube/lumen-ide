//! The question asked before an action that loses work (stopping a program, closing a
//! session): a modal of the module's own (`view-task-confirm`, `host.open_modal`: closed
//! only by ×, Esc or its buttons — never by a click outside; nothing behind it gets keys,
//! text or the mouse). Its title says what's done to what, its body what's lost; "Cancel"
//! has the keys (a stray ↵ loses nothing), Tab / ← / → move them to the action (in the
//! danger color).
//!
//! The view that asks hands the question to the host; a "yes" stays on the question ([`Yes`])
//! and the window's Task Manager, the one holding that question, does the action. Closed any
//! other way, nothing is done.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use i18n::t;
use kernel::{Point, Rect};
use ui_components::ds::{self, Ctx, radius};
use ui_components::{
    Viewer, ViewerAction, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider, WithHost,
};
use ui_kit::draw_text;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-task-confirm";
/// What the modal's "yes" sends the window's Task Manager.
pub const CONFIRMED: &str = "tasks-confirmed";

/// Whether a question was answered "yes", shared by the modal and the view that asked.
/// Compared by identity.
#[derive(Debug, Clone, Default)]
pub struct Yes(Arc<AtomicBool>);

impl Yes {
    fn say(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// It was said (taken once).
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
}

impl PartialEq for Yes {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Yes {}

/// A question: its title, what's lost, the action's label and icon, its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    pub title: String,
    pub body: String,
    pub button: String,
    pub icon: &'static str,
    pub yes: Yes,
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct ConfirmProvider;

impl ViewerProvider for ConfirmProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ConfirmView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Yes,
    No,
}

const PAD: f32 = 24.0;
/// Above the body's first line.
const TOP: f32 = 8.0;
const LINE_H: f32 = 22.0;
const LINES: usize = 3;
/// The narrowest the question is (logical pixels), as the app's own dialogs.
const WIDTH: f32 = 440.0;
const FOOTER_H: f32 = 64.0;

/// The modal's view.
#[derive(Debug, Default)]
pub struct ConfirmView {
    question: Option<Question>,
    /// The action's button has the keys (else "Cancel").
    on_yes: bool,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
    /// The body's lines for a width (wrapped once).
    lines: Option<(u32, Vec<String>)>,
}

impl ConfirmView {
    /// The question it asks.
    pub fn question(&self) -> Option<&Question> {
        self.question.as_ref()
    }

    /// The action's button has the keys.
    pub fn on_yes(&self) -> bool {
        self.on_yes
    }

    /// Answered: "yes" kept for the view that asked (and it told), or nothing done.
    pub fn answer(&mut self, yes: bool) {
        if yes && let Some(q) = &self.question {
            q.yes.say();
            // The modal is gone by then: the window's active view is the one that asked (any
            // other ignores it; the Task Manager also looks at its next poll).
            self.action = Some(ViewerAction::WithHost(WithHost::once(|host| {
                host.view_command(CONFIRMED);
            })));
        } else {
            self.action = Some(ViewerAction::Close);
        }
        self.done = true;
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }
}

impl Viewer for ConfirmView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(question) = data.downcast_ref::<Question>() {
            self.question = Some(question.clone());
            self.on_yes = false;
            self.lines = None;
        }
    }

    fn modal_height(&self) -> Option<f32> {
        // As many lines as its body wrapped to last (the most before its first paint).
        let lines = self.lines.as_ref().map_or(LINES, |(_, l)| l.len().max(1));
        Some(TOP + LINE_H * lines as f32 + FOOTER_H + TOP)
    }

    /// As wide as the app's own questions, wider only for a long title.
    fn modal_width(&self) -> Option<f32> {
        Some(WIDTH)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        self.question.as_ref().map(|q| q.icon)
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, .. } = ctx;
        let Some(question) = &self.question else { return };
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let layer = scene.push_layer(Some(b));
        let left = b.min_x() + px(PAD);
        let right = b.max_x() - px(PAD);
        let font = ds.ui_font(0.0);
        let width = (right - left).max(0.0);
        let key = width as u32;
        if self.lines.as_ref().is_none_or(|(at, _)| *at != key) {
            self.lines = Some((key, ds::wrap(text, &question.body, &font, width, LINES)));
        }
        let mut y = b.min_y() + px(TOP);
        for line in self.lines.as_ref().map_or(&[][..], |(_, l)| l.as_slice()) {
            let base = ds::optical_baseline(y, px(LINE_H), &font);
            draw_text(layer, &text.shape_line(line, &font), left.round(), base, p.text_muted);
            y += px(LINE_H);
        }
        let cancel = t("tasks.cancel");
        let bw = ds::primary_width(text, &ds, &[&cancel, &question.button]);
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(FOOTER_H) + (px(FOOTER_H) - bh) / 2.0;
        let yes = Rect::new(right - bw, by, bw, bh);
        let no = Rect::new(yes.min_x() - px(10.0) - bw, by, bw, bh);
        ds::secondary_button(layer, text, &ds, no, &cancel, true, self.hover == Some(Hit::No));
        ds::danger_button(
            layer,
            text,
            &ds,
            yes,
            &question.button,
            true,
            self.hover == Some(Hit::Yes),
        );
        ds::focus_ring(layer, &ds, if self.on_yes { yes } else { no }, px(radius::MD));
        self.hits.push((no, Hit::No));
        self.hits.push((yes, Hit::Yes));
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        match self.hit(p) {
            Some(Hit::Yes) => self.answer(true),
            Some(Hit::No) => self.answer(false),
            None => {}
        }
        self.action.take()
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ui_components::ViewerCursor> {
        self.hit(p).map(|_| ui_components::ViewerCursor::Pointer)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Escape => self.answer(false),
            ViewerKey::Enter => self.answer(self.on_yes),
            ViewerKey::Tab | ViewerKey::Left | ViewerKey::Right => self.on_yes = !self.on_yes,
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/confirm.rs"]
mod tests;
