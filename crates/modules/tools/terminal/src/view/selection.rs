//! The mouse over the grid: the cell under it, a selection begun, dragged (scrolling past
//! the edge) and ended, the link under it and the file it names.

use super::*;

impl TerminalView {
    /// The visible cell (row, column) under `p`; outside the grid, the nearest one when
    /// `clamp` (dragging a selection past the edge).
    pub(super) fn cell_at(&self, p: Point, clamp: bool) -> Option<(u16, u16)> {
        let (w, h) = self.cell;
        let a = self.grid_area;
        if w <= 0.0 || h <= 0.0 || self.grid.rows == 0 || (!clamp && !a.contains(p)) {
            return None;
        }
        let col = ((p.x - a.min_x()) / w).floor().clamp(0.0, f32::from(self.grid.cols) - 1.0);
        // The grid drawn shifted down while scrolled by part of a line.
        let top = a.min_y() + self.smooth.shift(h);
        let row = ((p.y - top) / h).floor().clamp(0.0, f32::from(self.grid.rows) - 1.0);
        Some((row as u16, col as u16))
    }

    /// How many lines a step scrolls with the mouse at `p` while selecting: up (positive)
    /// above the grid, down below it, more the farther it is (at most [`AUTO_MOST`]); 0 inside.
    pub(super) fn edge_lines(&self, p: Point) -> i32 {
        let a = self.grid_area;
        let line = self.cell.1.max(1.0);
        let (past, up) = if p.y < a.min_y() {
            (a.min_y() - p.y, true)
        } else if p.y > a.max_y() {
            (p.y - a.max_y(), false)
        } else {
            return 0;
        };
        let lines = ((past / line).floor() as i32 + 1).min(AUTO_MOST);
        if up { lines } else { -lines }
    }

    /// The history scrolled a step, at `now`, while the mouse is held past an edge of the
    /// grid selecting (one step every [`AUTO_EVERY`]), the selection's end following. `true`
    /// if it scrolled.
    pub(super) fn auto_scroll(&mut self, now: std::time::Instant) -> bool {
        let Some(p) = self.drag_at.filter(|_| self.selecting) else {
            self.auto_next = None;
            return false;
        };
        let lines = self.edge_lines(p);
        if lines == 0 {
            self.auto_next = None;
            return false;
        }
        if self.auto_next.is_some_and(|due| now < due) {
            return false;
        }
        self.scroll_lines(lines);
        self.show_scrolled();
        self.auto_next = Some(now + AUTO_EVERY);
        self.follow_drag(p);
        true
    }

    /// The mouse pressed on the grid: starts a selection (a double click selects the word).
    pub fn begin_selection(&mut self, p: Point, clicks: u32) -> bool {
        let Some((row, col)) = self.cell_at(p, false) else { return false };
        let at = self.grid.point(row, col);
        self.selection = Some(TermSelection { anchor: at, head: at });
        self.selecting = true;
        if clicks >= 2
            && let Some(word) = word_at(&self.grid.row_chars(row), col)
        {
            self.selection = Some(TermSelection {
                anchor: self.grid.point(row, word.start),
                head: self.grid.point(row, word.end.saturating_sub(1)),
            });
            self.selecting = false;
        }
        true
    }

    /// The mouse moved with the button down: the selection follows it; past the grid's top
    /// or bottom, the history scrolls that way too (and goes on while it's held there).
    pub fn drag_selection(&mut self, p: Point) -> bool {
        if !self.selecting {
            return false;
        }
        self.drag_at = Some(p);
        let scrolled = self.auto_scroll(std::time::Instant::now());
        scrolled | self.follow_drag(p)
    }

    /// The selection's moving end where the mouse is (clamped to the grid's edges).
    fn follow_drag(&mut self, p: Point) -> bool {
        let Some((row, col)) = self.cell_at(p, true) else { return false };
        let head = self.grid.point(row, col);
        match &mut self.selection {
            Some(selection) if selection.head != head => {
                selection.head = head;
                true
            }
            _ => false,
        }
    }

    /// The button released: an empty selection (a plain click) is dropped; another one is
    /// copied at once when the settings say so.
    pub fn end_selection(&mut self) -> bool {
        let was = std::mem::take(&mut self.selecting);
        self.drag_at = None;
        self.auto_next = None;
        if self.selection.is_some_and(|s| s.is_empty()) {
            self.selection = None;
        }
        if self.selection.is_some()
            && self.config().copy_on_select
            && let Some(text) = self.selected_text()
        {
            self.action = Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text)));
        }
        was
    }

    /// The link under `p`: a URL, or a file that exists (relative to the terminal's start
    /// directory, `~` or absolute).
    pub fn link_under(&self, p: Point) -> Option<(i32, std::ops::Range<u16>, TerminalLink)> {
        if !self.config().highlight_links {
            return None;
        }
        let (row, col) = self.cell_at(p, false)?;
        let link = links(&self.grid.row_chars(row)).into_iter().find(|l| l.cols.contains(&col))?;
        let target = match link.target {
            LinkTarget::Url(url) => TerminalLink::Url(url),
            LinkTarget::File { path, line, column } => {
                let path = self.resolve_path(&path)?;
                TerminalLink::File { path, line, column }
            }
        };
        Some((self.grid.point(row, 0).line, link.cols, target))
    }

    fn resolve_path(&self, written: &str) -> Option<PathBuf> {
        // `~/` (and PowerShell's `~\\` on Windows): the home folder.
        let home = written
            .strip_prefix("~/")
            .or_else(|| cfg!(windows).then(|| written.strip_prefix("~\\")).flatten());
        let path = match home {
            Some(rest) => std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(rest))?,
            None => PathBuf::from(written),
        };
        let path = if path.is_absolute() { path } else { self.cwd.as_ref()?.join(path) };
        path.is_file().then_some(path)
    }
}
