//! A diagram's changes and their history: every edit is a [`Transaction`] (cells put in,
//! taken out or replaced, in order), applied to the document, which gives back the one that
//! undoes it. The history keeps those (at most so many, so many bytes), a drag's steps
//! merged into one; undo applies one and keeps what redoes it.

use crate::cell::Cell;
use crate::document::Document;

/// One change to a page's cells.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// `cell` put in at `at` (the cells after it move up).
    Insert { page: usize, at: usize, cell: Box<Cell> },
    /// The cell at `at` taken out.
    Remove { page: usize, at: usize },
    /// The cell at `at` replaced by `cell`.
    Replace { page: usize, at: usize, cell: Box<Cell> },
    /// A page's setting (`grid`, `gridSize`, `background`…) set (`None`: taken away).
    Setting { page: usize, name: String, value: Option<String> },
}

impl Change {
    /// What it holds, roughly (the history's budget).
    fn bytes(&self) -> usize {
        match self {
            Self::Insert { cell, .. } | Self::Replace { cell, .. } => cell.weight(),
            Self::Remove { .. } => 32,
            Self::Setting { name, value, .. } => {
                64 + name.len() + value.as_ref().map_or(0, String::len)
            }
        }
    }
}

/// Changes made together (one step of the history), in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Transaction {
    pub changes: Vec<Change>,
}

impl Transaction {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    fn bytes(&self) -> usize {
        self.changes.iter().map(Change::bytes).sum()
    }
}

impl Document {
    /// `tx` applied; what undoes it is returned. A change whose place isn't there (never
    /// from a transaction made for this document) is skipped.
    pub fn apply(&mut self, tx: Transaction) -> Transaction {
        let mut undo = Vec::with_capacity(tx.changes.len());
        for change in tx.changes {
            if let Some(inverse) = self.apply_one(change) {
                undo.push(inverse);
            }
        }
        undo.reverse();
        Transaction { changes: undo }
    }

    fn apply_one(&mut self, change: Change) -> Option<Change> {
        match change {
            Change::Insert { page, at, cell } => {
                let cells = &mut self.pages.get_mut(page)?.cells;
                if at > cells.len() {
                    return None;
                }
                cells.insert(at, *cell);
                Some(Change::Remove { page, at })
            }
            Change::Remove { page, at } => {
                let cells = &mut self.pages.get_mut(page)?.cells;
                if at >= cells.len() {
                    return None;
                }
                let cell = cells.remove(at);
                Some(Change::Insert { page, at, cell: Box::new(cell) })
            }
            Change::Replace { page, at, cell } => {
                let slot = self.pages.get_mut(page)?.cells.get_mut(at)?;
                let before = std::mem::replace(slot, *cell);
                Some(Change::Replace { page, at, cell: Box::new(before) })
            }
            Change::Setting { page, name, value } => {
                let p = self.pages.get_mut(page)?;
                let before = p.setting(&name).map(str::to_owned);
                match &value {
                    Some(v) => p.set_setting(&name, v),
                    None => p.remove_setting(&name),
                }
                Some(Change::Setting { page, name, value: before })
            }
        }
    }
}

/// The most steps kept, and the most bytes (as the text editor's history).
pub const HISTORY_STEPS: usize = 500;
pub const HISTORY_BYTES: usize = 32 << 20;

/// A step undone or redone by applying `tx`; `merge`: the steps of one gesture (a drag)
/// share it and become one.
#[derive(Debug, Clone)]
struct Step {
    tx: Transaction,
    merge: Option<u64>,
    bytes: usize,
}

/// What undoes the edits made, and what redoes those undone.
#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    bytes: usize,
    /// A new gesture starts: the next step isn't merged into the last one.
    sealed: bool,
}

impl History {
    /// An edit made (`undo`: what undoes it); `merge`: the gesture it belongs to (the same as
    /// the last step's: one step with it). What was undone can't be redone after it.
    pub fn record(&mut self, undo: Transaction, merge: Option<u64>) {
        if undo.is_empty() {
            return;
        }
        self.redo.clear();
        let same =
            !self.sealed && merge.is_some() && self.undo.last().is_some_and(|s| s.merge == merge);
        self.sealed = false;
        if same && let Some(last) = self.undo.last_mut() {
            // Undoing both: this one first, then the earlier one.
            let mut changes = undo.changes;
            changes.append(&mut last.tx.changes);
            last.tx = compact(Transaction { changes });
            self.bytes = self.bytes.saturating_sub(last.bytes);
            last.bytes = last.tx.bytes();
            self.bytes += last.bytes;
        } else {
            let bytes = undo.bytes();
            self.bytes += bytes;
            self.undo.push(Step { tx: undo, merge, bytes });
        }
        self.trim();
    }

    /// The next step is a new gesture (the mouse released).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// The last edit undone. `false`: nothing to undo.
    pub fn undo(&mut self, doc: &mut Document) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        self.bytes = self.bytes.saturating_sub(step.bytes);
        let redo = doc.apply(step.tx);
        let bytes = redo.bytes();
        self.redo.push(Step { tx: redo, merge: None, bytes });
        self.sealed = true;
        true
    }

    /// The last undo redone. `false`: nothing to redo.
    pub fn redo(&mut self, doc: &mut Document) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        let undo = doc.apply(step.tx);
        let bytes = undo.bytes();
        self.bytes += bytes;
        self.undo.push(Step { tx: undo, merge: None, bytes });
        self.sealed = true;
        self.trim();
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The oldest steps let go past the limits.
    fn trim(&mut self) {
        while self.undo.len() > HISTORY_STEPS || (self.bytes > HISTORY_BYTES && self.undo.len() > 1)
        {
            let old = self.undo.remove(0);
            self.bytes = self.bytes.saturating_sub(old.bytes);
        }
    }
}

/// A merged undo with only replacements: each cell's earliest state kept (the one the
/// gesture started from), the rest dropped.
fn compact(tx: Transaction) -> Transaction {
    if !tx.changes.iter().all(|c| matches!(c, Change::Replace { .. })) {
        return tx;
    }
    // Applied in order, the last replacement of a place wins: keep only that one.
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<Change> = tx
        .changes
        .into_iter()
        .rev()
        .filter(|c| match c {
            Change::Replace { page, at, .. } => seen.insert((*page, *at)),
            _ => true,
        })
        .collect();
    kept.reverse();
    Transaction { changes: kept }
}

/// A document and its history: an edit applied and recorded in one call.
#[derive(Debug, Clone)]
pub struct Diagram {
    pub doc: Document,
    pub history: History,
    /// Bumped by every change (what's drawn from it follows).
    pub version: u64,
    /// The version last saved.
    saved: u64,
}

impl Diagram {
    pub fn new(doc: Document) -> Self {
        Self { doc, history: History::default(), version: 0, saved: 0 }
    }

    /// `tx` applied and recorded (`merge`: the gesture it belongs to). `false`: it changed
    /// nothing.
    pub fn edit(&mut self, tx: Transaction, merge: Option<u64>) -> bool {
        if tx.is_empty() {
            return false;
        }
        let undo = self.doc.apply(tx);
        self.history.record(undo, merge);
        self.version += 1;
        true
    }

    pub fn undo(&mut self) -> bool {
        let done = self.history.undo(&mut self.doc);
        if done {
            self.version += 1;
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.history.redo(&mut self.doc);
        if done {
            self.version += 1;
        }
        done
    }

    /// Changed since saved (an undo back to the saved state still counts: compared by
    /// version).
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved
    }

    /// Written: what's on disk now.
    pub fn mark_saved(&mut self) {
        self.saved = self.version;
    }
}

#[cfg(test)]
#[path = "../tests/unit/edit.rs"]
mod tests;
