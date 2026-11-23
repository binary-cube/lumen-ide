//! The editor's rows: one per shown line, or, with word wrap on, as many as the line takes
//! at the text's width ([`WrapRows`]). Every place that turns a row into a line (the
//! frame, a click, the scroll, the cursor's row) asks here, so wrapped and unwrapped rows
//! are one thing: a line's part, from a byte to a byte ([`Seg`]).

use super::*;

/// The part of its line a row shows: bytes `from..to` of the line, whether it's the
/// line's first row (its number) and its last (what's after the text: a folded label, a
/// lens; else the wrap's mark at its end).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Seg {
    pub from: usize,
    pub to: usize,
    pub first: bool,
    pub last: bool,
}

impl Seg {
    /// The whole line (`len` bytes): its only row.
    pub fn whole(len: usize) -> Self {
        Self { from: 0, to: len, first: true, last: true }
    }

    /// Whether the line's byte `at` shows on this row (a break belongs to the row after
    /// it; the line's end to its last row).
    pub fn holds(&self, at: usize) -> bool {
        self.from <= at && (at < self.to || (self.last && at == self.to))
    }
}

impl EditorView {
    /// How text is shown (word wrap…): from now on, at the next frame.
    pub fn set_text_options(&mut self, options: &ui_components::TextOptions) {
        if self.wrap != options.word_wrap {
            self.wrap = options.word_wrap;
            if self.wrap {
                self.scroll_x = 0.0;
            }
        }
    }

    /// Lines wrap now (on, and the text's width known since a frame).
    pub fn wraps(&self) -> bool {
        self.wrap && self.wrap_columns > 0
    }

    /// The wrapped rows brought up to date (nothing when wrap is off or nothing changed).
    pub(super) fn refresh_rows(&mut self, doc: &Document, folds: &Folds) {
        if self.wraps() {
            self.wrap_rows.refresh(doc, folds, self.wrap_columns);
        }
    }

    /// How many rows are shown.
    pub(super) fn total_rows(&self, doc: &Document, folds: &Folds) -> u32 {
        if self.wraps() {
            self.wrap_rows.total()
        } else {
            folds.visible_rows(doc.buffer().line_count() as u32)
        }
    }

    /// Line `line`'s first row (a hidden one: its region's).
    pub(super) fn row_of_line(&self, folds: &Folds, line: u32) -> u32 {
        if self.wraps() { self.wrap_rows.first_row(folds, line) } else { folds.row_of(line) }
    }

    /// Row `row`'s line, and which of its rows it is.
    pub(super) fn row_at(&self, folds: &Folds, row: u32) -> (u32, u32) {
        if self.wraps() { self.wrap_rows.at(row) } else { (folds.line_of(row), 0) }
    }

    /// How many rows line `line` takes (1 unwrapped; a hidden one's region's line's).
    pub(super) fn rows_of_line(&self, folds: &Folds, line: u32) -> u32 {
        if !self.wraps() {
            return 1;
        }
        self.wrap_rows.rows(folds.shown_line(line)).max(1)
    }

    /// Row `row`'s line.
    pub(super) fn line_of_row(&self, folds: &Folds, row: u32) -> u32 {
        self.row_at(folds, row).0
    }

    /// The `k`th part of `line` (in `self.line`, loaded): the whole line when it doesn't
    /// wrap.
    pub(super) fn seg_of(&mut self, doc: &Document, k: u32) -> Seg {
        if !self.wraps() {
            return Seg::whole(self.line.len());
        }
        self.wrap_loaded(doc);
        let parts = &self.wrap_parts;
        let k = (k as usize).min(parts.len().saturating_sub(1));
        parts.get(k).map_or(Seg::whole(self.line.len()), |r| Seg {
            from: r.start,
            to: r.end,
            first: k == 0,
            last: k + 1 >= parts.len(),
        })
    }

    /// The loaded line's parts, wrapped once for it (its rows asked again and again —
    /// each row of a long line drawn, the cursor's row found — read them, never rewrap).
    fn wrap_loaded(&mut self, doc: &Document) {
        let Some((id, version, line)) = self.loaded else { return };
        let key = (id, version, line, self.wrap_columns, doc.tab_width());
        if self.wrap_parts_for == Some(key) {
            return;
        }
        ui_components::text_options::wrap_rows(
            &self.line,
            self.wrap_columns,
            doc.tab_width(),
            &mut self.wrap_parts,
        );
        self.wrap_parts_for = Some(key);
    }

    /// The row (and its part) the byte `offset` of `doc` shows on.
    pub(super) fn row_of_offset(
        &mut self,
        doc: &Document,
        folds: &Folds,
        offset: usize,
    ) -> (u32, Seg) {
        let buffer = doc.buffer();
        let line = buffer.line_of(offset);
        let first = self.row_of_line(folds, line as u32);
        if !self.wraps() {
            return (first, Seg::whole(buffer.line_len(line)));
        }
        self.load_line(doc, line);
        let at = offset - buffer.line_start(line);
        self.wrap_loaded(doc);
        // The part holding it: the last one starting at or before it (a break belongs to
        // the row after it).
        let k = self.wrap_parts.partition_point(|p| p.start <= at).saturating_sub(1) as u32;
        (first + k, self.seg_of(doc, k))
    }

    /// Visible row `i`'s place among the frame's rows, for the byte `offset` (its line
    /// loaded in `self.line`): the frame's row showing it.
    pub(super) fn visible_row_of(&self, line: usize, at: usize) -> Option<usize> {
        let first = self.lines.partition_point(|&l| (l as usize) < line);
        (first..self.lines.len())
            .take_while(|&i| self.lines[i] as usize == line)
            .find(|&i| self.segs.get(i).is_some_and(|s| s.holds(at)))
    }

    /// The frame's rows, `first_row` on (`count` of them at most): their lines in
    /// `self.lines`, their parts in `self.segs`.
    pub(super) fn fill_rows(&mut self, doc: &Document, folds: &Folds, first_row: u32, count: u32) {
        self.lines.clear();
        self.segs.clear();
        let end = (first_row + count).min(self.total_rows(doc, folds));
        let mut loaded: Option<u32> = None;
        for row in first_row..end {
            let (line, k) = self.row_at(folds, row);
            if loaded != Some(line) {
                self.load_line(doc, line as usize);
                loaded = Some(line);
            }
            let seg = self.seg_of(doc, k);
            self.lines.push(line);
            self.segs.push(seg);
        }
    }

    /// Visible row `i`'s part (the whole line when there's none).
    pub(super) fn seg(&self, i: usize) -> Seg {
        self.segs.get(i).copied().unwrap_or(Seg {
            from: 0,
            to: usize::MAX,
            first: true,
            last: true,
        })
    }

    /// Visible row `i` loaded: its line in `self.line` (and its inlays), the row's part of
    /// it as shown (tabs expanded) in `self.display`; returns where that part starts in the
    /// whole line's display (its bytes before it are left out).
    pub(super) fn load_row(&mut self, doc: &Document, i: usize, max_columns: usize) -> usize {
        let line = self.lines[i] as usize;
        self.load_line(doc, line);
        self.reload_row_display(doc, i, max_columns)
    }

    /// Visible row `i`'s part as shown, from the line already in `self.line` (masked, a
    /// secret's marks): as [`Self::load_row`].
    pub(super) fn reload_row_display(
        &mut self,
        doc: &Document,
        i: usize,
        max_columns: usize,
    ) -> usize {
        let tab = doc.tab_width();
        let seg = self.seg(i);
        let whole = seg.from == 0 && seg.last;
        let columns = if whole { max_columns } else { usize::MAX };
        expand_tabs_with(&self.line, columns, tab, &self.line_inlays, &mut self.display);
        if whole {
            return 0;
        }
        let to = seg.to.min(self.line.len());
        let base = source_to_display_with(&self.line, seg.from, tab, &self.line_inlays);
        let end = if seg.last {
            self.display.len()
        } else {
            source_to_display_with(&self.line, to, tab, &self.line_inlays)
        };
        self.display.truncate(end.min(self.display.len()));
        self.display.drain(..base.min(self.display.len()));
        base
    }

    /// Where a cursor at `head` goes moved `rows` rows (down when positive) while lines
    /// wrap: the row that far, at the same place along it (`goal`: that place, in columns
    /// from the row's start, kept from the first move); past the first or the last row, the
    /// text's start or end. Returns the byte and the place kept.
    #[allow(clippy::too_many_arguments)]
    pub fn moved_by_rows(
        &mut self,
        doc: &Document,
        folds: &Folds,
        text: &mut TextSystem,
        palette: &Palette,
        head: usize,
        goal: Option<usize>,
        rows: i64,
    ) -> (usize, usize) {
        let advance = self.layout(text, doc, palette).advance.max(1.0);
        self.refresh_rows(doc, folds);
        let buffer = doc.buffer();
        let (row, seg) = self.row_of_offset(doc, folds, head);
        let line = buffer.line_of(head);
        self.load_line(doc, line);
        let goal = goal.unwrap_or_else(|| {
            let x = self.x_in_row(text, doc, seg, head - buffer.line_start(line));
            (x / advance).round() as usize
        });
        let target = i64::from(row) + rows;
        let total = i64::from(self.total_rows(doc, folds));
        if target < 0 {
            return (0, goal);
        }
        if target >= total {
            return (buffer.len(), goal);
        }
        let (line, k) = self.row_at(folds, target as u32);
        self.load_line(doc, line as usize);
        let seg = self.seg_of(doc, k);
        let at = self.byte_in_row(text, doc, palette, seg, goal as f32 * advance);
        (buffer.line_start(line as usize) + at, goal)
    }

    /// While lines wrap, where Home / End go from `head` on its row: the row's start, and
    /// the last place before its end (the row after it starts at the end) — `None` on a
    /// line's only row, or where the line's own start or end is meant (its first row's
    /// Home, its last row's End, a second press).
    pub fn row_home_end(
        &mut self,
        doc: &Document,
        folds: &Folds,
        head: usize,
        end: bool,
    ) -> Option<usize> {
        if !self.wraps() {
            return None;
        }
        self.refresh_rows(doc, folds);
        let (_, seg) = self.row_of_offset(doc, folds, head);
        let start = doc.buffer().line_start(doc.buffer().line_of(head));
        let target = if end {
            if seg.last {
                return None;
            }
            let before =
                self.line[..seg.to].char_indices().next_back().map_or(seg.from, |(i, _)| i);
            start + before
        } else {
            if seg.first {
                return None;
            }
            start + seg.from
        };
        (target != head).then_some(target)
    }

    /// The `k`th line the last frame told a screen reader (a wrapped line once).
    pub fn told_line(&self, k: usize) -> Option<u32> {
        let mut told = None;
        let mut count = 0;
        for &line in &self.lines {
            if told != Some(line) {
                if count == k {
                    return Some(line);
                }
                told = Some(line);
                count += 1;
            }
        }
        None
    }
}
