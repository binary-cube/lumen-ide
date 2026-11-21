//! The chosen category's panel: its title and the line saying what it holds, then its
//! sections (each under its header), each a card of rows — a setting's label, the line
//! saying what it does, its control (a swatch with its hex, a segmented choice, pills, a
//! slider with its value in words, a switch, the name's field), "↺" while it differs from the
//! starting theme. While searching, the panel lists what's found across every category.

use super::*;
use model::{CARET, INTERFACE, SYNTAX, TokenGroup, keys_of};
use rows::{BackdropRow, FloatingRow};
use ui_components::ds::{ButtonKind, ButtonSpec, button_size};
use ui_components::form::{self, RowSpec};

/// A row's control.
#[derive(Debug, Clone)]
pub(crate) enum Control {
    /// A few short choices, side by side.
    Segmented(Row),
    /// Choices as pills, wrapped under the label (many, or long).
    Pills(Row),
    /// Values in order: a slider, its value said after it.
    Slider(Row),
    /// On / off.
    Switch(Row),
    /// A color (its key): its swatch and hex.
    Swatch(String),
    /// A layer's colors (the layer, its option): a swatch each, "+" / "−".
    Swatches(usize, &'static str),
    /// A corner's icons.
    Icons(Row),
    /// The theme's name.
    Name,
    /// One of the decorations' own settings (a switch for following the system, segments
    /// for the rest).
    Global(model::Global),
}

/// The pills' height and gap (logical pixels).
const PILL_H: f32 = 30.0;
/// A search result under the mouse: its fill's alpha of the text.
const FOUND_HOVER: f32 = 0.04;
const PILL_GAP: f32 = 8.0;
/// The swatch control's width (logical pixels).
const SWATCH_W: f32 = 168.0;

/// Where a row goes and what it is.
pub(crate) struct RowAt<'a> {
    pub col: &'a Column<'a>,
    pub y: f32,
    pub id: Option<SettingId>,
    pub label: String,
    pub detail: Option<String>,
    pub control: Control,
    /// It differs from the starting theme: its "↺" and what that does.
    pub reset: Option<Hit>,
}

impl Studio {
    /// The middle panel: the category (or what the search found), scrolled, clipped.
    pub(crate) fn paint_panel(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        input: &StudioInput,
        draft: &ThemeDraft,
        base: &ThemeDraft,
    ) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let (scene, text, icons) = (&mut *ctx.scene, &mut *ctx.text, ctx.icons);
        let px = |v: f32| ds.px(v);
        let list = self.list;
        let mut pen = LayerPainter::new(scene.push_layer(Some(list)), text, icons, ds);
        let (hits, sliders) = (self.hits.len(), self.sliders.len());
        // Twice when a jump from the search waits: once to find its row, once scrolled to it.
        for _ in 0..2 {
            self.headers.clear();
            self.rows_at.clear();
            self.name_field = None;
            self.hits.truncate(hits);
            self.sliders.truncate(sliders);
            pen.layer.quads.clear();
            pen.layer.images.clear();
            pen.layer.glyphs.clear();
            let pad = px(layout::PANEL_PAD);
            let w = (list.size.width - 2.0 * pad).min(px(layout::PANEL_MAX)).max(0.0);
            let col = Column { ds, x0: list.min_x() + pad, w };
            let top = list.min_y() + px(26.0) - self.scrolled().round();
            let mut y = top;
            if !self.query.text().trim().is_empty() {
                y = self.paint_found(&mut pen, &col, input, draft, y);
            } else {
                y = self.paint_title(&mut pen, &col, y, self.category);
                y = match self.category {
                    Category::General => self.paint_general(&mut pen, &col, input, draft, y),
                    Category::Interface => {
                        let y = self.paint_tokens(&mut pen, &col, draft, base, INTERFACE, y);
                        let y = self.paint_backdrop(&mut pen, &col, draft, base, y);
                        self.paint_floating(&mut pen, &col, draft, base, y)
                    }
                    Category::Syntax => self.paint_tokens(&mut pen, &col, draft, base, SYNTAX, y),
                    Category::Caret => self.paint_caret(&mut pen, &col, draft, base, y),
                    Category::Effects => self.paint_decorations(&mut pen, &col, draft, base, y),
                    Category::Options => self.paint_options(&mut pen, &col, draft, base, y),
                };
            }
            // `top` and `y` are both offset by the scroll; the end keeps the padding below it.
            self.content_h = y - top + px(52.0);
            if !self.finish_jump() {
                break;
            }
        }
        // The name's field, over its row, clipped to the panel.
        if let Some(field) = self.name_field
            && let Some(clip) = field.intersect(&self.list)
        {
            self.name.paint(
                scene.push_layer(Some(clip)),
                text,
                field,
                self.editing_name(),
                &t("studio.default_name"),
                &name_style(&ds),
            );
        }
    }

    /// A category's title and the line under it; returns where its sections start.
    fn paint_title(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        y: f32,
        c: Category,
    ) -> f32 {
        let ds = &col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let title = pen.text.shape_line(&c.label(), &font);
        draw_text(pen.layer, &title, col.x0, ds::optical_baseline(y, px(28.0), &font), p.text);
        let small = ds.ui_font(-1.0);
        let mut y = y + px(32.0);
        for line in ds::wrap(pen.text, &c.detail(), &small, col.w, 2) {
            let shaped = pen.text.shape_line(&line, &small);
            draw_text(
                pen.layer,
                &shaped,
                col.x0,
                ds::optical_baseline(y, px(20.0), &small),
                p.text_muted,
            );
            y += px(20.0);
        }
        y + px(24.0)
    }

    /// A section's header (see `ds::section_header`): the space above it (and below the
    /// section before) unless it's the first; returns where its content starts.
    pub(crate) fn header(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        mut y: f32,
        id: &'static str,
        icon: &str,
        title: &str,
    ) -> f32 {
        let ds = &col.ds;
        let space = if self.headers.is_empty() { 0.0 } else { 2.0 * ds.px(ds::SECTION_SPACE) };
        y += space;
        let below = ds::section_header(pen, (col.x0, y, col.w), icon, title);
        self.headers.push((id, Rect::new(col.x0, y, col.w, below - y), space));
        below
    }

    /// Rows in a card: each drawn by `rows` (given where it starts, returning where it
    /// ends), a line between them, the card under them. Returns the y after the card.
    pub(crate) fn group(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        y: f32,
        count: usize,
        mut row: impl FnMut(&mut Self, &mut LayerPainter<'_>, usize, f32) -> f32,
    ) -> f32 {
        let ds = &col.ds;
        let at = pen.layer.quads.len();
        let mut bottom = y;
        for i in 0..count {
            if i > 0 {
                form::row_divider(pen.layer, ds, col.x0, col.x0 + col.w, bottom);
            }
            bottom = row(self, pen, i, bottom);
        }
        if count > 0 {
            let rect = Rect::new(col.x0, y, col.w, bottom - y);
            pen.layer.quads.insert(at, form::group_quad(ds, rect));
        }
        bottom
    }

    /// A setting's row and its control; returns the y below it.
    pub(crate) fn paint_setting(
        &mut self,
        pen: &mut LayerPainter<'_>,
        draft: &ThemeDraft,
        at: RowAt<'_>,
    ) -> f32 {
        let col = at.col;
        let ds = &col.ds;
        let px = |v: f32| ds.px(v);
        let inner_w = col.w - 2.0 * px(form::ROW_PAD) - px(form::RESET_W) - px(8.0);
        let (control_w, control_h) =
            self.control_size(pen.text, ds, draft, &at.control, inner_w, col.w);
        let highlight = match (&self.highlight, &at.id) {
            (Some(h), Some(id)) if h == id => 1.0,
            _ => 0.0,
        };
        let reset_hovered = at.reset.as_ref().is_some_and(|r| self.hover.as_ref() == Some(r));
        let area = form::setting_row(
            pen.layer,
            pen.text,
            ds,
            pen.icons,
            RowSpec {
                x: col.x0,
                y: at.y,
                width: col.w,
                label: &at.label,
                detail: at.detail.as_deref(),
                control_w,
                control_h,
                icon: None,
                modified: at.reset.is_some(),
                reset_hovered,
                highlight,
            },
        );
        if let (Some(r), Some(hit)) = (area.reset, at.reset.clone()) {
            self.push_list_hit(r, hit);
        }
        if let Some(id) = at.id {
            self.rows_at.push((id, area.rect));
        }
        self.paint_control(pen, ds, draft, &at.control, area.control);
        area.rect.max_y()
    }

    /// How wide and tall (logical) a control is, for a row whose controls may take `room`.
    fn control_size(
        &self,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        draft: &ThemeDraft,
        control: &Control,
        room: f32,
        width: f32,
    ) -> (f32, f32) {
        let px = |v: f32| ds.px(v);
        let full = width - 2.0 * px(form::ROW_PAD);
        match control {
            Control::Segmented(row) => {
                let labels = row.choices(draft);
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let w = ds::segmented_width(text, ds, &refs);
                if w <= room * 0.62 {
                    (w, 32.0)
                } else {
                    (full, pills_height(text, ds, &labels, full) / ds.scale)
                }
            }
            Control::Pills(row) => {
                let labels = row.choices(draft);
                (full, pills_height(text, ds, &labels, full) / ds.scale)
            }
            Control::Icons(row) => {
                let n = row.choices(draft).len() as f32;
                let w = n * px(PILL_H) + (n - 1.0).max(0.0) * px(PILL_GAP);
                (w.min(full), PILL_H)
            }
            Control::Slider(_) => {
                ((room * 0.55).clamp(px(220.0), px(340.0)).min(full), form::VALUE_SLIDER_H)
            }
            Control::Switch(_) => (px(36.0), 20.0),
            Control::Swatch(_) => (px(SWATCH_W), 26.0),
            Control::Swatches(layer, key) => {
                let n = draft
                    .decorations
                    .layers
                    .get(*layer)
                    .and_then(|l| match l.spec.get(key) {
                        Some(theming::decorations::FieldValue::Colors(list)) => Some(list.len()),
                        Some(theming::decorations::FieldValue::Color(_)) => Some(1),
                        _ => None,
                    })
                    .unwrap_or(1) as f32
                    + 2.0;
                ((n * px(32.0)).min(full), 26.0)
            }
            Control::Name => ((room * 0.6).clamp(px(220.0), px(340.0)).min(full), 34.0),
            Control::Global(model::Global::ReduceMotion) => (px(36.0), 20.0),
            Control::Global(g) => {
                let (choices, _) = Studio::global_choices(*g);
                let refs: Vec<&str> = choices.iter().map(|(_, l)| l.as_str()).collect();
                (ds::segmented_width(text, ds, &refs).min(full), 32.0)
            }
        }
    }

    /// Draws a control in its place.
    fn paint_control(
        &mut self,
        pen: &mut LayerPainter<'_>,
        ds: &Ctx<'_>,
        draft: &ThemeDraft,
        control: &Control,
        r: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let mid = r.min_y() + r.size.height / 2.0;
        match control {
            Control::Segmented(row) if r.size.height <= px(32.0) + 1.0 => {
                let labels = row.choices(draft);
                let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
                let hovered = match &self.hover {
                    Some(Hit::Choice(h, i)) if h == row => Some(*i),
                    _ => None,
                };
                let current = row.current(draft);
                let mut segs = Vec::with_capacity(refs.len());
                ds::segmented_with(
                    pen.layer,
                    pen.text,
                    ds,
                    ds::SegmentedSpec {
                        right: r.max_x(),
                        mid,
                        labels: &refs,
                        selected: current,
                        hovered,
                    },
                    |i, seg| segs.push((seg, Hit::Choice(*row, i))),
                );
                for (seg, hit) in segs {
                    self.push_list_hit(seg, hit);
                }
            }
            Control::Segmented(row) | Control::Pills(row) => {
                self.paint_pills(pen, ds, draft, *row, r);
            }
            Control::Icons(row) => {
                let current = row.current(draft);
                let side = px(PILL_H);
                let mut x = r.min_x();
                for (i, name) in row.choices(draft).iter().enumerate() {
                    let cell = Rect::new(x, r.min_y(), side, side);
                    let hovered = self.hover == Some(Hit::Choice(*row, i));
                    let spec = ButtonSpec::new(ButtonKind::Ghost)
                        .icon(name)
                        .on(i == current)
                        .hovered(hovered);
                    ds::button(pen.layer, pen.text, pen.icons, ds, cell, spec);
                    self.push_list_hit(cell, Hit::Choice(*row, i));
                    x += side + px(PILL_GAP);
                }
            }
            Control::Slider(row) => {
                let lit = self.slider_held == Some(*row) || self.slider_focused == Some(*row);
                let area = form::value_slider(
                    pen.layer,
                    pen.text,
                    ds,
                    form::ValueSliderSpec {
                        x: r.min_x(),
                        mid,
                        width: r.size.width,
                        stops: row.stops(draft),
                        current: row.current(draft),
                        value: &row.value_text(draft),
                        hovered: lit,
                    },
                );
                self.sliders.push((area, *row));
            }
            Control::Switch(row) => {
                let on = row.current(draft) == 1;
                let hovered = self.hover == Some(Hit::Choice(*row, usize::from(!on)));
                let sw = ds::switch(pen.layer, ds, r.min_x(), mid, on, hovered);
                self.push_list_hit(sw, Hit::Choice(*row, usize::from(!on)));
            }
            Control::Swatch(key) => {
                let color = draft.get(key).unwrap_or(Rgba::WHITE);
                let editing = self.picking.as_ref().is_some_and(|(k, _)| k == key);
                let hovered = self.hover == Some(Hit::Token(key.clone()));
                let hex = Hex.format(color, ColorFormat::Hex);
                ds::ColorField::new(color, &hex)
                    .hovered(hovered)
                    .editing(editing)
                    .paint(pen.layer, pen.text, pen.icons, ds, r);
                self.push_list_hit(r, Hit::Token(key.clone()));
            }
            Control::Swatches(i, key) => self.paint_swatches(pen, ds, draft, *i, key, r),
            Control::Global(g) => {
                let (choices, current) = Studio::global_choices(*g);
                if *g == model::Global::ReduceMotion {
                    let on = current == 1;
                    let hit = Hit::Global(*g, i64::from(!on));
                    let sw = ds::switch(
                        pen.layer,
                        ds,
                        r.min_x(),
                        mid,
                        on,
                        self.hover == Some(hit.clone()),
                    );
                    self.push_list_hit(sw, hit);
                    return;
                }
                let refs: Vec<&str> = choices.iter().map(|(_, l)| l.as_str()).collect();
                let selected = choices.iter().position(|(v, _)| *v == current).unwrap_or(0);
                let hovered =
                    choices.iter().position(|(v, _)| self.hover == Some(Hit::Global(*g, *v)));
                let mut segs = Vec::with_capacity(refs.len());
                ds::segmented_with(
                    pen.layer,
                    pen.text,
                    ds,
                    ds::SegmentedSpec { right: r.max_x(), mid, labels: &refs, selected, hovered },
                    |i, seg| segs.push((seg, Hit::Global(*g, choices[i].0))),
                );
                for (seg, hit) in segs {
                    self.push_list_hit(seg, hit);
                }
            }
            Control::Name => {
                // The field itself is drawn with the name (its frame the shared field's).
                let field = r;
                self.push_list_hit(field, Hit::Name);
                self.name_field = Some(field);
            }
        }
    }

    /// A row's choices as pills, wrapped in `r`.
    fn paint_pills(
        &mut self,
        pen: &mut LayerPainter<'_>,
        ds: &Ctx<'_>,
        draft: &ThemeDraft,
        row: Row,
        r: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let h = px(PILL_H);
        let current = row.current(draft);
        let (mut x, mut y) = (r.min_x(), r.min_y());
        for (i, label) in row.choices(draft).iter().enumerate() {
            let w = ds::choice_width(pen.text, ds, label);
            if x + w > r.max_x() && x > r.min_x() {
                x = r.min_x();
                y += h + px(PILL_GAP);
            }
            let spec = ds::ChoiceSpec {
                x,
                mid: y + h / 2.0,
                label,
                selected: i == current,
                hovered: self.hover == Some(Hit::Choice(row, i)),
            };
            let cell = ds::choice(pen.layer, pen.text, ds, spec);
            self.push_list_hit(cell, Hit::Choice(row, i));
            x += w + px(PILL_GAP);
        }
    }

    /// A layer's colors: a swatch each (each opens the picker), "+" / "−" within its limits.
    fn paint_swatches(
        &mut self,
        pen: &mut LayerPainter<'_>,
        ds: &Ctx<'_>,
        draft: &ThemeDraft,
        i: usize,
        key: &'static str,
        r: Rect,
    ) {
        use theming::decorations::{FieldKind, FieldValue};
        use theming::studio::layer_color_key;
        let px = |v: f32| ds.px(v);
        let Some(deco) = draft.decorations.layers.get(i) else { return };
        let Some(field) = deco.spec.kind().field(key) else { return };
        let colors = match deco.spec.get(key) {
            Some(FieldValue::Color(c)) => vec![c],
            Some(FieldValue::Colors(list)) => list,
            _ => return,
        };
        let side = px(button_size::MEDIUM);
        let mut x = r.min_x();
        let mid = r.min_y() + r.size.height / 2.0;
        for (k, color) in colors.iter().enumerate() {
            let cell = Rect::new(x, (mid - side / 2.0).round(), side, side);
            let token = layer_color_key(i, key, k);
            let editing = self.picking.as_ref().is_some_and(|(key, _)| *key == token);
            let hovered = self.hover == Some(Hit::Token(token.clone()));
            ds::ColorSwatch::new(*color)
                .hovered(hovered)
                .editing(editing)
                .paint(pen.layer, ds, cell);
            self.push_list_hit(cell, Hit::Token(token));
            x += side + px(6.0);
        }
        if let FieldKind::Colors { min, max } = field.kind {
            for (sign, hit, shown) in [
                ("+", Hit::AddColor(i, key), colors.len() < max),
                ("−", Hit::DropColor(i, key), colors.len() > min),
            ] {
                if !shown {
                    continue;
                }
                let cell = Rect::new(x, (mid - side / 2.0).round(), side, side);
                let spec = ButtonSpec::new(ButtonKind::Soft)
                    .label(sign)
                    .hovered(self.hover == Some(hit.clone()));
                ds::button(pen.layer, pen.text, pen.icons, ds, cell, spec);
                self.push_list_hit(cell, hit);
                x += side + px(6.0);
            }
        }
    }

    /// General: the theme's name, then the themes to start from (a gallery, filtered by
    /// appearance).
    fn paint_general(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        input: &StudioInput,
        draft: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let ds = col.ds;
        let px = |v: f32| ds.px(v);
        let mut y = self.header(pen, col, y, "name", "tag", &t("studio.section_name"));
        y = self.group(pen, col, y, 1, |s, pen, _, y| {
            s.paint_setting(
                pen,
                draft,
                RowAt {
                    col,
                    y,
                    id: Some(SettingId::Name),
                    label: t("studio.name"),
                    detail: Some(t("studio.name_detail")),
                    control: Control::Name,
                    reset: None,
                },
            )
        });
        y = self.header(pen, col, y, "start", "layers", &t("studio.section_start"));
        // The filter (and how many there are), then the gallery.
        let labels: Vec<String> = BaseFilter::ALL.iter().map(|f| f.label()).collect();
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let width = ds::segmented_width(pen.text, &ds, &refs);
        let mid = y + px(16.0);
        let selected = BaseFilter::ALL.iter().position(|f| *f == self.base_filter).unwrap_or(0);
        let hovered = match &self.hover {
            Some(Hit::BaseFilter(f)) => BaseFilter::ALL.iter().position(|x| x == f),
            _ => None,
        };
        let mut segs = Vec::new();
        ds::segmented_with(
            pen.layer,
            pen.text,
            &ds,
            ds::SegmentedSpec { right: col.x0 + width, mid, labels: &refs, selected, hovered },
            |i, r| segs.push((r, Hit::BaseFilter(BaseFilter::ALL[i]))),
        );
        for (r, hit) in segs {
            self.push_list_hit(r, hit);
        }
        let shown: Vec<usize> = (0..input.themes.len())
            .filter(|i| input.themes[*i].id != PREVIEW_ID && self.passes(*i))
            .collect();
        let font = ds.ui_font(-2.0);
        let count =
            pen.text.shape_line(&i18n::tr!("studio.themes_count", n = shown.len() as i64), &font);
        draw_text(
            pen.layer,
            &count,
            col.x0 + col.w - count.width,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            ds.palette.text_muted,
        );
        y = mid + px(16.0) + px(14.0);
        let grid = form::Grid::new(col.x0, y, col.w, px(150.0), px(112.0), px(12.0));
        for (n, i) in shown.iter().enumerate() {
            let cell = grid.cell(n);
            let theme = &input.themes[*i];
            let current = theme.id == input.current;
            let badge = current.then(|| (t("studio.in_use"), ds::Tone::Accent));
            let thumb = form::gallery_card(
                pen.layer,
                pen.text,
                &ds,
                form::GalleryCardSpec {
                    rect: cell,
                    thumb_h: 64.0,
                    title: &theme.name,
                    detail: None,
                    badge: badge.as_ref().map(|(l, tone)| (l.as_str(), *tone)),
                    selected: *i == self.base,
                    hovered: self.hover == Some(Hit::Base(*i)),
                },
            );
            if let Some(Some(d)) = self.thumbs.get(*i) {
                paint_thumb(pen.layer, &ds, thumb, d);
            }
            self.push_list_hit(cell, Hit::Base(*i));
        }
        if !shown.is_empty() {
            self.rows_at
                .push((SettingId::Base, Rect::new(col.x0, y, col.w, grid.height(shown.len()))));
        }
        let _ = draft;
        y + grid.height(shown.len())
    }

    /// Whether theme `i` passes the gallery's filter.
    fn passes(&self, i: usize) -> bool {
        match self.base_filter {
            BaseFilter::All => true,
            f => match self.thumbs.get(i) {
                Some(Some(d)) => rows::dark(d) == (f == BaseFilter::Dark),
                _ => true,
            },
        }
    }

    /// Interface or syntax colors, by group: a row each (its swatch and hex, "↺").
    fn paint_tokens(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        groups: &[TokenGroup],
        mut y: f32,
    ) -> f32 {
        for group in groups {
            let keys: Vec<&'static str> = keys_of(group, draft).collect();
            if keys.is_empty() {
                continue;
            }
            y = self.header(
                pen,
                col,
                y,
                group.id,
                group_icon(group.id),
                &model::group_name(group.id),
            );
            y = self.group(pen, col, y, keys.len(), |s, pen, n, y| {
                let key = keys[n];
                let reset =
                    (draft.get(key) != base.get(key)).then(|| Hit::ResetToken(key.to_owned()));
                s.paint_setting(
                    pen,
                    draft,
                    RowAt {
                        col,
                        y,
                        id: Some(SettingId::Token(key)),
                        label: model::token_name(key),
                        detail: Some(model::token_detail(key)),
                        control: Control::Swatch(key.to_owned()),
                        reset,
                    },
                )
            });
        }
        y
    }

    /// The backdrop under the windows over the editor: its color, how it shows the window
    /// behind, how much of the color, how far it blurs (seen at once on the studio's own
    /// dialogs and every overlay).
    fn paint_backdrop(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let name = model::group_name("backdrop");
        let y = self.header(pen, col, y, "backdrop", "layers", &name);
        let key = theming::studio::BACKDROP_COLOR;
        self.group(pen, col, y, 1 + BackdropRow::ALL.len(), |s, pen, n, y| {
            let at = match n.checked_sub(1).and_then(|i| BackdropRow::ALL.get(i)) {
                None => RowAt {
                    col,
                    y,
                    id: Some(SettingId::BackdropColor),
                    label: t("studio.backdrop_color"),
                    detail: Some(t("studio.backdrop_color_detail")),
                    control: Control::Swatch(key.to_owned()),
                    reset: (draft.get(key) != base.get(key))
                        .then(|| Hit::ResetToken(key.to_owned())),
                },
                Some(row) => {
                    let r = Row::Backdrop(*row);
                    RowAt {
                        col,
                        y,
                        id: Some(SettingId::Backdrop(*row)),
                        label: r.label(draft),
                        detail: r.detail(draft),
                        control: if r.is_slider(draft) {
                            Control::Slider(r)
                        } else {
                            Control::Segmented(r)
                        },
                        reset: rows::differs(r, draft, base).then_some(Hit::ResetRow(r)),
                    }
                }
            };
            s.paint_setting(pen, draft, at)
        })
    }

    /// The floating panels' glass (the menu drawer's): opaque or glass, the panel's opacity,
    /// how far it blurs (seen on the preview's dialog).
    fn paint_floating(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let name = model::group_name("floating");
        let y = self.header(pen, col, y, "floating", "menu", &name);
        self.group(pen, col, y, FloatingRow::ALL.len(), |s, pen, n, y| {
            let r = Row::Floating(FloatingRow::ALL[n]);
            let at = RowAt {
                col,
                y,
                id: Some(SettingId::Floating(FloatingRow::ALL[n])),
                label: r.label(draft),
                detail: r.detail(draft),
                control: if r.is_slider(draft) {
                    Control::Slider(r)
                } else {
                    Control::Segmented(r)
                },
                reset: rows::differs(r, draft, base).then_some(Hit::ResetRow(r)),
            };
            s.paint_setting(pen, draft, at)
        })
    }

    /// The caret: its shape, then how it moves.
    fn paint_caret(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        mut y: f32,
    ) -> f32 {
        for (group, caret_rows) in CARET {
            let icon = if *group == "shape" { "pen.text-cursor" } else { "play" };
            y = self.header(pen, col, y, group, icon, &model::group_name(group));
            y = self.group(pen, col, y, caret_rows.len(), |s, pen, n, y| {
                let row = Row::Caret(caret_rows[n]);
                let control = if row.is_slider(draft) {
                    Control::Slider(row)
                } else {
                    Control::Segmented(row)
                };
                s.paint_setting(
                    pen,
                    draft,
                    RowAt {
                        col,
                        y,
                        id: Some(SettingId::Caret(caret_rows[n])),
                        label: row.label(draft),
                        detail: row.detail(draft),
                        control,
                        reset: rows::differs(row, draft, base).then_some(Hit::ResetRow(row)),
                    },
                )
            });
        }
        y
    }

    /// The theme's own settings, a row each.
    pub(crate) fn paint_options(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        draft: &ThemeDraft,
        base: &ThemeDraft,
        y: f32,
    ) -> f32 {
        use theming::options::OptionKind;
        let y = self.header(pen, col, y, "options", "gear", &t("studio.section_options"));
        self.group(pen, col, y, draft.options.len(), |s, pen, n, y| {
            let row = Row::Option(n);
            let control = match draft.options[n].kind {
                OptionKind::Toggle => Control::Switch(row),
                OptionKind::Choice(_) => Control::Segmented(row),
                OptionKind::Slider { .. } => Control::Slider(row),
                OptionKind::Color { .. } => {
                    Control::Swatch(theming::studio::option_color_key(&draft.options[n].id))
                }
            };
            s.paint_setting(
                pen,
                draft,
                RowAt {
                    col,
                    y,
                    id: Some(SettingId::Option(n)),
                    label: row.label(draft),
                    detail: None,
                    control,
                    reset: rows::differs(row, draft, base).then_some(Hit::ResetRow(row)),
                },
            )
        })
    }

    /// What the search found, across every category: a row each (its name, where it is),
    /// a click shows it.
    fn paint_found(
        &mut self,
        pen: &mut LayerPainter<'_>,
        col: &Column<'_>,
        input: &StudioInput,
        draft: &ThemeDraft,
        y: f32,
    ) -> f32 {
        let ds = &col.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let entries = model::catalog(draft, input, false);
        let found = model::search(&entries, self.query.text());
        let title = i18n::tr!("studio.found", n = found.len() as i64);
        let mut y = ds::section_title(pen.layer, pen.text, ds, col.x0, y, &title, None) + px(16.0);
        if found.is_empty() {
            let font = ds.ui_font(-1.0);
            let shaped = pen.text.shape_line(&t("studio.found_none"), &font);
            draw_text(
                pen.layer,
                &shaped,
                col.x0,
                ds::optical_baseline(y, px(20.0), &font),
                p.text_muted,
            );
            return y + px(20.0);
        }
        let h = px(48.0);
        let label_font = ds.ui_font(-0.5);
        let small = ds.ui_font(-2.0);
        let rows: Vec<(String, String)> = found
            .iter()
            .map(|e| (e.label.clone(), format!("{} › {}", e.category.label(), e.group)))
            .collect();
        y = self.group(pen, col, y, rows.len(), |s, pen, n, y| {
            let r = Rect::new(col.x0, y, col.w, h);
            ds::RowHighlight::new(false, s.hover == Some(Hit::Found(n)))
                .hover(ds::HoverFill::Faint(FOUND_HOVER))
                .paint(pen.layer, ds, r);
            let x = col.x0 + px(form::ROW_PAD);
            let (label, place) = &rows[n];
            let shaped = pen.text.shape_line(label, &label_font);
            draw_text(
                pen.layer,
                &shaped,
                x,
                ds::optical_baseline(y + px(6.0), px(20.0), &label_font),
                p.text,
            );
            let shaped = pen.text.shape_line(place, &small);
            draw_text(
                pen.layer,
                &shaped,
                x,
                ds::optical_baseline(y + px(25.0), px(16.0), &small),
                p.text_muted,
            );
            s.push_list_hit(r, Hit::Found(n));
            y + h
        });
        y
    }
}

/// How tall (physical) `labels` are as pills wrapped in `width`.
fn pills_height(text: &mut TextSystem, ds: &Ctx<'_>, labels: &[String], width: f32) -> f32 {
    let px = |v: f32| ds.px(v);
    let (mut x, mut rows) = (0.0, 1);
    for label in labels {
        let w = ds::choice_width(text, ds, label);
        if x + w > width && x > 0.0 {
            x = 0.0;
            rows += 1;
        }
        x += w + px(PILL_GAP);
    }
    rows as f32 * px(PILL_H) + (rows - 1) as f32 * px(PILL_GAP)
}

/// A group's icon in its section header.
fn group_icon(id: &str) -> &'static str {
    match id {
        "backgrounds" => "layers",
        "text" => "file-text",
        "accent" => "palette",
        "states" => "circle-alert",
        "borders" => "square",
        "keywords" => "braces",
        "literals" => "hash",
        "comments" => "comment",
        "types" => "file-code",
        "variables" => "parameter",
        _ => "tag",
    }
}

/// A theme in miniature (a gallery card's): its window's background, a side panel, the
/// editor with three lines of code in its syntax colors and its accent.
pub(crate) fn paint_thumb(layer: &mut Layer, ds: &Ctx<'_>, r: Rect, d: &ThemeDraft) {
    let px = |v: f32| ds.px(v);
    let c = |k: &str| draft_color(d, k);
    layer.quad(Quad::filled(r, c("base")).rounded(px(radius::SM)));
    let inset = px(6.0);
    let side = Rect::new(
        r.min_x() + inset,
        r.min_y() + inset,
        (r.size.width * 0.24).round(),
        r.size.height - 2.0 * inset,
    );
    layer.quad(Quad::filled(side, c("panel")).rounded(px(3.0)));
    for k in 0..3 {
        let bar = Rect::new(
            side.min_x() + px(4.0),
            side.min_y() + px(6.0) + k as f32 * px(8.0),
            side.size.width * if k == 1 { 0.5 } else { 0.7 },
            px(3.0),
        );
        layer.quad(Quad::filled(bar, c("muted").with_alpha(0.6)).rounded(px(1.5)));
    }
    let card = Rect::new(
        side.max_x() + px(5.0),
        side.min_y(),
        r.max_x() - inset - side.max_x() - px(5.0),
        side.size.height,
    );
    layer.quad(Quad::filled(card, c("card")).rounded(px(3.0)));
    let lines: [(&str, f32, &str, f32); 3] = [
        ("syntax.keyword", 0.22, "syntax.function", 0.38),
        ("syntax.type", 0.18, "syntax.string", 0.46),
        ("syntax.variable", 0.3, "syntax.number", 0.12),
    ];
    for (k, (a, aw, b, bw)) in lines.iter().enumerate() {
        let y = card.min_y() + px(8.0) + k as f32 * px(9.0);
        let x = card.min_x() + px(7.0) + if k == 2 { px(8.0) } else { 0.0 };
        let w = card.size.width - px(14.0);
        layer
            .quad(Quad::filled(Rect::new(x, y, (w * aw).round(), px(3.5)), c(a)).rounded(px(1.75)));
        layer.quad(
            Quad::filled(
                Rect::new(x + (w * aw).round() + px(4.0), y, (w * bw).round(), px(3.5)),
                c(b),
            )
            .rounded(px(1.75)),
        );
    }
    let accent = Rect::new(card.max_x() - px(26.0), card.max_y() - px(12.0), px(20.0), px(6.0));
    layer.quad(Quad::filled(accent, c("accent")).rounded(px(3.0)));
}
