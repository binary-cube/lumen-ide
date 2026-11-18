//! The trees of the last few texts asked about (the document being typed in, the files a
//! Find Usages reads one occurrence after another): a keystroke parses again only what
//! changed (the edit between the closest text kept and the new one, then tree-sitter's
//! incremental parse), never the whole file from scratch on every question.

use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use code_intelligence::FileSymbols;

use kernel::cache::Purge;
use syntax_ts::tree_sitter::{InputEdit, Point, Tree};

/// How many texts are kept.
const SLOTS: usize = 4;

/// A text's tree, and its declarations when asked for (the inference's view of its names).
pub struct Parsed {
    pub tree: Tree,
    symbols: OnceLock<FileSymbols>,
}

impl Parsed {
    /// The declarations of `text` (this tree's), read once.
    pub fn symbols(&self, text: &str) -> &FileSymbols {
        self.symbols.get_or_init(|| crate::intelligence::symbols_of(&self.tree, text))
    }
}

#[derive(Default)]
struct Trees {
    /// The most recently used last.
    slots: Vec<(String, Arc<Parsed>)>,
}

impl Purge for Trees {
    fn purge(&mut self) {
        self.slots.clear();
    }
}

static TREES: LazyLock<Mutex<Trees>> = LazyLock::new(|| Mutex::new(Trees::default()));

/// The tree of `text` (kept for the next question).
pub fn parsed(text: &str) -> Option<Arc<Parsed>> {
    let mut trees = TREES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(i) = trees.slots.iter().position(|(t, _)| t == text) {
        let entry = trees.slots.remove(i);
        let parsed = Arc::clone(&entry.1);
        trees.slots.push(entry);
        return Some(parsed);
    }
    // The closest text kept (the longest common start): what's typed is an edit of it.
    let closest = trees
        .slots
        .iter()
        .enumerate()
        .map(|(i, (t, _))| (i, common_prefix(t, text)))
        .filter(|(_, n)| *n > 0)
        .max_by_key(|(_, n)| *n)
        .map(|(i, _)| i);
    let old = closest.map(|i| {
        let (old_text, old) = &trees.slots[i];
        let mut tree = old.tree.clone();
        tree.edit(&edit(old_text, text));
        tree
    });
    let tree = crate::intelligence::parse_from(text, old.as_ref())?;
    if trees.slots.len() >= SLOTS {
        trees.slots.remove(0);
    }
    let parsed = Arc::new(Parsed { tree, symbols: OnceLock::new() });
    trees.slots.push((text.to_owned(), Arc::clone(&parsed)));
    Some(parsed)
}

/// Lets the trees go (they're parsed again when asked).
pub fn release() -> bool {
    let mut trees = TREES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = !trees.slots.is_empty();
    trees.purge();
    had
}

/// The heap the kept texts take (their trees aren't counted: tree-sitter's own).
pub fn weight() -> u64 {
    let trees = TREES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    trees.slots.iter().map(|(t, _)| t.capacity() as u64).sum()
}

/// How many bytes `a` and `b` start with in common (at a character boundary).
fn common_prefix(a: &str, b: &str) -> usize {
    let n = a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count();
    let mut n = n.min(a.len()).min(b.len());
    while !b.is_char_boundary(n) || !a.is_char_boundary(n) {
        n -= 1;
    }
    n
}

/// The edit turning `old` into `new`: the changed middle between their common start and end.
fn edit(old: &str, new: &str) -> InputEdit {
    let start = common_prefix(old, new);
    let max_suffix = old.len().min(new.len()) - start;
    let mut suffix = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suffix)
        .take_while(|(x, y)| x == y)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    let (old_end, new_end) = (old.len() - suffix, new.len() - suffix);
    InputEdit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_position: point(old, start),
        old_end_position: point(old, old_end),
        new_end_position: point(new, new_end),
    }
}

/// The row and column (bytes) of `offset` in `text`.
fn point(text: &str, offset: usize) -> Point {
    let head = &text[..offset];
    let row = head.bytes().filter(|b| *b == b'\n').count();
    let column = head.rfind('\n').map_or(offset, |i| offset - i - 1);
    Point { row, column }
}

#[cfg(test)]
#[path = "../tests/unit/doctree.rs"]
mod tests;
