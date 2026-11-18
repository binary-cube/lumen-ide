//! Cursors and selections (multi-cursor).

use std::ops::Range;

use super::edit::{Assoc, OffsetMap, Transaction};

/// A selection: `anchor` is the fixed end, `head` is the cursor. Empty when the two coincide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
    /// The desired visual column for up/down movement (kept across shorter lines).
    pub goal: Option<u32>,
}

impl Selection {
    pub fn cursor(at: usize) -> Self {
        Self { anchor: at, head: at, goal: None }
    }

    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head, goal: None }
    }

    pub fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    fn map(&self, offsets: &OffsetMap<'_>) -> Self {
        Self {
            anchor: offsets.map(self.anchor, Assoc::After),
            head: offsets.map(self.head, Assoc::After),
            goal: None,
        }
    }
}

/// A document's set of cursors: sorted, non-overlapping, with one primary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selections {
    items: Vec<Selection>,
    primary: usize,
}

impl Selections {
    pub fn single(selection: Selection) -> Self {
        Self { items: vec![selection], primary: 0 }
    }

    /// Normalizes: sorts and merges overlapping selections.
    pub fn new(items: Vec<Selection>, primary: usize) -> Self {
        if items.is_empty() {
            return Self::single(Selection::cursor(0));
        }
        let primary_sel = items[primary.min(items.len() - 1)];
        let mut items = items;
        items.sort_by_key(|s| (s.range().start, s.range().end));
        let mut merged: Vec<Selection> = Vec::with_capacity(items.len());
        for sel in items {
            match merged.last_mut() {
                Some(last) if overlaps(last, &sel) => {
                    let start = last.range().start.min(sel.range().start);
                    let end = last.range().end.max(sel.range().end);
                    let forward = last.head >= last.anchor;
                    *last = if forward {
                        Selection::new(start, end)
                    } else {
                        Selection::new(end, start)
                    };
                }
                _ => merged.push(sel),
            }
        }
        // The identical selection first; otherwise the one it was absorbed into on merge.
        let primary = merged
            .iter()
            .position(|s| s.anchor == primary_sel.anchor && s.head == primary_sel.head)
            .or_else(|| {
                let r = primary_sel.range();
                merged.iter().position(|s| s.range().start <= r.start && r.end <= s.range().end)
            })
            .unwrap_or(0);
        Self { items: merged, primary }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Selection> {
        self.items.iter()
    }

    pub fn as_slice(&self) -> &[Selection] {
        &self.items
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn primary(&self) -> Selection {
        self.items[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    /// The selections after applying a transaction.
    pub fn map(&self, tx: &Transaction) -> Self {
        let offsets = tx.offsets();
        Self::new(self.items.iter().map(|s| s.map(&offsets)).collect(), self.primary)
    }

    /// Transforms each selection; the result is normalized.
    pub fn transform(&self, f: impl FnMut(&Selection) -> Selection) -> Self {
        Self::new(self.items.iter().map(f).collect(), self.primary)
    }

    /// Adds a selection and makes it primary.
    pub fn add(&self, selection: Selection) -> Self {
        let mut items = self.items.clone();
        items.push(selection);
        let primary = items.len() - 1;
        Self::new(items, primary)
    }

    /// Keeps only the primary cursor.
    pub fn only_primary(&self) -> Self {
        Self::single(self.primary())
    }

    pub fn clamp(&self, len: usize) -> Self {
        self.transform(|s| Selection {
            anchor: s.anchor.min(len),
            head: s.head.min(len),
            goal: s.goal,
        })
    }
}

fn overlaps(a: &Selection, b: &Selection) -> bool {
    let (ra, rb) = (a.range(), b.range());
    if ra.is_empty() || rb.is_empty() {
        // A cursor merges with a selection only if it is strictly inside it.
        return ra == rb
            || (rb.start > ra.start && rb.start < ra.end)
            || (ra.start > rb.start && ra.start < rb.end);
    }
    rb.start < ra.end
}

#[cfg(test)]
#[path = "../../tests/unit/domain/selection.rs"]
mod tests;
