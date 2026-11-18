//! The Change Signature preview (a tab): the parameters as a table — name, type, default,
//! and for a new one the value the existing calls pass — moved with ↑ ↓, removed with ✕,
//! added with "+"; the new signature above, and every call as it will read below (found on
//! a background thread, rewritten as the table is edited).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::t;
use kernel::{Point, Rect};
use refactor::signature::{Calls, NewParam, Param, SignatureContext, find_calls, render_params};
use refactor::{NameStyle, Plan, valid_name};
use render_api::Quad;
use text_layout::Font;
use ui_components::ds::{self, Ctx, Tone, radius, space};
use ui_components::{Viewer, ViewerAction, ViewerKey, ViewerPaint};
use ui_kit::{TextInput, TextInputStyle, draw_text};

use crate::page::{MIN_LIST, PageScroll};
use crate::plan_list::PlanList;
use ui_components::ds::Pen;

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-signature";

/// What the view is opened with.
#[derive(Debug, Clone)]
pub struct SignatureData {
    pub context: SignatureContext,
}

/// The table's columns.
const NAME: usize = 0;
const TYPE: usize = 1;
const DEFAULT: usize = 2;
const VALUE: usize = 3;

/// A parameter's row: what it becomes and its fields.
#[derive(Debug, Clone)]
struct ParamRow {
    new: NewParam,
    fields: [TextInput; 4],
}

impl ParamRow {
    fn new(new: NewParam) -> Self {
        let p = new.param();
        let value = match &new {
            NewParam::Added { value, .. } => value.clone(),
            NewParam::Kept { .. } => String::new(),
        };
        let field = |text: &str| TextInput::new(text);
        let fields = [field(&p.name), field(&p.type_hint), field(&p.default), field(&value)];
        Self { new, fields }
    }

    fn is_added(&self) -> bool {
        matches!(self.new, NewParam::Added { .. })
    }

    /// The fields' text into the parameter.
    fn sync(&mut self) {
        let name = self.fields[NAME].text().trim().trim_start_matches('$').to_owned();
        let type_hint = self.fields[TYPE].text().trim().to_owned();
        let default = self.fields[DEFAULT].text().trim().to_owned();
        let typed_value = self.fields[VALUE].text().trim().to_owned();
        let param = self.new.param_mut();
        param.name = name;
        param.type_hint = type_hint;
        param.default = default;
        if let NewParam::Added { value, .. } = &mut self.new {
            *value = typed_value;
        }
    }
}

/// What a click hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Field(usize, usize),
    Up(usize),
    Down(usize),
    Remove(usize),
    Add,
    Apply,
    Close,
}

#[derive(Default)]
pub struct SignatureView {
    context: Option<SignatureContext>,
    rows: Vec<ParamRow>,
    /// The field with the keyboard (row, column).
    focus: (usize, usize),
    calls: Option<Calls>,
    finding: Option<Receiver<Calls>>,
    cancel: Arc<AtomicBool>,
    plan: Option<Plan>,
    list: PlanList,
    page: PageScroll,
    action: Option<ViewerAction>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for SignatureView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignatureView").field("params", &self.new_params()).finish_non_exhaustive()
    }
}

impl Drop for SignatureView {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl SignatureView {
    /// The new signature's parameters, as edited.
    pub fn new_params(&self) -> Vec<NewParam> {
        self.rows.iter().map(|r| r.new.clone()).collect()
    }

    /// The parameters are valid: names that are identifiers, none twice.
    pub fn valid(&self) -> bool {
        let names: Vec<&str> = self.rows.iter().map(|r| r.new.param().name.as_str()).collect();
        names.iter().all(|n| valid_name(n, NameStyle::CODE))
            && names.iter().enumerate().all(|(i, n)| !names[..i].contains(n))
    }

    /// Something changed (the order, a name, a type, a default, a parameter added / removed).
    pub fn changed(&self) -> bool {
        let Some(context) = &self.context else { return false };
        let old = &context.target.old;
        self.rows.len() != old.len()
            || self.rows.iter().enumerate().any(|(i, r)| match &r.new {
                NewParam::Kept { from, param } => *from != i || param != &old[*from],
                NewParam::Added { .. } => true,
            })
    }

    pub fn can_apply(&self) -> bool {
        self.valid() && self.changed() && self.plan.as_ref().is_some_and(|p| p.chosen().0 > 0)
    }

    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Sets the calls found (from the background search, or directly in tests).
    pub fn set_calls(&mut self, calls: Calls) {
        self.calls = Some(calls);
        self.rebuild();
    }

    /// The plan again, for the table as it is now; the user's checkboxes are kept.
    fn rebuild(&mut self) {
        let (Some(context), Some(calls)) = (&self.context, &self.calls) else { return };
        let new = self.new_params();
        let before = self.plan.take();
        let chosen = |f: usize, c: usize| -> Option<bool> {
            let (path, list) = calls.files.get(f)?;
            let start = list.get(c)?.inside.start;
            before.as_ref()?.files.iter().find(|p| p.path == *path).and_then(|p| {
                p.occurrences.iter().find(|o| o.range.start == start).map(|o| o.chosen)
            })
        };
        let plan = calls.plan(&context.target, &new, context.syntax.as_ref(), &chosen);
        self.plan = Some(plan);
    }

    fn edited(&mut self) {
        if let Some(row) = self.rows.get_mut(self.focus.0) {
            row.sync();
        }
        self.rebuild();
    }

    fn field(&mut self) -> Option<&mut TextInput> {
        let (row, col) = self.focus;
        self.rows.get_mut(row).map(|r| &mut r.fields[col])
    }

    /// Types `text` into the focused field.
    pub fn type_text(&mut self, text: &str) {
        if let Some(field) = self.field() {
            field.insert(text.lines().next().unwrap_or_default());
        }
        self.edited();
    }

    /// Focuses a field (`col` 3, the calls' value, only for a new parameter).
    fn focus_field(&mut self, row: usize, col: usize) {
        let col =
            if self.rows.get(row).is_some_and(ParamRow::is_added) { col } else { col.min(DEFAULT) };
        self.focus = (row.min(self.rows.len().saturating_sub(1)), col);
        if let Some(field) = self.field() {
            field.end(false);
        }
    }

    /// Tab: the next field (row by row).
    fn next_field(&mut self) {
        let (row, col) = self.focus;
        let last = if self.rows.get(row).is_some_and(ParamRow::is_added) { VALUE } else { DEFAULT };
        if col < last {
            self.focus_field(row, col + 1);
        } else if row + 1 < self.rows.len() {
            self.focus_field(row + 1, NAME);
        } else {
            self.focus_field(0, NAME);
        }
    }

    /// Moves a parameter by `delta` places.
    pub fn move_row(&mut self, row: usize, delta: isize) {
        let to = row as isize + delta;
        if to < 0 || to as usize >= self.rows.len() {
            return;
        }
        self.rows.swap(row, to as usize);
        self.focus = (to as usize, self.focus.1);
        self.rebuild();
    }

    pub fn remove_row(&mut self, row: usize) {
        if row < self.rows.len() {
            self.rows.remove(row);
            self.focus = (row.min(self.rows.len().saturating_sub(1)), NAME);
            self.rebuild();
        }
    }

    /// Adds a parameter at the end (before a variadic one); the calls get its value.
    pub fn add_row(&mut self) {
        let at = match self.rows.last() {
            Some(last) if last.new.param().variadic => self.rows.len() - 1,
            _ => self.rows.len(),
        };
        let name = (1..)
            .map(|i| format!("param{i}"))
            .find(|n| self.rows.iter().all(|r| r.new.param().name != *n));
        let param = Param { name: name.unwrap_or_default(), ..Param::default() };
        self.rows.insert(at, ParamRow::new(NewParam::Added { param, value: String::new() }));
        self.focus = (at, NAME);
        if let Some(field) = self.field() {
            field.select_all();
        }
        self.rebuild();
    }

    pub fn apply(&self) -> Option<ViewerAction> {
        if !self.can_apply() {
            return None;
        }
        Some(ViewerAction::ApplyRefactoring(self.plan.clone()?))
    }

    fn search(&mut self) {
        let Some(context) = self.context.clone() else { return };
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = Arc::clone(&self.cancel);
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("refactor-signature", move || {
            let cancelled = || cancel.load(Ordering::Relaxed);
            let calls = find_calls(
                &context.target,
                context.syntax.as_ref(),
                context.sources.as_ref(),
                context.resolver.as_ref(),
                &cancelled,
            );
            if !cancelled()
                && tx.send(calls).is_ok()
                && let Some(wake) = waker
            {
                wake();
            }
        });
        self.finding = Some(rx);
    }

    /// The new signature, on one line: `name($a, int $b = 2)`.
    fn signature_line(&self) -> String {
        let Some(context) = &self.context else { return String::new() };
        let params = render_params(&self.new_params(), "", context.syntax.as_ref());
        format!("{}({params})", context.target.name)
    }
}

impl Viewer for SignatureView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<SignatureData>() else { return };
        let old = &data.context.target.old;
        self.rows = old
            .iter()
            .enumerate()
            .map(|(from, param)| ParamRow::new(NewParam::Kept { from, param: param.clone() }))
            .collect();
        self.context = Some(data.context.clone());
        self.calls = None;
        self.plan = None;
        self.focus = (0, NAME);
        self.search();
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.finding else { return false };
        match rx.try_recv() {
            Ok(calls) => {
                self.finding = None;
                self.set_calls(calls);
                true
            }
            // Still searching: nothing new to draw (its "searching" stays as it is).
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.finding = None;
                self.set_calls(Calls::default());
                true
            }
        }
    }

    fn wants_text(&self) -> bool {
        !self.rows.is_empty()
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.type_text(text);
        true
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Enter => {
                self.action = self.apply();
                return self.action.is_some();
            }
            ViewerKey::Escape => {
                self.action = Some(ViewerAction::Close);
                return true;
            }
            ViewerKey::Tab => {
                self.next_field();
                return true;
            }
            ViewerKey::Up => {
                self.focus_field(self.focus.0.saturating_sub(1), self.focus.1);
                return true;
            }
            ViewerKey::Down => {
                self.focus_field(self.focus.0 + 1, self.focus.1);
                return true;
            }
            _ => {}
        }
        let Some(field) = self.field() else { return false };
        match key {
            ViewerKey::Backspace => field.backspace(false),
            ViewerKey::Delete => field.delete(),
            ViewerKey::Left => field.move_left(false, false),
            ViewerKey::Right => field.move_right(false, false),
            ViewerKey::Home => field.home(false),
            ViewerKey::End => field.end(false),
            _ => return false,
        }
        self.edited();
        true
    }

    fn command(&mut self, id: &str) -> bool {
        if id == "select_all"
            && let Some(field) = self.field()
        {
            field.select_all();
            return true;
        }
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // The page's scrollbar; nothing under the keys.
        if self.page.press(p) || !self.page.contains(p) {
            return None;
        }
        if self.list.contains(p) {
            return self.list.click(self.plan.as_mut()?, p);
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)?;
        match hit {
            Hit::Field(row, col) => {
                self.focus = (row, col);
                if let Some(field) = self.field() {
                    field.click(p.x);
                }
            }
            Hit::Up(row) => self.move_row(row, -1),
            Hit::Down(row) => self.move_row(row, 1),
            Hit::Remove(row) => self.remove_row(row),
            Hit::Add => self.add_row(),
            Hit::Apply => return self.apply(),
            Hit::Close => return Some(ViewerAction::Close),
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h);
        let changed = hover != self.hover;
        self.hover = hover;
        self.list.hover(p) | self.page.hover(p) | changed
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.page.scroll(&mut self.list, dy)
    }

    fn drag(&mut self, p: Point) -> bool {
        self.page.drag(p)
    }

    fn release(&mut self) -> bool {
        self.page.release()
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
        let footer_h = px(40.0);
        // The page above the keys, scrolled when the group is short.
        let (page_area, y0) = self.page.begin(b, footer_h, px(28.0));
        let layer = ctx.scene.push_layer(Some(page_area));
        let width = px(920.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = y0;
        let page = Page {
            x0,
            width,
            small: ds.ui_font(-1.0),
            font: ds.ui_font(0.0),
            code: ds.code_font(0.0),
        };
        self.paint_header(Pen { layer, text: ctx.text, ds: &ds }, &page, y);
        y += px(48.0);
        self.paint_signature(Pen { layer, text: ctx.text, ds: &ds }, &page, y);
        y += px(50.0);
        y = self.paint_params(Pen { layer, text: ctx.text, ds: &ds }, &page, y);

        // The note: invalid names, or how many calls.
        let small = &page.small;
        let note = if self.valid() {
            (PlanList::summary(self.plan.as_ref()), p.text_muted)
        } else {
            (t("refactor.invalid_params"), p.danger)
        };
        let shaped = ctx.text.shape_line(&note.0, small);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), small), note.1);
        y += px(28.0);

        // The keys, at the bottom; the calls above them.
        let foot = Rect::new(b.min_x(), b.max_y() - footer_h, b.size.width, footer_h);
        let (apply, next, cancel) =
            (t("refactor.apply_key"), t("refactor.next_field"), t("refactor.cancel_key"));
        let hints: &[ui_kit::KeyHint<'_>] =
            &[(&["↵"], &apply), (&["⇥"], &next), (&["esc"], &cancel)];
        ds::footer_in_card(ctx.scene, ctx.text, &ds, foot, hints, radius::CARD);
        let bottom = foot.min_y() - px(8.0);
        let area = self.page.list_area(x0, width, y, bottom, px(MIN_LIST));
        self.page.paint_bar(ctx.scene, p, ctx.scale);
        let Some(plan) = &self.plan else { return };
        let root = self.context.as_ref().and_then(|c| c.sources.root());
        self.list.paint(ctx.scene, ctx.text, &ds, plan, root, area);
    }
}

impl SignatureView {
    /// The header at `y`: the title, the function, `esc`.
    fn paint_header(&mut self, pen: Pen<'_, '_>, page: &Page, y: f32) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let x0 = page.x0;
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let title = text.shape_line(&t("refactor.signature_title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        let name = self.context.as_ref().map_or(String::new(), |c| c.target.name.clone());
        let bx = x0 + title.width + px(space::MD);
        ds::badge(
            layer,
            text,
            ds,
            ds::BadgeSpec {
                label: &name,
                x: bx,
                mid: y + px(14.0),
                align_right: false,
                tone: Tone::Accent,
            },
        );
        let hovered = self.hover == Some(Hit::Close);
        let cap = ds::keycap(layer, text, ds, "esc", x0 + page.width, y + px(14.0), hovered);
        self.hits.push((cap, Hit::Close));
    }

    /// The new signature at `y`, and the button that applies it.
    fn paint_signature(&mut self, pen: Pen<'_, '_>, page: &Page, y: f32) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Page { x0, width, ref code, .. } = *page;
        let button_w = ds::primary_width(text, ds, &[&t("refactor.apply")]);
        let line = Rect::new(x0, y, width - button_w - px(space::MD), px(ds::BUTTON_H));
        layer.quad(
            Quad::filled(line, p.text.with_alpha(0.035))
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        let signature =
            ds::ellipsize(text, &self.signature_line(), code, line.size.width - px(24.0));
        let shaped = text.shape_line(&signature, code);
        draw_text(
            layer,
            &shaped,
            x0 + px(12.0),
            ds::optical_baseline(y, line.size.height, code),
            p.text,
        );
        let button = Rect::new(line.max_x() + px(space::MD), y, button_w, px(ds::BUTTON_H));
        let hovered = self.hover == Some(Hit::Apply);
        ds::primary_button(
            layer,
            text,
            ds,
            button,
            &t("refactor.apply"),
            self.can_apply(),
            hovered,
        );
        self.hits.push((button, Hit::Apply));
    }

    /// The table from `y`: the columns' titles, then a row per parameter, then "+ Add
    /// parameter". Returns where it ends.
    fn paint_params(&mut self, pen: Pen<'_, '_>, page: &Page, mut y: f32) -> f32 {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Page { x0, ref small, .. } = *page;
        let buttons_w = px(96.0);
        let gap = px(8.0);
        let cols_w = page.width - buttons_w;
        let widths = [0.28, 0.24, 0.24, 0.24].map(|f: f32| (cols_w * f - gap).floor());
        let labels = [
            t("refactor.param_name"),
            t("refactor.param_type"),
            t("refactor.param_default"),
            t("refactor.param_value"),
        ];
        let mut table =
            Table { x0, cols_w, gap, widths, top: y, row_h: px(36.0), field_h: px(30.0) };
        for (c, label) in labels.iter().enumerate() {
            let shaped = text.shape_line(label, small);
            draw_text(layer, &shaped, table.col_x(c) + px(2.0), y + small.size, p.text_muted);
        }
        y += px(24.0);
        table.top = y;
        let names: Vec<String> = self.rows.iter().map(|r| r.new.param().name.clone()).collect();
        for i in 0..self.rows.len() {
            self.paint_param_row(Pen { layer, text, ds }, page, &table, &names, i);
        }
        y += self.rows.len() as f32 * table.row_h + px(6.0);
        // + Add parameter
        let add = text.shape_line(&format!("+ {}", t("refactor.param_add")), small);
        let add_rect = Rect::new(x0, y, add.width + px(16.0), px(26.0));
        if self.hover == Some(Hit::Add) {
            layer.quad(Quad::filled(add_rect, p.accent.with_alpha(0.08)).rounded(px(radius::SM)));
        }
        draw_text(layer, &add, x0 + px(8.0), ds::optical_baseline(y, px(26.0), small), p.accent);
        self.hits.push((add_rect, Hit::Add));
        y + px(36.0)
    }

    /// Parameter `i`'s row: its fields (an existing one's value a dash: the calls keep their
    /// argument), then ↑ ↓ ✕.
    fn paint_param_row(
        &mut self,
        pen: Pen<'_, '_>,
        page: &Page,
        table: &Table,
        names: &[String],
        i: usize,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (small, field_h) = (&page.small, table.field_h);
        let style = |focused_border| TextInputStyle {
            font: page.code.clone(),
            background: p.input_background,
            border: p.input_border,
            border_focused: focused_border,
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: px(radius::SM),
            border_width: ds.line(),
            padding: px(8.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let row = &self.rows[i];
        let top = table.top + i as f32 * table.row_h;
        let fy = top + (table.row_h - field_h) / 2.0;
        let name = &names[i];
        let bad = !valid_name(name, NameStyle::CODE) || names[..i].contains(name);
        for (c, &w) in table.widths.iter().enumerate() {
            let field = Rect::new(table.col_x(c), fy, w, field_h);
            if c == VALUE && !row.is_added() {
                // An existing parameter: the calls keep their argument.
                let dash = text.shape_line("—", small);
                draw_text(
                    layer,
                    &dash,
                    field.min_x() + px(8.0),
                    ds::optical_baseline(fy, field_h, small),
                    p.text_muted,
                );
                continue;
            }
            let focused = self.focus == (i, c);
            let border = if c == NAME && bad { p.danger } else { p.accent };
            let placeholder =
                if c == VALUE { t("refactor.param_value_hint") } else { String::new() };
            row.fields[c].paint(layer, text, field, focused, &placeholder, &style(border));
            self.hits.push((field, Hit::Field(i, c)));
        }
        self.paint_row_buttons(Pen { layer, text, ds }, page, table, i, fy);
    }

    /// Parameter `i`'s ↑ ↓ ✕ (the moves it can't make dimmed), at `fy`.
    fn paint_row_buttons(
        &mut self,
        pen: Pen<'_, '_>,
        page: &Page,
        table: &Table,
        i: usize,
        fy: f32,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let (font, field_h) = (&page.font, table.field_h);
        let bx0 = table.x0 + table.cols_w;
        for (k, (glyph, hit)) in
            [("↑", Hit::Up(i)), ("↓", Hit::Down(i)), ("✕", Hit::Remove(i))].into_iter().enumerate()
        {
            let r = Rect::new(bx0 + k as f32 * px(32.0), fy, px(28.0), field_h);
            let disabled = matches!(hit, Hit::Up(0))
                || matches!(hit, Hit::Down(d) if d + 1 == self.rows.len());
            if self.hover == Some(hit) && !disabled {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.06)).rounded(px(radius::SM)));
            }
            let shaped = text.shape_line(glyph, font);
            let color = if disabled {
                p.text_muted.with_alpha(0.35)
            } else if matches!(hit, Hit::Remove(_)) && self.hover == Some(hit) {
                p.danger
            } else {
                p.text_muted
            };
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (r.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(fy, field_h, font),
                color,
            );
            if !disabled {
                self.hits.push((r, hit));
            }
        }
    }
}

/// The page's column and the fonts its parts share.
struct Page {
    x0: f32,
    width: f32,
    small: Font,
    font: Font,
    code: Font,
}

/// The parameters' table: its columns (left edge, the fields' width, their widths and the
/// gap between them), where its rows start, their height and their fields'.
struct Table {
    x0: f32,
    cols_w: f32,
    gap: f32,
    widths: [f32; 4],
    top: f32,
    row_h: f32,
    field_h: f32,
}

impl Table {
    /// Where column `c` starts.
    fn col_x(&self, c: usize) -> f32 {
        self.x0 + self.widths[..c].iter().map(|w| w + self.gap).sum::<f32>()
    }
}

#[cfg(test)]
#[path = "../tests/unit/signature_view.rs"]
mod tests;
