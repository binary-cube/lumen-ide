//! The look every table shares, whatever lists it (the data grid, the Task Manager's
//! parts): a header band with each column's name and, under it, a muted subtitle (its type,
//! its unit); short lines between the headers, one taller in the accent where an edge is
//! held or under the mouse (dragged to resize); thin lines between the columns down the
//! rows; rows alternating (every second one faintly tinted). The tables keep their own
//! structure (groups, trees, a row's details) and draw their cells; these are the parts
//! that must look the same in all of them.

use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};

use crate::ds::{self, Ctx, radius};

/// A header's height (logical pixels): its name's line and its subtitle's.
pub const HEADER_H: f32 = 44.0;
/// A header's name: its baseline from the header's top (logical pixels).
const NAME_BASELINE: f32 = 20.0;
/// Its subtitle's baseline.
const SUBTITLE_BASELINE: f32 = 36.0;
/// How far the lines between the headers stay from the band's edges (logical pixels), and
/// the one held or under the mouse.
const EDGE_INSET: f32 = 8.0;
const EDGE_INSET_LIT: f32 = 4.0;
/// A cell's padding on each side (logical pixels).
pub const CELL_PAD: f32 = 10.0;
/// A header under the mouse: the text's alpha it's lit with.
const HEADER_HOVER: f32 = 0.05;

/// A header's name's font (and what measures it).
pub fn name_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-0.5).with_weight(Font::SEMIBOLD)
}

/// A header's subtitle's font.
pub fn subtitle_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-2.0)
}

/// The header's band behind the names.
pub fn header_band(layer: &mut Layer, ctx: &Ctx<'_>, header: Rect) {
    layer.quad(Quad::filled(header, ctx.fade(ctx.palette.text.with_alpha(0.03))));
}

/// The header's band at the top of a table with rounded corners (`radius`, physical): its
/// top corners follow the table's, its bottom stays straight over the rows.
/// In layers of its own (each part clipped, nothing drawn twice: the fill is translucent),
/// pushed before the header's texts.
pub fn header_band_rounded(
    scene: &mut render_api::Scene,
    ctx: &Ctx<'_>,
    header: Rect,
    radius: f32,
) {
    let r = radius.min(header.size.height / 2.0);
    let fill = ctx.fade(ctx.palette.text.with_alpha(0.03));
    let corners = Rect::new(header.min_x(), header.min_y(), header.size.width, r);
    let round = Rect::new(header.min_x(), header.min_y(), header.size.width, 2.0 * r);
    scene.push_layer(Some(corners)).quad(Quad::filled(round, fill).rounded(r));
    let rest =
        Rect::new(header.min_x(), header.min_y() + r, header.size.width, header.size.height - r);
    scene.push_layer(Some(rest)).quad(Quad::filled(rest, fill));
}

/// The line under the header, across the table.
pub fn header_rule(layer: &mut Layer, ctx: &Ctx<'_>, header: Rect) {
    ds::divider(layer, ctx, header.min_x(), header.max_x(), header.max_y() - ctx.line());
}

/// A column's header, as drawn.
#[derive(Debug, Clone, Copy)]
pub struct HeaderCell<'a> {
    /// The column's area in the header band.
    pub rect: Rect,
    pub name: &'a str,
    /// Muted, under the name (its type, its unit); empty: none.
    pub subtitle: &'a str,
    /// Numbers are right-aligned (their names too, over them).
    pub right: bool,
    /// Under the mouse (a click sorts by it).
    pub hovered: bool,
    /// Sorted by it: descending or not (an arrow in the accent after its name).
    pub sorted: Option<bool>,
    /// Its name in full color (sorted by it, or every column alike); else muted.
    pub strong: bool,
    /// The room kept between its edges and its texts (physical pixels; [`CELL_PAD`] as a
    /// rule, a table's own gap where its cells use one).
    pub pad: f32,
}

impl<'a> HeaderCell<'a> {
    /// The header of `name` in `rect`, at the rule's padding (`ctx`'s [`CELL_PAD`]): no
    /// subtitle, left-aligned, muted, not sorted, not under the mouse.
    pub fn new(ctx: &Ctx<'_>, rect: Rect, name: &'a str) -> Self {
        Self {
            rect,
            name,
            subtitle: "",
            right: false,
            hovered: false,
            sorted: None,
            strong: false,
            pad: ctx.px(CELL_PAD),
        }
    }

    /// Muted, under the name (its type, its unit).
    pub fn subtitle(self, subtitle: &'a str) -> Self {
        Self { subtitle, ..self }
    }

    /// Numbers: right-aligned, the names too.
    pub fn right(self, right: bool) -> Self {
        Self { right, ..self }
    }

    pub fn hovered(self, hovered: bool) -> Self {
        Self { hovered, ..self }
    }

    /// Sorted by it: descending or not.
    pub fn sorted(self, sorted: Option<bool>) -> Self {
        Self { sorted, ..self }
    }

    /// Its name in full color.
    pub fn strong(self, strong: bool) -> Self {
        Self { strong, ..self }
    }

    /// Its own room between its edges and its texts (physical pixels).
    pub fn pad(self, pad: f32) -> Self {
        Self { pad, ..self }
    }
}

/// A column's header: its hover, its name (with the order's arrow) and its subtitle, each
/// cut with `…` to the column. The room on its left a caller drew in (a key's icon) is left
/// out with `start`.
pub fn header_cell(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    cell: &HeaderCell<'_>,
    start: f32,
) {
    let p = ctx.palette;
    let r = cell.rect;
    // To a screen reader: a column's header named by its name (its texts drawn in it).
    let _told = column_header(cell.name, r, None);
    if cell.hovered {
        header_hover(layer, ctx, r);
    }
    let pad = cell.pad;
    let left = r.min_x() + pad + start;
    let room = (r.max_x() - pad - left).max(0.0);
    let name_font = name_font(ctx);
    let base = name_baseline(ctx, r.min_y());
    let color = ctx.fade(if cell.strong { p.text } else { p.text_muted });
    let arrow = cell.sorted.map(|descending| if descending { "↓" } else { "↑" });
    let arrow_w = arrow.map_or(0.0, |a| text.shape_line(a, &name_font).width + ctx.px(3.0));
    let name_room = (room - arrow_w).max(0.0);
    let name_w = text.shape_line(cell.name, &name_font).width.min(name_room);
    let name_x = if cell.right { left + room - arrow_w - name_w } else { left };
    let (drawn, _) =
        ds::draw_ellipsized(layer, text, cell.name, &name_font, (name_x, base), name_room, color);
    if let Some(arrow) = arrow {
        let shaped = text.shape_line(arrow, &name_font);
        ui_kit::draw_text(layer, &shaped, name_x + drawn + ctx.px(3.0), base, ctx.fade(p.accent));
    }
    if !cell.subtitle.is_empty() {
        let font = subtitle_font(ctx);
        let base = subtitle_baseline(ctx, r.min_y());
        let w = text.shape_line(cell.subtitle, &font).width.min(room);
        let x = if cell.right { left + room - w } else { left };
        let muted = ctx.fade(p.text_muted);
        ds::draw_ellipsized(layer, text, cell.subtitle, &font, (x, base), room, muted);
    }
}

/// A column's header to a screen reader, named `name` (the texts drawn while it's held are
/// that name), its place among the columns (`column`: its cells say the same place, so a
/// cell is read with its header): for a table drawing its own headers too.
pub fn column_header(
    name: &str,
    rect: Rect,
    column: Option<usize>,
) -> Option<ui_kit::access::Opened> {
    ui_kit::access::open(|| {
        let header = ui_kit::access::Described::new(ui_kit::access::Role::ColumnHeader, name, rect);
        match column {
            Some(column) => header.index(column),
            None => header,
        }
    })
}

/// A column's header under the mouse (a click sorts by it, a tooltip tells what it is): a
/// faint rounded highlight inside its area, the same in every table.
pub fn header_hover(layer: &mut Layer, ctx: &Ctx<'_>, r: Rect) {
    let lit = Rect::new(
        r.min_x() + ctx.px(2.0),
        r.min_y() + ctx.px(3.0),
        (r.size.width - ctx.px(4.0)).max(0.0),
        (r.size.height - ctx.px(6.0)).max(0.0),
    );
    ds::RowHighlight::new(false, true)
        .radius(radius::SM)
        .hover(ds::HoverFill::Faint(HEADER_HOVER))
        .paint(layer, ctx, lit);
}

/// A header's name's baseline in a header band at `top` (physical pixels).
pub fn name_baseline(ctx: &Ctx<'_>, top: f32) -> f32 {
    (top + ctx.px(NAME_BASELINE)).round()
}

/// A header's subtitle's baseline in a header band at `top`.
pub fn subtitle_baseline(ctx: &Ctx<'_>, top: f32) -> f32 {
    (top + ctx.px(SUBTITLE_BASELINE)).round()
}

/// The line between two headers at `x`: short and faint; taller in the accent where the
/// edge is held or under the mouse (`lit`).
pub fn header_edge(layer: &mut Layer, ctx: &Ctx<'_>, x: f32, header: Rect, lit: bool) {
    let p = ctx.palette;
    let (inset, color) =
        if lit { (ctx.px(EDGE_INSET_LIT), p.accent) } else { (ctx.px(EDGE_INSET), p.border) };
    let line = Rect::new(
        (x - ctx.line() / 2.0).round(),
        header.min_y() + inset,
        ctx.line().max(1.0),
        (header.size.height - 2.0 * inset).max(0.0),
    );
    layer.quad(Quad::filled(line, ctx.fade(color)));
}

/// The lines between the columns down the rows, at `edges` (physical x), from `top` for
/// `height`, those outside `clip` left out.
pub fn column_lines(
    layer: &mut Layer,
    ctx: &Ctx<'_>,
    edges: impl IntoIterator<Item = f32>,
    top: f32,
    height: f32,
    clip: Rect,
) {
    let color = ctx.fade(ctx.palette.border.with_alpha(0.55));
    for x in edges {
        let x = x.round();
        if x >= clip.min_x() && x <= clip.max_x() && height > 0.0 {
            layer.quad(Quad::filled(Rect::new(x, top, ctx.line(), height), color));
        }
    }
}

/// The tint of the row at place `index` (every second one), behind its cells.
pub fn stripe(layer: &mut Layer, ctx: &Ctx<'_>, row: Rect, index: usize) {
    if index % 2 == 1 {
        layer.quad(Quad::filled(row, ctx.fade(stripe_color(ctx))));
    }
}

/// The alternate rows' tint.
pub fn stripe_color(ctx: &Ctx<'_>) -> Rgba {
    ctx.palette.text.with_alpha(0.018)
}

#[cfg(test)]
#[path = "../tests/unit/table.rs"]
mod tests;
