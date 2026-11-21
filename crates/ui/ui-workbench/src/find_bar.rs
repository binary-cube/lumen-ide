//! The Find / Replace bar: a strip across the full editor width, below the tabs.

use document::SearchQuery;
use i18n::{t, tr};
use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use text_layout::{Font, TextSystem};
use ui_kit::{TextInput, TextInputStyle, centered_baseline, draw_icon, draw_text};

use crate::palette::Palette;
use crate::{Painter, ds};

/// Height of a bar row, in logical pixels.
const ROW: f32 = 42.0;

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

/// The bar's UI state (text fields, focus). Each tab has one.
#[derive(Debug, Clone)]
pub struct FindBar {
    pub query: TextInput,
    pub replace: TextInput,
    pub show_replace: bool,
    pub focus: FindField,
    panel: Rect,
    hits: Vec<(Rect, FindHit)>,
    hover: Option<FindHit>,
}

impl Default for FindBar {
    fn default() -> Self {
        Self {
            query: TextInput::default(),
            replace: TextInput::default(),
            show_replace: false,
            focus: FindField::Query,
            panel: Rect::default(),
            hits: Vec::new(),
            hover: None,
        }
    }
}

impl FindBar {
    pub fn focused_input_ref(&self) -> &TextInput {
        match self.focus {
            FindField::Query => &self.query,
            FindField::Replace => &self.replace,
        }
    }

    pub fn focused_input(&mut self) -> &mut TextInput {
        match self.focus {
            FindField::Query => &mut self.query,
            FindField::Replace => &mut self.replace,
        }
    }

    pub fn contains(&self, p: Point) -> bool {
        self.panel.contains(p)
    }

    pub fn hit(&self, p: Point) -> Option<FindHit> {
        self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The mouse moved; `true` if the hovered button changed.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p).filter(|h| !matches!(h, FindHit::Field(_)));
        let changed = hover != self.hover;
        self.hover = hover;
        changed
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

    /// The search field: its container, the magnifier, the options (Cc / W / .*), × (with
    /// text) and the text.
    fn paint_query_field(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        field: Rect,
        spec: &FindBarSpec<'_>,
    ) {
        let (palette, icons) = (look.palette, look.icons);
        let px = |v: f32| look.px(v);
        let (top, field_h) = (field.min_y(), field.size.height);
        let query_focused = spec.focused && self.focus == FindField::Query;
        look.paint_field(layer, field, query_focused);
        if let Some(id) = icons.id("search") {
            let size = px(14.0);
            let r = Rect::new(
                field.min_x() + px(9.0),
                (top + (field_h - size) / 2.0).round(),
                size,
                size,
            );
            draw_icon(layer, icons, id, r, palette.text_muted);
        }
        // Inside, on the right: × (with text), then Cc / W / .*.
        let toggles_w = self.paint_toggles(layer, text, look, field, spec.options);
        let mut input_right = field.max_x() - px(4.0) - toggles_w - px(4.0);
        if !self.query.text().is_empty() {
            let r = Rect::new(
                input_right - px(22.0),
                top + (field_h - px(22.0)) / 2.0,
                px(22.0),
                px(22.0),
            );
            // The same round × as the other search fields.
            let ctx = crate::ds::Ctx::new(palette, look.scale);
            let mid = r.min_y() + r.size.height / 2.0;
            let hovered = look.hover == Some(FindHit::Clear);
            crate::ds::clear_button(
                layer,
                icons,
                &ctx,
                r.min_x() + r.size.width / 2.0,
                mid,
                hovered,
            );
            self.hits.push((r, FindHit::Clear));
            input_right = r.min_x() - px(2.0);
        }
        let input = Rect::new(
            field.min_x() + px(30.0),
            top,
            input_right - field.min_x() - px(30.0),
            field_h,
        );
        let placeholder = t("find.placeholder");
        self.query.paint(layer, text, input, query_focused, &placeholder, &look.input_style);
        self.hits.push((
            Rect::new(field.min_x(), top, input_right - field.min_x(), field_h),
            FindHit::Field(FindField::Query),
        ));
    }

    /// The options inside the search field, on its right (Cc / W / .*): highlighted when
    /// on. Returns how wide they are together.
    fn paint_toggles(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut TextSystem,
        look: &Look<'_>,
        field: Rect,
        options: &SearchQuery,
    ) -> f32 {
        let px = |v: f32| look.px(v);
        let toggle = px(26.0);
        let toggles_w = 3.0 * toggle + 2.0 * px(2.0);
        let mut x = field.max_x() - px(4.0) - toggles_w;
        for (label, active, hit) in [
            ("Cc", options.case_sensitive, FindHit::ToggleCase),
            ("W", options.whole_word, FindHit::ToggleWord),
            (".*", options.regex, FindHit::ToggleRegex),
        ] {
            let r = Rect::new(x, field.min_y() + px(3.0), toggle, field.size.height - px(6.0));
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .label(label)
                .rest_tone(ds::Tone::Neutral)
                .on(active)
                .hovered(look.hover == Some(hit));
            ds::button(layer, text, look.icons, &look.ctx(), r, spec);
            self.hits.push((r, hit));
            x += toggle + px(2.0);
        }
        toggles_w
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
        let replace_focused = focused && self.focus == FindField::Replace;
        look.paint_field(layer, field, replace_focused);
        let input = Rect::new(field.min_x() + px(30.0), top, field.size.width - px(36.0), field_h);
        self.replace.paint(
            layer,
            text,
            input,
            replace_focused,
            &t("find.replace_placeholder"),
            &look.input_style,
        );
        self.hits.push((field, FindHit::Field(FindField::Replace)));

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
    /// The fields draw only their text; the bar draws the container (background, border,
    /// the magnifier and the options inside it).
    input_style: TextInputStyle,
    row: f32,
    field_h: f32,
    button: f32,
    line: f32,
    hover: Option<FindHit>,
}

impl<'a> Look<'a> {
    fn new(scale: f32, palette: &'a Palette, icons: &'a Icons, hover: Option<FindHit>) -> Self {
        let px = |v: f32| (v * scale).round();
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size));
        let small = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.0));
        let input_style = TextInputStyle {
            font,
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: palette.text,
            placeholder: palette.text_muted,
            selection: palette.selection,
            caret: palette.caret,
            radius: 0.0,
            border_width: 0.0,
            padding: px(2.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        Self {
            palette,
            icons,
            scale,
            small,
            input_style,
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

    /// A field's container: background and border (accent when focused).
    fn paint_field(&self, layer: &mut render_api::Layer, field: Rect, focused: bool) {
        let palette = self.palette;
        let border = if focused { palette.accent } else { palette.input_border };
        layer.quad(
            Quad::filled(field, palette.input_background)
                .rounded((6.0 * self.scale).round())
                .bordered(self.line, border),
        );
    }
}

/// The baseline centering `font`'s text in a row `h` tall at `top`.
fn centered(top: f32, h: f32, font: &Font, text: &mut TextSystem) -> f32 {
    centered_baseline(top, h, &text.metrics(font))
}
