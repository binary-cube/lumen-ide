//! A tab's text and state: its text (copied once per version), its marks, the tabs of every group,
//! a mirror kept in sync with the document it shows.

use super::*;

/// A file's size and modification time.
pub(super) type DiskStamp = workspace::Stamp;

/// What was flagged in a tab's text, with what it was computed for.
#[derive(Debug, Default)]
pub(super) struct TabDiagnostics {
    /// The syntax errors (from the last snapshot) and its version.
    pub errors: Vec<syntax_ts::SyntaxError>,
    /// Everything flagged in the text (syntax errors, features newer than the level,
    /// deprecated names), sorted by position, with messages in the UI language (once per
    /// snapshot / analysis).
    pub marks: Vec<Mark>,
    /// The ranges of the errors and of the warnings among `marks` (sorted), for drawing.
    pub error_ranges: Vec<std::ops::Range<usize>>,
    pub warning_ranges: Vec<std::ops::Range<usize>>,
    /// The deprecated ones among them (struck through).
    pub struck_ranges: Vec<std::ops::Range<usize>>,
    /// For what the errors were computed: (parse count, level issues generation).
    pub errors_version: Option<(u64, u64, u64)>,
    /// The language features newer than the chosen level, the level they were checked
    /// against, and a counter bumped when they change.
    pub level_issues: Vec<code_intelligence::LevelIssue>,
    pub checked_level: Option<String>,
    pub issues_generation: u64,
    /// The deprecated names the text uses.
    pub deprecated: Vec<code_intelligence::Deprecated>,
    /// What the text declares but never uses, and its ranges (dimmed).
    pub unused: Vec<code_intelligence::Unused>,
    pub unused_ranges: Vec<std::ops::Range<usize>>,
    /// What the text needs the project's manifest to declare.
    pub requirements: Vec<code_intelligence::Requirement>,
    /// What's wrong knowing the project, and whether it was checked against the complete
    /// index.
    pub problems: Vec<code_intelligence::Problem>,
    pub checked_ready: bool,
}

impl TabDiagnostics {
    /// The marks found (sorted by position here) and the ranges drawn for them, computed
    /// for `version` (see `errors_version`).
    pub fn set_marks(&mut self, mut marks: Vec<Mark>, version: (u64, u64, u64)) {
        marks.sort_by_key(|m| m.range.start);
        self.error_ranges = marks.iter().filter(|m| !m.warning).map(|m| m.range.clone()).collect();
        let warnings = |struck: bool| -> Vec<std::ops::Range<usize>> {
            marks
                .iter()
                .filter(|m| m.warning && !m.weak && m.struck == struck)
                .map(|m| m.range.clone())
                .collect()
        };
        self.warning_ranges = warnings(false);
        self.struck_ranges = warnings(true);
        self.unused_ranges = marks.iter().filter(|m| m.weak).map(|m| m.range.clone()).collect();
        self.marks = marks;
        self.errors_version = Some(version);
    }
}

impl Tab {
    /// Its file changed on disk since it was last seen (opened, saved, reloaded).
    pub(super) fn changed_on_disk(&self, disk: &dyn Disk) -> bool {
        let Some(path) = self.doc.path() else { return false };
        self.disk_stamp.is_some_and(|seen| disk.stamp(path) != Some(seen))
    }

    /// Its file as it is now on disk: seen.
    pub(super) fn seen_on_disk(&mut self, disk: &dyn Disk) {
        self.disk_stamp = self.doc.path().and_then(|p| disk.stamp(p));
    }

    /// The whole text (copied once per document version).
    /// The text, copied: from the cache when it's current (no rope walk), else from the
    /// buffer without keeping it (a tab not shown holds no extra copy).
    pub(super) fn text_copy(&self) -> String {
        let version = self.doc.version();
        match self.text.borrow().as_ref().filter(|(v, _)| *v == version) {
            Some((_, text)) => text.to_string(),
            None => self.doc.buffer().text(),
        }
    }

    pub(super) fn text(&self) -> Arc<str> {
        let version = self.doc.version();
        let mut cached = self.text.borrow_mut();
        if let Some((_, text)) = cached.as_ref().filter(|(v, _)| *v == version) {
            return Arc::clone(text);
        }
        let text: Arc<str> = Arc::from(self.doc.buffer().text());
        *cached = Some((version, Arc::clone(&text)));
        text
    }

    pub(super) fn new(
        doc: Document,
        font: FontFamily,
        untitled: Option<u32>,
        language: LanguageId,
    ) -> Self {
        let version = doc.version();
        Self {
            pinned: false,
            disk_read_only: false,
            text_told: 0,
            disk_changed: false,
            // Seen by the caller once it's made (`seen_on_disk`).
            disk_stamp: None,
            saved_text: None,
            synced_from: None,
            viewer: None,
            toolbar: None,
            doc,
            view: EditorView::new(font),
            find: FindState::default(),
            find_bar: FindBar::default(),
            find_open: false,
            untitled,
            syntax: DocumentSyntax::new(language),
            folds: Folds::default(),
            folds_version: version,
            symbols_version: None,
            diagnostics: TabDiagnostics::default(),
            occurrences: Vec::new(),
            occurrences_key: None,
            marks_generation: 0,
            text: std::cell::RefCell::new(None),
            highlight: Memo::default(),
            seen: std::time::Instant::now(),
            selection_steps: smart_select::SelectionSteps::default(),
            insight: insight::TabInsight::default(),
            concealed: concealed::TabConcealed::default(),
        }
    }

    /// Collapses / expands the region starting on `line`. Collapsing one that hides the
    /// cursor moves the cursor to the region's first line (otherwise keeping the cursor
    /// visible would expand it again at once).
    pub(super) fn toggle_fold(&mut self, line: u32) {
        if self.folds.toggle(line) {
            self.cursor_out_of_folds();
        }
    }

    /// A cursor left inside a region just collapsed goes to the end of the region's first
    /// line (the folds keep the cursor visible by expanding: the collapse would be undone).
    pub(super) fn cursor_out_of_folds(&mut self) {
        let head = self.doc.selections().primary().head;
        let cursor = self.doc.buffer().line_of(head) as u32;
        let shown = self.folds.shown_line(cursor);
        if shown != cursor {
            let end = self.doc.buffer().line_end(shown as usize);
            commands::click(&mut self.doc, end, false, false);
        }
    }

    /// Runs `f` on the document with its collapsed regions taken whole: the
    /// commands on whole lines take a folded row's every line, and (`widen`) a selection
    /// reaching a region covers it. A command that changed nothing gets the selection back
    /// as it was. Without collapsed regions: `f` alone.
    pub(super) fn folded<R>(&mut self, widen: bool, f: impl FnOnce(&mut Document) -> R) -> R {
        if self.folds.hidden_ranges().is_empty() {
            return f(&mut self.doc);
        }
        self.doc.set_line_groups(self.folds.folded_lines());
        let original = self.doc.selections().clone();
        let widened =
            if widen { self.folds.widen(self.doc.buffer(), &original) } else { original.clone() };
        let changed = widened != original;
        if changed {
            self.doc.set_selections(widened.clone());
        }
        let version = self.doc.version();
        let out = f(&mut self.doc);
        self.doc.set_line_groups(Vec::new());
        if changed && self.doc.version() == version && *self.doc.selections() == widened {
            self.doc.set_selections(original);
        }
        out
    }

    /// Keeps the folding regions aligned with the text and doesn't leave the cursor in a
    /// hidden region (it expands it).
    pub(super) fn sync_folds(&mut self) {
        if self.folds_version != self.doc.version() {
            match self.doc.changes_since(self.folds_version) {
                Some(changes) => self.folds.map_changes(&changes),
                None => self.folds.expand_all(),
            }
            self.folds_version = self.doc.version();
        }
        let head = self.doc.selections().primary().head;
        let line = self.doc.buffer().line_of(head) as u32;
        self.folds.reveal(line);
    }

    /// The name its icon is found by: a virtual tab's whole name (its `view-*` extension
    /// says what it shows, a module's rule giving it its icon), else its title.
    pub(super) fn icon_name(&self) -> String {
        match self.doc.path().filter(|p| is_virtual(p)).and_then(Path::file_name) {
            Some(name) => name.to_string_lossy().into_owned(),
            // A module's bar renames the tab (`console [shop]`): its file's name still finds
            // the icon's color.
            None => match self.doc.path().and_then(Path::file_name) {
                Some(name) if self.toolbar.is_some() => name.to_string_lossy().into_owned(),
                _ => self.title(),
            },
        }
    }

    pub(super) fn title(&self) -> String {
        // A module's bar names its file (a console: `console [shop]`).
        if let Some(title) = self.toolbar.as_ref().and_then(|t| t.bar.title()) {
            return title;
        }
        // A view naming itself (a program's terminal: its session's title).
        if let Some(title) = self.viewer.as_ref().and_then(|v| v.viewer.live_title()) {
            return title.to_owned();
        }
        // Virtual tabs (the canvas) are shown without an extension.
        if let Some(path) = self.doc.path().filter(|p| is_virtual(p))
            && let Some(stem) = path.file_stem()
        {
            return stem.to_string_lossy().into_owned();
        }
        match (self.doc.path().and_then(Path::file_name), self.untitled) {
            (Some(name), _) => name.to_string_lossy().into_owned(),
            (None, Some(n)) if n > 1 => i18n::tr!("files.untitled_n", n = n),
            (None, _) => i18n::t("files.untitled"),
        }
    }

    /// The area of the find strip (at the top, full width) and of the editor below it.
    pub(super) fn sync_bounds(&mut self, area: Rect, scale: f32) {
        // A module's bar first, right under the breadcrumb (in the gap's place), the text
        // below it: never under it.
        let area = match &mut self.toolbar {
            Some(toolbar) => {
                let gap = (EDITOR_GAP * scale).round();
                let h = toolbar.height(scale).min(area.size.height);
                toolbar.bounds = Rect::new(area.min_x(), area.min_y() - gap, area.size.width, h);
                // The gap stays between the bar and the text.
                Rect::new(
                    area.min_x(),
                    area.min_y() + h,
                    area.size.width,
                    (area.size.height - h).max(0.0),
                )
            }
            None => area,
        };
        let find_h =
            if self.find_open { self.find_bar.height(scale).min(area.size.height) } else { 0.0 };
        let mut editor = Rect::new(
            area.min_x(),
            area.min_y() + find_h,
            area.size.width,
            area.size.height - find_h,
        );
        if let Some(v) = &mut self.viewer {
            v.bounds = editor;
            // Split: the halves at the tab's ratio, the separator between them.
            if v.splits() {
                (editor, v.bounds) = v.split.layout(editor, scale);
            } else {
                v.split.splitter.set_bounds(Rect::default(), scale);
            }
        }
        self.view.set_bounds(editor, scale);
    }

    /// Its document is the file's text (no viewer, or a preview beside the text — a
    /// Markdown's, a CSV's data): saved, reloaded and counted as any text file's.
    pub(super) fn holds_text(&self) -> bool {
        self.viewer.as_ref().is_none_or(|v| v.kind == ui_workbench::ViewerKind::Preview)
    }

    pub(super) fn editor_visible(&self) -> bool {
        self.viewer.as_ref().is_none_or(TabViewer::editor_visible)
    }

    /// The occurrences of the selected word (double click or a single-word selection), as
    /// whole words. Recomputed only when the word or the document changes.
    pub(super) fn sync_occurrences(&mut self, support: &LanguageSupport) {
        let selection = self.doc.selections().primary().range();
        let buffer = self.doc.buffer();
        let word = if self.doc.selections().len() == 1
            && !selection.is_empty()
            && selection.len() <= 128
        {
            buffer.slice(selection)
        } else {
            String::new()
        };
        let language = self.syntax.language().clone();
        let word_chars = support.languages.get(&language).map_or("", |l| l.word_chars);
        let is_word = |c: char| c.is_alphanumeric() || c == '_' || word_chars.contains(c);
        let valid = !word.is_empty() && word.chars().all(is_word);
        let key = valid.then(|| (self.doc.version(), word.clone()));
        if key == self.occurrences_key {
            return;
        }
        self.occurrences.clear();
        self.occurrences_key = key;
        self.marks_generation += 1;
        if !valid {
            return;
        }
        let text = self.text();
        for (start, _) in text.match_indices(&word) {
            let end = start + word.len();
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            if before.is_some_and(is_word) || after.is_some_and(is_word) {
                continue;
            }
            self.occurrences.push(start..end);
            if self.occurrences.len() >= 5000 {
                break;
            }
        }
        // A single occurrence (the selected one) isn't worth highlighting.
        if self.occurrences.len() < 2 {
            self.occurrences.clear();
        }
    }

    /// Unsaved changes: in the document or in the viewer (e.g. a rotated image).
    pub(super) fn is_dirty(&self) -> bool {
        self.doc.is_dirty() || self.viewer.as_ref().is_some_and(|v| v.viewer.is_dirty())
    }

    /// The find strip: right under the breadcrumb (the gap above the editor moves below
    /// the strip, so no empty band separates it from the breadcrumb).
    pub(super) fn find_area(&self, area: Rect, scale: f32) -> Rect {
        let gap = (EDITOR_GAP * scale).round();
        // Under a module's bar when there's one.
        let top = self.toolbar.as_ref().map_or(area.min_y() - gap, |t| t.bounds.max_y());
        Rect::new(area.min_x(), top, area.size.width, self.view.bounds().min_y() - gap - top)
    }

    /// Recompiles the search from the bar's field, keeping the options (a large document's
    /// whole search sent to `sink`'s worker, the match selected when it's back).
    pub(super) fn update_query(&mut self, sink: &super::find_search::FindSink) {
        let query = SearchQuery {
            pattern: self.find_bar.query.text().to_owned(),
            ..self.find.query.clone()
        };
        self.find.set_query(query);
        sink.start(self);
        self.find.select_from_cursor(&mut self.doc);
    }
}

/// Every tab: the current group's, then the other groups' (fields passed apart, so the
/// caller can use the rest of the workbench meanwhile).
pub(super) fn all_tabs<'a>(
    tabs: &'a [Tab],
    groups: &'a [GroupState],
) -> impl Iterator<Item = &'a Tab> {
    tabs.iter().chain(groups.iter().flat_map(|g| g.tabs.iter()))
}

/// The tabs on screen: the main group's active one and each split group's — what can show
/// a change (asked whether it moves or wants a frame; the others are asked when shown).
pub(super) fn shown_tabs<'a>(
    tabs: &'a [Tab],
    active: usize,
    groups: &'a [GroupState],
) -> impl Iterator<Item = &'a Tab> {
    tabs.get(active).into_iter().chain(groups.iter().filter_map(|g| g.tabs.get(g.active)))
}

pub(super) fn all_tabs_mut<'a>(
    tabs: &'a mut [Tab],
    groups: &'a mut [GroupState],
) -> impl Iterator<Item = &'a mut Tab> {
    tabs.iter_mut().chain(groups.iter_mut().flat_map(|g| g.tabs.iter_mut()))
}

/// Makes the text of `mirror` the same as that of `source`, replacing only the differing
/// part (one undo step; the cursors move with the text). The "saved" state follows the source.
pub(super) fn sync_text(source: &Document, mirror: &mut Document) {
    let (a, b) = (mirror.buffer(), source.buffer());
    // The common start, compared chunk by chunk (no copy of either text).
    let common = common_prefix(a.chunks(), b.chunks());
    let prefix = a.floor_char_boundary(common).min(b.floor_char_boundary(common));
    if prefix != a.len() || prefix != b.len() {
        // The common end, a window at a time from the end of both texts.
        const WINDOW: usize = 4096;
        let max_suffix = (a.len() - prefix).min(b.len() - prefix);
        let mut suffix = 0;
        while suffix < max_suffix {
            let take = WINDOW.min(max_suffix - suffix);
            let (ea, eb) = (a.len() - suffix, b.len() - suffix);
            let wa = a.slice(a.floor_char_boundary(ea - take)..ea);
            let wb = b.slice(b.floor_char_boundary(eb - take)..eb);
            // Whole windows (they start on a char boundary, UTF-8 being self-synchronizing:
            // where the equal bytes end in one text, they end on a boundary in the other).
            let window = wa.len().min(wb.len()).min(max_suffix - suffix);
            let same = wa
                .bytes()
                .rev()
                .zip(wb.bytes().rev())
                .take(window)
                .take_while(|(x, y)| x == y)
                .count();
            suffix += same;
            if same < window || window == 0 {
                break;
            }
        }
        // Back to a char boundary in both texts.
        while suffix > 0
            && (a.floor_char_boundary(a.len() - suffix) != a.len() - suffix
                || b.floor_char_boundary(b.len() - suffix) != b.len() - suffix)
        {
            suffix -= 1;
        }
        let replacement = b.slice(prefix..b.len() - suffix);
        let tx = document::Transaction::single(prefix..a.len() - suffix, &replacement);
        // A locked mirror follows too: the change was made where the file isn't locked.
        mirror.apply_external(tx, None, document::EditKind::Command);
    }
    if !source.is_dirty()
        && mirror.is_dirty()
        && let Some(path) = source.path()
    {
        mirror.mark_saved(path.to_path_buf());
    }
}

/// A text's fingerprint, from its chunks (the same however it's cut).
pub(super) fn text_fingerprint<'a>(chunks: impl Iterator<Item = &'a str>) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for chunk in chunks {
        h.write(chunk.as_bytes());
    }
    h.finish()
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/tab.rs"]
mod tests;

impl Workbench {
    /// The tabs on screen ([`shown_tabs`]).
    pub(super) fn shown_tabs(&self) -> impl Iterator<Item = &Tab> {
        shown_tabs(&self.tabs, self.active, &self.editors.groups)
    }
}
