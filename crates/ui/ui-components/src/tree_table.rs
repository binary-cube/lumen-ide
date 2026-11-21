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

/// The header's height (logical pixels).
pub const HEADER_H: f32 = 28.0;
/// A row's height (logical pixels).
pub const ROW_H: f32 = 26.0;
/// A level's indentation (logical pixels).
pub const INDENT: f32 = 16.0;
/// The gutter of signs, when a row has one (logical pixels).
pub const GUTTER: f32 = 20.0;
/// A pill among a cell's words: its height, the room on each side of its label, and its
/// fill's alpha (of its ink; of the text when neutral).
const PILL_H: f32 = 17.0;
const PILL_PAD: f32 = 6.0;
const PILL_FILL: f32 = 0.16;
const PILL_NEUTRAL_FILL: f32 = 0.07;
/// A cell's bar: its thickness, its widest, the least room it shows in, the gap before its
/// label, its track's alpha and the least a fill shows (logical pixels).
const BAR_H: f32 = 6.0;
const BAR_MAX: f32 = 90.0;
const BAR_MIN_ROOM: f32 = 16.0;
const BAR_GAP: f32 = 8.0;
const BAR_TRACK: f32 = 0.07;
const BAR_MIN_FILL: f32 = 2.0;

/// Where a column's cells sit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

/// A column: its title (and what it is, in its tooltip), where its cells sit, its width.
#[derive(Debug, Clone, PartialEq)]
pub struct TableColumn {
    pub title: String,
    pub tip: Option<String>,
    pub align: Align,
    /// The narrowest it's drawn (logical pixels).
    pub min_width: f32,
    /// Its width (logical pixels); the first column (the tree) takes what the others leave.
    pub width: f32,
}

impl TableColumn {
    pub fn new(title: impl Into<String>, width: f32) -> Self {
        Self { title: title.into(), tip: None, align: Align::Left, min_width: 48.0, width }
    }

    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }

    pub fn with_tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    pub fn with_min(mut self, min: f32) -> Self {
        self.min_width = min;
        self
    }
}

/// A color as a row or a cell asks for it, worked out with the theme when drawn: a tone of
/// the design system, a severity's, or the heat of a share (per mille, 0–1000).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Tone(Tone),
    Severity(Level),
    Heat(u16),
}

impl Ink {
    /// The heat of share `t` (0..1).
    pub fn heat(t: f64) -> Self {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        Self::Heat((t * 1000.0).round() as u16)
    }

    pub fn color(self, p: &Palette) -> Rgba {
        match self {
            Ink::Tone(t) => t.color(p),
            Ink::Severity(l) => l.color(p),
            Ink::Heat(h) => p.heat(f32::from(h.min(1000)) / 1000.0),
        }
    }
}

impl From<Tone> for Ink {
    fn from(tone: Tone) -> Self {
        Ink::Tone(tone)
    }
}

impl From<Level> for Ink {
    fn from(level: Level) -> Self {
        Ink::Severity(level)
    }
}

/// How a piece of a cell's text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    Normal,
    Strong,
    Muted,
    /// In the code font (a condition, a name as written).
    Code,
    /// In a color of its own (a factor that's far off).
    Ink(Ink),
    /// Small, in a rounded pill of its color (`×120 ↑`, `+12 nodes`).
    Pill(Ink),
}

/// A piece of a cell's text.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    pub fn new(text: impl Into<String>, style: SpanStyle) -> Self {
        Self { text: text.into(), style }
    }
}

/// What a cell shows.
#[derive(Debug, Clone, PartialEq)]
pub enum CellValue {
    Empty,
    /// Text in pieces (`Seq Scan` strong, `on orders` normal, `(id = 1)` muted).
    Text(Vec<Span>),
    /// A number, written with its digits grouped and at most `decimals` after the point,
    /// and its unit after it, dimmed (`1 204 rows`, `0.42 ms`).
    Number {
        value: f64,
        decimals: u8,
        unit: Option<String>,
    },
    /// A share (`fraction` 0..1) as a bar in `ink`, its label beside it.
    Bar {
        fraction: f32,
        ink: Ink,
        label: String,
    },
    /// Two parts of one whole (`a` and `b`, 0..1 together), its label beside it.
    Split {
        a: f32,
        b: f32,
        label: String,
    },
    Badge {
        label: String,
        tone: Tone,
    },
    Icon {
        name: &'static str,
        tone: Tone,
    },
}

/// A cell: what it shows, and what its tooltip says (the exact value, what it means); an
/// icon before it, an outline around it (the figure a problem is about).
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub value: CellValue,
    pub tip: Option<String>,
    /// An icon before what it shows, in its color (a spill's disk).
    pub lead: Option<(&'static str, Ink)>,
    /// Outlined in this color (the figure a finding is about).
    pub outline: Option<Ink>,
}

impl TableCell {
    pub fn empty() -> Self {
        Self::of(CellValue::Empty)
    }

    /// A cell showing `value`.
    pub fn of(value: CellValue) -> Self {
        Self { value, tip: None, lead: None, outline: None }
    }

    /// A bar of `fraction` in `ink`, `label` beside it.
    pub fn bar(fraction: f32, ink: impl Into<Ink>, label: impl Into<String>) -> Self {
        Self::of(CellValue::Bar { fraction, ink: ink.into(), label: label.into() })
    }

    pub fn with_lead(mut self, icon: &'static str, ink: impl Into<Ink>) -> Self {
        self.lead = Some((icon, ink.into()));
        self
    }

    pub fn outlined(mut self, ink: Option<Ink>) -> Self {
        self.outline = ink;
        self
    }

    pub fn text(text: impl Into<String>) -> Self {
        Self::spans(vec![Span::new(text, SpanStyle::Normal)])
    }

    pub fn spans(spans: Vec<Span>) -> Self {
        Self::of(CellValue::Text(spans))
    }

    pub fn number(value: f64, decimals: u8, unit: Option<String>) -> Self {
        Self::of(CellValue::Number { value, decimals, unit })
    }

    pub fn with_tip(mut self, tip: impl Into<String>) -> Self {
        self.tip = Some(tip.into());
        self
    }

    /// Its text, as copied (a bar's label, a number as written).
    pub fn plain(&self) -> String {
        match &self.value {
            CellValue::Empty => String::new(),
            CellValue::Text(spans) => spans.iter().map(|s| s.text.as_str()).collect(),
            CellValue::Number { value, decimals, unit } => {
                let n = number_text(*value, *decimals);
                match unit {
                    Some(u) => format!("{n} {u}"),
                    None => n,
                }
            }
            CellValue::Bar { label, .. } | CellValue::Split { label, .. } => label.clone(),
            CellValue::Badge { label, .. } => label.clone(),
            CellValue::Icon { name, .. } => (*name).to_owned(),
        }
    }
}

/// A row: its parent (an earlier row: rows come parents first), its icon, its cells (the
/// first one the tree's), how it stands out.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub parent: Option<usize>,
    pub icon: Option<&'static str>,
    pub cells: Vec<TableCell>,
    /// On the path marked (an accent stripe at its left, its first piece of text stronger).
    pub marked: bool,
    /// A sign in the gutter (its icon, its color) and what its tooltip says.
    pub sign: Option<(&'static str, Ink)>,
    pub sign_tip: Option<String>,
    /// A light background in this color.
    pub tint: Option<Ink>,
    /// Said after its text while it's folded (`+1 240 nodes · 3 %`).
    pub folded_note: Option<String>,
}

impl TableRow {
    pub fn new(parent: Option<usize>, cells: Vec<TableCell>) -> Self {
        Self {
            parent,
            icon: None,
            cells,
            marked: false,
            sign: None,
            sign_tip: None,
            tint: None,
            folded_note: None,
        }
    }

    /// A sign in the gutter, its tooltip.
    pub fn with_sign(mut self, icon: &'static str, ink: impl Into<Ink>, tip: String) -> Self {
        self.sign = Some((icon, ink.into()));
        self.sign_tip = Some(tip);
        self
    }

    pub fn tinted(mut self, ink: Option<Ink>) -> Self {
        self.tint = ink;
        self
    }

    pub fn with_folded_note(mut self, note: Option<String>) -> Self {
        self.folded_note = note;
        self
    }

    pub fn with_icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn marked(mut self, marked: bool) -> Self {
        self.marked = marked;
        self
    }
}

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
}

impl TreeTable {
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

    fn hit(&self, p: Point) -> Option<Hit> {
        if self.header.contains(p) {
            // A column's left edge (the tree's width follows the others': theirs are dragged).
            let edge = self.scale * 4.0;
            for (i, &(x, _)) in self.xs.iter().enumerate().skip(1) {
                if (p.x - (x - self.hscroll)).abs() <= edge {
                    return Some(Hit::Edge(i));
                }
            }
            let column = self.column_at(p.x)?;
            return Some(Hit::Header(column));
        }
        if !self.body.contains(p) {
            return None;
        }
        let shown = self.shown_cached();
        let slot =
            TreeList::new(self.row_h, shown.len()).row_at(p.y - self.body.min_y() + self.scroll)?;
        let row = shown[slot];
        let column = self.column_at(p.x).unwrap_or(0);
        let chevron = column == 0 && self.has_children(row) && {
            let x0 = self.xs[0].0 - self.hscroll;
            let cx = x0 + self.scale * (6.0 + INDENT * self.depth(row) as f32);
            p.x >= cx - self.scale * 2.0 && p.x <= cx + self.scale * 16.0
        };
        Some(Hit::Row { row, column, chevron })
    }

    fn column_at(&self, x: f32) -> Option<usize> {
        self.xs
            .iter()
            .position(|&(l, w)| x >= l - self.hscroll && x < l + w - self.hscroll)
            .or_else(|| (!self.xs.is_empty() && x >= self.body.min_x()).then(|| self.xs.len() - 1))
    }

    /// The selection kept in view.
    fn bring_into_view(&mut self) {
        let Some(s) = self.selected else { return };
        let Some(slot) = self.shown_rows().iter().position(|&r| r == s) else { return };
        let top = slot as f32 * self.row_h;
        let height = self.body.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if top + self.row_h > self.scroll + height {
            self.scroll = top + self.row_h - height;
        }
    }

    // ─── Input ───

    /// A press at `p` (`clicks`: 2 opens or folds the row).
    pub fn press(&mut self, p: Point, clicks: u32) -> Option<TableEvent> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        match self.hit(p)? {
            Hit::Edge(i) => {
                let w = self.columns[i].width;
                self.resizing = Some((i, p.x, w));
                None
            }
            Hit::Header(_) => None,
            Hit::Row { row, chevron, .. } => {
                if chevron || (clicks >= 2 && self.has_children(row)) {
                    self.selected = Some(row);
                    self.toggle(row);
                    return Some(TableEvent::Toggled(row));
                }
                self.selected = Some(row);
                Some(TableEvent::Selected(row))
            }
        }
    }

    /// The mouse moved with its button held: a column's edge or the scrollbar dragged.
    pub fn drag(&mut self, p: Point) -> bool {
        if let Some((i, from, width)) = self.resizing {
            // Its left edge dragged: leftwards widens it.
            let w = width - (p.x - from) / self.scale.max(0.01);
            let before = self.columns[i].width;
            self.set_width(i, w);
            return self.columns[i].width != before;
        }
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        false
    }

    pub fn release(&mut self) -> bool {
        self.resizing.take().is_some() | self.bar.release()
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed || bar
    }

    /// The pointer left it.
    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    pub fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let shown = self.shown_rows().len();
        let (before, hbefore) = (self.scroll, self.hscroll);
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll(shown));
        self.hscroll = (self.hscroll + dx).clamp(0.0, self.max_hscroll());
        self.scroll != before || self.hscroll != hbefore
    }

    /// The keys of a tree: ↑ ↓, Home / End, PgUp / PgDn move the choice; → opens (or goes
    /// to the first child), ← folds (or goes to the parent), ↵ opens or folds.
    pub fn key(&mut self, key: ViewerKey) -> bool {
        let shown = self.visible();
        if shown.is_empty() {
            return false;
        }
        let at = self.selected.and_then(|s| shown.iter().position(|&r| r == s));
        let page = ((self.body.size.height / self.row_h.max(1.0)).floor() as usize).max(1);
        let last = shown.len() - 1;
        let to = |slot: usize| Some(shown[slot.min(last)]);
        let next = match key {
            ViewerKey::Down => at.map_or(to(0), |a| to(a + 1)),
            ViewerKey::Up => at.map_or(to(last), |a| to(a.saturating_sub(1))),
            ViewerKey::PageDown => at.map_or(to(0), |a| to(a + page)),
            ViewerKey::PageUp => at.map_or(to(0), |a| to(a.saturating_sub(page))),
            ViewerKey::Home => to(0),
            ViewerKey::End => to(last),
            ViewerKey::Right => {
                let Some(s) = self.selected else { return false };
                if self.has_children(s) && !self.is_open(s) {
                    self.set_open(s, true);
                    return true;
                }
                if self.has_children(s) { Some(s + 1) } else { return false }
            }
            ViewerKey::Left => {
                let Some(s) = self.selected else { return false };
                if self.has_children(s) && self.is_open(s) {
                    self.set_open(s, false);
                    return true;
                }
                match self.rows[s].parent {
                    Some(p) => Some(p),
                    None => return false,
                }
            }
            ViewerKey::Enter => {
                let Some(s) = self.selected else { return false };
                return self.toggle(s);
            }
            _ => return false,
        };
        let changed = next != self.selected;
        self.selected = next;
        self.reveal = true;
        changed
    }

    /// A typed key: `*` opens everything under the chosen row (everything, when none is).
    pub fn char_key(&mut self, c: char) -> bool {
        if c != '*' {
            return false;
        }
        match self.selected {
            None => self.set_all_open(true),
            Some(s) => {
                // Rows come parents first: a row is under `s` when its parent is.
                let mut under = vec![false; self.rows.len()];
                for i in s..self.rows.len() {
                    under[i] = i == s || self.rows[i].parent.is_some_and(|p| under[p]);
                    if under[i] && self.children[i] {
                        self.open[i] = true;
                    }
                }
                self.open_generation += 1;
            }
        }
        true
    }

    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Edge(_) => Some(ViewerCursor::ResizeHorizontal),
            Hit::Row { chevron: true, .. } => Some(ViewerCursor::Pointer),
            _ => None,
        }
    }

    /// The tooltip at `p`: a column's title says what it is; a cell, its own (else its text,
    /// when it's cut).
    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        // The gutter's sign: what it means.
        if self.gutter > 0.0
            && p.x < self.body.min_x() + self.gutter
            && let Some(Hit::Row { row, .. }) = self.hit(p)
        {
            let slot = self.shown_cached().iter().position(|&r| r == row)?;
            let r = Rect::new(
                self.body.min_x(),
                self.body.min_y() + slot as f32 * self.row_h - self.scroll,
                self.gutter,
                self.row_h,
            );
            return Some(Tip::new(r, self.rows.get(row)?.sign_tip.clone()?));
        }
        match self.hit(p)? {
            Hit::Header(i) => {
                let c = self.columns.get(i)?;
                let (x, w) = self.xs[i];
                let r =
                    Rect::new(x - self.hscroll, self.header.min_y(), w, self.header.size.height);
                Some(Tip::new(r, c.tip.clone()?))
            }
            Hit::Row { row, column, .. } => {
                let slot = self.shown_cached().iter().position(|&r| r == row)?;
                let cell = self.rows.get(row)?.cells.get(column)?;
                let (x, w) = self.xs[column];
                let r = Rect::new(
                    x - self.hscroll,
                    self.body.min_y() + slot as f32 * self.row_h - self.scroll,
                    w,
                    self.row_h,
                );
                let tip = cell.tip.clone().or_else(|| {
                    let plain = cell.plain();
                    (plain.chars().count() > 24).then_some(plain)
                })?;
                Some(Tip::new(r, tip))
            }
            Hit::Edge(_) => None,
        }
    }

    // ─── Drawing ───

    /// Draws it in `bounds`: the header, the rows in view, the scrollbar.
    pub fn paint(&mut self, ctx: TablePaint<'_>) {
        let TablePaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.scale = scale;
        self.row_h = px(ROW_H).round();
        self.gutter = if self.rows.iter().any(|r| r.sign.is_some()) { px(GUTTER) } else { 0.0 };
        self.header = Rect::new(b.min_x(), b.min_y(), b.size.width, px(HEADER_H).round());
        self.body = Rect::new(
            b.min_x(),
            self.header.max_y(),
            b.size.width,
            (b.size.height - self.header.size.height).max(0.0),
        );
        let base = scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background));
        self.lay_columns(b.min_x() + self.gutter, b.size.width - self.gutter);
        let shown = self.shown_rows().len();
        if self.reveal {
            self.reveal = false;
            self.bring_into_view();
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll(shown));
        self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let code = ds.code_font(-1.0);
        let small = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        // The header.
        {
            let layer = scene.push_layer(Some(self.header));
            layer.quad(Quad::filled(self.header, p.text.with_alpha(0.03)));
            let title_font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
            for (i, c) in self.columns.iter().enumerate() {
                let (x, w) = self.xs[i];
                let x = x - self.hscroll;
                let r = Rect::new(x, self.header.min_y(), w, self.header.size.height);
                if matches!(self.hover, Some(Hit::Header(h)) if h == i) && c.tip.is_some() {
                    layer.quad(Quad::filled(r, p.text.with_alpha(0.04)));
                }
                let room = w - px(16.0);
                let title = ds::ellipsize(text, &c.title, &title_font, room.max(0.0));
                let shaped = text.shape_line(&title, &title_font);
                let tx = match c.align {
                    Align::Left => x + px(if i == 0 { 10.0 } else { 8.0 }),
                    Align::Right => x + w - px(8.0) - shaped.width,
                };
                let base = ds::optical_baseline(r.min_y(), r.size.height, &title_font);
                draw_text(layer, &shaped, tx.round(), base, p.text_muted);
                if i + 1 < self.columns.len() {
                    let dragged = matches!(self.hover, Some(Hit::Edge(e)) if e == i + 1)
                        || self.resizing.is_some_and(|(e, ..)| e == i + 1);
                    let color = if dragged { p.accent } else { p.border };
                    let edge = (x + w).round() - ds.line();
                    layer.quad(Quad::filled(
                        Rect::new(edge, r.min_y() + px(6.0), ds.line(), r.size.height - px(12.0)),
                        color,
                    ));
                }
            }
            ds::divider(layer, &ds, b.min_x(), b.max_x(), self.header.max_y() - ds.line());
        }
        if self.rows.is_empty() {
            let area = Rect::new(
                self.body.min_x(),
                self.body.min_y(),
                self.body.size.width,
                (self.body.size.height * 0.8).max(0.0),
            );
            let base = scene.push_layer(Some(self.body));
            ds::empty_state(base, text, &ds, area, &self.empty, None);
            return;
        }
        // The rows in view.
        let range = TreeList::new(self.row_h, shown).visible(self.scroll, self.body.size.height);
        let rows: Vec<usize> = self.shown_rows()[range.clone()].to_vec();
        let layer = scene.push_layer(Some(self.body));
        let hovered_row = match self.hover {
            Some(Hit::Row { row, .. }) => Some(row),
            _ => None,
        };
        for (k, &row) in rows.iter().enumerate() {
            let slot = range.start + k;
            let y = (self.body.min_y() + slot as f32 * self.row_h - self.scroll).round();
            let full = Rect::new(b.min_x(), y, b.size.width, self.row_h);
            let r = &self.rows[row];
            if let Some(ink) = r.tint {
                layer.quad(Quad::filled(full, ink.color(p).with_alpha(0.08)));
            }
            if self.selected == Some(row) {
                layer.quad(Quad::filled(full, p.tree_selection));
            } else if hovered_row == Some(row) {
                layer.quad(Quad::filled(full, p.tree_hover));
            }
            if r.marked {
                layer.quad(Quad::filled(
                    Rect::new(b.min_x(), y, px(2.0).round(), self.row_h),
                    p.accent,
                ));
            }
            let mid = y + self.row_h / 2.0;
            if let Some((icon, ink)) = r.sign
                && let Some(id) = icons.id(icon)
            {
                let s = px(13.0);
                let ir = Rect::new(
                    (b.min_x() + (self.gutter - s) / 2.0).round(),
                    (mid - s / 2.0).round(),
                    s,
                    s,
                );
                draw_icon(layer, icons, id, ir, ink.color(p));
            }
            // Folded: what it hides said after its text.
            let folded = match &r.folded_note {
                Some(note) if self.children[row] && !self.open[row] => {
                    r.cells.first().map(|first| {
                        let mut cell = first.clone();
                        if let CellValue::Text(spans) = &mut cell.value {
                            spans.push(Span::new(
                                note.clone(),
                                SpanStyle::Pill(Ink::Tone(Tone::Neutral)),
                            ));
                        }
                        cell
                    })
                }
                _ => None,
            };
            for (column, cell) in r.cells.iter().enumerate().take(self.xs.len()) {
                let cell = match (&folded, column) {
                    (Some(f), 0) => f,
                    _ => cell,
                };
                let (x, w) = self.xs[column];
                let x = x - self.hscroll;
                let pad = px(8.0);
                let mut left = x + pad;
                let right = x + w - pad;
                if column == 0 {
                    let depth = self.depth(row);
                    let x_of = |level: usize| x + px(6.0 + INDENT * level as f32 + 7.0);
                    tree::guides(layer, 0..depth, x_of, (y, self.row_h), ds.line(), p.indent_guide);
                    left = x + px(6.0 + INDENT * depth as f32);
                    if self.children[row] {
                        let s = px(14.0);
                        let at = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                        tree::chevron(layer, icons, at, self.open[row], p.text_muted);
                    }
                    left += px(18.0);
                    if let Some(icon) = r.icon
                        && let Some(id) = icons.id(icon)
                    {
                        let s = px(14.0);
                        let ir = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                        let color = if r.marked { p.accent } else { p.text_muted };
                        draw_icon(layer, icons, id, ir, color);
                        left += s + px(6.0);
                    }
                }
                if let Some(ink) = cell.outline {
                    let inset = px(3.0).round();
                    let o = Rect::new(
                        (x + inset).round(),
                        y + inset,
                        (w - 2.0 * inset).max(0.0).round(),
                        self.row_h - 2.0 * inset,
                    );
                    layer.quad(
                        Quad::filled(o, ink.color(p).with_alpha(0.06))
                            .rounded(px(4.0))
                            .bordered(ds.line().max(px(1.0)), ink.color(p).with_alpha(0.85)),
                    );
                }
                if let Some((icon, ink)) = cell.lead
                    && let Some(id) = icons.id(icon)
                {
                    let s = px(13.0);
                    let ir = Rect::new(left.round(), (mid - s / 2.0).round(), s, s);
                    draw_icon(layer, icons, id, ir, ink.color(p));
                    left += s + px(5.0);
                }
                let room = (right - left).max(0.0);
                let align = self.columns[column].align;
                let cell_mid = mid;
                let fonts = Fonts { normal: &font, strong: &strong, code: &code, small: &small };
                paint_cell(
                    &mut crate::LayerPainter::new(layer, text, icons, ds),
                    CellPlace { left, room, mid: cell_mid, top: y, height: self.row_h, align },
                    cell,
                    &fonts,
                    r.marked && column == 0,
                );
            }
        }
        let content = self.content_height(shown);
        self.bar.paint(scene, p, scale, self.body, content, self.scroll);
    }
}

/// Where a cell is drawn.
#[derive(Clone, Copy)]
struct CellPlace {
    left: f32,
    room: f32,
    mid: f32,
    top: f32,
    height: f32,
    align: Align,
}

struct Fonts<'a> {
    normal: &'a Font,
    strong: &'a Font,
    code: &'a Font,
    small: &'a Font,
}

/// A pill's text color: a neutral one muted, others in their own color.
fn pill_color(ink: Ink, p: &Palette) -> Rgba {
    match ink {
        Ink::Tone(Tone::Neutral) => p.text_muted,
        ink => ink.color(p),
    }
}

/// A cell's bar's track before its label (starting at `label_x`): as wide as there's room,
/// at most [`BAR_MAX`]; none in too little room.
fn bar_track(ds: &ds::Ctx<'_>, at: &CellPlace, label_x: f32) -> Option<Rect> {
    let px = |v: f32| ds.px(v);
    let bar_w = (label_x - px(BAR_GAP) - at.left).min(px(BAR_MAX));
    if bar_w <= px(BAR_MIN_ROOM) {
        return None;
    }
    let h = px(BAR_H);
    Some(Rect::new(at.left.round(), (at.mid - h / 2.0).round(), bar_w.round(), h))
}

fn paint_cell(
    pen: &mut crate::LayerPainter<'_>,
    at: CellPlace,
    cell: &TableCell,
    fonts: &Fonts<'_>,
    marked: bool,
) {
    let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
    let p = ds.palette;
    let px = |v: f32| ds.px(v);
    let base_of = |font: &Font| ds::optical_baseline(at.top, at.height, font);
    match &cell.value {
        CellValue::Empty => {}
        CellValue::Text(spans) => {
            // Each piece after the one before, the last ones cut to the room.
            let mut pieces: Vec<(String, &Font, Rgba, SpanStyle)> = Vec::new();
            let mut used = 0.0;
            for (i, span) in spans.iter().enumerate() {
                let (font, color) = match span.style {
                    SpanStyle::Normal if marked && i == 0 => (fonts.strong, p.text),
                    SpanStyle::Normal => (fonts.normal, p.text),
                    SpanStyle::Strong => (fonts.strong, p.text),
                    SpanStyle::Muted => (fonts.normal, p.text_muted),
                    SpanStyle::Code => (fonts.code, p.text_muted),
                    SpanStyle::Ink(ink) => (fonts.strong, ink.color(p)),
                    SpanStyle::Pill(ink) => (fonts.small, pill_color(ink, p)),
                };
                let pill = matches!(span.style, SpanStyle::Pill(_));
                let gap = if i > 0 { px(6.0) } else { 0.0 };
                let pad = if pill { px(12.0) } else { 0.0 };
                let left = at.room - used - gap - pad;
                if left < px(12.0) {
                    break;
                }
                let shown = ds::ellipsize(text, &span.text, font, left);
                let w = text.shape_line(&shown, font).width + pad;
                pieces.push((shown, font, color, span.style));
                used += gap + w;
            }
            let mut x = match at.align {
                Align::Left => at.left,
                Align::Right => at.left + at.room - used,
            };
            for (i, (shown, font, color, style)) in pieces.iter().enumerate() {
                if i > 0 {
                    x += px(6.0);
                }
                let shaped = text.shape_line(shown, font);
                if let SpanStyle::Pill(ink) = style {
                    let fill = match ink {
                        Ink::Tone(Tone::Neutral) => p.text.with_alpha(PILL_NEUTRAL_FILL),
                        ink => ink.color(p).with_alpha(PILL_FILL),
                    };
                    ds::Badge::new(shown)
                        .font(font)
                        .color(*color)
                        .fill(fill)
                        .size(ds::BadgeSize::Custom(PILL_H))
                        .pad(PILL_PAD)
                        .baseline(ds::BadgeBaseline::At(base_of(font)))
                        .paint(layer, text, ds, x, at.mid);
                    x += shaped.width + px(2.0 * PILL_PAD);
                    continue;
                }
                draw_text(layer, &shaped, x.round(), base_of(font), *color);
                x += shaped.width;
            }
        }
        CellValue::Number { value, decimals, unit } => {
            let n = number_text(*value, *decimals);
            let shaped = text.shape_line(&n, fonts.normal);
            let unit_shaped = unit.as_ref().map(|u| text.shape_line(u, fonts.normal));
            let unit_w = unit_shaped.as_ref().map_or(0.0, |u| u.width + px(4.0));
            let w = shaped.width + unit_w;
            let x = match at.align {
                Align::Left => at.left,
                Align::Right => at.left + at.room - w,
            };
            let base = base_of(fonts.normal);
            draw_text(layer, &shaped, x.round(), base, p.text);
            if let Some(u) = unit_shaped {
                draw_text(layer, &u, (x + shaped.width + px(4.0)).round(), base, p.text_muted);
            }
        }
        CellValue::Bar { fraction, ink, label } => {
            let shaped = text.shape_line(label, fonts.normal);
            let label_x = at.left + at.room - shaped.width;
            draw_text(layer, &shaped, label_x.round(), base_of(fonts.normal), p.text);
            if let Some(track) = bar_track(ds, &at, label_x) {
                ds::ProgressBar::of(*fraction)
                    .color(ink.color(p))
                    .track_alpha(BAR_TRACK)
                    .min_fill(BAR_MIN_FILL)
                    .snapped()
                    .paint_bar(layer, ds, track);
            }
        }
        CellValue::Split { a, b, label } => {
            let shaped = text.shape_line(label, fonts.normal);
            let label_x = at.left + at.room - shaped.width;
            draw_text(layer, &shaped, label_x.round(), base_of(fonts.normal), p.text);
            if let Some(track) = bar_track(ds, &at, label_x) {
                let parts = [
                    ds::ProgressSegment::new(*a, p.accent),
                    ds::ProgressSegment::new(*b, p.warning),
                ];
                ds::ProgressBar::split(&parts)
                    .track_alpha(BAR_TRACK)
                    .snapped()
                    .paint_bar(layer, ds, track);
            }
        }
        CellValue::Badge { label, tone } => {
            let (x, align_right) = match at.align {
                Align::Left => (at.left, false),
                Align::Right => (at.left + at.room, true),
            };
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec { label, x, mid: at.mid, align_right, tone: *tone },
            );
        }
        CellValue::Icon { name, tone } => {
            if let Some(id) = icons.id(name) {
                let s = px(14.0);
                let x = match at.align {
                    Align::Left => at.left,
                    Align::Right => at.left + at.room - s,
                };
                let ir = Rect::new(x.round(), (at.mid - s / 2.0).round(), s, s);
                draw_icon(layer, icons, id, ir, tone.color(p));
            }
        }
    }
}

/// `value` with its thousands grouped by a thin space and at most `decimals` decimals (the
/// trailing zeros dropped): `12 340`, `0.42`, `1 204.5`.
pub fn number_text(value: f64, decimals: u8) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let text = format!("{:.*}", decimals as usize, value.abs());
    let (int, frac) = match text.split_once('.') {
        Some((i, f)) => (i.to_owned(), f.trim_end_matches('0').to_owned()),
        None => (text, String::new()),
    };
    let mut grouped = String::new();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push('\u{2009}');
        }
        grouped.push(c);
    }
    let sign = if value < 0.0 && (int != "0" || !frac.is_empty()) { "-" } else { "" };
    if frac.is_empty() { format!("{sign}{grouped}") } else { format!("{sign}{grouped}.{frac}") }
}

#[cfg(test)]
#[path = "../tests/unit/tree_table.rs"]
mod tests;
