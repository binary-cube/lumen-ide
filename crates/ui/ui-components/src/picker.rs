//! A list to choose one from, opened over a field (a dropdown's): a search field on top
//! (typing filters by name and detail), the rows — an avatar of initials in a tint of its
//! own (or an icon), the name, a muted detail —, a check on the chosen one, `↵` on the one
//! under the keys, a scrollbar when they don't all fit, the keys at the bottom. Below the
//! field, or above when there's more room there. Its colors are the theme's.
//!
//! It keeps what's typed, the highlighted row and the scroll; the view keeps it open, sends
//! it the keys, the text, the clicks and the wheel while it is, and paints it last (it
//! draws in layers of its own, over everything).
//!
//! How a row's content is drawn is injected ([`PickerRows`]): [`ItemRows`] by default (an
//! avatar, the name, the detail in a column of its own), [`TableRows`] for columns under a
//! header that stays while the rows scroll. The list keeps what's common: the search, the
//! row's background, `↵` and the check (their room kept: nothing moves), the scroll.

use kernel::{Point, Rect};
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{TextInput, draw_text, paint_key_hints};

use crate::ds::{self, Ctx, Elevation, radius};
use crate::scrollbar::EdgeScrollbar;
use crate::{KeyReach, ViewerKey};

/// A row.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PickerItem {
    pub label: String,
    /// Muted, after the name (an id, an address).
    pub detail: String,
    /// An avatar of the name's initials (else `icon`, else nothing).
    pub avatar: bool,
    pub icon: Option<&'static str>,
    /// The table's cells after the first column (the name's): see [`TableRows`].
    pub cells: Vec<String>,
}

impl PickerItem {
    /// A row with an avatar.
    pub fn person(label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { label: label.into(), detail: detail.into(), avatar: true, ..Self::default() }
    }

    #[must_use]
    pub fn with_cells(mut self, cells: Vec<String>) -> Self {
        self.cells = cells;
        self
    }
}

/// What a row's content is drawn with.
#[allow(missing_debug_implementations)]
pub struct RowPaint<'a, 'c> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a icons::Icons,
    pub ctx: &'a Ctx<'c>,
}

/// How a picker draws its rows' content (and a header over them): injected by the view
/// that opens it ([`Picker::open_with`]).
pub trait PickerRows: std::fmt::Debug + Send {
    /// A header over the rows, fixed while they scroll (logical pixels; 0: none).
    fn header_height(&self) -> f32 {
        0.0
    }
    /// Made ready for `items` in a list `width` wide (physical pixels): the columns'
    /// places, once (not per row, not per frame).
    fn prepare(&mut self, text: &mut TextSystem, ctx: &Ctx<'_>, items: &[PickerItem], width: f32);
    fn paint_header(&self, _p: &mut RowPaint<'_, '_>, _area: Rect) {}
    /// Item's content in `content` (its row but the room kept for `↵` and the check).
    fn paint_row(&self, p: &mut RowPaint<'_, '_>, item: &PickerItem, content: Rect);
}

/// The avatar (or icon) and the name (the leading part of every row).
fn paint_lead(p: &mut RowPaint<'_, '_>, item: &PickerItem, content: Rect, name_room: f32) {
    let (ctx, pal) = (p.ctx, p.ctx.palette);
    let px = |v: f32| ctx.px(v);
    let mid = content.min_y() + content.size.height / 2.0;
    let s = px(20.0);
    let badge = Rect::new(content.min_x(), (mid - s / 2.0).round(), s, s);
    if item.avatar {
        let tint = crate::tag_colors::for_key(&item.label, pal);
        p.layer.quad(Quad::filled(badge, tint.with_alpha(0.16)).rounded(s / 2.0));
        let letters = ctx.ui_font(-4.0).with_weight(Font::SEMIBOLD);
        let shaped = p.text.shape_line(&ds::initials(&item.label), &letters);
        let lx = (badge.min_x() + (s - shaped.width) / 2.0).round();
        draw_text(p.layer, &shaped, lx, ds::optical_baseline(badge.min_y(), s, &letters), tint);
    } else if let Some(id) = item.icon.and_then(|i| p.icons.id(i)) {
        let is = px(14.0);
        let at = Rect::new(badge.min_x() + (s - is) / 2.0, (mid - is / 2.0).round(), is, is);
        ui_kit::draw_icon(p.layer, p.icons, id, at, pal.text_muted);
    }
    let lead = if item.avatar || item.icon.is_some() { px(LEAD) } else { 0.0 };
    let font = name_font(ctx);
    let name = ds::ellipsize(p.text, &item.label, &font, name_room - lead);
    let shaped = p.text.shape_line(&name, &font);
    let base = ds::optical_baseline(content.min_y(), content.size.height, &font);
    draw_text(p.layer, &shaped, content.min_x() + lead, base, pal.text);
}

/// What its keys at the bottom do: pick, select, close.
fn hint_labels() -> (String, String, String) {
    (i18n::t("picker.hint_pick"), i18n::t("picker.hint_select"), i18n::t("picker.hint_close"))
}

/// The room an avatar or an icon takes before the name (logical pixels).
const LEAD: f32 = 28.0;

fn name_font(ctx: &Ctx<'_>) -> Font {
    ctx.ui_font(-0.5)
}

/// The default rows: the avatar, the name, the detail muted in a column of its own — after
/// the longest name of them all (not of those shown: it never moves while scrolling).
#[derive(Debug, Default)]
pub struct ItemRows {
    /// Where the detail starts, from the content's left (physical pixels).
    detail_at: f32,
}

impl PickerRows for ItemRows {
    fn prepare(&mut self, text: &mut TextSystem, ctx: &Ctx<'_>, items: &[PickerItem], width: f32) {
        let font = name_font(ctx);
        let lead =
            if items.iter().any(|i| i.avatar || i.icon.is_some()) { ctx.px(LEAD) } else { 0.0 };
        // No details (a list of names): the names have the whole row.
        if items.iter().all(|i| i.detail.is_empty()) {
            self.detail_at = width + ctx.px(12.0);
            return;
        }
        let widest =
            items.iter().map(|i| text.shape_line(&i.label, &font).width).fold(0.0, f32::max);
        self.detail_at = (lead + widest + ctx.px(20.0)).min(width * 0.58);
    }

    fn paint_row(&self, p: &mut RowPaint<'_, '_>, item: &PickerItem, content: Rect) {
        paint_lead(p, item, content, (self.detail_at - p.ctx.px(12.0)).max(p.ctx.px(40.0)));
        let x = content.min_x() + self.detail_at;
        if item.detail.is_empty() || content.max_x() <= x {
            return;
        }
        let small = p.ctx.ui_font(-1.5);
        let detail = ds::ellipsize(p.text, &item.detail, &small, content.max_x() - x);
        let shaped = p.text.shape_line(&detail, &small);
        let base = ds::optical_baseline(content.min_y(), content.size.height, &small);
        draw_text(p.layer, &shaped, x, base, p.ctx.palette.text_muted);
    }
}

/// A table's column.
#[derive(Debug, Clone, PartialEq)]
pub struct PickerColumn {
    pub title: String,
    /// Logical pixels; 0: it takes the room the others leave (the first, the name's).
    pub width: f32,
    /// Its cells against its right edge (numbers).
    pub right: bool,
}

/// Rows as a table: the first column the avatar and the name, the others an item's
/// [`cells`](PickerItem::cells), each in its column; the columns' titles in a header that
/// stays while the rows scroll.
#[derive(Debug, Default)]
pub struct TableRows {
    pub columns: Vec<PickerColumn>,
    /// Each column's left and width from the content's left (physical pixels).
    spans: Vec<(f32, f32)>,
}

impl TableRows {
    pub fn new(columns: Vec<PickerColumn>) -> Self {
        Self { columns, spans: Vec::new() }
    }
}

/// A table's header height (logical pixels).
const TABLE_HEADER: f32 = 28.0;

impl PickerRows for TableRows {
    fn header_height(&self) -> f32 {
        TABLE_HEADER
    }

    fn prepare(&mut self, _: &mut TextSystem, ctx: &Ctx<'_>, _: &[PickerItem], width: f32) {
        let gap = ctx.px(16.0);
        let fixed: f32 = self.columns.iter().map(|c| ctx.px(c.width) + gap).sum();
        let fills = self.columns.iter().filter(|c| c.width <= 0.0).count().max(1) as f32;
        let fill = ((width - fixed) / fills).max(ctx.px(40.0));
        let mut x = 0.0;
        self.spans = self
            .columns
            .iter()
            .map(|c| {
                let w = if c.width > 0.0 { ctx.px(c.width) } else { fill };
                let span = (x, w);
                x += w + gap;
                span
            })
            .collect();
    }

    fn paint_header(&self, p: &mut RowPaint<'_, '_>, area: Rect) {
        let font = p.ctx.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let base = ds::optical_baseline(area.min_y(), area.size.height, &font);
        for (column, (x, w)) in self.columns.iter().zip(&self.spans) {
            let title = ds::ellipsize(p.text, &column.title, &font, *w);
            let shaped = p.text.shape_line(&title, &font);
            let at = area.min_x() + x + if column.right { w - shaped.width } else { 0.0 };
            draw_text(p.layer, &shaped, at, base, p.ctx.palette.text_muted);
        }
    }

    fn paint_row(&self, p: &mut RowPaint<'_, '_>, item: &PickerItem, content: Rect) {
        let small = p.ctx.ui_font(-0.5);
        for (i, (column, (x, w))) in self.columns.iter().zip(&self.spans).enumerate() {
            if i == 0 {
                let cell = Rect::new(content.min_x() + x, content.min_y(), *w, content.size.height);
                paint_lead(p, item, cell, *w);
                continue;
            }
            let Some(value) = item.cells.get(i - 1) else { continue };
            let value = ds::ellipsize(p.text, value, &small, *w);
            let shaped = p.text.shape_line(&value, &small);
            let at = content.min_x() + x + if column.right { w - shaped.width } else { 0.0 };
            let base = ds::optical_baseline(content.min_y(), content.size.height, &small);
            draw_text(p.layer, &shaped, at, base, p.ctx.palette.text_muted);
        }
    }
}

/// The rows' drawing a picker was opened with ([`ItemRows`] by default).
#[derive(Debug)]
struct Render(Box<dyn PickerRows>);

impl Default for Render {
    fn default() -> Self {
        Self(Box::new(ItemRows::default()))
    }
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerEvent {
    /// Item `i` (of those it was opened with) was chosen: it closed.
    Chosen(usize),
    /// Several to choose ([`Picker::open_many`]): item `i` ticked or not; it stays open.
    Toggled(usize),
    /// Closed without choosing (Esc, a click outside).
    Closed,
}

/// The rows shown at most (fewer where there's less room).
const ROWS: usize = 6;
/// As the git log's filter lists (one look for every list to choose from).
const ROW_H: f32 = 28.0;
const SEARCH_H: f32 = 46.0;
/// Its search field's height (as the other filters').
const FIELD_H: f32 = 30.0;
const FOOTER_H: f32 = 34.0;
/// More items than this: the search field shows.
const SEARCH_FROM: usize = 6;

#[derive(Debug, Default)]
pub struct Picker {
    open: bool,
    items: Vec<PickerItem>,
    /// The chosen ones (several when `many`).
    chosen: Vec<usize>,
    many: bool,
    query: TextInput,
    /// The items the search keeps (their indices), and the one under the keys (in it).
    shown: Vec<usize>,
    highlight: usize,
    scroll: f32,
    /// The highlighted row brought into view at the next paint.
    reveal: bool,
    /// Where it went (the last paint): all of it, its search field, the rows' area, each row.
    area: Rect,
    search: Rect,
    /// The search's × (once something is typed), and whether the mouse is on it.
    clear: Option<Rect>,
    clear_hover: bool,
    list: Rect,
    rows: Vec<(Rect, usize)>,
    hover: Option<usize>,
    row_h: f32,
    bar: EdgeScrollbar,
    /// How its rows are drawn, and for which width / scale they were made ready.
    render: Render,
    prepared: Option<(u32, u32)>,
    /// The width its rows need (the longest name, its avatar or icon, `↵` and the check),
    /// measured once at its scale.
    natural: Option<(u32, f32)>,
    /// Suggestions under a text field ([`Picker::open_suggestions`]): the field filters it
    /// (no search of its own), nothing is lit until the arrows move.
    suggest: bool,
    moved: bool,
}

impl Picker {
    /// Opens it on `items`, `chosen` highlighted and in view, drawn as [`ItemRows`].
    pub fn open(&mut self, items: Vec<PickerItem>, chosen: Option<usize>) {
        self.open_with(items, chosen, Box::new(ItemRows::default()));
    }

    /// [`open`](Self::open), its rows drawn by `rows` (a table…).
    pub fn open_with(
        &mut self,
        items: Vec<PickerItem>,
        chosen: Option<usize>,
        rows: Box<dyn PickerRows>,
    ) {
        self.start(items, chosen.into_iter().collect(), false, rows);
    }

    /// Opens it to choose several of `items` (`chosen` ticked): a row ticked or not, the
    /// list staying open ([`PickerEvent::Toggled`]).
    pub fn open_many(
        &mut self,
        items: Vec<PickerItem>,
        chosen: Vec<usize>,
        rows: Box<dyn PickerRows>,
    ) {
        self.start(items, chosen, true, rows);
    }

    /// Opens it as suggestions completing a text field (types as they're typed): `items`
    /// already filtered by the field, which keeps the keys for its text — no search of its
    /// own, opened again as the field changes. `chosen`: the field's value among them
    /// (ticked and lit); `lit_first`: the first one lit (something typed: ↵ takes it).
    /// Otherwise nothing is lit until ↓ / ↑ (↵ stays the form's).
    pub fn open_suggestions(
        &mut self,
        items: Vec<PickerItem>,
        chosen: Option<usize>,
        lit_first: bool,
    ) {
        let any = !items.is_empty();
        self.start(items, chosen.into_iter().collect(), false, Box::new(ItemRows::default()));
        self.suggest = true;
        self.moved = any && (chosen.is_some() || lit_first);
    }

    /// Its rows have a check's place (several to choose, or one chosen): else `↵` takes the
    /// right edge.
    fn checks(&self) -> bool {
        self.many || !self.chosen.is_empty()
    }

    fn start(
        &mut self,
        items: Vec<PickerItem>,
        chosen: Vec<usize>,
        many: bool,
        rows: Box<dyn PickerRows>,
    ) {
        let first = chosen.first().copied();
        *self = Self {
            open: true,
            items,
            chosen,
            many,
            reveal: true,
            render: Render(rows),
            ..Self::default()
        };
        self.filter();
        self.highlight = first.and_then(|c| self.shown.iter().position(|&i| i == c)).unwrap_or(0);
    }

    /// Item `i` taken: chosen (it closes), or — several to choose — ticked or not.
    fn take(&mut self, i: usize) -> PickerEvent {
        if !self.many {
            self.open = false;
            return PickerEvent::Chosen(i);
        }
        match self.chosen.iter().position(|&c| c == i) {
            Some(at) => {
                self.chosen.remove(at);
            }
            None => self.chosen.push(i),
        }
        PickerEvent::Toggled(i)
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// It has a search field (many items): it takes the text typed.
    pub fn searches(&self) -> bool {
        !self.suggest && self.items.len() > SEARCH_FROM
    }

    /// `p` is on it (a click there is its own).
    pub fn contains(&self, p: Point) -> bool {
        self.open && self.area.contains(p)
    }

    /// The items the search keeps, in order.
    pub fn shown(&self) -> Vec<usize> {
        self.shown.clone()
    }

    /// The item under the keys.
    pub fn highlighted(&self) -> Option<usize> {
        if self.suggest && !self.moved {
            return None;
        }
        self.shown.get(self.highlight).copied()
    }

    fn filter(&mut self) {
        let query = self.query.text().trim().to_lowercase();
        self.shown = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                query.is_empty()
                    || item.label.to_lowercase().contains(&query)
                    || item.detail.to_lowercase().contains(&query)
                    || item.cells.iter().any(|c| c.to_lowercase().contains(&query))
            })
            .map(|(i, _)| i)
            .collect();
        self.highlight = 0;
        self.scroll = 0.0;
    }

    /// Text typed (the search).
    pub fn text_input(&mut self, typed: &str) -> bool {
        if !self.open || !self.searches() {
            return false;
        }
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        self.query.insert(&typed);
        self.filter();
        true
    }

    /// A key, while open (all of them are its own).
    pub fn key(&mut self, key: ViewerKey, reach: KeyReach) -> Option<PickerEvent> {
        if !self.open {
            return None;
        }
        let len = self.shown.len();
        let page = ROWS.saturating_sub(1).max(1);
        // Suggestions: the first ↓ (↑) lights the first (last) one.
        if self.suggest && !self.moved && matches!(key, ViewerKey::Down | ViewerKey::Up) {
            self.moved = len > 0;
            self.highlight = if key == ViewerKey::Up { len.saturating_sub(1) } else { 0 };
            self.reveal = true;
            return None;
        }
        match key {
            ViewerKey::Escape => {
                self.open = false;
                return Some(PickerEvent::Closed);
            }
            ViewerKey::Enter => {
                let chosen = self.highlighted()?;
                return Some(self.take(chosen));
            }
            ViewerKey::Down if len > 0 => self.highlight = (self.highlight + 1) % len,
            ViewerKey::Up if len > 0 => self.highlight = (self.highlight + len - 1) % len,
            ViewerKey::PageDown if len > 0 => self.highlight = (self.highlight + page).min(len - 1),
            ViewerKey::PageUp => self.highlight = self.highlight.saturating_sub(page),
            ViewerKey::Backspace if self.searches() => {
                self.query.backspace(reach.word);
                self.filter();
            }
            ViewerKey::Delete if self.searches() => {
                self.query.delete();
                self.filter();
            }
            ViewerKey::Left => self.query.move_left(reach.word, reach.extend),
            ViewerKey::Right => self.query.move_right(reach.word, reach.extend),
            ViewerKey::Home => self.query.home(reach.extend),
            ViewerKey::End => self.query.end(reach.extend),
            _ => {}
        }
        self.reveal = true;
        None
    }

    /// An edit command on the search (`select_all`, `cut`, `undo`, `redo`); `false`: not
    /// one of its own.
    pub fn command(&mut self, id: &str) -> bool {
        if !self.open || !self.searches() {
            return false;
        }
        let changed = match id {
            "select_all" => {
                self.query.select_all();
                return true;
            }
            "cut" if !self.query.selected_text().is_empty() => {
                self.query.insert("");
                true
            }
            "undo" => self.query.undo(),
            "redo" => self.query.redo(),
            _ => return false,
        };
        if changed {
            self.filter();
        }
        true
    }

    /// What's selected in the search (copied).
    pub fn selected_text(&self) -> Option<String> {
        let selected = self.query.selected_text();
        (self.open && !selected.is_empty()).then(|| selected.to_owned())
    }

    /// A press: a row chosen, outside closes it, elsewhere on it nothing.
    pub fn press(&mut self, p: Point) -> Option<PickerEvent> {
        if !self.open {
            return None;
        }
        if !self.area.contains(p) {
            self.open = false;
            return Some(PickerEvent::Closed);
        }
        if self.clear.is_some_and(|c| c.contains(p)) {
            self.query.set_text("");
            self.filter();
            return None;
        }
        if self.search.contains(p) {
            self.query.press(p.x, 1);
            return None;
        }
        let (_, i) = self.rows.iter().find(|(r, _)| r.contains(p))?;
        let chosen = self.shown.get(*i).copied()?;
        Some(self.take(chosen))
    }

    /// The width its rows need (physical pixels): the longest name, its lead (an avatar or
    /// an icon), the marks' room, the paddings — at least a search field's when it has one.
    fn natural_width(&mut self, text: &mut TextSystem, ctx: &Ctx<'_>) -> f32 {
        let key = (ctx.scale * 100.0) as u32;
        if let Some((at, width)) = self.natural
            && at == key
        {
            return width;
        }
        let font = name_font(ctx);
        let widest =
            self.items.iter().map(|i| text.shape_line(&i.label, &font).width).fold(0.0, f32::max);
        let lead = if self.items.iter().any(|i| i.avatar || i.icon.is_some()) { 40.0 } else { 0.0 };
        let least = if self.searches() { 260.0 } else { 180.0 };
        // Its keys at the bottom read whole too.
        let (pick, select, close) = hint_labels();
        let hints: [ui_kit::KeyHint<'_>; 3] =
            [(&["↑", "↓"], &pick), (&["↵"], &select), (&["esc"], &close)];
        let keys = ui_kit::key_hints_width(text, &hints, &ctx.palette.key_hints(ctx.scale));
        let width = (widest + ctx.px(18.0 + lead + 76.0 + 14.0 + 12.0))
            .max(keys + ctx.px(28.0))
            .max(ctx.px(least));
        self.natural = Some((key, width));
        width
    }

    /// The cursor over it: a hand on a row, the text cursor in the search, the arrow
    /// elsewhere on it (`None`: not over it — the view's own).
    pub fn cursor_at(&self, p: Point) -> Option<crate::ViewerCursor> {
        if !self.contains(p) {
            return None;
        }
        Some(if self.clear.is_some_and(|c| c.contains(p)) {
            crate::ViewerCursor::Pointer
        } else if self.search.contains(p) {
            crate::ViewerCursor::Text
        } else if self.rows.iter().any(|(r, _)| r.contains(p)) {
            crate::ViewerCursor::Pointer
        } else {
            crate::ViewerCursor::Arrow
        })
    }

    /// The mouse moved: the row under it lit.
    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i);
        let changed = hover != self.hover;
        self.hover = hover;
        let clear = self.clear.is_some_and(|c| c.contains(p));
        let clear_changed = clear != self.clear_hover;
        self.clear_hover = clear;
        changed | clear_changed | self.bar.hover(p)
    }

    /// The wheel over it.
    pub fn scroll(&mut self, dy: f32) -> bool {
        let max = (self.shown.len() as f32 * self.row_h - self.list.size.height).max(0.0);
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    /// A frame when its scrollbar next fades.
    pub fn next_frame(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    /// Paints it from `anchor` (the field it opened from) within `bounds`, `hints`' keys at
    /// its bottom, in layers of its own.
    /// Where: under (or over) `anchor`, within `bounds`.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ctx: &Ctx<'_>,
        (anchor, bounds): (Rect, Rect),
        placeholder: &str,
    ) {
        self.rows.clear();
        self.search = Rect::default();
        self.clear = None;
        if !self.open {
            return;
        }
        let p = ctx.palette;
        let px = |v: f32| ctx.px(v);
        self.row_h = px(ROW_H);
        let search_h = if self.searches() { px(SEARCH_H) } else { px(6.0) };
        let header_h = px(self.render.0.header_height());
        let footer_h = px(FOOTER_H);
        let chrome = search_h + header_h + footer_h + px(6.0);
        // Below if it fits there whole, else on the side with more room.
        let gap = px(4.0);
        let fits = |room: f32| ((room - chrome) / self.row_h).floor().max(1.0) as usize;
        let wanted = self.shown.len().clamp(1, ROWS);
        let below = fits(bounds.max_y() - anchor.max_y() - gap);
        let above = fits(anchor.min_y() - gap - bounds.min_y());
        let down = below >= wanted || below >= above;
        let visible = wanted.min(if down { below } else { above });
        let height = chrome + visible as f32 * self.row_h;
        // As wide as its field, never narrower than its rows need (one opened from a button:
        // the `+`), within its bounds.
        let width = anchor.size.width.max(self.natural_width(text, ctx)).min(bounds.size.width);
        let top = if down { anchor.max_y() + gap } else { anchor.min_y() - gap - height };
        let left = anchor.min_x().min(bounds.max_x() - width).max(bounds.min_x());
        let r = Rect::new(left, top, width, height);
        self.area = r;
        let layer = scene.push_layer(None);
        ds::surface_with(layer, ctx, r, Elevation::High, radius::LG, p.hover_background);
        // The search.
        if self.searches() {
            let field = Rect::new(
                r.min_x() + px(8.0),
                r.min_y() + px(8.0),
                r.size.width - px(16.0),
                px(FIELD_H),
            );
            ds::search_input(
                &mut crate::LayerPainter::new(layer, text, icons, *ctx),
                field,
                &self.query,
                true,
                placeholder,
            );
            self.search = field;
            if !self.query.text().is_empty() {
                let (cx, mid) = (field.max_x() - px(16.0), field.min_y() + field.size.height / 2.0);
                self.clear = Some(ds::clear_button(layer, icons, ctx, cx, mid, self.clear_hover));
            }
            ds::divider(layer, ctx, r.min_x(), r.max_x(), r.min_y() + search_h);
        }
        // The keys, at the bottom.
        let footer = Rect::new(r.min_x(), r.max_y() - footer_h, r.size.width, footer_h);
        layer.quad(Quad::filled(footer, ctx.fade(p.text.with_alpha(0.022))));
        ds::divider(layer, ctx, r.min_x(), r.max_x(), footer.min_y());
        let (pick, select, close) = hint_labels();
        let style = p.key_hints(ctx.scale);
        let room = footer.size.width - px(28.0);
        let full: [ui_kit::KeyHint<'_>; 3] =
            [(&["↑", "↓"], &pick), (&["↵"], &select), (&["esc"], &close)];
        // Narrow: the keys alone (what they do left out), else nothing.
        let bare: [ui_kit::KeyHint<'_>; 3] = [(&["↑", "↓"], ""), (&["↵"], ""), (&["esc"], "")];
        let hints = [&full[..], &bare[..]]
            .into_iter()
            .find(|h| ui_kit::key_hints_width(text, h, &style) <= room)
            .unwrap_or(&[]);
        let row = Rect::new(footer.min_x() + px(14.0), footer.min_y(), room, footer_h);
        paint_key_hints(layer, text, hints, row, &style);
        // The rows, scrolled.
        let list = Rect::new(
            r.min_x(),
            r.min_y() + search_h + header_h + px(3.0),
            r.size.width,
            visible as f32 * self.row_h,
        );
        self.list = list;
        // The content's room in a row: from its left padding to the marks' room (kept,
        // whether shown or not: nothing moves when `↵` comes).
        let marks = px(if self.checks() { 54.0 } else { 32.0 });
        let content_x = list.min_x() + px(16.0);
        let content_w = (list.max_x() - px(14.0) - marks - content_x).max(0.0);
        let key = (content_w as u32, (ctx.scale * 100.0) as u32);
        if self.prepared != Some(key) {
            self.render.0.prepare(text, ctx, &self.items, content_w);
            self.prepared = Some(key);
        }
        // The header (a table's), fixed over the rows.
        if header_h > 0.0 {
            let area = Rect::new(content_x, r.min_y() + search_h, content_w, header_h);
            let mut pen = RowPaint { layer: &mut *layer, text: &mut *text, icons, ctx };
            self.render.0.paint_header(&mut pen, area);
            ds::divider(layer, ctx, r.min_x(), r.max_x(), area.max_y());
        }
        let content = self.shown.len() as f32 * self.row_h;
        let max = (content - list.size.height).max(0.0);
        if self.reveal {
            self.reveal = false;
            let at = self.highlight as f32 * self.row_h;
            if at < self.scroll {
                self.scroll = at;
            } else if at + self.row_h > self.scroll + list.size.height {
                self.scroll = at + self.row_h - list.size.height;
            }
        }
        self.scroll = self.scroll.clamp(0.0, max);
        let scroll = self.scroll.round();
        if self.shown.is_empty() {
            let none = i18n::t("picker.none");
            ds::empty_state(layer, text, ctx, list, &none, None);
            return;
        }
        let rows = scene.push_layer(Some(list));
        let first = (scroll / self.row_h) as usize;
        let count = (list.size.height / self.row_h) as usize + 2;
        let shown: Vec<(usize, usize)> =
            self.shown.iter().copied().enumerate().skip(first).take(count).collect();
        let right = list.max_x() - px(14.0);
        for (at, i) in shown {
            let top = list.min_y() + at as f32 * self.row_h - scroll;
            let row = Rect::new(
                list.min_x() + px(6.0),
                top + px(1.0),
                list.size.width - px(12.0),
                self.row_h - px(2.0),
            );
            let lit = at == self.highlight && (!self.suggest || self.moved);
            // As the git log's lists: the row under the keys (or the mouse) a shade darker.
            if lit || self.hover == Some(at) {
                rows.quad(Quad::filled(row, p.text.with_alpha(0.06)).rounded(px(radius::MD)));
            }
            let mid = top + self.row_h / 2.0;
            // On the right: the check on the chosen one, `↵` on the one under the keys.
            if self.chosen.contains(&i)
                && let Some(id) = icons.id("check")
            {
                let cs = px(14.0);
                let at = Rect::new(right - cs, (mid - cs / 2.0).round(), cs, cs);
                ui_kit::draw_icon(rows, icons, id, at, p.accent);
            }
            if lit {
                let at = if self.checks() { right - px(22.0) } else { right };
                let hint: [ui_kit::KeyHint<'_>; 1] = [(&["↵"], "")];
                let w = ui_kit::key_hints_width(text, &hint, &style);
                let cap = Rect::new((at - w).round(), top, w, self.row_h);
                paint_key_hints(rows, text, &hint, cap, &style);
            }
            let content = Rect::new(content_x - px(6.0), top, content_w + px(6.0), self.row_h);
            let mut pen = RowPaint { layer: &mut *rows, text: &mut *text, icons, ctx };
            self.render.0.paint_row(&mut pen, &self.items[i], content);
            if let Some(visible_row) = row.intersect(&list) {
                self.rows.push((visible_row, at));
            }
        }
        self.bar.paint(scene, p, ctx.scale, list, content, scroll);
    }
}

#[cfg(test)]
#[path = "../tests/unit/picker.rs"]
mod tests;
