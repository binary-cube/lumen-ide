//! Find / Replace for a document. Each tab has its own [`FindState`].

use std::ops::Range;

use crate::domain::buffer::TextBuffer;
use crate::domain::document::Document;
use crate::domain::edit::{Edit, TextChange, Transaction};
use crate::domain::history::EditKind;
use crate::domain::search::{Matcher, SearchError, SearchQuery, next_match, prev_match};
use crate::domain::selection::{Selection, Selections};

#[derive(Debug, Clone, Default)]
pub struct FindState {
    pub query: SearchQuery,
    pub replacement: String,
    matcher: Option<Matcher>,
    error: Option<SearchError>,
    matches: Vec<Range<usize>>,
    /// The document version for which `matches` are valid.
    matched_version: Option<u64>,
    current: Option<usize>,
    /// Bumped by every query: a whole search's answer for an older one is dropped.
    generation: u64,
    /// A whole search handed out ([`FindState::whole_search`]) for this generation: the
    /// matches wait for it instead of being searched here.
    waiting: Option<u64>,
    /// [`FindState::select_from_cursor`] asked while waiting: done when the answer comes.
    select_when_found: bool,
}

/// A whole document searched elsewhere (a worker): the query's generation, the document's
/// version, its text and the search.
#[derive(Debug, Clone)]
pub struct WholeSearch {
    pub generation: u64,
    pub version: u64,
    pub buffer: TextBuffer,
    pub matcher: Matcher,
}

impl WholeSearch {
    /// The search, run (on the worker).
    pub fn run(&self) -> Vec<Range<usize>> {
        self.matcher.find_all(&self.buffer)
    }
}

impl FindState {
    /// Recompiles the search after the pattern or options change.
    pub fn set_query(&mut self, query: SearchQuery) {
        self.query = query;
        match Matcher::new(&self.query) {
            Ok(m) => {
                self.matcher = m;
                self.error = None;
            }
            Err(e) => {
                self.matcher = None;
                self.error = Some(e);
            }
        }
        self.matched_version = None;
        self.current = None;
        self.generation += 1;
        self.waiting = None;
        self.select_when_found = false;
    }

    /// The whole search the matches need now, for a document of at least `min_len` bytes
    /// (searched elsewhere, not on the caller's thread): `None` when they're up to date, a
    /// search is already out, or edits since the last matches are searched here (only their
    /// lines). Until [`Self::found`] the matches stay as they were.
    pub fn whole_search(&mut self, doc: &Document, min_len: usize) -> Option<WholeSearch> {
        let matcher = self.matcher.as_ref()?;
        let fresh = self.matched_version == Some(doc.version());
        let incremental = self
            .matched_version
            .is_some_and(|v| doc.changes_since(v).is_some_and(|c| c.len() <= INCREMENTAL_CHANGES));
        if fresh || incremental || self.waiting.is_some() || doc.buffer().len() < min_len {
            return None;
        }
        self.waiting = Some(self.generation);
        Some(WholeSearch {
            generation: self.generation,
            version: doc.version(),
            buffer: doc.buffer().clone(),
            matcher: matcher.clone(),
        })
    }

    /// A whole search's answer (`matches` for `version` of the document). `true`: the
    /// selection was asked for meanwhile ([`Self::select_from_cursor`]) and is to be made
    /// now; an answer for an older query is dropped.
    pub fn found(&mut self, generation: u64, version: u64, matches: Vec<Range<usize>>) -> bool {
        if self.waiting != Some(generation) {
            return false;
        }
        self.waiting = None;
        self.matches = matches;
        self.matched_version = Some(version);
        self.current = self.current.filter(|&i| i < self.matches.len());
        std::mem::take(&mut self.select_when_found)
    }

    /// What's asked now (a key going to the next match, a replace) doesn't wait for a whole
    /// search that's out: the matches are searched here, its answer dropped.
    fn settle(&mut self) {
        if self.waiting.take().is_some() {
            self.matched_version = None;
            self.select_when_found = false;
        }
    }

    /// A whole search is out: the matches wait for it.
    pub fn is_searching(&self) -> bool {
        self.waiting.is_some()
    }

    /// The index of the match selected by the last navigation (in [`FindState::matches`]).
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }

    pub fn error(&self) -> Option<&SearchError> {
        self.error.as_ref()
    }

    /// The matches, recomputed only if the document has changed: after edits, only on the
    /// lines they touched (typing with the bar open doesn't search the whole file again).
    pub fn matches(&mut self, doc: &Document) -> &[Range<usize>] {
        if self.matched_version != Some(doc.version()) && self.waiting.is_none() {
            let updated = match (&self.matcher, self.matched_version) {
                (Some(matcher), Some(version)) => doc.changes_since(version).and_then(|changes| {
                    updated_matches(matcher, doc.buffer(), &self.matches, &changes)
                }),
                _ => None,
            };
            self.matches = updated.unwrap_or_else(|| {
                self.matcher.as_ref().map(|m| m.find_all(doc.buffer())).unwrap_or_default()
            });
            self.matched_version = Some(doc.version());
            self.current = self.current.filter(|&i| i < self.matches.len());
        }
        &self.matches
    }

    /// (current index starting from 1, total) for display, e.g. "3/17".
    pub fn counter(&mut self, doc: &Document) -> (usize, usize) {
        let total = self.matches(doc).len();
        (self.current.map_or(0, |i| i + 1), total)
    }

    /// Selects the match at/after the cursor (used while typing in the search field).
    pub fn select_from_cursor(&mut self, doc: &mut Document) {
        if self.waiting.is_some() {
            self.select_when_found = true;
            return;
        }
        let from = doc.selections().primary().range().start;
        self.select(doc, |m| next_match(m, from));
    }

    pub fn find_next(&mut self, doc: &mut Document) {
        self.settle();
        let from = doc.selections().primary().range().end;
        self.select(doc, |m| next_match(m, from));
    }

    pub fn find_prev(&mut self, doc: &mut Document) {
        self.settle();
        let from = doc.selections().primary().range().start;
        self.select(doc, |m| prev_match(m, from));
    }

    /// Replaces the selected match (if it is selected) and moves to the next one.
    pub fn replace_current(&mut self, doc: &mut Document) {
        self.settle();
        let selected = doc.selections().primary().range();
        let matches = self.matches(doc).to_vec();
        if let (Some(m), Some(matcher)) = (matches.iter().find(|m| **m == selected), &self.matcher)
        {
            let matched = doc.buffer().slice(m.clone());
            let text = matcher.replacement(&matched, &self.replacement, self.query.regex);
            let end = m.start + text.len();
            doc.apply(
                Transaction::single(m.clone(), text),
                Some(Selections::single(Selection::cursor(end))),
                EditKind::Command,
            );
        }
        self.find_next(doc);
    }

    /// Replaces all matches, as a single undo step. Returns how many there were.
    pub fn replace_all(&mut self, doc: &mut Document) -> usize {
        self.settle();
        let Some(matcher) = self.matcher.clone() else { return 0 };
        let matches = self.matches(doc).to_vec();
        let edits: Vec<Edit> = matches
            .iter()
            .map(|m| {
                let matched = doc.buffer().slice(m.clone());
                Edit::new(
                    m.clone(),
                    matcher.replacement(&matched, &self.replacement, self.query.regex),
                )
            })
            .collect();
        let count = edits.len();
        doc.apply(Transaction::new(edits), None, EditKind::Command);
        self.current = None;
        count
    }

    fn select(&mut self, doc: &mut Document, pick: impl FnOnce(&[Range<usize>]) -> Option<usize>) {
        let matches = self.matches(doc);
        let Some(index) = pick(matches) else {
            self.current = None;
            return;
        };
        let m = matches[index].clone();
        self.current = Some(index);
        doc.set_selections(Selections::single(Selection::new(m.start, m.end)));
    }
}

/// More changes than this since the matches were made: searched again whole.
const INCREMENTAL_CHANGES: usize = 256;

/// `old` (the matches before `changes`, in order) after them: the ones away from the
/// changed lines moved with the text, those lines searched again. `None`: too much
/// changed (a whole search costs the same).
fn updated_matches(
    matcher: &Matcher,
    buffer: &TextBuffer,
    old: &[Range<usize>],
    changes: &[TextChange],
) -> Option<Vec<Range<usize>>> {
    if changes.len() > INCREMENTAL_CHANGES {
        return None;
    }
    // Each change is in the text the ones before it made: matches and changed zones follow
    // them in turn (a match the change touches goes; its line is searched again).
    let mut kept = old.to_vec();
    let mut dirty: Vec<Range<usize>> = Vec::with_capacity(changes.len());
    for c in changes {
        let shift = |x: usize| x - c.old_end + c.new_end;
        kept.retain_mut(|m| {
            if m.end <= c.start {
                true
            } else if m.start >= c.old_end {
                *m = shift(m.start)..shift(m.end);
                true
            } else {
                false
            }
        });
        for d in &mut dirty {
            let map = |x: usize, inside: usize| {
                if x <= c.start {
                    x
                } else if x >= c.old_end {
                    shift(x)
                } else {
                    inside
                }
            };
            let (a, b) = (map(d.start, c.start), map(d.end, c.new_end));
            *d = a..b.max(a);
        }
        dirty.push(c.start..c.new_end);
    }
    // The changed zones' lines, merged.
    let mut lines: Vec<(usize, usize)> =
        dirty.iter().map(|d| (buffer.line_of(d.start), buffer.line_of(d.end))).collect();
    lines.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(lines.len());
    for (a, b) in lines {
        match merged.last_mut() {
            Some((_, end)) if a <= *end + 1 => *end = (*end).max(b),
            _ => merged.push((a, b)),
        }
    }
    let touched: usize = merged.iter().map(|(a, b)| b - a + 1).sum();
    if touched > buffer.line_count() / 2 {
        return None;
    }
    let mut out = Vec::with_capacity(kept.len());
    let mut i = 0;
    for (first, last) in merged {
        let from = buffer.line_start(first);
        let next =
            if last + 1 < buffer.line_count() { buffer.line_start(last + 1) } else { usize::MAX };
        while i < kept.len() && kept[i].start < from {
            out.push(kept[i].clone());
            i += 1;
        }
        while i < kept.len() && kept[i].start < next {
            i += 1;
        }
        matcher.find_in_lines(buffer, first..last + 1, &mut out);
    }
    out.extend_from_slice(&kept[i..]);
    Some(out)
}

#[cfg(test)]
#[path = "../../tests/unit/application/find.rs"]
mod tests;
