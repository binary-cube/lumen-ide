//! Drawing the grid: the toolbar (pages, size, refresh, transposed, find, the data
//! extractor's pill), the `WHERE` / `ORDER BY` fields, the headers and the rows in view (or
//! one row transposed), the bottom bar (the rows, what the selection adds up to), the page
//! sizes' popover and the extractors'.

use kernel::{Point, Rect, Rgba};
use render_api::{GlyphInstance, Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::{TextInputStyle, draw_text};

use super::grid::{
    BOTTOM_H, CELL_CHARS, CELL_PAD, DataGrid, FILTER_H, FIND_H, Field, GridPaint, HEADER_H, Hit,
    Mark, PopRow, ROW_H, TOOLBAR_H, horizontal_bar, tool_hit,
};
use super::layout::MIN_WIDTH;
use super::selection::CellPos;
use super::source::{Align, CellMark, CellRef, GridSource, RowState, SourceStatus, Total};
use super::toolbar::{self, Metrics, Tool, ToolbarSpec};
use crate::ds::{self, Ctx, radius};
use crate::scrollbar::ScrollMetrics;

mod bars;
mod popups;
mod table;

/// Narrower than this (logical pixels, the fields' room), `WHERE` goes over `ORDER BY`.
const STACK_FILTERS_BELOW: f32 = 440.0;
/// How much of a row the stacked fields share (they're closer than two rows apart).
const FILTER_OVERLAP: f32 = 8.0;

/// `n` with its thousands grouped as the language writes them.
pub fn grouped(n: u64) -> String {
    let sep = if i18n::locale().starts_with("ro") { '.' } else { ',' };
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// A cell's text as drawn: its first line, cut to [`CELL_CHARS`] characters.
fn cell_line(buffer: &mut String, value: &CellRef<'_>) {
    buffer.clear();
    match value {
        // Text up to 64 KB: only the part drawn is copied (per visible cell, per frame).
        CellRef::Text(t) | CellRef::Json(t) | CellRef::Other(t) => {
            let cut = first_line_cut(t);
            buffer.push_str(&t[..cut]);
            if cut < t.len() {
                buffer.push_str(" …");
            }
        }
        _ => {
            value.write_display(buffer);
            let cut = first_line_cut(buffer);
            if cut < buffer.len() {
                buffer.truncate(cut);
                buffer.push_str(" …");
            }
        }
    }
}

/// Where a cell's text is cut as drawn: its first line, at most [`CELL_CHARS`] characters.
fn first_line_cut(text: &str) -> usize {
    let cut = text.char_indices().nth(CELL_CHARS).map_or(text.len(), |(at, _)| at);
    text[..cut].find(['\n', '\r']).unwrap_or(cut)
}

/// `value` drawn in `room` (from `x`, on `baseline`), cut with "…" when it doesn't fit —
/// shaped once (the text system keeps it), the glyphs that fit kept.
fn draw_fitted(
    layer: &mut Layer,
    text: &mut TextSystem,
    value: &str,
    font: &Font,
    (x, room, baseline): (f32, f32, f32),
    color: Rgba,
    align: Align,
) {
    let shaped = text.shape_line(value, font);
    if shaped.width <= room {
        let x = match align {
            Align::Left => x,
            Align::Right => x + room - shaped.width,
            Align::Center => x + (room - shaped.width) / 2.0,
        };
        draw_text(layer, &shaped, x.round(), baseline, color);
        return;
    }
    let dots = text.shape_line("…", font);
    let limit = room - dots.width;
    let mut end = 0.0;
    for g in shaped.glyphs.iter().take_while(|g| g.x + g.advance <= limit) {
        layer.glyphs.push(GlyphInstance {
            key: g.key,
            origin: Point::new(x.round() + g.x, baseline + g.y),
            color,
        });
        end = g.x + g.advance;
    }
    draw_text(layer, &dots, (x + end).round(), baseline, color);
}

/// The size of the grid's icon buttons (its toolbar, its find bar, the record's arrows).
const ICON_BUTTON: f32 = ds::button_size::LARGE;

/// A ghost icon button of the grid at `(x, mid)`, drawn as `spec` says. Returns its area.
fn grid_button(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    at: (f32, f32),
    spec: ds::ButtonSpec<'_>,
) -> Rect {
    let r = ds::square_button(ds, at, ICON_BUTTON);
    ds::button(layer, text, icons, ds, r, spec);
    r
}

fn field_style(ds: &Ctx<'_>, error: bool) -> TextInputStyle {
    let p = ds.palette;
    TextInputStyle {
        font: ds.code_font(-1.0),
        background: p.input_background,
        border: if error { p.danger.with_alpha(0.7) } else { p.input_border },
        border_focused: if error { p.danger } else { p.accent },
        text: p.text,
        placeholder: p.text_muted.with_alpha(0.7),
        selection: p.selection,
        caret: p.caret,
        radius: ds.px(radius::MD),
        border_width: ds.line(),
        padding: ds.px(10.0),
        caret_width: ds.px(1.5).max(1.0),
        lead: 0.0,
        trail: 0.0,
    }
}

impl<S: GridSource> DataGrid<S> {
    /// Draws the grid in `ctx.bounds`.
    pub fn paint(&mut self, ctx: GridPaint<'_>) {
        let GridPaint { bounds: b, scale, text, scene, palette, icons } = ctx;
        let ds = Ctx::new(palette, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        self.marks.clear();
        self.geometry.bounds = b;
        self.geometry.scale = scale;
        self.geometry.row_h = px(ROW_H);
        self.sync_new_rows();
        self.refresh_find();

        let layer = scene.push_layer(Some(b));
        let mut y = b.min_y();
        let toolbar = Rect::new(b.min_x(), y, b.size.width, px(TOOLBAR_H));
        self.paint_toolbar(layer, text, icons, &ds, toolbar);
        y = toolbar.max_y();
        ds::divider(layer, &ds, b.min_x(), b.max_x(), y);
        let caps = self.source.capabilities();
        if caps.sort || caps.filter || caps.limit.is_some() {
            // Narrow: `WHERE` over `ORDER BY`, each the whole width.
            let h = if self.filters_stacked(&ds, b.size.width) {
                2.0 * px(FILTER_H) - px(FILTER_OVERLAP)
            } else {
                px(FILTER_H)
            };
            let row = Rect::new(b.min_x(), y + ds.line(), b.size.width, h);
            self.paint_filters(layer, text, icons, &ds, row);
            y = row.max_y();
            ds::divider(layer, &ds, b.min_x(), b.max_x(), y);
        }
        if self.find.open {
            let row = Rect::new(b.min_x(), y + ds.line(), b.size.width, px(FIND_H));
            self.paint_find(layer, text, icons, &ds, row);
            y = row.max_y();
            ds::divider(layer, &ds, b.min_x(), b.max_x(), y);
        }
        let bottom = Rect::new(b.min_x(), b.max_y() - px(BOTTOM_H), b.size.width, px(BOTTOM_H));
        let area = Rect::new(
            b.min_x(),
            y + ds.line(),
            b.size.width,
            (bottom.min_y() - y - ds.line()).max(0.0),
        );
        if self.source.columns().is_empty() {
            self.geometry.header = Rect::default();
            self.geometry.body = Rect::default();
            self.paint_without_columns(layer, text, &ds, area);
        } else if self.transpose {
            self.paint_record(scene, text, icons, &ds, area);
        } else {
            self.paint_table(scene, text, icons, &ds, area);
        }
        let layer = scene.push_layer(Some(b));
        self.paint_bottom(layer, text, &ds, bottom);
        if self.big.is_some() {
            self.paint_big(scene, text, &ds, area);
        }
        if self.field_tip.is_some() && self.big.is_none() {
            let layer = scene.push_layer(None);
            self.paint_field_tip(layer, text, icons, &ds);
        }
        if self.extract_open {
            self.paint_extractors(scene, text, icons, &ds);
        } else {
            self.extract_area = None;
        }
    }

    /// No columns yet: still reading the structure, or it failed.
    fn paint_without_columns(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        match self.source.status() {
            SourceStatus::Failed(why) => {
                ds::empty_state(layer, text, ds, area, &i18n::t("grid.failed"), Some(&why));
            }
            _ => {
                let label = i18n::t("grid.loading");
                let loader = ds::Loader::with_text(&label);
                let w = loader.width(text, ds);
                let mid = area.min_y() + area.size.height / 2.0;
                ds::loader(
                    layer,
                    text,
                    ds,
                    area.min_x() + (area.size.width - w) / 2.0,
                    mid,
                    &loader,
                );
            }
        }
    }

    // ─── Toolbar ───

    // ─── WHERE / ORDER BY ───

    // ─── Find ───

    // ─── The rows ───

    // ─── Bottom bar ───

    // ─── The large editor ───

    // ─── The data extractors' popover ───
}

/// Tool `tool`'s icon (none: a text).
pub(super) fn tool_icon(tool: Tool) -> Option<&'static str> {
    Some(match tool {
        Tool::First => "arrow-left-to-line",
        Tool::Back => "chevron-left",
        Tool::Forward => "chevron-right",
        Tool::Last => "arrow-right-to-line",
        Tool::PageSize => "layers",
        Tool::Cancel => "x",
        Tool::Add => "plus",
        Tool::Delete => "trash",
        Tool::Revert => "undo",
        Tool::Refresh => "refresh",
        Tool::Find => "search",
        Tool::Transpose => "transpose",
        Tool::More => "more-horizontal",
        Tool::Extractor => "copy",
        Tool::Range | Tool::Of | Tool::Loader | Tool::Submit => return None,
    })
}

/// Tool `tool`'s name (its tooltip, its row in the "…" menu).
pub(super) fn tool_label<S: GridSource>(grid: &DataGrid<S>, tool: Tool) -> String {
    let key = match tool {
        Tool::First => "grid.tip_first",
        Tool::Back => "grid.tip_back",
        Tool::Forward => "grid.tip_forward",
        Tool::Last => "grid.tip_last",
        Tool::PageSize => {
            return match grid.paging.size {
                Some(n) => {
                    i18n::tr!("grid.tip_page_size", size = i18n::tr!("grid.page_size", count = n))
                }
                None => i18n::tr!("grid.tip_page_size", size = i18n::t("grid.page_all")),
            };
        }
        Tool::Cancel => "grid.tip_cancel",
        Tool::Add => "grid.add_row",
        Tool::Delete => "grid.delete_rows",
        Tool::Revert => "grid.revert_all",
        Tool::Submit => "grid.tip_submit",
        Tool::Refresh => "grid.refresh",
        Tool::Find => "grid.tip_find",
        Tool::Transpose => {
            if grid.transpose {
                "grid.untranspose"
            } else {
                "grid.transpose"
            }
        }
        Tool::More => "grid.tip_more",
        Tool::Extractor => {
            return i18n::tr!("grid.more_extractor", name = grid.extractor().name());
        }
        Tool::Range | Tool::Of | Tool::Loader => return String::new(),
    };
    i18n::t(key)
}

#[cfg(test)]
#[path = "../../tests/unit/data_grid/paint.rs"]
mod tests;
