//! "Extend Selection" (⌥↑) and "Shrink Selection" (⌥↓): each cursor's selection grows to
//! the next construct around it — the word, then the syntax tree's nodes (what's inside
//! brackets or quotes before them), up to the whole file — and shrinks back the way it
//! came. Every language with a grammar alike (its embedded code too); a file without one
//! grows by the word, the line, the file. Only in the editor: elsewhere the keys move as
//! they always did.

use super::*;
use document::{Selection, Selections};

/// What "Shrink Selection" goes back through: the selections before each step, and the
/// one the last step made (another selection made since forgets them).
#[derive(Debug, Default)]
pub(super) struct SelectionSteps {
    before: Vec<Selections>,
    made: Option<Selections>,
}

impl Workbench {
    /// Grows (`grow`) or shrinks the active editor's selections. `true` if they changed.
    pub fn extend_selection(&mut self, text: &mut TextSystem, grow: bool) -> bool {
        let in_editor = self.focus == Focus::Editor
            && !self.keyboard_in_overlay()
            && self.tabs.get(self.active).is_some_and(|t| t.editor_visible());
        if !in_editor {
            let motion = if grow { Motion::Up } else { Motion::Down };
            return self.edit_key(text, EditKey::Move { motion, extend: false });
        }
        let Some(tab) = self.tabs.get_mut(self.active) else { return false };
        let current = tab.doc.selections().clone();
        if tab.selection_steps.made.as_ref() != Some(&current) {
            tab.selection_steps = SelectionSteps::default();
        }
        let next = if grow {
            let language = tab.syntax.language().clone();
            let word_chars = self.support.languages.get(&language).map_or("", |l| l.word_chars);
            let next = grown(tab, &current, word_chars);
            if next == current {
                return false;
            }
            tab.selection_steps.before.push(current);
            next
        } else {
            match tab.selection_steps.before.pop() {
                Some(before) => before,
                None => return false,
            }
        };
        tab.selection_steps.made = Some(next.clone());
        tab.doc.set_selections(next);
        tab.view.reveal_cursor(&tab.doc, &tab.folds, text, &self.palette);
        true
    }
}

/// Each selection grown to the next range around it (the primary stays the primary).
fn grown(tab: &Tab, current: &Selections, word_chars: &str) -> Selections {
    let buffer = tab.doc.buffer();
    current.transform(|selection| {
        let range = selection.range();
        let mut steps: Vec<std::ops::Range<usize>> = Vec::new();
        // The word first (a cursor in it, or a part of it selected).
        let word = commands::word_range(&tab.doc, range.start, word_chars);
        if word.start <= range.start && range.end <= word.end && word.len() > range.len() {
            steps.push(word);
        }
        match tab.syntax.snapshot() {
            Some(snapshot) => steps.extend(snapshot.selection_ranges(range.clone())),
            None => {
                // No grammar: the line's text, the whole line, the file.
                let line = buffer.line_of(range.start);
                let (start, end) = (buffer.line_start(line), buffer.line_end(line));
                let content = buffer.slice(start..end);
                let lead = content.len() - content.trim_start().len();
                let trail = content.len() - content.trim_end().len();
                steps.push(start + lead..end - trail);
                if buffer.line_of(range.end) == line {
                    steps.push(start..end);
                }
                steps.push(0..buffer.len());
            }
        }
        let next = steps
            .into_iter()
            .filter(|r| r.end <= buffer.len())
            .find(|r| r.start <= range.start && range.end <= r.end && r.len() > range.len());
        match next {
            Some(r) => Selection::new(r.start, r.end),
            None => *selection,
        }
    })
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/smart_select.rs"]
mod tests;
