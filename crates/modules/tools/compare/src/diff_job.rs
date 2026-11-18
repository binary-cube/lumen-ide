//! The differences found on a thread of their own (a big file never blocks the interface), taken
//! when ready.

use super::*;

impl Compare {
    /// The differences found: the rows, the picks (resolving: each side's change alone
    /// taken, the conflicts waiting), straight to the first one (resolving: conflict).
    pub(crate) fn diffed(&mut self, found: MergeChunks) {
        self.chunks = found.chunks;
        self.picks = if let Some(resolving) = &mut self.resolving {
            resolving.chosen = vec![false; found.picks.len()];
            resolving.initial.clone_from(&found.picks);
            resolving.conflicts = found.conflicts;
            found.picks
        } else {
            vec![self.default_pick(); self.changes()]
        };
        self.loaded = true;
        self.rebuild_rows();
        if !self.change_rows.is_empty() {
            let conflict =
                self.resolving.as_ref().and_then(|r| r.conflicts.iter().position(|c| *c));
            self.current = conflict.unwrap_or(0);
            self.reveal_first = true;
        }
    }

    /// Finds the differences on a thread; `poll` takes them (the app woken when they're
    /// found, once the view was painted).
    pub(crate) fn diff_in_background(&mut self) {
        let (left, right) = (self.left.clone(), self.right.clone());
        let base = self.resolving.as_ref().map(|r| r.base.clone());
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = Arc::clone(&self.waker.0);
        kernel::work::run("compare-diff", move || {
            let found = match base {
                Some(base) => MergeChunks::of(&diff::merge3(&base, &left, &right)),
                None => MergeChunks { chunks: diff_lines(&left, &right), ..MergeChunks::default() },
            };
            let _ = tx.send((None, found));
            if let Some(wake) = waker.lock().ok().and_then(|w| w.clone()) {
                wake();
            }
        });
        self.diffing = Some(rx);
    }

    /// A side saved as `lines` (the composed result): the differences found again from it on
    /// a thread, the side replaced when they come (what's shown stays meanwhile, its rows
    /// still those of the sides it shows).
    pub(crate) fn diff_saved(&mut self, side: Side, lines: Vec<String>) {
        let (mut left, mut right) = (self.left.clone(), self.right.clone());
        match side {
            Side::Left => left.clone_from(&lines),
            Side::Right => right.clone_from(&lines),
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = Arc::clone(&self.waker.0);
        kernel::work::run("compare-diff", move || {
            let found = MergeChunks { chunks: diff_lines(&left, &right), ..MergeChunks::default() };
            let _ = tx.send((Some((side, lines)), found));
            if let Some(wake) = waker.lock().ok().and_then(|w| w.clone()) {
                wake();
            }
        });
        self.diffing = Some(rx);
        self.saved = true;
    }

    /// The saved side's differences come: the comparison goes on from it.
    pub(crate) fn saved_diffed(&mut self, side: Side, lines: Vec<String>, found: MergeChunks) {
        match side {
            Side::Left => self.left = lines,
            Side::Right => self.right = lines,
        }
        self.saved = false;
        self.chunks = found.chunks;
        self.picks = vec![self.default_pick(); self.changes()];
        self.current = 0;
        self.rebuild_rows();
    }
}
