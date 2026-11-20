//! The rename preview (a tab): the new name's field, then every place the rename changes,
//! file by file — each with a checkbox, its line and the line as it will read — and the
//! files renamed with the symbol. The places are found on a background thread; the
//! preview follows the name as it's typed (only the replacements change, nothing is
//! searched again).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use refactor::application::{Conflict, RenameContext, plan_rename};
use refactor::{Plan, valid_name};
use render_api::Quad;
use text_layout::Font;
use ui_components::control::{Control, Controls};
use ui_components::ds::{self, Ctx, Tone, radius, space};
use ui_components::tool_panel::KeyReach;
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::{TextInput, TextInputStyle, draw_text};

use crate::name_field::NameField;
use crate::page::{MIN_LIST, PageScroll};
#[cfg(test)]
pub use crate::plan_list::Row;
use crate::plan_list::{Places, PlanList};

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-rename";

/// What the view is opened with.
#[derive(Debug, Clone)]
pub struct RenameData {
    pub context: RenameContext,
    /// The new name already typed (renaming a class's file); `None`: the old one, selected.
    pub name: Option<String>,
}

/// What a click hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Apply,
    /// The `esc` key cap: drops the refactoring.
    Close,
}

/// The name field's place among the controls (see [`RenameView::with_controls`]).
const FIELD: usize = 2;

/// The worker a new name's clashes are found on.
const CONFLICTS_THREAD: &str = "refactor-conflicts";

#[derive(Default)]
pub struct RenameView {
    context: Option<RenameContext>,
    /// What is typed before the name and not shown in the field (a PHP variable's `$`).
    prefix: String,
    name: TextInput,
    /// Where the name's field was drawn last.
    name_area: Option<Rect>,
    /// A key or a text reached the name: the preview follows it.
    edited: bool,
    plan: Option<Plan>,
    planning: Option<Receiver<Plan>>,
    /// What the typed name would clash with.
    conflicts: Vec<Conflict>,
    /// The clashes being found for the name typed last, on a worker (the index read, maybe a
    /// file: never at a key on the UI thread); an older name's never come.
    checking: Option<Receiver<Vec<Conflict>>>,
    /// Stops the search when the view closes or searches again.
    cancel: Arc<AtomicBool>,
    /// An action produced by a key (Enter applies), picked up by the app.
    action: Option<ViewerAction>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    list: PlanList,
    page: PageScroll,
    /// Its controls' state of use (see [`RenameView::with_controls`]).
    controls: Controls,
    /// What a press on the places decided, taken by the click.
    clicked: Option<ViewerAction>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for RenameView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenameView").field("name", &self.new_name()).finish_non_exhaustive()
    }
}

impl Drop for RenameView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl RenameView {
    /// The name as it will be written (`$` included for a PHP variable).
    pub fn new_name(&self) -> String {
        format!("{}{}", self.prefix, self.name.text())
    }

    fn old_name(&self) -> &str {
        self.context.as_ref().map_or("", |c| c.target.name.as_str())
    }

    /// The typed name is a valid identifier, other than the old one.
    pub fn name_ok(&self) -> bool {
        let Some(context) = &self.context else { return false };
        let name = self.new_name();
        name != context.target.name && valid_name(&name, context.target.style)
    }

    /// Can be applied: a valid new name and at least one place chosen.
    pub fn can_apply(&self) -> bool {
        self.name_ok() && self.plan.as_ref().is_some_and(|p| p.chosen().0 > 0)
    }

    /// The plan, once found (for tests and the app).
    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Sets the found plan (from the background search, or directly in tests).
    pub fn set_plan(&mut self, plan: Plan) {
        self.plan = Some(plan);
        self.name_changed();
    }

    /// The name was edited: the replacements follow it.
    fn name_changed(&mut self) {
        let name = self.new_name();
        // Nothing typed yet: the places show the old name.
        let shown =
            if self.name.text().is_empty() { self.old_name().to_owned() } else { name.clone() };
        if let Some(plan) = &mut self.plan {
            plan.set_replacement(&shown);
        }
        self.checking = None;
        let Some(context) = self.context.as_ref().filter(|_| self.name_ok()) else {
            self.conflicts = Vec::new();
            return;
        };
        let check = Arc::clone(&context.conflicts);
        // Here and now without a waker (tests).
        let Some(wake) = self.waker.clone() else {
            self.conflicts = check.conflicts(&name);
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run(CONFLICTS_THREAD, move || {
            let _ = tx.send(check.conflicts(&name));
            wake();
        });
        self.checking = Some(rx);
    }

    /// What the typed name clashes with (for tests and the app).
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Types `text` into the name field.
    pub fn type_name(&mut self, text: &str) {
        self.with_controls(|c, parts| c.text(parts, text));
        self.follow_name();
    }

    /// The name was reached by a key or a text: the preview follows it.
    fn follow_name(&mut self) {
        if std::mem::take(&mut self.edited) {
            self.name_changed();
        }
    }

    /// The action applying the chosen places (`None` if it can't be applied).
    pub fn apply(&self) -> Option<ViewerAction> {
        if !self.can_apply() {
            return None;
        }
        Some(ViewerAction::ApplyRefactoring(self.plan.clone()?))
    }

    #[cfg(test)]
    pub(crate) fn toggle(&mut self, row: Row) {
        if let Some(plan) = &mut self.plan {
            PlanList::toggle(plan, row);
        }
    }

    fn search(&mut self) {
        let Some(context) = self.context.clone() else { return };
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("refactor-rename", move || {
            let cancelled = || cancel.load(Ordering::Relaxed);
            let plan = plan_rename(
                &context.target,
                context.sources.as_ref(),
                context.resolver.as_ref(),
                &cancelled,
            );
            if !cancelled()
                && tx.send(plan).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.planning = Some(rx);
    }

    /// Its controls (the page's scrollbar, the places, the name's field), the topmost first.
    /// The field is the page's only one with keys: it has the focus from `set_data` on.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut places =
            Places { list: &mut self.list, plan: self.plan.as_mut(), clicked: &mut self.clicked };
        let mut field =
            NameField { input: &mut self.name, area: self.name_area, edited: &mut self.edited };
        let mut parts: [&mut dyn Control; 3] = [&mut self.page, &mut places, &mut field];
        f(&mut self.controls, &mut parts)
    }
}

impl Viewer for RenameView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        // The keys' part from the first data on: a press beside it leaves it the focus.
        if self.controls.focused().is_none() {
            self.controls = Controls::sticky();
            self.with_controls(|c, parts| c.focus(parts, Some(FIELD)));
        }
        let Some(data) = data.downcast_ref::<RenameData>() else { return };
        let name = data.context.target.name.clone();
        // The language's sigil (`$total`) stays: only the name after it is typed.
        let sigil = data.context.target.style.sigil.filter(|s| name.starts_with(*s));
        self.prefix = sigil.map(String::from).unwrap_or_default();
        self.name.set_text(&name[self.prefix.len()..]);
        self.name.select_all();
        self.context = Some(data.context.clone());
        if let Some(new) = &data.name {
            self.name.set_text(new);
            self.name.end(false);
        }
        self.plan = None;
        self.conflicts = Vec::new();
        self.name_changed();
        self.search();
    }

    fn poll(&mut self) -> bool {
        // The newest name's clashes.
        let checked = self.checking.as_ref().and_then(|rx| rx.try_recv().ok());
        let mut changed = false;
        if let Some(conflicts) = checked {
            self.checking = None;
            changed = conflicts != self.conflicts;
            self.conflicts = conflicts;
        }
        let Some(rx) = &self.planning else { return changed };
        match rx.try_recv() {
            Ok(plan) => {
                self.planning = None;
                self.set_plan(plan);
                true
            }
            // Still searching: nothing new to draw (its "searching" stays as it is).
            Err(std::sync::mpsc::TryRecvError::Empty) => changed,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.planning = None;
                self.plan = Some(Plan::default());
                true
            }
        }
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.type_name(text);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.with_controls(|c, parts| c.key(parts, key, KeyReach::default())).is_some() {
            self.follow_name();
            return true;
        }
        match key {
            ViewerKey::Enter => {
                self.action = self.apply();
                self.action.is_some()
            }
            // Esc: the refactoring is dropped, back to the editor.
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                true
            }
            _ => false,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // Nothing under the keys.
        if !self.page.contains(p) {
            return None;
        }
        // The page's scrollbar, a place, the name's field.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return self.clicked.take();
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        match hit {
            Hit::Apply => self.apply(),
            Hit::Close => Some(ViewerAction::Close),
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.page.scroll(&mut self.list, dy)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        // Rounded like the editor's card it fills (its bottom corners show).
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let width = px(860.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let footer_h = px(40.0);
        // The page above the keys, scrolled when the group is short.
        let (page_area, mut y) = self.page.begin(b, footer_h, px(28.0));
        let layer = ctx.scene.push_layer(Some(page_area));
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.0);
        let font = ds.ui_font(0.0);

        // Header: the title and the old name.
        let title = ctx.text.shape_line(&t("refactor.rename_title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        let old = self.old_name().to_owned();
        ds::badge(
            layer,
            ctx.text,
            &ds,
            ds::BadgeSpec {
                label: &old,
                x: x0 + title.width + px(space::MD),
                mid: y + px(14.0),
                align_right: false,
                tone: Tone::Accent,
            },
        );
        // `esc` closes it (clickable too).
        let hovered = self.hover == Some(Hit::Close);
        let cap = ds::keycap(layer, ctx.text, &ds, "esc", x0 + width, y + px(14.0), hovered);
        self.hits.push((cap, Hit::Close));
        y += px(48.0);

        // The new name and the button.
        let (apply, anyway) = (t("refactor.apply"), t("refactor.apply_anyway"));
        let button_w = ds::primary_width(ctx.text, &ds, &[&apply, &anyway]);
        let field = Rect::new(x0, y, width - button_w - px(space::MD), px(ds::BUTTON_H));
        let style = TextInputStyle {
            font,
            background: p.input_background,
            border: p.input_border,
            border_focused: if self.name_ok() || self.name.text().is_empty() {
                p.accent
            } else {
                p.danger
            },
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: px(radius::MD),
            border_width: ds.line(),
            padding: px(12.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        self.name.paint(layer, ctx.text, field, true, &t("refactor.new_name"), &style);
        self.name_area = Some(field);
        let button = Rect::new(field.max_x() + px(space::MD), y, button_w, px(ds::BUTTON_H));
        let hovered = self.hover == Some(Hit::Apply);
        ds::primary_button(
            layer,
            ctx.text,
            &ds,
            button,
            if self.conflicts.is_empty() { &apply } else { &anyway },
            self.can_apply(),
            hovered,
        );
        self.hits.push((button, Hit::Apply));
        y += px(46.0);
        let note = if !self.name.text().is_empty() && !self.name_ok() && self.new_name() != old {
            (t("refactor.invalid_name"), p.danger)
        } else {
            (PlanList::summary(self.plan.as_ref()), p.text_muted)
        };
        let shaped = ctx.text.shape_line(&note.0, &small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), note.1);
        y += px(24.0);
        // The clashes: what already has the name, where.
        for conflict in &self.conflicts {
            let place = tr!(
                "refactor.conflict",
                name = self.new_name().as_str(),
                code = conflict.line_text.as_str(),
                file = kernel::paths::file_name(&conflict.path).as_str(),
                line = conflict.line + 1
            );
            let place = ds::ellipsize(ctx.text, &format!("⚠ {place}"), &small, width);
            let shaped = ctx.text.shape_line(&place, &small);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), p.danger);
            y += px(22.0);
        }
        y += px(4.0);

        // The keys, at the bottom; the places above them.
        let foot = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let (apply, cancel) = (t("refactor.apply_key"), t("refactor.cancel_key"));
        let hints: &[ui_kit::KeyHint<'_>] = &[(&["↵"], &apply), (&["esc"], &cancel)];
        ds::footer_in_card(ctx.scene, ctx.text, &ds, foot, hints, radius::CARD);
        let bottom = foot.min_y() - px(8.0);
        let area = self.page.list_area(x0, width, y, bottom, px(MIN_LIST));
        self.page.paint_bar(ctx.scene, p, ctx.scale);
        let Some(plan) = &self.plan else { return };
        let root = self.context.as_ref().and_then(|c| c.sources.root());
        self.list.paint(ctx.scene, ctx.text, &ds, plan, root, area);
    }
}

#[cfg(test)]
#[path = "../tests/unit/rename_view.rs"]
mod tests;
