//! Several files of one list chosen together: ⌘ / Ctrl adds or takes one, ⇧ a run of them; what
//! the checkbox and the menu do then reaches them all.

use super::*;

impl CommitView {
    /// A click on file `i`'s row, with the modifiers its press came with.
    pub(super) fn pick(&mut self, i: usize, staged: bool) {
        let mods = std::mem::take(&mut self.mods);
        let same_list = self.selected.as_ref().is_some_and(|(_, on)| *on == staged);
        let Some(path) = self.files.get(i).map(|f| f.path.clone()) else { return };
        if mods.shift && same_list {
            self.mark_run_to(&path, staged);
            self.show(path, staged);
        } else if mods.primary && same_list {
            self.mark_toggled(path, staged);
        } else {
            self.select(i, staged);
        }
    }

    /// From the anchor to `path`, in the list's order.
    fn mark_run_to(&mut self, path: &str, staged: bool) {
        let list: Vec<&str> =
            self.list(staged).into_iter().map(|i| self.files[i].path.as_str()).collect();
        let at = |wanted: &str| list.iter().position(|p| *p == wanted);
        let to = at(path);
        let from = self.anchor.as_deref().and_then(at).or(to);
        let (Some(from), Some(to)) = (from, to) else { return };
        let run = list[from.min(to)..=from.max(to)].iter().map(|p| (*p).to_owned()).collect();
        self.marked = run;
    }

    /// `path` added to what's chosen, or taken out (the last one stays).
    fn mark_toggled(&mut self, path: String, staged: bool) {
        if !self.marked.remove(&path) {
            self.marked.insert(path.clone());
            self.anchor = Some(path.clone());
            self.show(path, staged);
            return;
        }
        match self.marked.iter().next().cloned() {
            Some(other) => {
                if self.selected.as_ref().is_some_and(|(shown, _)| *shown == path) {
                    self.show(other, staged);
                }
                // A run from the one taken out would choose it again: from the shown one.
                if self.anchor.as_deref() == Some(path.as_str()) {
                    self.anchor = self.selected.as_ref().map(|(shown, _)| shown.clone());
                }
            }
            None => {
                self.marked.insert(path);
            }
        }
    }

    /// `path` shown beside the lists.
    fn show(&mut self, path: String, staged: bool) {
        self.selected = Some((path, staged));
        self.scrolls[DIFF] = 0.0;
        self.diff_x = 0.0;
        self.load_diff();
    }

    /// File `i` of that list is among several chosen.
    pub(super) fn among_marked(&self, i: usize, staged: bool) -> bool {
        self.marked.len() > 1
            && self.selected.as_ref().is_some_and(|(_, on)| *on == staged)
            && self.files.get(i).is_some_and(|f| self.marked.contains(&f.path))
    }

    /// What acting on file `i` reaches: everything chosen if it's among it, else just it.
    pub(super) fn reached(&self, i: usize, staged: bool) -> Vec<String> {
        if self.among_marked(i, staged) {
            self.marked.iter().cloned().collect()
        } else {
            self.files.get(i).map(|f| f.path.clone()).into_iter().collect()
        }
    }

    /// The rows as they're shown: the staged list's, then the other's.
    pub(super) fn rows(&self) -> Vec<(usize, bool)> {
        let staged = self.list(true).into_iter().map(|i| (i, true));
        staged.chain(self.list(false).into_iter().map(|i| (i, false))).collect()
    }

    /// The shown file's row, among [`Self::rows`].
    pub(super) fn shown_row(&self) -> Option<(usize, bool)> {
        let (path, staged) = self.selected.as_ref()?;
        self.rows().into_iter().find(|(i, on)| on == staged && self.files[*i].path == *path)
    }

    /// The file `delta` rows away shown (across both lists, stopping at their ends), its row
    /// brought into view.
    pub(super) fn step(&mut self, delta: isize) {
        let rows = self.rows();
        if rows.is_empty() {
            return;
        }
        let at = self.shown_row().and_then(|r| rows.iter().position(|x| *x == r));
        let to = match at {
            Some(at) => (at as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            None => 0,
        };
        // At an end already: nothing moves (its changes aren't read again).
        if at == Some(to) {
            return;
        }
        let (i, staged) = rows[to];
        self.select(i, staged);
        self.reveal = true;
    }

    /// ⇧↑ / ⇧↓: the file `delta` rows away on the shown file's list shown, everything from
    /// the anchor to it chosen (stopping at the list's ends).
    pub(super) fn extend(&mut self, delta: isize) {
        let Some((path, staged)) = self.selected.clone() else { return };
        let list = self.list(staged);
        let Some(at) = list.iter().position(|i| self.files[*i].path == path) else { return };
        let to = (at as isize + delta).clamp(0, list.len() as isize - 1) as usize;
        if to == at {
            return;
        }
        let next = self.files[list[to]].path.clone();
        if self.anchor.is_none() {
            self.anchor = Some(path);
        }
        self.mark_run_to(&next, staged);
        self.show(next, staged);
        self.reveal = true;
    }

    /// The shown file (with what's chosen with it) moved to the other list.
    pub(super) fn toggle_shown(&mut self) {
        if let Some((i, staged)) = self.shown_row() {
            self.toggle(i, staged);
        }
    }

    /// Opening the shown file (one that was deleted has nothing to open).
    pub(super) fn open_shown(&self) -> Option<ViewerAction> {
        let (i, staged) = self.shown_row()?;
        let file = &self.files[i];
        let state = if staged { file.staged } else { file.unstaged };
        if state == Some(FileState::Deleted) {
            return None;
        }
        let path = self.repo.as_ref()?.root().join(&file.path);
        Some(ViewerAction::OpenFile { path, line: None, column: None })
    }

    /// Everything on the shown list chosen (⌘A in the lists).
    pub(super) fn mark_all(&mut self) {
        let Some((_, staged)) = self.selected.clone() else { return };
        let list = self.list(staged);
        self.marked = list.into_iter().map(|i| self.files[i].path.clone()).collect();
    }

    /// Where the shown file's row is in `staged`'s list (its place), if it's there.
    pub(super) fn shown_place(&self, staged: bool) -> Option<usize> {
        let (path, on) = self.selected.as_ref()?;
        if *on != staged {
            return None;
        }
        self.list(staged).into_iter().position(|i| self.files[i].path == *path)
    }

    /// After the lists were read again: only what's still on the shown list stays chosen.
    pub(super) fn keep_marked(&mut self) {
        let Some((shown, staged)) = self.selected.clone() else {
            self.marked.clear();
            return;
        };
        let on_list: std::collections::BTreeSet<String> =
            self.list(staged).into_iter().map(|i| self.files[i].path.clone()).collect();
        self.marked.retain(|path| on_list.contains(path));
        // A run starting from a file gone from the list starts from the shown one.
        if !self.anchor.as_ref().is_some_and(|a| on_list.contains(a)) {
            self.anchor = Some(shown.clone());
        }
        self.marked.insert(shown);
    }
}
