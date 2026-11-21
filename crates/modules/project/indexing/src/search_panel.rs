//! The "Search in Project" panel (⌘⇧F): a search field and the results (file, line,
//! code snippet), over the app. The index does the search; the panel only displays (its
//! view, `search_view`, does the rest).

use i18n::{t, tr};
use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Scene};
use text_layout::{Font, FontMetrics, TextSystem};
use text_search::Hit;
use ui_components::control::Control;
use ui_components::search_field::SearchField;
use ui_kit::{KeyHint, TextInput, centered_baseline, draw_icon, draw_text};
use workspace::FileMatch;

use ui_components::ds::{self, Ctx, Elevation, Tone, radius, space};
use ui_components::{Painter, Palette};

use crate::semantic::SearchMode;

mod field_paint;
mod fields;
mod list;
mod rows;

use rows::{paint_file_row, paint_header, paint_name_row, paint_recent_row};

/// The search's options: case (`Cc`), whole words (`W`), a regular expression (`.*`). Any
/// of them on: every occurrence is found (`exact`), not the index's best.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

impl SearchOptions {
    pub fn exact(&self) -> bool {
        self.case || self.word || self.regex
    }
}

/// One of [`SearchOptions`], as its button in the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchOption {
    Case,
    Word,
    Regex,
}

impl SearchOption {
    /// The buttons, left to right, with their labels.
    const ALL: [(Self, &'static str); 3] =
        [(Self::Case, "Cc"), (Self::Word, "W"), (Self::Regex, ".*")];

    /// Its tooltip's key.
    pub fn tooltip_key(self) -> &'static str {
        match self {
            Self::Case => "search.option_case",
            Self::Word => "search.option_word",
            Self::Regex => "search.option_regex",
        }
    }
}

/// An exact search's state: still reading the files; stopped at its limit (there are more).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExactState {
    pub done: bool,
    pub capped: bool,
}

/// What the user can do in the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectSearchAction {
    Open(usize),
    /// A file whose name matches: opened.
    OpenNamed(usize),
    /// A matching folder: shown in the project tree.
    OpenFolder(usize),
    /// A recent search: it is put into the field.
    Recent(usize),
    /// A recent search's ×: it's forgotten; the header's "Clear all": every one.
    ForgetRecent(usize),
    ClearRecent,
    /// Downloads the model for semantic search.
    EnableSemantic,
    /// The folder's button: its row shown (or hidden, when no folder is set).
    ToggleScope,
    /// A click in the search's field or the folder's: the keyboard goes there.
    FocusQuery,
    FocusScope,
    FocusMask,
    /// A folder suggested for the one typed: it's taken.
    Suggestion(usize),
    /// One of the search's options turned on or off.
    Toggle(SearchOption),
    /// The replacement's button: its row shown or hidden; a click in its field; every
    /// occurrence found replaced.
    ToggleReplace,
    FocusReplace,
    ReplaceAll,
    Close,
}

#[derive(Debug, Default)]
pub struct ProjectSearch {
    open: bool,
    /// What's searched: the app's search field (its × empties it).
    pub input: SearchField,
    /// The folder searched in, relative to the project (empty: all of it).
    pub scope: TextInput,
    /// The files searched, as a mask (`*.php, !*.min.js`; empty: all of them), beside the
    /// folder; the keyboard in it; where its field was drawn.
    pub mask: TextInput,
    in_mask: bool,
    mask_field: Rect,
    /// The folder's row shows (its button pressed, or a folder given).
    scope_open: bool,
    /// The keyboard is in the folder's field (else in the search's).
    in_scope: bool,
    /// The folders the one typed goes on to, and the one lit among them.
    suggestions: Vec<String>,
    suggestion: usize,
    /// Where the folder's button, the two fields and the suggestions were drawn.
    scope_button: Rect,
    scope_hover: bool,
    hover_replace: bool,
    hover_replace_all: bool,
    query_field: Rect,
    scope_field: Rect,
    suggestion_rows: Vec<(Rect, usize)>,
    /// What replaces what's found, whether its row shows, the keyboard in it; its button in
    /// the field, its field and its Replace All (whether there's something to replace).
    pub replace: TextInput,
    replace_open: bool,
    in_replace: bool,
    replace_button: Rect,
    replace_field: Rect,
    replace_all: Rect,
    can_replace: bool,
    /// The search's options, their buttons in the field (and the one under the mouse).
    pub options: SearchOptions,
    option_buttons: Vec<(Rect, SearchOption)>,
    option_hover: Option<SearchOption>,
    /// What's typed isn't a pattern (a regular expression's mistake): why.
    error: Option<String>,
    /// An exact search's state (`None`: the index's search).
    exact: Option<ExactState>,
    results: Vec<Hit>,
    /// The files whose name matches (first: what's searched is often a file), then the
    /// folders whose name does — each in its own section, above what the files hold.
    named: Vec<FileMatch>,
    folders: Vec<FileMatch>,
    /// In the list: the files by name, the folders, then what the files hold (or the
    /// recent searches).
    /// The named files are a file name's pattern's list: they scroll (none fixed).
    listing: bool,
    selected: usize,
    first: usize,
    viewport: Rect,
    scale: f32,
    panel: Rect,
    /// The drawn rows and the list index each one shows.
    rows: Vec<(Rect, usize)>,
    /// The names cut short to fit (where, the whole of it: their tooltips).
    tips: Vec<(Rect, String)>,
    enable: Rect,
    /// The `esc` key in the field (clickable: closes).
    esc: Rect,
    esc_hover: bool,
    hover: Option<usize>,
    /// Recent searches (those after which a result was opened), newest first.
    recent: Vec<String>,
    /// The drawn × of the recent searches (where, which one) and the one under the mouse;
    /// "Clear all" in their header and the mouse on it.
    forget_buttons: Vec<(Rect, usize)>,
    forget_hover: Option<usize>,
    clear_all: Rect,
    clear_hover: bool,
    /// The panel's appearance (fade and a slight slide), as in settings.
    appear: Option<ui_components::anim::Tween>,
    /// The scrolled rows' scrollbar (the app's: thin, wider under the mouse, dragged), and
    /// where it was drawn: the rows' area, a row's height, how many rows.
    bar: ui_components::EdgeScrollbar,
    bar_area: Option<(Rect, f32, usize)>,
}

const VISIBLE: usize = 7;
/// The scrolled rows shown when files or folders by name take the top of the list.
const VISIBLE_UNDER_FOLDERS: usize = 4;
/// The files a file name's pattern lists shown at once (one line each: as many as fit in
/// the results' height, under their header).
const LISTED_VISIBLE: usize = 9;
const RECENT: usize = 6;
/// The folders suggested for the one typed, at most.
pub const SUGGESTIONS: usize = 6;

/// A result's representative line for `query`: the first line containing a word
/// from the query (otherwise the first non-empty one). Returns (0-based line in the file,
/// its text).
pub fn focus_line<'a>(hit: &'a Hit, query: &str) -> (u32, &'a str) {
    let words: Vec<String> =
        query.split_whitespace().filter(|w| w.len() > 1).map(str::to_lowercase).collect();
    let lines = || hit.preview.lines().enumerate();
    let found = lines()
        .find(|(_, l)| {
            let l = l.to_lowercase();
            words.iter().any(|w| l.contains(w.as_str()))
        })
        .or_else(|| lines().find(|(_, l)| !l.trim().is_empty()));
    match found {
        Some((i, l)) => (hit.start_line + i as u32, l.trim()),
        None => (hit.start_line, ""),
    }
}

impl ProjectSearch {
    pub fn open(&mut self) {
        if !self.open {
            let mut appear =
                ui_components::anim::Tween::new(0.0, std::time::Duration::from_millis(150));
            appear.retarget(1.0);
            self.appear = Some(appear);
        }
        self.open = true;
        self.input.input.select_all();
        self.selected = 0;
        self.first = 0;
    }

    pub fn set_results(&mut self, results: Vec<Hit>) {
        self.results = results;
        self.selected = 0;
        self.first = 0;
    }

    /// The files and the folders whose name matches (a few, over the text's results).
    pub fn set_named(&mut self, files: Vec<FileMatch>, folders: Vec<FileMatch>) {
        self.named = files;
        self.folders = folders;
        self.listing = false;
        self.selected = 0;
        self.first = 0;
    }

    /// The files a file name's pattern found (`*.png`: maybe hundreds): the rows that
    /// scroll, the window as tall as ever.
    pub fn set_listed(&mut self, files: Vec<FileMatch>) {
        self.named = files;
        self.folders.clear();
        self.results.clear();
        self.listing = true;
        self.selected = 0;
        self.first = 0;
    }

    pub fn folder(&self, index: usize) -> Option<&FileMatch> {
        self.folders.get(index)
    }

    pub fn named(&self, index: usize) -> Option<&FileMatch> {
        self.named.get(index)
    }

    /// The rows by name, above the scrolled ones (always shown).
    fn fixed(&self) -> usize {
        if self.listing { self.folders.len() } else { self.named.len() + self.folders.len() }
    }

    pub fn result(&self, index: usize) -> Option<&Hit> {
        self.results.get(index)
    }

    /// The first of the scrolled rows shown (a test's view of the scrolling).
    #[cfg(test)]
    pub(crate) fn first_row(&self) -> usize {
        self.first
    }

    pub fn recent(&self, index: usize) -> Option<&str> {
        self.recent.get(index).map(String::as_str)
    }

    /// The recent searches (newest first).
    pub(crate) fn recent_searches(&self) -> &[String] {
        &self.recent
    }

    pub(crate) fn set_recent_searches(&mut self, recent: Vec<String>) {
        self.recent = recent;
    }

    /// Keeps the current search among the recent ones (after a result was opened).
    pub fn remember(&mut self) {
        let query = self.input.text().trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.recent.retain(|q| *q != query);
        self.recent.insert(0, query);
        self.recent.truncate(RECENT);
    }

    /// Recent search `index` forgotten (the selection kept in the list).
    pub fn forget_recent(&mut self, index: usize) {
        if index >= self.recent.len() {
            return;
        }
        self.recent.remove(index);
        self.selected = self.selected.min(self.recent.len().saturating_sub(1));
        self.first = self.first.min(self.recent.len().saturating_sub(VISIBLE));
        self.forget_hover = None;
    }

    /// Every recent search forgotten.
    pub fn clear_recent(&mut self) {
        self.recent.clear();
        self.selected = 0;
        self.first = 0;
        self.forget_hover = None;
        self.clear_hover = false;
    }

    /// The field is empty: recent searches are shown.
    pub fn is_animating(&self) -> bool {
        self.open && self.appear.is_some_and(|a| a.animating())
    }

    pub fn showing_recent(&self) -> bool {
        self.input.text().trim().is_empty()
    }

    fn count(&self) -> usize {
        if self.showing_recent() {
            self.recent.len()
        } else if self.listing {
            self.named.len()
        } else {
            self.fixed() + self.results.len()
        }
    }

    /// How many file rows fit (fewer when folders are listed above them).
    fn file_rows(&self) -> usize {
        if self.fixed() == 0 { VISIBLE } else { VISIBLE_UNDER_FOLDERS }
    }

    /// The rows that scroll: the recent searches or the files (the folders stay put).
    fn scrolled(&self) -> (usize, usize) {
        if self.showing_recent() {
            (self.recent.len(), VISIBLE)
        } else if self.listing {
            (self.named.len(), LISTED_VISIBLE)
        } else {
            (self.results.len(), self.file_rows())
        }
    }

    /// What Enter / a click on list index `index` does.
    pub fn action(&self, index: usize) -> Option<ProjectSearchAction> {
        if self.showing_recent() {
            return (index < self.recent.len()).then_some(ProjectSearchAction::Recent(index));
        }
        if self.listing {
            return (index < self.named.len()).then_some(ProjectSearchAction::OpenNamed(index));
        }
        let (named, fixed) = (self.named.len(), self.fixed());
        if index < named {
            Some(ProjectSearchAction::OpenNamed(index))
        } else if index < fixed {
            Some(ProjectSearchAction::OpenFolder(index - named))
        } else {
            (index - fixed < self.results.len()).then_some(ProjectSearchAction::Open(index - fixed))
        }
    }

    /// The whole name of one cut short under `p` (its tooltip).
    pub fn tip_at(&self, p: Point) -> Option<(Rect, String)> {
        self.tips.iter().find(|(r, _)| r.contains(p)).cloned()
    }

    /// The action of the selected row.
    pub fn selected_action(&self) -> Option<ProjectSearchAction> {
        self.action(self.selected)
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.count();
        if count == 0 {
            return;
        }
        self.selected = ui_kit::step_index(self.selected, delta, count);
        // Only what the files hold scrolls; the rows by name above it are always shown.
        let fixed = if self.showing_recent() { 0 } else { self.fixed() };
        let Some(row) = self.selected.checked_sub(fixed) else { return };
        let visible = self.scrolled().1;
        if row < self.first {
            self.first = row;
        } else if row >= self.first + visible {
            self.first = row + 1 - visible;
        }
    }

    pub fn set_viewport(&mut self, viewport: Rect, scale: f32) {
        self.viewport = viewport;
        self.scale = scale;
    }

    pub fn hit(&self, p: Point) -> Option<ProjectSearchAction> {
        // Outside the panel: nothing (it closes only with esc).
        if !self.open || !self.panel.contains(p) {
            return None;
        }
        if self.esc.contains(p) {
            return Some(ProjectSearchAction::Close);
        }
        if self.scope_button.contains(p) {
            return Some(ProjectSearchAction::ToggleScope);
        }
        if let Some((_, option)) = self.option_at(p) {
            return Some(ProjectSearchAction::Toggle(option));
        }
        if self.replace_button.contains(p) {
            return Some(ProjectSearchAction::ToggleReplace);
        }
        if self.replace_open && self.replace_all.contains(p) && self.can_replace {
            return Some(ProjectSearchAction::ReplaceAll);
        }
        if self.replace_open && self.replace_field.contains(p) {
            return Some(ProjectSearchAction::FocusReplace);
        }
        if let Some((_, i)) = self.suggestion_rows.iter().find(|(r, _)| r.contains(p)) {
            return Some(ProjectSearchAction::Suggestion(*i));
        }
        if self.scope_open && self.mask_field.contains(p) {
            return Some(ProjectSearchAction::FocusMask);
        }
        if self.scope_open && self.scope_field.contains(p) {
            return Some(ProjectSearchAction::FocusScope);
        }
        if self.query_field.contains(p) {
            return Some(ProjectSearchAction::FocusQuery);
        }
        if self.enable.contains(p) {
            return Some(ProjectSearchAction::EnableSemantic);
        }
        if self.showing_recent() {
            if self.clear_all.contains(p) {
                return Some(ProjectSearchAction::ClearRecent);
            }
            if let Some((_, i)) = self.forget_buttons.iter().find(|(r, _)| r.contains(p)) {
                return Some(ProjectSearchAction::ForgetRecent(*i));
            }
        }
        let (_, i) = self.rows.iter().find(|(r, _)| r.contains(p))?;
        self.action(*i)
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.rows.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i);
        let esc = self.esc.contains(p);
        let scope = self.scope_button.contains(p);
        let option = self.option_at(p).map(|(_, o)| o);
        let replace = self.replace_button.contains(p);
        let replace_all = self.replace_open && self.replace_all.contains(p);
        let forget = self.forget_buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, i)| *i);
        let clear = self.clear_all.contains(p);
        // The search's × lit under the mouse.
        let field = Control::hover(&mut self.input, Some(p));
        let changed = field
            || forget != self.forget_hover
            || clear != self.clear_hover
            || hover != self.hover
            || esc != self.esc_hover
            || scope != self.scope_hover
            || option != self.option_hover
            || replace != self.hover_replace
            || replace_all != self.hover_replace_all;
        self.forget_hover = forget;
        self.clear_hover = clear;
        self.hover_replace = replace;
        self.hover_replace_all = replace_all;
        self.option_hover = option;
        self.hover = hover;
        self.esc_hover = esc;
        self.scope_hover = scope;
        changed
    }

    /// The mouse is on the `esc` key, the folder's button or an option's (clickable).
    pub fn over_esc(&self, p: Point) -> bool {
        self.esc.contains(p)
            || self.scope_button.contains(p)
            || self.replace_button.contains(p)
            || self.option_at(p).is_some()
            || (self.can_replace && self.replace_all.contains(p))
    }

    /// A press on the scrollbar: its thumb taken, or the list moved there. `true` if on it.
    pub fn press_bar(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.press(p) else { return false };
        self.scroll_to(offset);
        true
    }

    /// The scrollbar's thumb dragged to `p`. `true` if it was being dragged.
    pub fn drag_bar(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        self.scroll_to(offset);
        true
    }

    /// The mouse released: the thumb let go. `true` if it was held.
    pub fn release_bar(&mut self) -> bool {
        self.bar.release()
    }

    /// `p` is on the scrolled rows' scrollbar (the tests find its thumb).
    #[cfg(test)]
    pub fn on_bar(&self, p: Point) -> bool {
        self.bar.contains(p)
    }

    /// The mouse at `p` over the scrolled rows' scrollbar (it widens): `true` if it changed.
    pub fn hover_bar(&mut self, p: Point) -> bool {
        self.bar.hover(p)
    }

    /// The scrolled rows from `offset` (physical pixels, as the scrollbar gives it).
    fn scroll_to(&mut self, offset: f32) {
        let Some((_, row_h, _)) = self.bar_area.filter(|(_, h, _)| *h > 0.0) else { return };
        let (count, visible) = self.scrolled();
        let first = (offset / row_h).round().max(0.0) as usize;
        self.first = first.min(count.saturating_sub(visible));
    }

    pub fn scroll(&mut self, rows: isize) {
        let (count, visible) = self.scrolled();
        let max = count.saturating_sub(visible) as isize;
        self.first = (self.first as isize + rows).clamp(0, max) as usize;
    }

    pub fn paint(
        &mut self,
        mode: &SearchMode,
        file_icon: &dyn Fn(&str) -> (IconId, Rgba),
        painter: Painter<'_>,
    ) {
        let Painter { text, scene, palette, icons } = painter;
        self.rows.clear();
        self.tips.clear();
        self.forget_buttons.clear();
        self.clear_all = Rect::default();
        self.suggestion_rows.clear();
        self.option_buttons.clear();
        self.enable = Rect::default();
        self.bar_area = None;
        if !self.open {
            return;
        }
        let f = self.layout(palette, icons);
        let p = f.panel;

        let layer = scene.push_layer(None);
        ds::backdrop(layer, &f.ctx, self.viewport);
        ds::surface(layer, &f.ctx, p, Elevation::High, radius::XL);

        self.paint_field(text, scene, &f);
        if self.replace_open {
            self.paint_replace(text, scene, &f);
        }
        if self.scope_open {
            self.paint_scope(text, scene, &f);
        }
        let layer = scene.push_layer(Some(p));
        let head = f.head_h();
        ds::divider(layer, &f.ctx, p.min_x(), p.max_x(), p.min_y() + head);

        // The list: recent searches (empty field) or the results; otherwise an empty state.
        let list = Rect::new(
            p.min_x(),
            p.min_y() + head + f.ctx.line(),
            p.size.width,
            f.list_h - f.ctx.line(),
        );
        let fonts = Fonts::new(text, &f.ctx);
        if f.rows + f.folders == 0 {
            let searching = self.exact.is_some_and(|e| !e.done);
            let (title, detail) = if let Some(error) = &self.error {
                (t("search.bad_pattern"), error.clone())
            } else if f.recent {
                (t("search.empty_title"), t("search.empty_detail"))
            } else if searching {
                (t("search.searching"), String::new())
            } else {
                (t("search.no_results"), t("search.no_results_detail"))
            };
            ds::empty_state(layer, text, &f.ctx, list, &title, Some(&detail));
        } else {
            self.paint_list(layer, text, &f, &fonts, list, file_icon);
            if let Some((area, row_h, count)) = self.bar_area {
                let (content, offset) = (count as f32 * row_h, self.first as f32 * row_h);
                self.bar.paint(scene, palette, self.scale, area, content, offset);
            }
        }
        self.paint_footer(mode, text, scene, &f, &fonts);
    }

    /// The panel's measures this frame (and its place, kept for the clicks).
    fn layout<'a>(&mut self, palette: &'a Palette, icons: &'a Icons) -> Frame<'a> {
        let shown = self.appear.map_or(1.0, |a| a.value());
        let ctx = Ctx::new(palette, self.scale).with_opacity(shown);
        let px = |v: f32| ctx.px(v);
        let v = self.viewport;
        let width = px(720.0).min(v.size.width - px(48.0));
        let field_h = px(60.0);
        let footer_h = px(42.0);
        let recent = self.showing_recent();
        // A file listed by its name: a row of one line, as the recent searches'.
        let row_h = if recent || self.listing { px(38.0) } else { px(56.0) };
        let folder_h = px(38.0);
        let header_h = px(28.0);
        // The scrolled rows (recent searches or files) and the fixed folder rows above them.
        let (count, visible) = self.scrolled();
        let rows = count.min(visible);
        let (named, folders) =
            if recent || self.listing { (0, 0) } else { (self.named.len(), self.folders.len()) };
        // A header over each section shown (the scrolled rows' only under another one).
        let headers = if recent || self.listing {
            1
        } else {
            let fixed = usize::from(named > 0) + usize::from(folders > 0);
            fixed + usize::from(fixed > 0 && rows > 0)
        };
        let folders = named + folders;
        // The list as tall whatever it shows (empty, the recent searches, a few results,
        // a file pattern's files): the results' height, so the window never jumps as one
        // types — taller only when folders over the results need it.
        let full = VISIBLE as f32 * px(56.0) + px(space::SM) * 2.0;
        let list_h = if rows + folders == 0 {
            full
        } else {
            let needed = rows as f32 * row_h
                + folders as f32 * folder_h
                + headers as f32 * header_h
                + px(space::SM) * 2.0;
            needed.max(full)
        };
        let scope_h = if self.scope_open { px(50.0) } else { 0.0 };
        let replace_h = if self.replace_open { px(50.0) } else { 0.0 };
        let suggest_h = if self.has_suggestions() {
            self.suggestions.len() as f32 * px(SUGGESTION_H) + px(space::SM)
        } else {
            0.0
        };
        let height = field_h + replace_h + scope_h + suggest_h + list_h + footer_h;
        self.panel = Rect::new(
            (v.min_x() + (v.size.width - width) / 2.0).round(),
            (v.min_y() + v.size.height * 0.14 + (1.0 - shown) * px(10.0)).round(),
            width,
            height,
        );
        Frame {
            ctx,
            icons,
            panel: self.panel,
            field_h,
            replace_h,
            scope_h,
            suggest_h,
            footer_h,
            list_h,
            row_h,
            folder_h,
            header_h,
            recent,
            count,
            visible,
            rows,
            folders,
        }
    }
}

/// A folder suggested's row (logical pixels).
const SUGGESTION_H: f32 = 32.0;

/// The panel's measures this frame, computed once and shared by its parts.
struct Frame<'a> {
    ctx: Ctx<'a>,
    icons: &'a Icons,
    panel: Rect,
    field_h: f32,
    /// The replacement's row and the folder's (0: hidden), the folders suggested under it
    /// (0: none).
    replace_h: f32,
    scope_h: f32,
    suggest_h: f32,
    footer_h: f32,
    list_h: f32,
    /// A scrolled row's height (a recent search or a file).
    row_h: f32,
    folder_h: f32,
    header_h: f32,
    /// The field is empty: the recent searches are listed.
    recent: bool,
    /// The scrolled rows: all of them and how many fit.
    count: usize,
    visible: usize,
    /// The scrolled rows drawn and the fixed rows by name above them (files and folders).
    rows: usize,
    folders: usize,
}

/// The list's fonts and their metrics.
struct Fonts {
    font: Font,
    metrics: FontMetrics,
    small: Font,
    small_metrics: FontMetrics,
    code: Font,
    code_metrics: FontMetrics,
}

impl Frame<'_> {
    /// What's above the list: the field, the folder's row and its suggestions.
    fn head_h(&self) -> f32 {
        self.field_h + self.replace_h + self.scope_h + self.suggest_h
    }
}

impl Fonts {
    fn new(text: &mut TextSystem, ctx: &Ctx<'_>) -> Self {
        let font = ctx.ui_font(0.5);
        let metrics = text.metrics(&font);
        let small = ctx.ui_font(-1.0);
        let small_metrics = text.metrics(&small);
        let code = ctx.code_font(-1.0);
        let code_metrics = text.metrics(&code);
        Self { font, metrics, small, small_metrics, code, code_metrics }
    }
}

/// A file row's result and the query its line is chosen by.
struct FileRow<'a> {
    hit: &'a Hit,
    query: &'a str,
}
