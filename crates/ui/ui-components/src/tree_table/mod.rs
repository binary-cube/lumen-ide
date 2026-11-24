//! A tree laid out on columns ([`TreeTable`]): rows with a depth, opened and folded, the
//! first column the tree itself (the indentation guides, the chevron, an icon, its text in
//! pieces of their own weight), the others cells of their kind — text, a number with its
//! unit, a bar for a share, a bar in two parts, a badge, an icon — under a header whose
//! columns are resized by dragging their edge. One row is chosen (the keys move it, open and
//! fold), a row may be marked (an accent stripe and its text stronger: a path through the
//! tree), carry a sign in a gutter of its own and a tint, and each cell its tooltip.
//!
//! What a query's plan shows (the console's Plan tab) and, later, its profile; any view of a
//! tree with figures may use it. The rows shown (the open ones) are worked out once per
//! change of the rows or of what's open ([`kernel::Memo`]); a frame draws only the rows in
//! view ([`TreeList`]).

use icons::Icons;
use kernel::{Memo, Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{draw_icon, draw_text};

use crate::ds::{self, Ctx, Tone};
use crate::palette::Palette;
use crate::scrollbar::EdgeScrollbar;
use crate::severity::Level;
use crate::tooltip::Tip;
use crate::tree::{self, TreeList};
use crate::viewer::{ViewerCursor, ViewerKey};

mod cells;
mod input;
mod paint;

pub use cells::*;
pub use paint::number_text;
use paint::{NumberText, PILL_PAD};

/// The header's height (logical pixels).
pub const HEADER_H: f32 = 28.0;
/// A row's height (logical pixels).
pub const ROW_H: f32 = 26.0;
/// A level's indentation (logical pixels).
pub const INDENT: f32 = 16.0;
/// The gutter of signs, when a row has one (logical pixels).
pub const GUTTER: f32 = 20.0;

/// What [`TreeTable::paint`] draws with.
pub struct TablePaint<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
}

impl std::fmt::Debug for TablePaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TablePaint").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

/// What a press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableEvent {
    Selected(usize),
    /// A row opened or folded (its chevron, a double click).
    Toggled(usize),
}

/// Where a point falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    /// A column's title.
    Header(usize),
    /// The edge after a column (dragged: its width).
    Edge(usize),
    /// A row (its chevron or not), a cell of it.
    Row { row: usize, column: usize, chevron: bool },
}

/// A tree on columns (see the module's doc).
#[derive(Debug, Default)]
pub struct TreeTable {
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    depth: Vec<usize>,
    children: Vec<bool>,
    open: Vec<bool>,
    /// Bumped when the rows change, and when what's open does.
    generation: u64,
    open_generation: u64,
    shown: Memo<(u64, u64, u64), Vec<usize>>,
    /// The text the rows are filtered by (lower case; empty: none), and its generation.
    filter: String,
    filter_generation: u64,
    selected: Option<usize>,
    hover: Option<Hit>,
    scroll: f32,
    hscroll: f32,
    /// The selection is brought into view at the next paint.
    reveal: bool,
    bar: EdgeScrollbar,
    empty: String,
    // The geometry last drawn (physical pixels).
    scale: f32,
    header: Rect,
    body: Rect,
    row_h: f32,
    gutter: f32,
    /// Each column's left and width, before the horizontal scroll.
    xs: Vec<(f32, f32)>,
    content_w: f32,
    resizing: Option<(usize, f32, f32)>,
    /// The tree's column's width wanted, and what it was worked out for.
    tree_width: Option<((u64, u32), f32)>,
    /// The texts a number is written in, kept from frame to frame (nothing allocated once
    /// they've grown).
    numbers: NumberText,
    /// What the last press did, kept for its owner when it's a part of a form
    /// ([`Self::take_event`]).
    event: Option<TableEvent>,
}

impl TreeTable {
    /// What the last press did, told once (its owner holds it among its controls).
    pub fn take_event(&mut self) -> Option<TableEvent> {
        self.event.take()
    }

    pub fn new(columns: Vec<TableColumn>) -> Self {
        Self { columns, scale: 1.0, ..Self::default() }
    }

    /// The text shown when it has no rows.
    pub fn set_empty(&mut self, text: impl Into<String>) {
        self.empty = text.into();
    }

    /// Its header's rectangle last drawn (its line along the bottom).
    pub fn header_rect(&self) -> Rect {
        self.header
    }

    pub fn columns(&self) -> &[TableColumn] {
        &self.columns
    }

    /// The columns replaced (their widths as given).
    pub fn set_columns(&mut self, columns: Vec<TableColumn>) {
        self.columns = columns;
    }

    /// Column `i`'s width (logical pixels; the tree's is what's left).
    pub fn set_width(&mut self, i: usize, width: f32) {
        if let Some(c) = self.columns.get_mut(i) {
            c.width = width.max(c.min_width);
        }
    }

    /// Its rows replaced, `open(i)` saying which start open (the selection kept when it's
    /// still a row).
    pub fn set_rows(&mut self, rows: Vec<TableRow>, open: impl Fn(usize) -> bool) {
        let n = rows.len();
        let mut depth = vec![0; n];
        let mut children = vec![false; n];
        for i in 0..n {
            if let Some(parent) = rows[i].parent.filter(|&p| p < i) {
                depth[i] = depth[parent] + 1;
                children[parent] = true;
            }
        }
        self.open = (0..n).map(&open).collect();
        self.rows = rows;
        self.depth = depth;
        self.children = children;
        self.generation += 1;
        self.open_generation += 1;
        if self.selected.is_some_and(|s| s >= n) {
            self.selected = None;
        }
    }

    pub fn rows(&self) -> &[TableRow] {
        &self.rows
    }

    pub fn depth(&self, row: usize) -> usize {
        self.depth.get(row).copied().unwrap_or(0)
    }

    pub fn has_children(&self, row: usize) -> bool {
        self.children.get(row).copied().unwrap_or(false)
    }

    pub fn is_open(&self, row: usize) -> bool {
        self.open.get(row).copied().unwrap_or(false)
    }

    /// Changes with its rows, with what's open and with its filter (a view drawing it again
    /// then).
    pub fn generation(&self) -> u64 {
        self.generation + self.open_generation + self.filter_generation
    }

    /// Only the rows whose text has `text` (any case) shown, with their parents (opened or
    /// not); empty: every row as opened. `true` if it changed.
    pub fn set_filter(&mut self, text: &str) -> bool {
        let text = text.to_lowercase();
        if text == self.filter {
            return false;
        }
        self.filter = text;
        self.filter_generation += 1;
        self.scroll = 0.0;
        self.reveal = true;
        true
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Row `row` has the filter's text (every row does without one).
    pub fn matches(&self, row: usize) -> bool {
        self.filter.is_empty()
            || self.rows.get(row).is_some_and(|r| {
                r.cells.iter().any(|c| c.plain().to_lowercase().contains(&self.filter))
            })
    }

    /// The rows shown, in order (those whose parents are all open).
    pub fn visible(&mut self) -> Vec<usize> {
        self.shown_rows().to_vec()
    }

    fn shown_rows(&mut self) -> &[usize] {
        let key = (self.generation, self.open_generation, self.filter_generation);
        let (rows, open, filter) = (&self.rows, &self.open, &self.filter);
        self.shown.get_or_update(key, || {
            let mut out = Vec::with_capacity(rows.len());
            if !filter.is_empty() {
                // The rows matching and their parents, whatever is folded.
                let mut keep = vec![false; rows.len()];
                for i in (0..rows.len()).rev() {
                    let hit = rows[i]
                        .cells
                        .iter()
                        .any(|c| c.plain().to_lowercase().contains(filter.as_str()));
                    if hit {
                        keep[i] = true;
                    }
                    if keep[i]
                        && let Some(p) = rows[i].parent.filter(|&p| p < i)
                    {
                        keep[p] = true;
                    }
                }
                out.extend((0..rows.len()).filter(|&i| keep[i]));
                return out;
            }
            // A row is hidden when any of its parents is folded.
            let mut hidden = vec![false; rows.len()];
            for i in 0..rows.len() {
                if let Some(p) = rows[i].parent.filter(|&p| p < i) {
                    hidden[i] = hidden[p] || !open[p];
                }
                if !hidden[i] {
                    out.push(i);
                }
            }
            out
        })
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Row `row` chosen (`None`: none), its parents opened and brought into view.
    pub fn select(&mut self, row: Option<usize>) {
        let row = row.filter(|&r| r < self.rows.len());
        if let Some(r) = row {
            let mut at = self.rows[r].parent;
            while let Some(p) = at {
                if !self.open[p] {
                    self.open[p] = true;
                    self.open_generation += 1;
                }
                at = self.rows[p].parent;
            }
        }
        self.selected = row;
        self.reveal = true;
    }

    /// Row `row` opened or folded.
    pub fn set_open(&mut self, row: usize, open: bool) -> bool {
        if !self.has_children(row) || self.open[row] == open {
            return false;
        }
        self.open[row] = open;
        self.open_generation += 1;
        // A chosen row folded away: its parent chosen.
        if !open && let Some(s) = self.selected {
            let mut at = self.rows[s].parent;
            while let Some(p) = at {
                if p == row {
                    self.selected = Some(row);
                    break;
                }
                at = self.rows[p].parent;
            }
        }
        true
    }

    pub fn toggle(&mut self, row: usize) -> bool {
        let open = !self.is_open(row);
        self.set_open(row, open)
    }

    /// Every row opened (`true`) or folded.
    pub fn set_all_open(&mut self, open: bool) {
        for i in 0..self.rows.len() {
            if self.children[i] {
                self.open[i] = open;
            }
        }
        self.open_generation += 1;
        if !open && let Some(s) = self.selected {
            // The chosen row's top-level row stays chosen.
            let mut top = s;
            while let Some(p) = self.rows[top].parent {
                top = p;
            }
            self.selected = Some(top);
        }
    }

    /// Row `row`'s cells as text, between tabs (what's copied).
    pub fn row_text(&self, row: usize) -> Option<String> {
        let r = self.rows.get(row)?;
        Some(r.cells.iter().map(TableCell::plain).collect::<Vec<_>>().join("\t"))
    }

    /// The rows shown as text, indented two spaces a level, their cells between tabs.
    pub fn text(&mut self) -> String {
        let mut out = String::new();
        for i in self.visible() {
            out.push_str(&"  ".repeat(self.depth(i)));
            out.push_str(&self.row_text(i).unwrap_or_default());
            out.push('\n');
        }
        out
    }

    /// How wide the tree's column must be (logical pixels) for the rows shown to keep their
    /// words whole: the indentation, the chevron, the icon and the text (at most the first
    /// 2 000 rows shown are measured). Worked out once per change of the rows, what's open
    /// and the scale.
    pub fn tree_width(&mut self, text: &mut TextSystem, palette: &Palette, scale: f32) -> f32 {
        let key = (self.generation(), scale.to_bits());
        if let Some(&(k, w)) = self.tree_width.as_ref()
            && k == key
        {
            return w;
        }
        let ds = Ctx::new(palette, 1.0);
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let code = ds.code_font(-1.0);
        let small = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        let shown: Vec<usize> = self.shown_rows().iter().take(2_000).copied().collect();
        let mut widest: f32 = 0.0;
        for row in shown {
            let r = &self.rows[row];
            let Some(cell) = r.cells.first() else { continue };
            let mut w = 6.0 + INDENT * self.depth(row) as f32 + 18.0 + 8.0;
            if r.icon.is_some() {
                w += 20.0;
            }
            if cell.lead.is_some() {
                w += 18.0;
            }
            match &cell.value {
                CellValue::Text(spans) => {
                    for (i, span) in spans.iter().enumerate() {
                        let f = match span.style {
                            SpanStyle::Normal if r.marked && i == 0 => &strong,
                            SpanStyle::Normal | SpanStyle::Muted => &font,
                            SpanStyle::Strong | SpanStyle::Ink(_) => &strong,
                            SpanStyle::Code => &code,
                            SpanStyle::Pill(_) => &small,
                        };
                        let pad = if matches!(span.style, SpanStyle::Pill(_)) {
                            2.0 * PILL_PAD
                        } else {
                            0.0
                        };
                        w += text.shape_line(&span.text, f).width
                            + pad
                            + if i > 0 { 6.0 } else { 0.0 };
                    }
                }
                other => w += text.shape_line(&TableCell::of(other.clone()).plain(), &font).width,
            }
            widest = widest.max(w);
        }
        self.tree_width = Some((key, widest));
        widest
    }

    // ─── Geometry ───

    fn content_height(&self, rows: usize) -> f32 {
        rows as f32 * self.row_h
    }

    fn max_scroll(&self, rows: usize) -> f32 {
        (self.content_height(rows) - self.body.size.height).max(0.0)
    }

    fn max_hscroll(&self) -> f32 {
        (self.content_w - self.body.size.width).max(0.0)
    }

    /// The columns' lefts and widths for `width` (physical): the tree's the rest, never
    /// under its least; the others as set.
    fn lay_columns(&mut self, left: f32, width: f32) {
        let px = |v: f32| v * self.scale;
        let others: f32 = self.columns.iter().skip(1).map(|c| px(c.width.max(c.min_width))).sum();
        let tree_min = self.columns.first().map_or(0.0, |c| px(c.min_width.max(120.0)));
        let tree = (width - others).max(tree_min);
        let mut x = left;
        self.xs.clear();
        for (i, c) in self.columns.iter().enumerate() {
            let w = if i == 0 { tree } else { px(c.width.max(c.min_width)) };
            self.xs.push((x, w));
            x += w;
        }
        self.content_w = x - left;
    }

    /// The rows shown as last worked out (by a paint, a key): what the pointer finds.
    fn shown_cached(&self) -> &[usize] {
        self.shown.value().map_or(&[], Vec::as_slice)
    }

    // ─── Input ───

    // ─── Drawing ───
}

#[cfg(test)]
#[path = "../../tests/unit/tree_table.rs"]
mod tests;
