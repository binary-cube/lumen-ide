//! The undo/redo history, with grouping of consecutive keystrokes.

use super::buffer::TextBuffer;
use super::edit::{TextChange, Transaction};
use super::selection::Selections;

/// How many undo groups we keep…
const LIMIT: usize = 1000;
/// …and how much text they may hold (inserted + removed, both directions): a few
/// "replace all" on a large file must not pin hundreds of MB.
const BYTE_BUDGET: usize = 32 * 1024 * 1024;

/// The kind of a change, used for undo grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Typed text; consecutive keystrokes are grouped by word.
    Typing,
    /// Deletion with Backspace / Delete; consecutive deletions are grouped.
    Deletion,
    /// Any other command (paste, indentation, replace): a separate step.
    Command,
}

#[derive(Debug, Clone)]
struct Step {
    tx: Transaction,
    inverse: Transaction,
}

impl Step {
    /// The text this step keeps alive.
    fn bytes(&self) -> usize {
        let text = |t: &Transaction| t.edits().iter().map(|e| e.text.len()).sum::<usize>();
        text(&self.tx) + text(&self.inverse)
    }
}

#[derive(Debug, Clone)]
struct Group {
    steps: Vec<Step>,
    before: Selections,
    after: Selections,
    kind: EditKind,
    /// The last step inserted text ending with whitespace.
    ends_with_space: bool,
    /// The text its steps keep alive.
    bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    /// Oldest first (evicted from the front).
    undo: std::collections::VecDeque<Group>,
    /// The text held by `undo`.
    undo_bytes: usize,
    redo: Vec<Group>,
    /// The next change must start a new group.
    sealed: bool,
}

impl History {
    /// The text the history keeps alive (undo and redo), in bytes.
    pub fn bytes(&self) -> usize {
        self.undo_bytes + self.redo.iter().map(|g| g.bytes).sum::<usize>()
    }

    pub fn record(
        &mut self,
        tx: Transaction,
        inverse: Transaction,
        before: Selections,
        after: Selections,
        kind: EditKind,
    ) {
        self.redo.clear();
        let inserted: String = tx.edits().iter().map(|e| e.text.as_str()).collect();
        let starts_word = inserted.chars().next().is_some_and(|c| !c.is_whitespace());
        let ends_with_space = inserted.chars().last().is_some_and(char::is_whitespace);

        let merge = !self.sealed
            && kind != EditKind::Command
            && !inserted.contains('\n')
            && self.undo.back().is_some_and(|g| {
                g.kind == kind && !(kind == EditKind::Typing && g.ends_with_space && starts_word)
            });

        let step = Step { tx, inverse };
        let bytes = step.bytes();
        self.undo_bytes += bytes;
        match (merge, self.undo.back_mut()) {
            (true, Some(group)) => {
                group.steps.push(step);
                group.after = after;
                group.ends_with_space = ends_with_space;
                group.bytes += bytes;
            }
            _ => {
                let steps = vec![step];
                self.undo.push_back(Group { steps, before, after, kind, ends_with_space, bytes });
            }
        }
        // The newest group always stays, even if it alone is over budget.
        while self.undo.len() > 1
            && (self.undo.len() > LIMIT || self.undo_bytes > BYTE_BUDGET)
            && let Some(old) = self.undo.pop_front()
        {
            self.undo_bytes -= old.bytes;
        }
        self.sealed = kind == EditKind::Command;
    }

    /// Closes the current group (e.g. after a cursor movement).
    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last group; returns the selections before it and the changes made.
    pub fn undo(&mut self, buffer: &mut TextBuffer) -> Option<(Selections, Vec<TextChange>)> {
        let group = self.undo.pop_back()?;
        self.undo_bytes -= group.bytes;
        let mut changes = Vec::new();
        for step in group.steps.iter().rev() {
            changes.extend(step.inverse.apply(buffer).1);
        }
        let selections = group.before.clone();
        self.redo.push(group);
        self.sealed = true;
        Some((selections, changes))
    }

    /// Redoes the last undone group; returns the selections after it and the changes made.
    pub fn redo(&mut self, buffer: &mut TextBuffer) -> Option<(Selections, Vec<TextChange>)> {
        let group = self.redo.pop()?;
        let mut changes = Vec::new();
        for step in &group.steps {
            changes.extend(step.tx.apply(buffer).1);
        }
        let selections = group.after.clone();
        self.undo_bytes += group.bytes;
        self.undo.push_back(group);
        self.sealed = true;
        Some((selections, changes))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/history.rs"]
mod tests;
