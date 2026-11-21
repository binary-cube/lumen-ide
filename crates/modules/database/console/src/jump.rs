//! "Query consoles of …" (⇧⌘F10, plan 39i): a popover of the module's own
//! (`view-console-jump`, `host.open_overlay`) listing a source's consoles under a filter
//! field that has the keyboard as it opens. What's typed is matched fuzzily — as the file
//! search does — against each console's name and its text: the first line of its text that
//! matches is shown under its name, what matched lit. "New Query Console" stays first;
//! ↵ opens the first match (or what's chosen with ↑ ↓). A console's row renames it or
//! deletes it ("Database Consoles": its pencil and its bin, its right-click menu).
//!
//! The texts are those of the open tabs (unsaved changes included) or read from their
//! files in the background (a little of each: [`TEXT_MAX`]); the list is drawn only where
//! it's seen (any number of consoles) and filtered only when the filter or the texts change.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use database::console::{ConsoleId, JumpRequest};
use i18n::t;
use kernel::{FuzzyStyle, Point, ProjectId, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Thumb};

use crate::field::SearchPart;
use ui_components::ds::{
    self, ButtonKind, ButtonSpec, Ctx, Elevation, Tone, button_size, radius, space,
};
use ui_components::search_field::SearchField;
use ui_components::{
    KeyReach, Tip, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};
use ui_kit::{KeyHint, draw_icon, draw_text};

/// The popover's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-console-jump";

/// The most bytes of a console's text looked at by the filter.
pub const TEXT_MAX: usize = 256 * 1024;

/// A console as the popover lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpConsole {
    pub id: ConsoleId,
    pub name: String,
    /// Its last run (`ultima rulare 543 ms`), or that it never ran.
    pub detail: String,
    pub file: PathBuf,
    /// Its text when a tab has it open (else read from its file).
    pub text: Option<String>,
}

/// What the popover shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpData {
    pub project: ProjectId,
    pub request: JumpRequest,
    pub source_name: String,
    pub consoles: Vec<JumpConsole>,
}

/// What's done with the row chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JumpPick {
    New,
    Open(ConsoleId),
    Rename(ConsoleId),
    Delete(ConsoleId),
}

/// The popover's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct JumpProvider;

impl ViewerProvider for JumpProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(JumpView::default()))
    }
}

// ─── The filter (pure) ───

/// A console found by the filter: which (its index), the name's characters matched (byte
/// ranges), the line of its text that matched (trimmed) with what matched in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub index: usize,
    pub name_lit: Vec<std::ops::Range<usize>>,
    pub line: Option<(String, Vec<std::ops::Range<usize>>)>,
    score: i32,
}

/// Names break into words at these (`orders_by_day`, `report-2`).
const NAMES: FuzzyStyle = FuzzyStyle {
    separators: &['_', '-', ' ', '.'],
    consecutive_bonus: 5,
    boundary_bonus: 8,
    gap_penalty: true,
    prefer_boundaries: true,
};

/// A text's words (SQL).
const TEXTS: FuzzyStyle = FuzzyStyle {
    separators: &[' ', '\t', '.', ',', '(', ')', '_', '"', '\'', '='],
    consecutive_bonus: 5,
    boundary_bonus: 6,
    gap_penalty: true,
    prefer_boundaries: true,
};

/// The positions `fuzzy_match` gives, as byte ranges of their characters.
fn ranges(text: &str, positions: &[usize]) -> Vec<std::ops::Range<usize>> {
    positions
        .iter()
        .map(|&at| at..at + text[at..].chars().next().map_or(1, char::len_utf8))
        .collect()
}

/// The first line of `text` that matches `query` (lowercase characters): one holding it
/// whole first (any case), else one holding its characters close together (within three
/// times its length). The line trimmed, what matched in it, the score.
fn line_match(text: &str, query: &[char]) -> Option<(String, Vec<std::ops::Range<usize>>, i32)> {
    let mut end = text.len().min(TEXT_MAX);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = &text[..end];
    let needle: String = query.iter().collect();
    let trimmed = |line: &str| {
        let lead = line.len() - line.trim_start().len();
        (line.trim().to_owned(), lead)
    };
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.len() == line.len()
            && let Some(at) = lower.find(&needle)
        {
            let (shown, lead) = trimmed(line);
            // The places lit in the line: here only the one found.
            #[allow(clippy::single_range_in_vec_init)]
            let lit = vec![at - lead..at - lead + needle.len()];
            return Some((shown, lit, 40));
        }
    }
    let span = query.len() * 3 + 2;
    for line in text.lines() {
        // From each of the first character's places (a few), the closest match.
        let lower = line.to_lowercase();
        if lower.len() != line.len() {
            continue;
        }
        let starts = lower.match_indices(query[0]).map(|(at, _)| at).take(32);
        let close = starts.filter_map(|start| {
            let (score, positions) = kernel::fuzzy_match(&line[start..], query, &TEXTS)?;
            let (first, last) = (positions[0], *positions.last().unwrap_or(&positions[0]));
            (last - first <= span)
                .then(|| (score, positions.into_iter().map(|p| p + start).collect::<Vec<_>>()))
        });
        let Some((score, positions)) = close.max_by_key(|(score, _)| *score) else { continue };
        let (shown, lead) = trimmed(line);
        let lit = ranges(line, &positions)
            .into_iter()
            .filter(|r| r.start >= lead)
            .map(|r| r.start - lead..r.end - lead)
            .collect();
        return Some((shown, lit, score));
    }
    None
}

/// The consoles `query` finds among `items` (their names, their texts when known), the best
/// first (a name's match before a text's, then by score, then in their order); an empty
/// query: all of them, in their order.
pub fn filter(query: &str, items: &[(&str, Option<&str>)]) -> Vec<Found> {
    let query: Vec<char> =
        query.trim().chars().flat_map(char::to_lowercase).filter(|c| !c.is_whitespace()).collect();
    if query.is_empty() {
        return (0..items.len())
            .map(|index| Found { index, name_lit: Vec::new(), line: None, score: 0 })
            .collect();
    }
    let mut found: Vec<Found> = items
        .iter()
        .enumerate()
        .filter_map(|(index, (name, text))| {
            let by_name = kernel::fuzzy_match(name, &query, &NAMES);
            let by_text = text.and_then(|t| line_match(t, &query));
            if by_name.is_none() && by_text.is_none() {
                return None;
            }
            let name_score = by_name.as_ref().map_or(0, |(s, _)| 1000 + s * 2);
            let text_score = by_text.as_ref().map_or(0, |(_, _, s)| *s);
            Some(Found {
                index,
                name_lit: by_name.map(|(_, p)| ranges(name, &p)).unwrap_or_default(),
                line: by_text.map(|(line, lit, _)| (line, lit)),
                score: name_score + text_score,
            })
        })
        .collect();
    found.sort_by(|a, b| b.score.cmp(&a.score).then(a.index.cmp(&b.index)));
    found
}

// ─── The view ───

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Esc,
    /// A row (0: "New Query Console"; then the matches).
    Row(usize),
    Rename(usize),
    Delete(usize),
    Inside,
}

/// The filter's place among the popover's parts.
const FILTER: usize = 1;

/// A row (logical pixels).
pub const ROW_H: f32 = 56.0;
/// The rows shown at most before the list scrolls.
pub const VISIBLE: usize = 7;
const HEADER_H: f32 = 56.0;
const FIELD_H: f32 = 34.0;
const FOOTER_H: f32 = 38.0;
const WIDTH: f32 = 520.0;
/// A row: the chosen one's accent alpha, the hovered one's text alpha.
const ROW_CHOSEN: f32 = 0.12;
const ROW_HOVER: f32 = 0.04;

/// The texts read in the background (by index), shared with their job.
type Texts = Arc<Mutex<Vec<Option<String>>>>;

/// The popover's view.
#[derive(Default)]
pub struct JumpView {
    data: Option<JumpData>,
    /// Each console's text (`None`: not read yet).
    texts: Texts,
    /// Changes as texts come in (what's found is worked out again).
    texts_seen: usize,
    /// The filter (the app's search field: ×, ⌘A; it has the keys the whole time).
    filter: SearchField,
    found: Vec<Found>,
    found_for: Option<(String, usize)>,
    /// The row chosen (0: "New Query Console").
    selected: usize,
    /// The first row shown.
    first: usize,
    panel: Rect,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
    reading: bool,
    /// The list's scrollbar (more rows than shown): the app's, a row at a time.
    bar: ui_components::EdgeScrollbar,
    /// A row's height, as last painted (physical pixels: the scrollbar's unit).
    unit: f32,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for JumpView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JumpView").field("data", &self.data).finish_non_exhaustive()
    }
}

impl JumpView {
    /// The consoles found, in their order (their names).
    pub fn found_names(&mut self) -> Vec<String> {
        self.refilter();
        let Some(data) = &self.data else { return Vec::new() };
        self.found
            .iter()
            .filter_map(|f| data.consoles.get(f.index))
            .map(|c| c.name.clone())
            .collect()
    }

    /// What's found (the tests').
    pub fn found(&mut self) -> &[Found] {
        self.refilter();
        &self.found
    }

    /// The row chosen (0: "New Query Console").
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The filter typed (as if by the keyboard).
    pub fn set_filter(&mut self, text: &str) {
        self.filter.set_text(text);
        self.refilter();
    }

    /// Every console's text is known.
    pub fn texts_read(&self) -> bool {
        self.texts.lock().unwrap_or_else(PoisonError::into_inner).iter().all(Option::is_some)
    }

    fn texts_known(&self) -> usize {
        self.texts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|t| t.is_some())
            .count()
    }

    /// What's found worked out again when the filter or the texts changed. `true` if it was.
    fn refilter(&mut self) -> bool {
        let known = self.texts_known();
        let key = (self.filter.text().to_owned(), known);
        if self.found_for.as_ref() == Some(&key) {
            return false;
        }
        let Some(data) = &self.data else { return false };
        let texts = self.texts.lock().unwrap_or_else(PoisonError::into_inner).clone();
        let items: Vec<(&str, Option<&str>)> = data
            .consoles
            .iter()
            .zip(&texts)
            .map(|(c, t)| (c.name.as_str(), t.as_deref()))
            .collect();
        let typed_changed = self.found_for.as_ref().is_none_or(|(q, _)| *q != key.0);
        self.found = filter(self.filter.text(), &items);
        self.texts_seen = known;
        // Typed: the first match chosen (none: "New Query Console"); texts coming in keep it.
        if typed_changed {
            let typed = !self.filter.text().trim().is_empty();
            self.selected = if typed && !self.found.is_empty() { 1 } else { 0 };
            self.first = 0;
        } else {
            self.selected = self.selected.min(self.found.len());
        }
        self.found_for = Some(key);
        true
    }

    /// The rows: "New Query Console", then the consoles found.
    fn rows(&self) -> usize {
        self.found.len() + 1
    }

    fn step(&mut self, delta: isize) {
        self.selected = ui_kit::step_index(self.selected, delta, self.rows());
        self.keep_in_view();
    }

    fn keep_in_view(&mut self) {
        if self.selected < self.first {
            self.first = self.selected;
        } else if self.selected >= self.first + VISIBLE {
            self.first = self.selected + 1 - VISIBLE;
        }
    }

    /// Row `row` chosen (↵, a click): its console opened, or a new one made.
    pub fn choose(&mut self, row: usize) -> bool {
        self.refilter();
        let pick = if row == 0 {
            JumpPick::New
        } else {
            let Some(id) = self.console_at(row) else { return false };
            JumpPick::Open(id)
        };
        self.finish(pick)
    }

    fn console_at(&self, row: usize) -> Option<ConsoleId> {
        let found = self.found.get(row.checked_sub(1)?)?;
        Some(self.data.as_ref()?.consoles.get(found.index)?.id)
    }

    fn finish(&mut self, pick: JumpPick) -> bool {
        let Some(data) = &self.data else { return false };
        let request = data.request.clone();
        self.done = true;
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            crate::commands::jump_picked(host, request, pick);
        })));
        true
    }

    /// Row `row`'s console renamed / deleted (its buttons, its menu).
    /// The list scrolled to `offset` (the scrollbar's, a row being [`Self::unit`]): the
    /// nearest row first, within the ends. `true` if it moved.
    fn scroll_to(&mut self, offset: f32) -> bool {
        let max = self.rows().saturating_sub(VISIBLE);
        let first =
            if self.unit > 0.0 { (offset / self.unit).round().max(0.0) as usize } else { 0 };
        let first = first.min(max);
        let moved = first != self.first;
        self.first = first;
        moved
    }

    pub fn rename(&mut self, row: usize) -> bool {
        self.console_at(row).is_some_and(|id| self.finish(JumpPick::Rename(id)))
    }

    pub fn delete(&mut self, row: usize) -> bool {
        self.console_at(row).is_some_and(|id| self.finish(JumpPick::Delete(id)))
    }

    /// Its parts (the list's scrollbar, the filter), the topmost first, with their state of
    /// use. The thumb moves an offset in pixels; the list follows it to the nearest row. The
    /// filter has the keys the whole time; what's found follows what it changed.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let before = self.unit * self.first as f32;
        let mut offset = before;
        let mut edited = false;
        let mut list = Thumb { bar: &mut self.bar, offset: &mut offset };
        let mut filter = SearchPart { field: &mut self.filter, edited: &mut edited };
        let mut parts: [&mut dyn Control; 2] = [&mut list, &mut filter];
        self.controls.focus(&mut parts, Some(FILTER));
        let answer = f(&mut self.controls, &mut parts);
        if offset != before {
            self.scroll_to(offset);
        }
        if edited {
            self.refilter();
        }
        answer
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        if !self.panel.contains(p) {
            return None;
        }
        // On the list's scrollbar: no row under it.
        if self.bar.contains(p) {
            return Some(Hit::Inside);
        }
        Some(self.hits.iter().rev().find(|(r, _)| r.contains(p)).map_or(Hit::Inside, |(_, h)| *h))
    }

    /// The texts not known yet read from their files, in the background (once).
    fn read_texts(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        if self.reading || self.texts_read() {
            return;
        }
        let Some(data) = &self.data else { return };
        let Some(consoles) = crate::consoles::consoles(data.project) else { return };
        self.reading = true;
        let files: Vec<(usize, PathBuf)> = data
            .consoles
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                self.texts
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(*i)
                    .is_some_and(Option::is_none)
            })
            .map(|(i, c)| (i, c.file.clone()))
            .collect();
        let texts = Arc::clone(&self.texts);
        consoles.runtime().spawn(move || {
            for (i, file) in files {
                let text = read_head(&file);
                if let Some(slot) = texts.lock().unwrap_or_else(PoisonError::into_inner).get_mut(i)
                {
                    *slot = Some(text);
                }
            }
            if let Some(wake) = wake {
                wake();
            }
        });
    }
}

/// The first [`TEXT_MAX`] bytes of `file` as text (nothing when it can't be read).
pub fn read_head(file: &Path) -> String {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    if let Ok(f) = std::fs::File::open(file) {
        let _ = f.take(TEXT_MAX as u64).read_to_end(&mut bytes);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

impl Viewer for JumpView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<JumpData>() else { return };
        self.texts = Arc::new(Mutex::new(data.consoles.iter().map(|c| c.text.clone()).collect()));
        self.data = Some(data.clone());
        self.found_for = None;
        self.selected = 0;
        self.refilter();
        // The filter has the keys from now (its selection the copy's).
        self.controls = Controls::sticky();
        self.with_controls(|_, _| ());
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, typed: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, typed));
        true
    }

    fn poll(&mut self) -> bool {
        self.refilter()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: v, scale, text, scene, palette: p, icons, waker, .. } = ctx;
        self.read_texts(waker);
        self.refilter();
        let ds = Ctx::new(p, scale);
        let px = |x: f32| ds.px(x);
        self.hits.clear();
        let Some(data) = self.data.clone() else { return };
        let rows = self.rows();
        let shown = rows.min(VISIBLE);
        let pad = px(space::SM);
        let width = px(WIDTH).min(v.size.width - px(32.0));
        let field_h = px(FIELD_H) + px(space::SM) * 2.0;
        let height = px(HEADER_H) + field_h + pad + shown as f32 * px(ROW_H) + pad + px(FOOTER_H);
        let x = (v.min_x() + (v.size.width - width) / 2.0).max(v.min_x() + px(16.0));
        let y = (v.min_y() + v.size.height * 0.18)
            .min(v.max_y() - height - px(ds::POPOVER_GAP))
            .max(v.min_y() + px(8.0));
        let panel = Rect::new(x.round(), y.round(), width.round(), height);
        self.panel = panel;
        let layer = scene.push_layer(None);
        ds::surface_with(layer, &ds, panel, Elevation::High, radius::LG, p.hover_background);
        let layer = scene.push_layer(Some(panel));
        // The header: the icon in its tile, the title and the source, `esc`.
        let mid = panel.min_y() + px(HEADER_H) / 2.0;
        let tile = px(30.0);
        let tile_rect =
            Rect::new(panel.min_x() + px(space::LG), (mid - tile / 2.0).round(), tile, tile);
        layer
            .quad(Quad::filled(tile_rect, p.accent.with_alpha(0.14)).rounded(px(radius::SM + 1.0)));
        if let Some(id) = icons.id("console") {
            let s = px(16.0);
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(
                    tile_rect.min_x() + (tile - s) / 2.0,
                    tile_rect.min_y() + (tile - s) / 2.0,
                    s,
                    s,
                ),
                p.accent,
            );
        }
        let esc = ds::keycap(
            layer,
            text,
            &ds,
            "esc",
            panel.max_x() - px(space::LG),
            mid,
            self.hover == Some(Hit::Esc),
        );
        self.hits.push((esc, Hit::Esc));
        let tx = tile_rect.max_x() + px(space::MD);
        let title_font = ds.ui_font(0.5).with_weight(text_layout::Font::SEMIBOLD);
        let shaped = text.shape_line(&t("console.jump_title"), &title_font);
        draw_text(
            layer,
            &shaped,
            tx,
            ds::optical_baseline(mid - px(17.0), px(18.0), &title_font),
            p.text,
        );
        let small = ds.ui_font(-1.0);
        let subject =
            ds::ellipsize_middle(text, &data.source_name, &small, esc.min_x() - px(space::MD) - tx);
        let shaped = text.shape_line(&subject, &small);
        draw_text(
            layer,
            &shaped,
            tx,
            ds::optical_baseline(mid + px(1.0), px(16.0), &small),
            p.text_muted,
        );
        ds::divider(layer, &ds, panel.min_x(), panel.max_x(), panel.min_y() + px(HEADER_H));
        // The filter, with the keyboard.
        let field = Rect::new(
            panel.min_x() + px(space::LG),
            panel.min_y() + px(HEADER_H) + px(space::SM),
            panel.size.width - px(space::LG) * 2.0,
            px(FIELD_H),
        );
        let placeholder = t("console.jump_filter");
        self.filter.paint(layer, text, icons, &ds, (field, true), &placeholder);
        // The rows shown.
        let list_top = field.max_y() + px(space::SM) + pad;
        let list = Rect::new(panel.min_x(), list_top, panel.size.width, shown as f32 * px(ROW_H));
        self.first = self.first.min(rows.saturating_sub(VISIBLE));
        let rows_layer = scene.push_layer(Some(list));
        let label_font = ds.ui_font(0.0);
        let lit_font = ds.ui_font(0.0).with_weight(text_layout::Font::SEMIBOLD);
        let detail_font = ds.code_font(-1.5);
        let code_font = ds.code_font(-1.5);
        let row_h = px(ROW_H);
        for row in self.first..(self.first + shown).min(rows) {
            let top = list.min_y() + (row - self.first) as f32 * row_h;
            let rect = Rect::new(panel.min_x() + pad, top, panel.size.width - pad * 2.0, row_h);
            let chosen = row == self.selected;
            ds::RowHighlight::new(chosen, self.hover == Some(Hit::Row(row)))
                .selected_alpha(ROW_CHOSEN)
                .hover(ds::HoverFill::Faint(ROW_HOVER))
                .paint(rows_layer, &ds, rect);
            let inner = rect.min_x() + px(space::MD);
            let (icon, name, detail, name_lit, line) = if row == 0 {
                ("plus", t("console.new_console"), data.source_name.clone(), Vec::new(), None)
            } else {
                let found = &self.found[row - 1];
                let console = &data.consoles[found.index];
                (
                    "console",
                    console.name.clone(),
                    console.detail.clone(),
                    found.name_lit.clone(),
                    found.line.clone(),
                )
            };
            if let Some(id) = icons.id(icon) {
                let s = px(16.0);
                let color = if chosen { p.accent } else { p.text_muted };
                draw_icon(
                    rows_layer,
                    icons,
                    id,
                    Rect::new(inner, (top + (row_h - s) / 2.0).round(), s, s),
                    color,
                );
            }
            let text_x = inner + px(28.0);
            let mut right = rect.max_x() - px(space::MD);
            // A console's own buttons, while it's chosen or under the mouse.
            let hovered = matches!(self.hover, Some(Hit::Row(r) | Hit::Rename(r) | Hit::Delete(r)) if r == row);
            if row > 0 && (chosen || hovered) {
                for (icon, hit) in [("trash", Hit::Delete(row)), ("file-pen", Hit::Rename(row))] {
                    let b = ds::square_button(
                        &ds,
                        (right - px(button_size::MEDIUM), top + row_h / 2.0),
                        button_size::MEDIUM,
                    );
                    // Muted at rest, lit under the mouse.
                    let spec = ButtonSpec::new(ButtonKind::Ghost)
                        .icon(icon)
                        .rest_tone(Tone::Neutral)
                        .hovered(self.hover == Some(hit));
                    ds::button(rows_layer, text, icons, &ds, b, spec);
                    self.hits.push((b, hit));
                    right = b.min_x() - px(space::XS);
                }
            }
            // The name, what matched in it in bold and the accent's color.
            let shown_name = ds::ellipsize(text, &name, &label_font, right - text_x);
            let base = ds::optical_baseline(top + px(7.0), px(18.0), &label_font);
            let mut nx = text_x;
            for (i, ch) in shown_name.char_indices() {
                let lit = name_lit.iter().any(|r| r.start == i);
                let piece = &shown_name[i..i + ch.len_utf8()];
                let font = if lit { &lit_font } else { &label_font };
                let shaped = text.shape_line(piece, font);
                draw_text(
                    rows_layer,
                    &shaped,
                    nx.round(),
                    base,
                    if lit { p.accent } else { p.text },
                );
                nx += shaped.width;
            }
            // Under it: the line of its text that matched (lit), else its last run.
            match line {
                Some((line, lit)) => {
                    crate::listing::lit_code(
                        rows_layer,
                        text,
                        &ds,
                        (&line, &code_font),
                        (text_x, right),
                        (top + px(27.0), px(20.0)),
                        &lit,
                    );
                    // Its last run at the right of the name.
                    let shaped = text.shape_line(&detail, &detail_font);
                    if nx + px(16.0) + shaped.width < right {
                        draw_text(
                            rows_layer,
                            &shaped,
                            (right - shaped.width).round(),
                            ds::optical_baseline(top + px(7.0), px(18.0), &detail_font),
                            p.text_muted,
                        );
                    }
                }
                None => {
                    let detail = ds::ellipsize_middle(text, &detail, &detail_font, right - text_x);
                    let shaped = text.shape_line(&detail, &detail_font);
                    draw_text(
                        rows_layer,
                        &shaped,
                        text_x,
                        ds::optical_baseline(top + px(28.0), px(16.0), &detail_font),
                        if chosen { p.code_text } else { p.text_muted },
                    );
                }
            }
            self.hits.push((rect, Hit::Row(row)));
        }
        // The app's scrollbar when there are more rows than shown (a row its unit).
        self.unit = px(ROW_H);
        if rows > VISIBLE {
            let content = self.unit * rows as f32;
            let offset = self.unit * self.first as f32;
            self.bar.paint(scene, p, scale, list, content, offset);
        } else {
            self.bar = ui_components::EdgeScrollbar::default();
        }
        let layer = scene.push_layer(Some(panel));
        // Nothing found: said under "New Query Console".
        if self.found.is_empty() && !self.filter.text().trim().is_empty() && shown == 1 {
            let note = i18n::tr!("console.jump_none", query = self.filter.text().trim());
            let shaped = text.shape_line(&note, &small);
            draw_text(
                layer,
                &shaped,
                (panel.min_x() + (panel.size.width - shaped.width) / 2.0).round(),
                ds::optical_baseline(list.max_y(), pad, &small),
                p.text_muted,
            );
        }
        // The footer: the keys.
        let foot =
            Rect::new(panel.min_x(), panel.max_y() - px(FOOTER_H), panel.size.width, px(FOOTER_H));
        let (choose, open, close) = (t("keys.choose"), t("console.choose_open"), t("keys.close"));
        let hints: &[KeyHint<'_>] = &[(&["↑", "↓"], &choose), (&["↵"], &open), (&["esc"], &close)];
        ds::footer(layer, text, &ds, foot, hints);
    }

    /// A press on the list's scrollbar takes its thumb (a drag follows), on the filter puts
    /// its caret; elsewhere a row's or the popover's own.
    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            return None;
        }
        match self.hit(p) {
            // Outside it (a popover: nothing is lost) or on `esc`: it closes.
            None | Some(Hit::Esc) => return Some(ViewerAction::Close),
            Some(Hit::Row(row)) => {
                self.choose(row);
            }
            Some(Hit::Rename(row)) => {
                self.rename(row);
            }
            Some(Hit::Delete(row)) => {
                self.delete(row);
            }
            Some(Hit::Inside) => {}
        }
        self.action.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        let first = self.first;
        self.with_controls(|c, parts| c.drag(parts, p));
        self.first != first
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.press(p, 1)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let Some(Hit::Row(row) | Hit::Rename(row) | Hit::Delete(row)) = self.hit(p) else {
            return None;
        };
        if row == 0 {
            return None;
        }
        self.selected = row;
        Some(vec![
            ViewMenuEntry::item("open", t("console.choose_open")).with_icon("console"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item("rename", t("console.rename")).with_icon("file-pen"),
            ViewMenuEntry::item("delete", t("console.delete")).with_icon("trash"),
        ])
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let parts = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        let changed = hover != self.hover || parts;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::Inside => None,
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, hit) = self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
        let label = match hit {
            Hit::Rename(_) => t("console.rename"),
            Hit::Delete(_) => t("console.delete"),
            _ => return None,
        };
        Some(Tip::new(*rect, label))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let rows = (dy / (ROW_H * 0.8)).round() as isize;
        let max = self.rows().saturating_sub(VISIBLE) as isize;
        let first = (self.first as isize + rows).clamp(0, max) as usize;
        let moved = first != self.first;
        self.first = first;
        moved
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Enter => {
                let row = self.selected;
                self.choose(row);
            }
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Up => self.step(-1),
            ViewerKey::Down => self.step(1),
            ViewerKey::PageUp => self.step(-(VISIBLE as isize)),
            ViewerKey::PageDown => self.step(VISIBLE as isize),
            ViewerKey::Tab => self.step(if reach.extend { -1 } else { 1 }),
            // The filter's editing keys.
            key => {
                self.with_controls(|c, parts| c.key(parts, key, reach));
            }
        }
        true
    }

    fn selected_text(&self) -> Option<String> {
        self.controls.selected_text_with(|i| (i == FILTER).then_some(&self.filter as &dyn Control))
    }

    fn command(&mut self, id: &str) -> bool {
        let row = self.selected;
        match id {
            "open" => self.choose(row),
            "rename" => self.rename(row),
            "delete" => self.delete(row),
            id if id.starts_with("filter:") => {
                self.set_filter(&id["filter:".len()..]);
                true
            }
            // The filter's edit commands (⌘A, ⌘Z, ⇧⌘Z, ⌘X).
            id => self.with_controls(|c, parts| c.command(parts, id)).is_some(),
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/jump.rs"]
mod tests;
