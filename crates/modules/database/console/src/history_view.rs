//! The history window (plan 39i, the bar's 🕘, ⌥⌘E): a modal of the module's own
//! (`view-console-history`, `host.open_modal`: closed only by ×, Esc or its buttons) listing
//! the statements run on the console's source, newest first — each with how it ended, when,
//! how long, its rows and where it ran —, searched by the words typed (what matches lit),
//! the one chosen shown whole below. ↵ (or Insert) puts it into the console (after its
//! text, selected, never run), Run Again runs it as it is on the console's connection (its
//! parameters asked again: their values were never kept), Delete (or ⌦ at the field's end)
//! takes it out of the history. Clear History (its button, its menu) deletes the whole
//! source's history, its file too — asked first in the window's own footer ("Cancel" has
//! the keys: a stray ↵ loses nothing).
//!
//! The list is drawn only where it's seen (any number of entries); the entries are read
//! again only when the history changes (its generation), filtered only when the search or
//! they change.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use database::DataSourceId;
use database::console::ConsoleId;
use database::history::{HistoryEntry, HistoryId, Outcome, Search};
use i18n::t;
use kernel::{Point, ProjectId, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx, radius};
use ui_components::search_field::SearchField;
use ui_components::{
    EdgeScrollbar, KeyReach, Tip, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::{draw_icon, draw_text};
use workbench_model::WorkbenchHost;

use crate::field::SearchPart;
use crate::history::Histories;
use crate::runtime::Runtime;

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-console-history";

/// What the window is opened for: the console, its source, its file.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryRequest {
    pub project: ProjectId,
    pub console: ConsoleId,
    pub console_name: String,
    pub source: DataSourceId,
    pub source_name: String,
    pub color: Option<database::SourceColor>,
    /// The console's file (a statement inserted goes there).
    pub file: PathBuf,
}

/// What's done with the statement chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Put into the console (after its text, selected).
    Insert,
    /// Run as it is, on the console's connection.
    Run,
}

/// A statement chosen, waiting for [`PICKED`].
#[derive(Debug, Clone, PartialEq)]
pub struct Picked {
    pub pick: Pick,
    pub request: HistoryRequest,
    pub text: String,
}

/// What the statement chosen asks: put into its console, or run again.
fn act_on(host: &mut dyn WorkbenchHost, picked: &Picked) -> bool {
    match picked.pick {
        Pick::Insert => crate::profile::insert_into(host, &picked.request.file, &picked.text),
        Pick::Run => crate::commands::run_text(host, picked.request.console, &picked.text),
    }
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct HistoryProvider;

impl ViewerProvider for HistoryProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(HistoryView::default()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Row(usize),
    Insert,
    Run,
    Delete,
    /// Clear History…: asked first.
    Clear,
    /// The question's answers.
    ClearYes,
    ClearNo,
}

/// A row of the list (logical pixels).
pub const ROW_H: f32 = 50.0;
const FIELD_H: f32 = 36.0;
/// How far the count of what's found ends from the search's right edge (logical pixels):
/// with text, clear of its ×.
const COUNT_INSET: f32 = 14.0;
const COUNT_INSET_CLEAR: f32 = 34.0;
/// The search's place among the window's parts.
const SEARCH: usize = 1;
const FOOTER_H: f32 = 64.0;
/// A statement's row: the chosen one's accent alpha, the hovered one's text alpha.
const ROW_CHOSEN: f32 = 0.12;
const ROW_HOVER: f32 = 0.04;
/// The statement's box under the list: its lines at most, their height.
const PREVIEW_LINES: usize = 6;
const PREVIEW_ROW: f32 = 20.0;
/// The window's content (logical pixels).
pub const HEIGHT: f32 = 600.0;

/// Wakes the app.
type Waker = Arc<dyn Fn() + Send + Sync>;

/// The window's view.
#[derive(Default)]
pub struct HistoryView {
    request: Option<HistoryRequest>,
    histories: Option<Arc<Histories>>,
    runtime: Option<Runtime>,
    /// The source's entries, newest first (`None`: being read).
    entries: Option<Vec<HistoryEntry>>,
    /// The history's generation they were read at.
    seen: Option<u64>,
    /// The search (the app's search field: ×, ⌘A; it has the keys the whole time).
    query: SearchField,
    /// The entries shown (their indices), for the search and generation they were made for.
    shown: Vec<usize>,
    shown_for: Option<(String, u64)>,
    /// The entry chosen (in `shown`).
    selected: usize,
    scroll: f32,
    max_scroll: f32,
    list: Rect,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    action: Option<ViewerAction>,
    done: bool,
    woken: bool,
    /// The scale it was last drawn at (its keys move the list by rows).
    scale: f32,
    /// Clear History asked: the footer asks first (`Some(yes)`: whether the action's button
    /// has the keys — "Cancel" by default).
    clearing: Option<bool>,
    /// Its parts' state of use (see [`Self::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for HistoryView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistoryView").field("request", &self.request).finish_non_exhaustive()
    }
}

impl HistoryView {
    /// Its entries, as shown now (newest first, the search applied).
    pub fn shown(&self) -> Vec<&HistoryEntry> {
        let Some(entries) = &self.entries else { return Vec::new() };
        self.shown.iter().filter_map(|i| entries.get(*i)).collect()
    }

    /// The entry chosen.
    pub fn chosen(&self) -> Option<&HistoryEntry> {
        let i = *self.shown.get(self.selected)?;
        self.entries.as_ref()?.get(i)
    }

    /// The history is being read.
    pub fn loading(&self) -> bool {
        self.entries.is_none()
    }

    /// What's searched for.
    pub fn query(&self) -> &str {
        self.query.text()
    }

    /// The search typed (as if by the keyboard).
    pub fn set_query(&mut self, query: &str) {
        self.query.set_text(query);
        self.refilter();
    }

    /// Reads the entries again when the history changed, and filters them again when they
    /// or the search did. `true` if anything did.
    pub fn refresh(&mut self) -> bool {
        let (Some(histories), Some(request)) = (&self.histories, &self.request) else {
            return false;
        };
        let generation = histories.generation();
        let mut changed = false;
        if self.seen != Some(generation) || self.entries.is_none() {
            let entries = histories.entries(request.source);
            if entries.is_some() {
                self.seen = Some(generation);
            }
            changed = entries != self.entries;
            self.entries = entries;
        }
        self.refilter() || changed
    }

    fn refilter(&mut self) -> bool {
        let generation = self.seen.unwrap_or(u64::MAX);
        let key = (self.query.text().to_owned(), generation);
        if self.shown_for.as_ref() == Some(&key) {
            return false;
        }
        let chosen = self.chosen().map(|e| e.id);
        let search = Search::new(self.query.text());
        self.shown = self
            .entries
            .iter()
            .flatten()
            .enumerate()
            .filter(|(_, e)| search.matches(e))
            .map(|(i, _)| i)
            .collect();
        // The one chosen stays chosen while it's shown; a new search starts at the top.
        let same_search = self.shown_for.as_ref().is_some_and(|(q, _)| *q == key.0);
        self.selected = if same_search {
            chosen
                .and_then(|id| self.shown().iter().position(|e| e.id == id))
                .unwrap_or(self.selected.min(self.shown.len().saturating_sub(1)))
        } else {
            0
        };
        if !same_search {
            self.scroll = 0.0;
        }
        self.shown_for = Some(key);
        true
    }

    fn step(&mut self, delta: isize) {
        if !self.shown.is_empty() {
            self.selected = ui_kit::step_index(self.selected, delta, self.shown.len());
            self.keep_in_view();
        }
    }

    fn keep_in_view(&mut self) {
        let row = ROW_H * self.scale_hint();
        let top = self.selected as f32 * row;
        let height = self.list.size.height;
        if top < self.scroll {
            self.scroll = top;
        } else if height > 0.0 && top + row > self.scroll + height {
            self.scroll = top + row - height;
        }
    }

    /// The scale the list was last drawn at (rows' pixels).
    fn scale_hint(&self) -> f32 {
        if self.scale > 0.0 { self.scale } else { 1.0 }
    }

    /// The statement chosen put into the console (↵, Insert) or run (Run Again): the
    /// window closes.
    pub fn choose(&mut self, pick: Pick) -> bool {
        let (Some(request), Some(entry)) = (self.request.clone(), self.chosen().cloned()) else {
            return false;
        };
        let picked = Picked { pick, request, text: entry.text };
        self.done = true;
        self.action = Some(ViewerAction::WithHost(ui_components::WithHost::once(move |host| {
            act_on(host, &picked);
        })));
        true
    }

    /// The statement chosen taken out of the history.
    pub fn delete(&mut self) -> bool {
        let (Some(histories), Some(request)) = (self.histories.clone(), self.request.clone())
        else {
            return false;
        };
        let Some(id) = self.chosen().map(|e| e.id) else { return false };
        let removed = histories.remove(request.source, id);
        self.refresh();
        removed
    }

    /// Clear History asked: the footer asks first (nothing to clear: nothing asked).
    pub fn ask_clear(&mut self) -> bool {
        if self.entries.as_ref().is_none_or(Vec::is_empty) {
            return false;
        }
        self.clearing = Some(false);
        true
    }

    /// The footer asks whether to clear the history.
    pub fn asking_clear(&self) -> bool {
        self.clearing.is_some()
    }

    /// The question answered: the source's whole history deleted (`yes`, its file too) or
    /// kept.
    pub fn answer_clear(&mut self, yes: bool) -> bool {
        if self.clearing.take().is_none() {
            return false;
        }
        if !yes {
            return true;
        }
        let (Some(histories), Some(request)) = (self.histories.clone(), self.request.clone())
        else {
            return false;
        };
        let cleared = histories.clear(request.source);
        self.refresh();
        cleared
    }

    /// Entry `id` chosen (the tests', the snapshots').
    pub fn choose_entry(&mut self, id: HistoryId) -> bool {
        match self.shown().iter().position(|e| e.id == id) {
            Some(at) => {
                self.selected = at;
                self.keep_in_view();
                true
            }
            None => false,
        }
    }

    /// Its parts (the list's scrollbar, the search), the topmost first, with their state of
    /// use. The search has the keys the whole time (while Clear History asks, the question
    /// takes them first), its editing ones its own, the list's (↑ ↓, ↵, ⌦ at its end) the
    /// view's; what's shown follows what it changed.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut edited = false;
        let mut list = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut query = SearchPart { field: &mut self.query, edited: &mut edited };
        let mut parts: [&mut dyn Control; 2] = [&mut list, &mut query];
        self.controls.focus(&mut parts, Some(SEARCH));
        let answer = f(&mut self.controls, &mut parts);
        if edited {
            self.refilter();
        }
        answer
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// An entry's second line: when, how long, its rows, where.
    fn detail(&self, e: &HistoryEntry) -> String {
        let mut parts = Vec::new();
        if let Some(runtime) = &self.runtime {
            parts.push(runtime.time_text(e.at_ms));
        }
        if let Some(ms) = e.duration_ms {
            parts.push(crate::runner::millis(std::time::Duration::from_millis(ms)));
        }
        match (e.outcome, e.rows) {
            (Outcome::Ok, Some(rows)) => {
                parts.push(i18n::tr!("console.history_rows", count = rows))
            }
            (Outcome::Failed, _) => parts.push(t("console.history_failed")),
            (Outcome::Cancelled, _) => parts.push(t("console.history_cancelled")),
            _ => {}
        }
        if let Some(place) = e.place() {
            parts.push(place);
        }
        parts.join(" · ")
    }
}

impl HistoryView {
    /// The footer asking whether to clear the history: what's lost on the left, "Cancel"
    /// and "Clear History" (in the danger color) on the right, the keys' button ringed.
    fn paint_clear_question(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        row: Rect,
        on_yes: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let cancel = t("dialog.cancel");
        let yes = t("console.history_clear_yes");
        let bw = ds::primary_width(text, ds, &[&cancel, &yes]);
        let y = Rect::new(row.max_x() - bw, row.min_y(), bw, row.size.height);
        let n = Rect::new(y.min_x() - px(10.0) - bw, row.min_y(), bw, row.size.height);
        ds::secondary_button(layer, text, ds, n, &cancel, true, self.hover == Some(Hit::ClearNo));
        ds::danger_button(layer, text, ds, y, &yes, true, self.hover == Some(Hit::ClearYes));
        self.hits.push((n, Hit::ClearNo));
        self.hits.push((y, Hit::ClearYes));
        ds::focus_ring(layer, ds, if on_yes { y } else { n }, px(radius::MD));
        let count = self.entries.as_ref().map_or(0, Vec::len);
        let source = self.request.as_ref().map_or("", |r| r.source_name.as_str());
        let message = i18n::tr!("console.history_clear_ask", count = count, source = source);
        let font = ds.ui_font(-0.5);
        let shown =
            ds::ellipsize(text, &message, &font, (n.min_x() - px(14.0) - row.min_x()).max(0.0));
        let shaped = text.shape_line(&shown, &font);
        draw_text(
            layer,
            &shaped,
            row.min_x().round(),
            ds::optical_baseline(row.min_y(), row.size.height, &font),
            p.text,
        );
    }
}

/// The icon and color of how a run ended.
fn outcome_icon(outcome: Outcome, p: &ui_components::Palette) -> (&'static str, kernel::Rgba) {
    match outcome {
        Outcome::Ok => ("check", p.vcs_added),
        Outcome::Failed => ("circle-alert", p.danger),
        Outcome::Cancelled => ("stop", p.warning),
    }
}

impl Viewer for HistoryView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(request) = data.downcast_ref::<HistoryRequest>() else { return };
        let consoles = crate::consoles::consoles(request.project);
        self.histories = consoles.as_ref().map(|c| c.history());
        self.runtime = consoles.as_ref().map(|c| c.runtime());
        self.request = Some(request.clone());
        self.entries = None;
        self.seen = None;
        self.shown_for = None;
        self.refresh();
        // The search has the keys from now (its selection the copy's).
        self.controls = Controls::sticky();
        self.with_controls(|_, _| ());
    }

    fn subtitle(&self) -> Option<String> {
        let r = self.request.as_ref()?;
        Some(i18n::tr!(
            "console.confirm_subtitle",
            console = r.console_name.as_str(),
            source = r.source_name.as_str()
        ))
    }

    fn modal_height(&self) -> Option<f32> {
        Some(HEIGHT)
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("clock")
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        // The question in the footer: its own buttons say what the keys do.
        if self.clearing.is_some() {
            return Vec::new();
        }
        vec![
            (vec!["↑", "↓"], t("keys.choose")),
            (vec!["↵"], t("console.history_insert")),
            (vec!["⌦"], t("console.history_delete")),
        ]
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn wants_text(&self) -> bool {
        true
    }

    /// To the search (nothing while the question asks).
    fn text_input(&mut self, typed: &str) -> bool {
        if self.clearing.is_none() {
            self.with_controls(|c, parts| c.text(parts, typed));
        }
        true
    }

    fn poll(&mut self) -> bool {
        self.refresh()
    }

    fn is_loading(&self) -> bool {
        self.entries.is_none() && self.request.is_some()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, waker, .. } = ctx;
        self.scale = scale;
        if !self.woken
            && let (Some(histories), Some(wake)) = (&self.histories, waker)
        {
            let wake: Waker = wake;
            histories.add_wake("history-view", wake);
            self.woken = true;
        }
        self.refresh();
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.hits.clear();
        let pad = px(20.0);
        let left = b.min_x() + pad;
        let right = b.max_x() - pad;
        let layer = scene.push_layer(Some(b));
        // The search, how many it found at its right.
        let field = Rect::new(left, b.min_y() + px(4.0), right - left, px(FIELD_H));
        let placeholder = t("console.history_search");
        self.query.paint(layer, text, icons, &ds, (field, true), &placeholder);
        let total = self.entries.as_ref().map_or(0, Vec::len);
        if total > 0 {
            let count = if self.query.text().trim().is_empty() {
                i18n::tr!("console.history_count", count = total)
            } else {
                i18n::tr!("console.history_found", found = self.shown.len(), count = total)
            };
            let small = ds.ui_font(-1.0);
            let shaped = text.shape_line(&count, &small);
            let inset = if self.query.text().is_empty() { COUNT_INSET } else { COUNT_INSET_CLEAR };
            draw_text(
                layer,
                &shaped,
                (field.max_x() - px(inset) - shaped.width).round(),
                ds::optical_baseline(field.min_y(), field.size.height, &small),
                p.text_muted,
            );
        }
        // The list, the statement chosen under it, the buttons.
        let preview_h = px(PREVIEW_LINES as f32 * PREVIEW_ROW + 20.0);
        let list_top = field.max_y() + px(12.0);
        let list_bottom = b.max_y() - px(FOOTER_H) - preview_h - px(12.0);
        let list = Rect::new(
            b.min_x() + px(12.0),
            list_top,
            b.size.width - px(24.0),
            (list_bottom - list_top).max(px(ROW_H)),
        );
        self.list = list;
        let row_h = px(ROW_H);
        let shown = self.shown.len();
        let content = shown as f32 * row_h;
        self.max_scroll = (content - list.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let search = Search::new(self.query.text());
        match &self.entries {
            None => ds::empty_state(layer, text, &ds, list, &t("console.history_loading"), None),
            Some(entries) if entries.is_empty() => ds::empty_state(
                layer,
                text,
                &ds,
                list,
                &i18n::tr!(
                    "console.history_empty",
                    source = self.request.as_ref().map_or("", |r| r.source_name.as_str())
                ),
                Some(&t("console.history_empty_detail")),
            ),
            Some(_) if shown == 0 => ds::empty_state(
                layer,
                text,
                &ds,
                list,
                &i18n::tr!("console.history_none_found", query = self.query.text().trim()),
                None,
            ),
            Some(entries) => {
                let rows = scene.push_layer(Some(list));
                let first = (self.scroll / row_h).floor() as usize;
                let visible = (list.size.height / row_h).ceil() as usize + 1;
                let code_font = ds.code_font(-0.5);
                let small = ds.ui_font(-1.5);
                for (k, &i) in self.shown.iter().enumerate().skip(first).take(visible) {
                    let Some(entry) = entries.get(i) else { continue };
                    let top = list.min_y() + k as f32 * row_h - self.scroll;
                    let rect = Rect::new(list.min_x(), top, list.size.width, row_h);
                    let chosen = k == self.selected;
                    let hovered = self.hover == Some(Hit::Row(k));
                    let lit = Rect::new(
                        rect.min_x(),
                        rect.min_y() + px(2.0),
                        rect.size.width,
                        row_h - px(4.0),
                    );
                    ds::RowHighlight::new(chosen, hovered)
                        .selected_alpha(ROW_CHOSEN)
                        .hover(ds::HoverFill::Faint(ROW_HOVER))
                        .paint(rows, &ds, lit);
                    let (icon, color) = outcome_icon(entry.outcome, p);
                    if let Some(id) = icons.id(icon) {
                        let s = px(14.0);
                        let r = Rect::new(
                            (rect.min_x() + px(12.0)).round(),
                            (top + px(10.0) + (px(18.0) - s) / 2.0).round(),
                            s,
                            s,
                        );
                        draw_icon(rows, icons, id, r, color);
                    }
                    let x = rect.min_x() + px(36.0);
                    let max_x = rect.max_x() - px(12.0);
                    // Its first line (the text as one line: what matched lit).
                    let line = kernel::text::one_line(&entry.text).into_owned();
                    let lit = search.places(&line);
                    crate::listing::lit_code(
                        rows,
                        text,
                        &ds,
                        (&line, &code_font),
                        (x, max_x),
                        (top + px(5.0), px(20.0)),
                        &lit,
                    );
                    let detail = ds::ellipsize(text, &self.detail(entry), &small, max_x - x);
                    let shaped = text.shape_line(&detail, &small);
                    draw_text(
                        rows,
                        &shaped,
                        x.round(),
                        ds::optical_baseline(top + px(26.0), px(18.0), &small),
                        if chosen { p.text } else { p.text_muted },
                    );
                    self.hits.push((rect.intersect(&list).unwrap_or(rect), Hit::Row(k)));
                }
            }
        }
        self.bar.paint(scene, p, scale, list, content, self.scroll);
        let layer = scene.push_layer(Some(b));
        // The statement chosen, whole.
        let card = Rect::new(left, list.max_y() + px(12.0), right - left, preview_h);
        layer.quad(
            Quad::filled(card, p.editor_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.border),
        );
        if let Some(entry) = self.chosen().cloned() {
            let inner = Rect::new(
                card.min_x() + px(14.0),
                card.min_y() + px(10.0),
                card.size.width - px(28.0),
                card.size.height - px(20.0),
            );
            let code = scene.push_layer(Some(inner));
            let font = ds.code_font(-1.0);
            let lit = search.places(&entry.text);
            let mut at = 0;
            for (k, line) in entry.text.split('\n').enumerate() {
                if k >= PREVIEW_LINES {
                    break;
                }
                let line_lit = crate::listing::in_line(&lit, at, line);
                at += line.len() + 1;
                let line = line.trim_end_matches('\r');
                crate::listing::lit_code(
                    code,
                    text,
                    &ds,
                    (line, &font),
                    (inner.min_x(), inner.max_x()),
                    (inner.min_y() + k as f32 * px(PREVIEW_ROW), px(PREVIEW_ROW)),
                    &line_lit,
                );
            }
        }
        let layer = scene.push_layer(Some(b));
        let bh = px(ds::BUTTON_H);
        let by = b.max_y() - px(FOOTER_H) + (px(FOOTER_H) - bh) / 2.0;
        if let Some(on_yes) = self.clearing {
            self.paint_clear_question(
                layer,
                text,
                &ds,
                Rect::new(left, by, right - left, bh),
                on_yes,
            );
            return;
        }
        // The buttons: Delete and Clear History on the left; Run Again, Insert on the right.
        let enabled = self.chosen().is_some();
        let insert = t("console.history_insert");
        let w = ds::primary_width(text, &ds, &[&insert]);
        let g = Rect::new(right - w, by, w, bh);
        ds::primary_button(layer, text, &ds, g, &insert, enabled, self.hover == Some(Hit::Insert));
        self.hits.push((g, Hit::Insert));
        let run = t("console.history_run");
        let rw = ds::primary_width(text, &ds, &[&run]);
        let r = Rect::new(g.min_x() - px(10.0) - rw, by, rw, bh);
        ds::secondary_button(layer, text, &ds, r, &run, enabled, self.hover == Some(Hit::Run));
        self.hits.push((r, Hit::Run));
        let delete = t("console.history_delete");
        let dw = ds::primary_width(text, &ds, &[&delete]);
        let d = Rect::new(left, by, dw, bh);
        ds::secondary_button(
            layer,
            text,
            &ds,
            d,
            &delete,
            enabled,
            self.hover == Some(Hit::Delete),
        );
        self.hits.push((d, Hit::Delete));
        let clear = t("console.history_clear");
        let cw = ds::primary_width(text, &ds, &[&clear]);
        let c = Rect::new(d.max_x() + px(10.0), by, cw, bh);
        let any = total > 0;
        ds::secondary_button(layer, text, &ds, c, &clear, any, self.hover == Some(Hit::Clear));
        self.hits.push((c, Hit::Clear));
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // The question in the footer: only its buttons answer.
        if self.clearing.is_some() {
            match self.hit(p) {
                Some(Hit::ClearYes) => {
                    self.answer_clear(true);
                }
                Some(Hit::ClearNo) => {
                    self.answer_clear(false);
                }
                _ => {}
            }
            return None;
        }
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        match self.hit(p) {
            Some(Hit::Row(k)) => self.selected = k,
            Some(Hit::Insert) => {
                self.choose(Pick::Insert);
            }
            Some(Hit::Run) => {
                self.choose(Pick::Run);
            }
            Some(Hit::Delete) => {
                self.delete();
            }
            Some(Hit::Clear) => {
                self.ask_clear();
            }
            Some(Hit::ClearYes | Hit::ClearNo) | None => {}
        }
        self.action.take()
    }

    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        // A double click on a statement inserts it.
        if clicks >= 2
            && self.clearing.is_none()
            && let Some(Hit::Row(k)) = self.hit(p)
        {
            self.selected = k;
            self.choose(Pick::Insert);
            return self.action.take();
        }
        self.click(p)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if self.clearing.is_some() {
            return None;
        }
        let Some(Hit::Row(k)) = self.hit(p) else { return None };
        self.selected = k;
        Some(vec![
            ViewMenuEntry::item("insert", t("console.history_insert")).with_icon("file-pen"),
            ViewMenuEntry::item("run", t("console.history_run")).with_icon("play"),
            ViewMenuEntry::item("copy", t("console.history_copy")).with_icon("copy"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item("delete", t("console.history_delete")).with_icon("trash"),
            ViewMenuEntry::item("clear", t("console.history_clear")),
        ])
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some()
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self
            .hit(p)
            .filter(|h| self.clearing.is_none() || matches!(h, Hit::ClearYes | Hit::ClearNo));
        let changed = hover != self.hover;
        self.hover = hover;
        changed | self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.clearing.is_none()
            && let Some(cursor) = self.controls.cursor_at(p)
        {
            return Some(cursor);
        }
        match self.hit(p)? {
            Hit::ClearYes | Hit::ClearNo => Some(ViewerCursor::Pointer),
            _ if self.clearing.is_some() => None,
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (rect, hit) = self.hits.iter().rev().find(|(r, _)| r.contains(p))?;
        let Hit::Row(k) = hit else { return None };
        if self.clearing.is_some() {
            return None;
        }
        let i = *self.shown.get(*k)?;
        let entry = self.entries.as_ref()?.get(i)?;
        // The whole statement when its first line doesn't say it all.
        let text = database::console::summary(&entry.text, 400);
        Some(Tip::new(*rect, text))
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        if self.clearing.is_some() {
            return false;
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        before != self.scroll
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The question in the footer has every key: Esc / ↵ answer, Tab / ← / → move
        // between its buttons.
        if let Some(on_yes) = self.clearing {
            match key {
                ViewerKey::Escape => {
                    self.answer_clear(false);
                }
                ViewerKey::Enter => {
                    self.answer_clear(on_yes);
                }
                ViewerKey::Tab | ViewerKey::Left | ViewerKey::Right => {
                    self.clearing = Some(!on_yes);
                }
                _ => {}
            }
            return true;
        }
        match key {
            ViewerKey::Enter => {
                self.choose(Pick::Insert);
            }
            ViewerKey::Escape => self.action = Some(ViewerAction::Close),
            ViewerKey::Up => self.step(-1),
            ViewerKey::Down => self.step(1),
            ViewerKey::PageUp => self.step(-8),
            ViewerKey::PageDown => self.step(8),
            ViewerKey::Tab => return false,
            // The search's editing keys; ⌦ at its end (nothing to delete in it): the
            // statement chosen.
            key => {
                let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
                if !used && key == ViewerKey::Delete {
                    self.delete();
                }
            }
        }
        true
    }

    /// The search's selection, else the entry chosen.
    fn selected_text(&self) -> Option<String> {
        let search = |i| (i == SEARCH).then_some(&self.query as &dyn Control);
        self.controls.selected_text_with(search).or_else(|| self.chosen().map(|e| e.text.clone()))
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "insert" => self.choose(Pick::Insert),
            "run" => self.choose(Pick::Run),
            "delete" => self.delete(),
            "clear" => self.ask_clear(),
            "copy" => {
                let Some(text) = self.chosen().map(|e| e.text.clone()) else { return false };
                self.action =
                    Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text)));
                true
            }
            id if id.starts_with("search:") => {
                self.set_query(&id["search:".len()..]);
                true
            }
            // The search's edit commands (⌘A, ⌘Z, ⇧⌘Z, ⌘X).
            id => self.with_controls(|c, parts| c.command(parts, id)).is_some(),
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

#[cfg(test)]
#[path = "../tests/unit/history_view.rs"]
mod tests;
