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
use ui_kit::{draw_text, paint_key_hints};

use crate::control::{Control, Press};
use crate::ds::{self, Ctx, Elevation, radius};
use crate::scrollbar::EdgeScrollbar;
use crate::search_field::SearchField;
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
    /// The label's characters matched by what was typed (byte positions, in order): drawn
    /// in the accent by the rows that show them ([`PathRows`]).
    pub marks: Vec<usize>,
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
    /// What a row's tooltip says (`None`: none).
    fn tooltip(&self, _item: &PickerItem) -> Option<String> {
        None
    }
    /// Whether the list widens past its field for these rows (their names read whole).
    /// Rows that fit any width (a path shortened, told whole in its tooltip) keep the
    /// field's width: the list never grows and shrinks as its rows change while typing.
    fn widens(&self) -> bool {
        true
    }
}

/// Rows of paths (folders, files): the name first, the characters typed lit in the accent;
/// its folder after it, muted, cut in the middle when it doesn't fit; the whole path in the
/// tooltip. Nothing allocated as they're drawn.
#[derive(Debug, Default)]
pub struct PathRows;

/// The room between a path's name and its folder (logical pixels).
const PATH_GAP: f32 = 10.0;

impl PickerRows for PathRows {
    fn prepare(&mut self, _: &mut TextSystem, _: &Ctx<'_>, _: &[PickerItem], _: f32) {}

    /// Its paths shorten to the field's width (told whole in the tooltip).
    fn widens(&self) -> bool {
        false
    }

    fn paint_row(&self, p: &mut RowPaint<'_, '_>, item: &PickerItem, content: Rect) {
        let (ctx, pal) = (p.ctx, p.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let mid = content.min_y() + content.size.height / 2.0;
        let mut x = content.min_x();
        if let Some(id) = item.icon.and_then(|i| p.icons.id(i)) {
            let (s, is) = (px(20.0), px(14.0));
            let at = Rect::new(x + (s - is) / 2.0, (mid - is / 2.0).round(), is, is);
            ui_kit::draw_icon(p.layer, p.icons, id, at, ctx.fade(pal.text_muted));
            x += px(LEAD);
        }
        let path = item.label.trim_end_matches('/');
        let (folder, name, from) = match path.rfind('/') {
            Some(i) => (&path[..i], &path[i + 1..], i + 1),
            None => ("", path, 0),
        };
        let font = name_font(ctx);
        let base = ds::optical_baseline(content.min_y(), content.size.height, &font);
        let marks = ds::Marks { at: &item.marks, color: ctx.fade(pal.accent), from };
        let room = (content.max_x() - x).max(0.0);
        let (w, _) = ds::draw_ellipsized_marked(
            p.layer,
            p.text,
            name,
            &font,
            (x, base),
            room,
            (ctx.fade(pal.text), marks),
        );
        let fx = x + w + px(PATH_GAP);
        if folder.is_empty() || content.max_x() - fx < px(24.0) {
            return;
        }
        let small = ctx.ui_font(-1.5);
        let base = ds::optical_baseline(content.min_y(), content.size.height, &small);
        let muted = ctx.fade(pal.text_muted);
        ds::draw_ellipsized_middle(
            p.layer,
            p.text,
            folder,
            &small,
            (fx, base),
            content.max_x() - fx,
            muted,
        );
    }

    fn tooltip(&self, item: &PickerItem) -> Option<String> {
        Some(item.label.clone())
    }
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
        ds::Avatar::new(&item.label, tint).paint(p.layer, p.text, ctx, badge);
    } else if let Some(id) = item.icon.and_then(|i| p.icons.id(i)) {
        let is = px(14.0);
        let at = Rect::new(badge.min_x() + (s - is) / 2.0, (mid - is / 2.0).round(), is, is);
        ui_kit::draw_icon(p.layer, p.icons, id, at, pal.text_muted);
    }
    let lead = if item.avatar || item.icon.is_some() { px(LEAD) } else { 0.0 };
    let font = name_font(ctx);
    let base = ds::optical_baseline(content.min_y(), content.size.height, &font);
    let at = (content.min_x() + lead, base);
    ds::draw_ellipsized(p.layer, p.text, &item.label, &font, at, name_room - lead, pal.text);
}

/// What its keys at the bottom do: pick, select, close — written once per language, shared
/// by every picker.
fn hint_labels() -> std::sync::Arc<[String; 3]> {
    use std::sync::{Arc, Mutex};
    static KEPT: Mutex<Option<(u64, Arc<[String; 3]>)>> = Mutex::new(None);
    let now = i18n::generation();
    let Ok(mut kept) = KEPT.lock() else {
        return Arc::new(
            ["picker.hint_pick", "picker.hint_select", "picker.hint_close"].map(i18n::t),
        );
    };
    match &*kept {
        Some((seen, labels)) if *seen == now => labels.clone(),
        _ => {
            let labels = Arc::new(
                ["picker.hint_pick", "picker.hint_select", "picker.hint_close"].map(i18n::t),
            );
            *kept = Some((now, labels.clone()));
            labels
        }
    }
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
        let base = ds::optical_baseline(content.min_y(), content.size.height, &small);
        let (room, muted) = (content.max_x() - x, p.ctx.palette.text_muted);
        ds::draw_ellipsized(p.layer, p.text, &item.detail, &small, (x, base), room, muted);
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
/// The row under the keys or the mouse: the text's alpha it's lit with.
const ROW_LIT: f32 = 0.06;
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
    /// Its search (the app's search field: ×, ⌘A; it has the keys while the list is open).
    query: SearchField,
    /// The items the search keeps (their indices), and the one under the keys (in it).
    shown: Vec<usize>,
    highlight: usize,
    scroll: f32,
    /// The highlighted row brought into view at the next paint.
    reveal: bool,
    /// Where it went (the last paint): all of it, its search field, the rows' area, each row.
    area: Rect,
    search: Rect,
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
    /// Its search shown however few items there are ([`Picker::always_search`]).
    search_always: bool,
    /// The search was pressed: a drag selects in it.
    selecting: bool,
    /// What it did last as a control (a press, a key), taken by its view.
    event: Option<PickerEvent>,
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
        self.open_suggestions_with(items, (chosen, lit_first), Box::new(ItemRows::default()));
    }

    /// [`Self::open_suggestions`], its rows drawn by `rows` ([`PathRows`] for paths).
    pub fn open_suggestions_with(
        &mut self,
        items: Vec<PickerItem>,
        (chosen, lit_first): (Option<usize>, bool),
        rows: Box<dyn PickerRows>,
    ) {
        let any = !items.is_empty();
        self.start(items, chosen.into_iter().collect(), false, rows);
        self.suggest = true;
        self.moved = any && (chosen.is_some() || lit_first);
    }

    /// Its search shown however few items there are (a list one types into first).
    pub fn always_search(&mut self) {
        self.search_always = true;
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
        !self.suggest && (self.search_always || self.items.len() > SEARCH_FROM)
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
        Control::text(&mut self.query, typed);
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
            // The field's editing keys (what's typed changes: the rows filtered again).
            ViewerKey::Backspace | ViewerKey::Delete if self.searches() => {
                Control::key(&mut self.query, key, reach);
                self.filter();
            }
            ViewerKey::Left | ViewerKey::Right | ViewerKey::Home | ViewerKey::End => {
                Control::key(&mut self.query, key, reach);
            }
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
        if !matches!(id, "select_all" | "cut" | "undo" | "redo") {
            return false;
        }
        // What's typed changed (a cut, an undo): the rows filtered again.
        if Control::command(&mut self.query, id) && id != "select_all" {
            self.filter();
        }
        true
    }

    /// The mouse dragged after a press: its scrollbar's thumb followed (the rows scroll),
    /// or in the search what it covers selected.
    pub fn drag(&mut self, p: Point) -> bool {
        if !self.open {
            return false;
        }
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        if !self.selecting {
            return false;
        }
        Control::drag(&mut self.query, p)
    }

    /// The mouse let go (the thumb, a selection in the search).
    pub fn release(&mut self) -> bool {
        self.bar.release() | std::mem::take(&mut self.selecting)
    }

    /// Text pasted into the search: on one line (its breaks made spaces).
    pub fn paste(&mut self, text: &str) -> bool {
        self.text_input(&kernel::text::pasted_line(text))
    }

    /// What's selected in the search (copied).
    pub fn selected_text(&self) -> Option<String> {
        self.open.then(|| Control::selected_text(&self.query)).flatten()
    }

    /// A press: a row chosen, outside closes it, elsewhere on it nothing.
    pub fn press(&mut self, p: Point) -> Option<PickerEvent> {
        self.press_with(p, 1)
    }

    /// [`press`](Self::press) as the `clicks`-th of a quick series: in the search, two
    /// select a word, three all of it.
    pub fn press_with(&mut self, p: Point, clicks: u32) -> Option<PickerEvent> {
        self.selecting = false;
        if !self.open {
            return None;
        }
        if !self.area.contains(p) {
            self.open = false;
            return Some(PickerEvent::Closed);
        }
        // On its scrollbar: the thumb taken (or the rows jumped there); a drag follows.
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        // Its search: × empties it (all the rows back), else the caret put, a drag selecting.
        if self.search.contains(p) {
            if Control::press(&mut self.query, p, clicks) == Press::Held {
                self.selecting = true;
            } else if self.query.take_cleared() {
                self.filter();
            }
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
        let labels = hint_labels();
        let [pick, select, close] = &*labels;
        let hints: [ui_kit::KeyHint<'_>; 3] =
            [(&["↑", "↓"], pick), (&["↵"], select), (&["esc"], close)];
        let keys = ui_kit::key_hints_width(text, &hints, &ctx.palette.key_hints(ctx.scale));
        let width = (widest + ctx.px(18.0 + lead + 76.0 + 14.0 + 12.0))
            .max(keys + ctx.px(28.0))
            .max(ctx.px(least));
        self.natural = Some((key, width));
        width
    }

    /// The tooltip of the row at `p`, as its rows say ([`PickerRows::tooltip`]).
    pub fn tooltip_at(&self, p: Point) -> Option<crate::Tip> {
        let (rect, at) = self.rows.iter().find(|(r, _)| r.contains(p))?;
        let item = self.items.get(*self.shown.get(*at)?)?;
        Some(crate::Tip::new(*rect, self.render.0.tooltip(item)?))
    }

    /// The cursor over it: a hand on a row, the text cursor in the search, the arrow
    /// elsewhere on it (`None`: not over it — the view's own).
    pub fn cursor_at(&self, p: Point) -> Option<crate::ViewerCursor> {
        if !self.contains(p) {
            return None;
        }
        Some(if self.search.contains(p) {
            Control::cursor(&self.query, p).unwrap_or(crate::ViewerCursor::Text)
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
        let on_search = self.search.contains(p).then_some(p);
        let clear_changed = Control::hover(&mut self.query, on_search);
        changed | clear_changed | self.bar.hover(p)
    }

    /// Its scrollbar's thumb is held (the mouse's moves go to [`Self::drag`]).
    pub fn dragging(&self) -> bool {
        self.open && self.bar.is_dragging()
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
        // the `+`), within its bounds; rows that fit any width keep the field's (paths).
        let least = if self.render.0.widens() { self.natural_width(text, ctx) } else { 0.0 };
        let width = anchor.size.width.max(least).min(bounds.size.width);
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
            // The list open is its focus: the caret always in its field.
            self.query.paint(layer, text, icons, ctx, (field, true), placeholder);
            self.search = field;
            ds::divider(layer, ctx, r.min_x(), r.max_x(), r.min_y() + search_h);
        }
        // The keys, at the bottom.
        let footer = Rect::new(r.min_x(), r.max_y() - footer_h, r.size.width, footer_h);
        layer.quad(Quad::filled(footer, ctx.fade(p.text.with_alpha(ds::BAND_TINT))));
        ds::divider(layer, ctx, r.min_x(), r.max_x(), footer.min_y());
        let labels = hint_labels();
        let [pick, select, close] = &*labels;
        let style = p.key_hints(ctx.scale);
        let room = footer.size.width - px(28.0);
        let full: [ui_kit::KeyHint<'_>; 3] =
            [(&["↑", "↓"], pick), (&["↵"], select), (&["esc"], close)];
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
        // The rows in view, by their place among those shown (read in place, never copied).
        let in_view = first..(first + count).min(self.shown.len());
        let right = list.max_x() - px(14.0);
        // To a screen reader: a list of all its rows, the ones shown under it.
        let total = self.shown.len();
        let list_node = move || {
            ui_kit::access::Described::new(ui_kit::access::Role::List, "", list).count(total)
        };
        ui_kit::access::group(list_node, || {
            for at in in_view {
                let i = self.shown[at];
                let top = list.min_y() + at as f32 * self.row_h - scroll;
                let row = Rect::new(
                    list.min_x() + px(6.0),
                    top + px(1.0),
                    list.size.width - px(12.0),
                    self.row_h - px(2.0),
                );
                let lit = at == self.highlight && (!self.suggest || self.moved);
                // As the git log's lists: the row under the keys (or the mouse) a shade darker.
                ds::RowHighlight::new(false, lit || self.hover == Some(at))
                    .hover(ds::HoverFill::Faint(ROW_LIT))
                    .paint(rows, ctx, row);
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
                // A list's item to a screen reader, named by its label (its texts, drawn in it,
                // are that name), chosen when checked.
                let item = &self.items[i];
                let described = || {
                    use ui_kit::access::{Described, Role};
                    Described::new(Role::ListItem, item.label.as_str(), row)
                        .key(i as u64)
                        .index(at)
                        .selected(self.chosen.contains(&i))
                        .focused(lit)
                };
                ui_kit::access::group(described, || {
                    self.render.0.paint_row(&mut pen, item, content)
                });
                if let Some(visible_row) = row.intersect(&list) {
                    self.rows.push((visible_row, at));
                }
            }
        });
        self.bar.paint_in(scene, ctx, list, content, scroll);
    }
}

impl Picker {
    /// Where its rows were drawn last (under its search).
    pub fn list_area(&self) -> kernel::Rect {
        self.list
    }

    /// What it did last as a control ([`crate::control::Control`]): chosen, ticked, closed.
    pub fn take_event(&mut self) -> Option<PickerEvent> {
        self.event.take()
    }
}

/// An open list is a control over everything: a press outside it closes it (it's hit
/// anywhere while open), the keys and the text go to it.
impl crate::control::Control for Picker {
    fn tooltip(&self, p: Point) -> Option<crate::Tip> {
        self.tooltip_at(p)
    }

    fn area(&self) -> Option<kernel::Rect> {
        self.open.then_some(self.area)
    }

    fn hit(&self, _p: Point) -> bool {
        self.open
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        let was_held = self.dragging();
        self.event = self.press_with(p, clicks);
        if self.dragging() && !was_held || self.selecting { Press::Held } else { Press::Took }
    }

    fn drag(&mut self, p: Point) -> bool {
        Picker::drag(self, p)
    }

    fn release(&mut self) -> bool {
        Picker::release(self)
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        p.is_some_and(|p| Picker::hover(self, p))
    }

    /// Over its rows a hand, its search the text's; elsewhere while it's open the arrow
    /// (never what's under it).
    fn cursor(&self, p: Point) -> Option<crate::ViewerCursor> {
        self.cursor_at(p).or(self.open.then_some(crate::ViewerCursor::Arrow))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        Picker::scroll(self, dy)
    }

    fn focusable(&self) -> bool {
        self.open
    }

    /// An open list keeps Tab (it moves in it), the focus doesn't move on.
    fn keeps_tab(&self) -> bool {
        self.open
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if !self.open {
            return false;
        }
        self.event = Picker::key(self, key, reach);
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.text_input(typed)
    }

    /// Pasted into its search on one line (its breaks made spaces).
    fn paste(&mut self, text: &str) -> bool {
        Picker::paste(self, text)
    }

    fn selected_text(&self) -> Option<String> {
        Picker::selected_text(self)
    }

    fn command(&mut self, id: &str) -> bool {
        Picker::command(self, id)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.next_frame()
    }
}

#[cfg(test)]
#[path = "../tests/unit/picker.rs"]
mod tests;
