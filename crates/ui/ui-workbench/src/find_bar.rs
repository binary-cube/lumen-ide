//! The Find / Replace bar: a strip across the full editor width, below the tabs.

use document::SearchQuery;
use i18n::{t, tr};
use icons::Icons;
use kernel::{Point, Rect};
use render_api::Quad;
use text_layout::{Font, TextSystem};
use ui_components::control::{Control, Controls, Shown};
use ui_components::search_field::{SearchExtras, SearchField};
use ui_kit::{TextInput, centered_baseline, draw_text};

use crate::palette::Palette;
use crate::{Painter, ds};

/// Height of a bar row, in logical pixels.
const ROW: f32 = 42.0;
/// One option's width inside the search field (`Cc`, `W`, `.*`), logical pixels.
const TOGGLE_W: f32 = 26.0;
/// The room between two options, logical pixels.
const TOGGLE_GAP: f32 = 2.0;
/// The options' room on the search field's right: three options and the gaps between them.
const TOGGLES_W: f32 = 3.0 * TOGGLE_W + 2.0 * TOGGLE_GAP;
/// The fields' places among the bar's parts (its `Controls`).
const QUERY: usize = 0;
const REPLACE: usize = 1;

/// What [`FindBar::paint`] draws, and where.
#[derive(Debug, Clone, Copy)]
pub struct FindBarSpec<'a> {
    pub bounds: Rect,
    pub options: &'a SearchQuery,
    pub counter: (usize, usize),
    pub error: bool,
    pub focused: bool,
    pub scale: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindField {
    Query,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindHit {
    Field(FindField),
    /// The chevron on the left: shows / hides the replace row.
    ToggleReplace,
    /// The × inside the field: clears the query.
    Clear,
    ToggleCase,
    ToggleWord,
    ToggleRegex,
    Prev,
    Next,
    Close,
    ReplaceOne,
    ReplaceAll,
}

/// The bar's UI state (its fields, which one has the focus). Each tab has one.
///
/// The search field is the app's [`SearchField`] (its options and its count beside it,
/// its border red for a bad pattern); the replacement a plain field in the search pill's
/// look. Both are the bar's parts: a press puts the caret (two a word, three all), a drag
/// selects, and the focus is the bar's one ([`Controls`]).
#[derive(Debug, Default)]
pub struct FindBar {
    pub query: SearchField,
    pub replace: TextInput,
    pub show_replace: bool,
    controls: Controls,
    panel: Rect,
    hits: Vec<(Rect, FindHit)>,
    hover: Option<FindHit>,
}

impl FindBar {
    /// `f` with the bar's parts (the replace field only while its row shows).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut replace = Shown { part: &mut self.replace, shown: self.show_replace };
        let mut parts: [&mut dyn Control; 2] = [&mut self.query, &mut replace];
        f(&mut self.controls, &mut parts)
    }

    /// The field with the focus (the search one unless the replacement took it).
    pub fn focus(&self) -> FindField {
        match self.controls.focused() {
            Some(REPLACE) if self.show_replace => FindField::Replace,
            _ => FindField::Query,
        }
    }

    /// Gives the focus to `field`.
    pub fn set_focus(&mut self, field: FindField) {
        let to = match field {
            FindField::Query => QUERY,
            FindField::Replace => REPLACE,
        };
        self.with_controls(|controls, parts| controls.focus(parts, Some(to)));
    }

    pub fn focused_input_ref(&self) -> &TextInput {
        match self.focus() {
            FindField::Query => &self.query.input,
            FindField::Replace => &self.replace,
        }
    }

    pub fn focused_input(&mut self) -> &mut TextInput {
        match self.focus() {
            FindField::Query => &mut self.query.input,
            FindField::Replace => &mut self.replace,
        }
    }

    /// A press at `p` with `clicks` in a row: a button's hit, or a field's (its caret put
    /// there, the focus given to it; × empties the search field: [`FindHit::Clear`]).
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<FindHit> {
        if let Some(hit) = self.button_at(p) {
            return Some(hit);
        }
        let (i, _) = self.with_controls(|controls, parts| controls.press(parts, p, clicks))?;
        if self.query.take_cleared() {
            return Some(FindHit::Clear);
        }
        Some(FindHit::Field(if i == REPLACE { FindField::Replace } else { FindField::Query }))
    }

    /// A field holds the pointer (pressed, its selection follows the drag).
    pub fn held(&self) -> bool {
        self.controls.held()
    }

    /// The pointer dragged while a field holds it: its selection stretched there.
    pub fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|controls, parts| controls.drag(parts, p)).is_some_and(|(_, c)| c)
    }

    /// The pointer let go.
    pub fn release(&mut self) -> bool {
        self.with_controls(|controls, parts| controls.release(parts)).is_some_and(|(_, c)| c)
    }

    pub fn contains(&self, p: Point) -> bool {
        self.panel.contains(p)
    }

    /// What's at `p`: a button, the search field's ×, a field.
    pub fn hit(&self, p: Point) -> Option<FindHit> {
        if let Some(hit) = self.button_at(p) {
            return Some(hit);
        }
        if self.query.clear_area().is_some_and(|r| r.contains(p)) {
            return Some(FindHit::Clear);
        }
        if Control::hit(&self.query, p) {
            return Some(FindHit::Field(FindField::Query));
        }
        (self.show_replace && self.replace.contains(p))
            .then_some(FindHit::Field(FindField::Replace))
    }

    /// The button at `p` (the fields' own parts aside).
    fn button_at(&self, p: Point) -> Option<FindHit> {
        self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The mouse moved; `true` if the hovered button (or the search field's ×) changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.button_at(p);
        let changed = hover != self.hover;
        self.hover = hover;
        let over_field = if hover.is_some() { None } else { Some(p) };
        let field = self.with_controls(|controls, parts| controls.hover(parts, over_field));
        changed | field
    }

    /// Strip height (physical pixels): one row for search, another for replace.
    pub fn height(&self, scale: f32) -> f32 {
        let rows = if self.show_replace { 2.0 } else { 1.0 };
        (ROW * rows * scale).round() + scale.round()
    }

    /// Draws the bar as a strip across the full editor width, in `bounds`:
    /// the replace chevron, a wide field (magnifier, text, clear, Cc / W / .*), the result
    /// count, ↑ ↓ and close. `counter` = (current, total).
    pub fn paint(&mut self, spec: FindBarSpec<'_>, painter: Painter<'_>) {
        let FindBarSpec { bounds, focused, scale, .. } = spec;
        let Painter { text, scene, palette, icons } = painter;
        let look = Look::new(scale, palette, icons, self.hover);
        let px = |v: f32| look.px(v);
        let (row, field_h, line) = (look.row, look.field_h, look.line);
        self.panel = bounds;
        self.hits.clear();

        let layer = scene.push_layer(Some(bounds));
        layer.quad(Quad::filled(bounds, palette.panel_background));
        layer.quad(Quad::filled(
            Rect::new(bounds.min_x(), bounds.max_y() - line, bounds.size.width, line),
            palette.border,
        ));

        // ── Row 1 ──
        let top = bounds.min_y() + (row - field_h) / 2.0;
        let (chevron, prev) = self.paint_buttons(layer, text, &look, bounds, top);
        let counter_right = prev.min_x() - px(12.0);
        self.paint_counter(layer, text, &look, &spec, (counter_right, top));
        // The field: everything between the chevron and the counter.
        let left = chevron.max_x() + px(6.0);
        let counter_w = px(110.0);
        let field_w = (counter_right - counter_w - px(12.0) - left).max(px(160.0));
        self.paint_query_field(layer, text, &look, Rect::new(left, top, field_w, field_h), &spec);

        // ── Row 2: replace, aligned with the search field ──
        if self.show_replace {
            let field = Rect::new(left, top + row, field_w, field_h);
            self.paint_replace_row(layer, text, &look, field, focused);
        }
    }

    /// The first row's buttons: the replace chevron on the left; close, ↓ ↑ on the right,
    /// from the edge. Returns the chevron's and ↑'s rectangles.
    fn paint_buttons(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        bounds: Rect,
        top: f32,
    ) -> (Rect, Rect) {
        let px = |v: f32| look.px(v);
        let button = look.button;
        let chevron = Rect::new(
            bounds.min_x() + px(8.0),
            top + (look.field_h - button) / 2.0,
            button,
            button,
        );
        let chevron_icon = if self.show_replace { "chevron-down" } else { "chevron-right" };
        let hits = &mut self.hits;
        look.icon_button(layer, text, hits, chevron, chevron_icon, FindHit::ToggleReplace);
        let close = Rect::new(bounds.max_x() - px(10.0) - button, chevron.min_y(), button, button);
        look.icon_button(layer, text, hits, close, "x", FindHit::Close);
        let next = Rect::new(close.min_x() - px(12.0) - button, chevron.min_y(), button, button);
        let prev = Rect::new(next.min_x() - px(2.0) - button, chevron.min_y(), button, button);
        look.icon_button(layer, text, hits, prev, "chevron-up", FindHit::Prev);
        look.icon_button(layer, text, hits, next, "chevron-down", FindHit::Next);
        (chevron, prev)
    }

    /// The result count (or why there is none), right-aligned at `at` = (right, top).
    fn paint_counter(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        spec: &FindBarSpec<'_>,
        at: (f32, f32),
    ) {
        let ((current, total), error, palette) = (spec.counter, spec.error, look.palette);
        let (counter_right, top) = at;
        let has_query = !self.query.text().is_empty();
        let label = match (error, has_query, total) {
            (true, _, _) => t("find.invalid_regex"),
            (_, false, _) => String::new(),
            (_, true, 0) => t("find.no_results"),
            (_, true, total) if current == 0 => tr!("find.results", count = total),
            (_, true, total) => tr!("find.position", current = current, total = total),
        };
        let shaped = text.shape_line(&label, &look.small);
        let color =
            if error || (has_query && total == 0) { palette.danger } else { palette.text_muted };
        draw_text(
            layer,
            &shaped,
            (counter_right - shaped.width).round(),
            centered(top, look.field_h, &look.small, text),
            color,
        );
    }

    /// The search field (the app's: the magnifier, the text, × with text), the options
    /// (Cc / W / .*) in the room it keeps on its right; red when the pattern is bad.
    fn paint_query_field(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        field: Rect,
        spec: &FindBarSpec<'_>,
    ) {
        let query_focused = spec.focused && self.focus() == FindField::Query;
        let extras = SearchExtras { trailing: TOGGLES_W, invalid: spec.error };
        let placeholder = t("find.placeholder");
        let room = self.query.paint_with(
            (layer, text, look.icons),
            &look.ctx(),
            (field, query_focused),
            &placeholder,
            extras,
        );
        self.paint_toggles(layer, text, look, room, spec.options);
    }

    /// The options inside the search field, in the room it kept (`room`): highlighted when
    /// on.
    fn paint_toggles(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        room: Rect,
        options: &SearchQuery,
    ) {
        let px = |v: f32| look.px(v);
        let toggle = px(TOGGLE_W);
        let mut x = room.min_x();
        for (label, active, hit) in [
            ("Cc", options.case_sensitive, FindHit::ToggleCase),
            ("W", options.whole_word, FindHit::ToggleWord),
            (".*", options.regex, FindHit::ToggleRegex),
        ] {
            let r = Rect::new(x, room.min_y() + px(3.0), toggle, room.size.height - px(6.0));
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .label(label)
                .rest_tone(ds::Tone::Neutral)
                .toggle(active)
                .hovered(look.hover == Some(hit));
            ds::button(layer, text, look.icons, &look.ctx(), r, spec);
            self.hits.push((r, hit));
            x += toggle + px(TOGGLE_GAP);
        }
    }

    /// The replace row: its field (in `field`, under the search field) and the Replace /
    /// Replace All buttons after it.
    fn paint_replace_row(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        field: Rect,
        focused: bool,
    ) {
        let px = |v: f32| look.px(v);
        let (top, field_h) = (field.min_y(), field.size.height);
        let replace_focused = focused && self.focus() == FindField::Replace;
        // The search pill's look, its text where the search field's is (past the room its
        // magnifier takes there).
        let ctx = look.ctx();
        let style = ui_kit::TextInputStyle {
            font: ctx.ui_font(-0.5),
            ..ds::search_input_style(&ctx, field_h)
        };
        let placeholder = t("find.replace_placeholder");
        self.replace.paint(layer, text, field, replace_focused, &placeholder, &style);

        let mut x = field.max_x() + px(12.0);
        for (label, hit) in
            [(t("find.replace"), FindHit::ReplaceOne), (t("find.replace_all"), FindHit::ReplaceAll)]
        {
            let shaped = text.shape_line(&label, &look.small);
            let b = Rect::new(x, top, shaped.width + px(20.0), field_h);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Soft)
                .label(&label)
                .hovered(look.hover == Some(hit));
            ds::button(layer, text, look.icons, &look.ctx(), b, spec);
            self.hits.push((b, hit));
            x = b.max_x() + px(8.0);
        }
    }
}

/// What one paint of the bar draws with: the theme, the icons, the fonts and sizes
/// (physical pixels), the hovered button.
struct Look<'a> {
    palette: &'a Palette,
    icons: &'a Icons,
    scale: f32,
    small: Font,
    row: f32,
    field_h: f32,
    button: f32,
    line: f32,
    hover: Option<FindHit>,
}

impl<'a> Look<'a> {
    fn new(scale: f32, palette: &'a Palette, icons: &'a Icons, hover: Option<FindHit>) -> Self {
        let px = |v: f32| (v * scale).round();
        let small = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.0));
        Self {
            palette,
            icons,
            scale,
            small,
            row: px(ROW),
            field_h: px(30.0),
            button: px(ds::button_size::MEDIUM),
            line: scale.round().max(1.0),
            hover,
        }
    }

    fn px(&self, v: f32) -> f32 {
        (v * self.scale).round()
    }

    /// The design system's context at the bar's scale (for the shared buttons).
    fn ctx(&self) -> ds::Ctx<'a> {
        ds::Ctx::new(self.palette, self.scale)
    }

    /// A square ghost icon button, registered as `hit`.
    fn icon_button(
        &self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        hits: &mut Vec<(Rect, FindHit)>,
        r: Rect,
        icon: &str,
        hit: FindHit,
    ) {
        let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
            .icon(icon)
            .rest_tone(ds::Tone::Neutral)
            .hovered(self.hover == Some(hit));
        ds::button(layer, text, self.icons, &self.ctx(), r, spec);
        hits.push((r, hit));
    }
}

/// The baseline centering `font`'s text in a row `h` tall at `top`.
fn centered(top: f32, h: f32, font: &Font, text: &mut TextSystem) -> f32 {
    centered_baseline(top, h, &text.metrics(font))
}

#[cfg(test)]
#[path = "../tests/unit/find_bar.rs"]
mod tests;
