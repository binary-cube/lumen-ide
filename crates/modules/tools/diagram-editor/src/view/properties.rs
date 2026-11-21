//! The properties panel, on the sheet's right. Nothing chosen: the page's (Diagram — its
//! grid, the grid's step, its background). Cells chosen: their Style (fill, line, the line's
//! width, dashed, rounded, the text's size, weight, slant, alignment and color) and, for one
//! shape, its place and size (Arrange). Its rows are made at each paint from what's in memory
//! (the first chosen cell's style, the page's settings); each control one step of the
//! history. A number is changed by − / +, or typed (its value clicked; ↩ sets it, Esc not).

use i18n::t;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::control::Control;
use ui_components::ds::{self, Ctx, Swatch};
use ui_components::{KeyReach, ViewerKey};
use ui_kit::{TextInput, draw_text};

use super::DiagramView;
use super::parts::Asked;

/// The panel's width (logical pixels).
pub(super) const WIDTH: f32 = 240.0;
const ROW_H: f32 = 34.0;

/// The fills offered (draw.io's palette): the theme's, none, then colors.
const FILLS: [Swatch; 9] = [
    Swatch::Default,
    Swatch::None,
    Swatch::Color(Rgba::rgb(0xffffff)),
    Swatch::Color(Rgba::rgb(0xdae8fc)),
    Swatch::Color(Rgba::rgb(0xd5e8d4)),
    Swatch::Color(Rgba::rgb(0xffe6cc)),
    Swatch::Color(Rgba::rgb(0xfff2cc)),
    Swatch::Color(Rgba::rgb(0xf8cecc)),
    Swatch::Color(Rgba::rgb(0xe1d5e7)),
];

/// The lines and texts offered.
const LINES: [Swatch; 9] = [
    Swatch::Default,
    Swatch::None,
    Swatch::Color(Rgba::rgb(0x000000)),
    Swatch::Color(Rgba::rgb(0x6c8ebf)),
    Swatch::Color(Rgba::rgb(0x82b366)),
    Swatch::Color(Rgba::rgb(0xd79b00)),
    Swatch::Color(Rgba::rgb(0xd6b656)),
    Swatch::Color(Rgba::rgb(0xb85450)),
    Swatch::Color(Rgba::rgb(0x9673a6)),
];

/// A number of the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Number {
    GridSize,
    LineWidth,
    FontSize,
    X,
    Y,
    W,
    H,
}

impl Number {
    /// How much − / + changes it.
    fn step(self) -> f32 {
        match self {
            Self::GridSize => 5.0,
            Self::LineWidth | Self::FontSize => 1.0,
            Self::X | Self::Y | Self::W | Self::H => 10.0,
        }
    }

    /// Its smallest value.
    fn min(self) -> f32 {
        match self {
            Self::GridSize | Self::FontSize | Self::W | Self::H => 1.0,
            Self::LineWidth => 0.0,
            Self::X | Self::Y => f32::MIN,
        }
    }

    /// A style key it is (the others: the page's or the geometry's).
    fn style_key(self) -> Option<&'static str> {
        match self {
            Self::LineWidth => Some("strokeWidth"),
            Self::FontSize => Some("fontSize"),
            _ => None,
        }
    }
}

/// What a point of the panel does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Prop {
    Grid,
    Swatch(&'static str, Swatch),
    /// A style flag (`1` or taken away).
    Flag(&'static str),
    /// A bit of `fontStyle` (1: bold, 2: italic).
    FontBit(u32),
    Align(&'static str),
    Step(Number, f32),
    /// Its value clicked: typed.
    Type(Number),
}

/// The panel's state.
#[derive(Default)]
pub(super) struct Props {
    hits: Vec<(Rect, Prop)>,
    /// A number being typed.
    pub(super) typing: Option<(Number, TextInput)>,
    /// Where the first chosen cell is on its page and in the drawing's boxes, for the
    /// document's version it was found in (each paint asks it many times: found once).
    first: Option<(u64, usize, Option<usize>)>,
}

/// A color as draw.io writes it.
fn hex(c: Rgba) -> String {
    let [r, g, b, _] = c.to_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// Which of `swatches` a style value is (unset: the default).
fn chosen(swatches: &[Swatch], value: Option<&str>) -> Option<usize> {
    let wanted = match value {
        None | Some("default") => Swatch::Default,
        Some("none") => Swatch::None,
        Some(v) => Swatch::Color(crate::look::color(v)?),
    };
    swatches.iter().position(|s| *s == wanted)
}

impl DiagramView {
    /// The panel's area (the view's right, under the bar).
    pub(super) fn properties_area(&self) -> Rect {
        let top = self.toolbar_area().max_y();
        if !self.show_properties {
            return Rect::new(self.bounds.max_x(), top, 0.0, 0.0);
        }
        let w = (WIDTH * self.scale).min(self.bounds.size.width / 3.0);
        Rect::new(self.bounds.max_x() - w, top, w, (self.bounds.max_y() - top).max(0.0))
    }

    /// The first chosen cell (whose values the panel shows).
    fn first_chosen(&self) -> Option<&diagram_model::Cell> {
        let d = self.diagram.as_ref()?;
        let page = d.doc.pages.get(self.page)?;
        let id = self.selection.first()?;
        // Where it was found, while it's still there.
        if let Some((version, at, _)) = self.props.first
            && version == d.version
            && let Some(cell) = page.cells.get(at).filter(|c| c.id == *id)
        {
            return Some(cell);
        }
        page.cell(id)
    }

    /// The first chosen cell's box in the drawing.
    fn first_box(&self) -> Option<Rect> {
        let cell = self.first_chosen()?;
        let boxes = &self.drawing.as_ref()?.0.boxes;
        if let Some((_, _, Some(at))) = self.props.first
            && let Some((id, r)) = boxes.get(at)
            && *id == cell.id
        {
            return Some(*r);
        }
        boxes.iter().find(|(id, _)| *id == cell.id).map(|(_, r)| *r)
    }

    /// Where the first chosen cell is, found again when the document or the choice changed.
    fn find_first(&mut self) {
        let Some(d) = self.diagram.as_ref() else { return };
        let Some(id) = self.selection.first() else {
            self.props.first = None;
            return;
        };
        let current = self.props.first.is_some_and(|(version, at, _)| {
            version == d.version
                && d.doc
                    .pages
                    .get(self.page)
                    .and_then(|p| p.cells.get(at))
                    .is_some_and(|c| c.id == *id)
        });
        if current {
            return;
        }
        let Some(at) =
            d.doc.pages.get(self.page).and_then(|p| p.cells.iter().position(|c| c.id == *id))
        else {
            self.props.first = None;
            return;
        };
        let in_boxes =
            self.drawing.as_ref().and_then(|(dr, _)| dr.boxes.iter().position(|(b, _)| b == id));
        self.props.first = Some((d.version, at, in_boxes));
    }

    /// Number `n`'s value now.
    fn number(&self, n: Number) -> Option<f32> {
        let page = self.diagram.as_ref()?.doc.pages.get(self.page)?;
        if n == Number::GridSize {
            return Some(page.grid_size() as f32);
        }
        let cell = self.first_chosen()?;
        if let Some(key) = n.style_key() {
            let default = if n == Number::FontSize { 12.0 } else { 1.0 };
            return Some(cell.style.number(key).unwrap_or(default) as f32);
        }
        let r = self.first_box()?;
        Some(match n {
            Number::X => r.min_x(),
            Number::Y => r.min_y(),
            Number::W => r.size.width,
            _ => r.size.height,
        })
    }

    /// Number `n` set to `value` (one step).
    fn set_number(&mut self, n: Number, value: f32) -> bool {
        let value = value.max(n.min());
        let shown = |v: f32| diagram_model::number(f64::from(v));
        match n {
            Number::GridSize => self.page_setting("gridSize", Some(&shown(value))),
            Number::LineWidth | Number::FontSize => {
                let key = n.style_key().unwrap_or_default();
                self.style_chosen(key, Some(&shown(value)))
            }
            Number::X | Number::Y | Number::W | Number::H => {
                let Some(cell) = self.first_chosen().map(|c| c.id.clone()) else { return false };
                let Some(r) = self.first_box() else { return false };
                let (mut x, mut y, mut w, mut h) =
                    (r.min_x(), r.min_y(), r.size.width, r.size.height);
                match n {
                    Number::X => x = value,
                    Number::Y => y = value,
                    Number::W => w = value,
                    _ => h = value,
                }
                self.resize(&cell, r, Rect::new(x, y, w, h))
            }
        }
    }

    /// Style `key` of the chosen cells set (`None`: taken away), one step.
    fn style_chosen(&mut self, key: &str, value: Option<&str>) -> bool {
        if self.locked || self.selection.is_empty() {
            return false;
        }
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let ids: Vec<&str> = self.selection.iter().map(String::as_str).collect();
        let tx = diagram_model::ops::set_style(&d.doc, page, &ids, key, value);
        d.edit(tx, None)
    }

    /// Page setting `name` set (one step).
    fn page_setting(&mut self, name: &str, value: Option<&str>) -> bool {
        if self.locked {
            return false;
        }
        let page = self.page;
        let Some(d) = self.diagram.as_mut() else { return false };
        let tx = diagram_model::ops::set_page_setting(&d.doc, page, name, value);
        d.edit(tx, None)
    }

    /// A press on the panel. `true`: it was on it.
    pub(super) fn press_properties(&mut self, p: Point) -> bool {
        if !self.properties_area().contains(p) {
            self.end_typing(false);
            return false;
        }
        let Some(&(_, prop)) = self.props.hits.iter().find(|(r, _)| r.contains(p)) else {
            return true;
        };
        self.end_typing(false);
        if self.locked {
            return true;
        }
        match prop {
            Prop::Grid => {
                let on = self
                    .diagram
                    .as_ref()
                    .and_then(|d| d.doc.pages.get(self.page))
                    .is_some_and(diagram_model::Page::grid);
                self.page_setting("grid", Some(if on { "0" } else { "1" }));
            }
            Prop::Swatch(key, swatch) => {
                let value = match swatch {
                    Swatch::Default => None,
                    Swatch::None => Some("none".to_owned()),
                    Swatch::Color(c) => Some(hex(c)),
                };
                if key == "background" {
                    self.page_setting(key, value.as_deref());
                } else {
                    self.style_chosen(key, value.as_deref());
                }
            }
            Prop::Flag(key) => {
                let on = self.first_chosen().is_some_and(|c| c.style.flag(key));
                self.style_chosen(key, if on { None } else { Some("1") });
            }
            Prop::FontBit(bit) => {
                let style =
                    self.first_chosen().and_then(|c| c.style.number("fontStyle")).unwrap_or(0.0)
                        as u32;
                let next = style ^ bit;
                let value = (next != 0).then(|| next.to_string());
                self.style_chosen("fontStyle", value.as_deref());
            }
            Prop::Align(a) => {
                self.style_chosen("align", Some(a));
            }
            Prop::Step(n, sign) => {
                if let Some(v) = self.number(n) {
                    self.set_number(n, v + sign * n.step());
                }
            }
            Prop::Type(n) => {
                let shown =
                    self.number(n).map(|v| diagram_model::number(f64::from(v))).unwrap_or_default();
                let mut input = TextInput::new(&shown);
                input.select_all();
                self.props.typing = Some((n, input));
            }
        }
        true
    }

    /// The number typed ended: set (`set`) or let go.
    pub(super) fn end_typing(&mut self, set: bool) -> bool {
        let Some((n, input)) = self.props.typing.take() else { return false };
        if set
            && let Ok(v) = input.text().trim().replace(',', ".").parse::<f32>()
            && v.is_finite()
        {
            self.set_number(n, v);
        }
        true
    }

    /// The panel drawn.
    pub(super) fn paint_properties(&mut self, ctx: &mut ui_components::ViewerPaint<'_>) {
        let area = self.properties_area();
        if area.size.width <= 0.0 {
            return;
        }
        self.find_first();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.sidebar_background));
        ds::divider_vertical(layer, &ds, area.min_x(), area.min_y(), area.max_y());
        self.props.hits.clear();
        let label_font = ds.ui_font(-1.0);
        let head_font = ds.ui_font(-1.5).with_weight(text_layout::Font::SEMIBOLD);
        let (x0, control_x) = (area.min_x() + px(14.0), area.min_x() + px(96.0));
        let control_w = (area.max_x() - px(12.0) - control_x).max(px(40.0));
        let mut y = area.min_y() + px(10.0);
        let mut hits: Vec<(Rect, Prop)> = Vec::new();
        let heading = |layer: &mut render_api::Layer,
                       text: &mut text_layout::TextSystem,
                       y: &mut f32,
                       key: &str| {
            let shaped = text.shape_line(&t(key).to_uppercase(), &head_font);
            draw_text(layer, &shaped, x0, (*y + px(14.0)).round(), p.text_muted);
            *y += px(24.0);
        };
        let label = |layer: &mut render_api::Layer,
                     text: &mut text_layout::TextSystem,
                     y: f32,
                     key: &str| {
            let shaped = text.shape_line(&t(key), &label_font);
            draw_text(
                layer,
                &shaped,
                x0,
                (y + px(ROW_H) / 2.0 + label_font.size * 0.35).round(),
                p.text,
            );
        };
        let typing = self.props.typing.as_ref().map(|(n, _)| *n);
        let number_row = |view: &DiagramView,
                          layer: &mut render_api::Layer,
                          text: &mut text_layout::TextSystem,
                          y: &mut f32,
                          key: &str,
                          n: Number,
                          hits: &mut Vec<(Rect, Prop)>| {
            label(layer, text, *y, key);
            let rect =
                Rect::new(control_x, *y + px(4.0), control_w.min(px(120.0)), px(ROW_H - 8.0));
            let shown = view
                .number(n)
                .map(|v| diagram_model::number(f64::from(v.round_to_tenth())))
                .unwrap_or_default();
            let parts = ds::number_field(layer, text, &ds, rect, &shown, typing == Some(n));
            if let Some((_, input)) = view.props.typing.as_ref().filter(|(t, _)| *t == n) {
                let style = ui_kit::TextInputStyle {
                    font: label_font.clone(),
                    background: Rgba::TRANSPARENT,
                    border: Rgba::TRANSPARENT,
                    border_focused: Rgba::TRANSPARENT,
                    text: p.text,
                    placeholder: p.text_muted,
                    selection: p.selection,
                    caret: p.caret,
                    radius: 0.0,
                    border_width: 0.0,
                    padding: px(4.0),
                    caret_width: ctx.scale.max(1.0),
                    lead: 0.0,
                    trail: 0.0,
                };
                input.paint(layer, text, parts.value, true, "", &style);
            }
            hits.push((parts.minus, Prop::Step(n, -1.0)));
            hits.push((parts.plus, Prop::Step(n, 1.0)));
            hits.push((parts.value, Prop::Type(n)));
            *y += px(ROW_H);
        };
        let swatch_row = |layer: &mut render_api::Layer,
                          text: &mut text_layout::TextSystem,
                          y: &mut f32,
                          key: &str,
                          style_key: &'static str,
                          swatches: &[Swatch],
                          value: Option<&str>,
                          hits: &mut Vec<(Rect, Prop)>| {
            let shaped = text.shape_line(&t(key), &label_font);
            draw_text(layer, &shaped, x0, (*y + px(14.0)).round(), p.text);
            let areas =
                ds::swatch_row(layer, &ds, (x0, *y + px(32.0)), swatches, chosen(swatches, value));
            hits.extend(
                areas.into_iter().zip(swatches).map(|(r, s)| (r, Prop::Swatch(style_key, *s))),
            );
            *y += px(48.0);
        };
        let switch_row = |layer: &mut render_api::Layer,
                          text: &mut text_layout::TextSystem,
                          y: &mut f32,
                          key: &str,
                          on: bool,
                          prop: Prop,
                          hits: &mut Vec<(Rect, Prop)>| {
            label(layer, text, *y, key);
            let r = ds::switch(layer, &ds, control_x, *y + px(ROW_H) / 2.0, on, false);
            hits.push((r, prop));
            *y += px(ROW_H);
        };
        let page = self.diagram.as_ref().and_then(|d| d.doc.pages.get(self.page));
        if self.selection.is_empty() {
            heading(layer, ctx.text, &mut y, "diagram_editor.props_diagram");
            let (grid, background) =
                page.map_or((true, None), |pg| (pg.grid(), pg.background().map(str::to_owned)));
            switch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_grid",
                grid,
                Prop::Grid,
                &mut hits,
            );
            number_row(
                self,
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_grid_size",
                Number::GridSize,
                &mut hits,
            );
            swatch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_background",
                "background",
                &FILLS,
                background.as_deref(),
                &mut hits,
            );
        } else if let Some(cell) = self.first_chosen().cloned() {
            let vertex = cell.kind == diagram_model::CellKind::Vertex;
            let style = &cell.style;
            heading(layer, ctx.text, &mut y, "diagram_editor.props_style");
            if vertex {
                swatch_row(
                    layer,
                    ctx.text,
                    &mut y,
                    "diagram_editor.props_fill",
                    "fillColor",
                    &FILLS,
                    style.get("fillColor"),
                    &mut hits,
                );
            }
            swatch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_line",
                "strokeColor",
                &LINES,
                style.get("strokeColor"),
                &mut hits,
            );
            number_row(
                self,
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_line_width",
                Number::LineWidth,
                &mut hits,
            );
            switch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_dashed",
                style.flag("dashed"),
                Prop::Flag("dashed"),
                &mut hits,
            );
            if vertex {
                switch_row(
                    layer,
                    ctx.text,
                    &mut y,
                    "diagram_editor.props_rounded",
                    style.flag("rounded"),
                    Prop::Flag("rounded"),
                    &mut hits,
                );
            }
            heading(layer, ctx.text, &mut y, "diagram_editor.props_text");
            number_row(
                self,
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_font_size",
                Number::FontSize,
                &mut hits,
            );
            let bits = style.number("fontStyle").unwrap_or(0.0) as u32;
            switch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_bold",
                bits & 1 != 0,
                Prop::FontBit(1),
                &mut hits,
            );
            switch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_italic",
                bits & 2 != 0,
                Prop::FontBit(2),
                &mut hits,
            );
            label(layer, ctx.text, y, "diagram_editor.props_align");
            let labels = [
                t("diagram_editor.align_left"),
                t("diagram_editor.align_center"),
                t("diagram_editor.align_right"),
            ];
            let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
            let current = match style.get("align") {
                Some("left") => 0,
                Some("right") => 2,
                _ => 1,
            };
            let right = area.max_x() - px(12.0);
            ds::segmented_with(
                layer,
                ctx.text,
                &ds,
                ds::SegmentedSpec {
                    right,
                    mid: y + px(ROW_H) / 2.0,
                    labels: &refs,
                    selected: current,
                    hovered: None,
                },
                |i, r| {
                    hits.push((r, Prop::Align(["left", "center", "right"][i])));
                },
            );
            y += px(ROW_H + 6.0);
            swatch_row(
                layer,
                ctx.text,
                &mut y,
                "diagram_editor.props_font_color",
                "fontColor",
                &LINES,
                style.get("fontColor"),
                &mut hits,
            );
            if vertex && self.selection.len() == 1 {
                heading(layer, ctx.text, &mut y, "diagram_editor.props_arrange");
                for (key, n) in [
                    ("diagram_editor.props_x", Number::X),
                    ("diagram_editor.props_y", Number::Y),
                    ("diagram_editor.props_w", Number::W),
                    ("diagram_editor.props_h", Number::H),
                ] {
                    number_row(self, layer, ctx.text, &mut y, key, n, &mut hits);
                }
            }
        }
        self.props.hits = hits;
    }
}

/// A number rounded to a tenth (what the panel shows).
trait Tenth {
    fn round_to_tenth(self) -> f32;
}

impl Tenth for f32 {
    fn round_to_tenth(self) -> f32 {
        (self * 10.0).round() / 10.0
    }
}

#[cfg(test)]
impl DiagramView {
    /// Where `prop` is on the panel (its middle).
    pub(super) fn prop_for_tests(&self, prop: Prop) -> Option<Point> {
        self.props.hits.iter().find(|(_, p)| *p == prop).map(|(r, _)| r.center())
    }
}

/// A number typed in the panel as a control: a press never reaches it (its value clicked
/// starts it again, the panel's); while typed it has the keys (Tab included), the typing
/// (one line) and the edit commands; ↵ asks the view to set it, Esc to let it go.
pub(super) struct TypingPart<'a> {
    pub(super) typing: Option<&'a mut TextInput>,
    pub(super) asked: &'a mut Option<Asked>,
}

impl Control for TypingPart<'_> {
    fn focusable(&self) -> bool {
        self.typing.is_some()
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let Some(input) = self.typing.as_mut() else { return false };
        match key {
            ViewerKey::Enter | ViewerKey::Escape => {
                *self.asked = Some(Asked::EndTyping(key == ViewerKey::Enter));
                true
            }
            _ => Control::key(&mut **input, key, reach),
        }
    }

    fn text(&mut self, typed: &str) -> bool {
        let Some(input) = self.typing.as_mut() else { return false };
        input.insert(typed.lines().next().unwrap_or_default());
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.typing.as_ref().and_then(|i| Control::selected_text(&**i))
    }

    fn command(&mut self, id: &str) -> bool {
        self.typing.as_mut().is_some_and(|i| Control::command(&mut **i, id))
    }
}
