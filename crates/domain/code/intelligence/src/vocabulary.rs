//! What a language's suggestions are made from when they're a fixed list of words (a template
//! language's tags, filters, directives), as data apart from the code reading it: a module
//! ships a file, and replacing how the file is made (written by hand, generated) changes
//! nothing here.
//!
//! One word per line, its fields separated by tabs: `group`, `word`, then optionally what's
//! inserted (`¦` is where the cursor ends, none: the word) and what's shown beside it. `#`
//! starts a comment line.

use crate::{CompletionItem, CompletionKind, fuzzy_score};

/// Where the cursor ends in what's inserted.
const CURSOR: char = '¦';

#[derive(Debug, Clone, PartialEq, Eq)]
struct Word {
    group: &'static str,
    word: &'static str,
    insert: &'static str,
    detail: &'static str,
}

/// A language's words, by group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vocabulary {
    words: Vec<Word>,
}

impl Vocabulary {
    /// The words of `data`; a line without a group and a word is skipped.
    pub fn parse(data: &'static str) -> Self {
        let words = data
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let group = fields.next().filter(|g| !g.is_empty())?;
                let word = fields.next().filter(|w| !w.is_empty())?;
                let insert = fields.next().filter(|i| !i.is_empty()).unwrap_or(word);
                let detail = fields.next().unwrap_or_default();
                Some(Word { group, word, insert, detail })
            })
            .collect();
        Self { words }
    }

    /// How many words it has.
    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The words of `groups` that `query` matches, each as its group's kind, best first.
    pub fn suggest(&self, groups: &[(&str, CompletionKind)], query: &str) -> Vec<CompletionItem> {
        let mut items: Vec<CompletionItem> = self
            .words
            .iter()
            .filter_map(|w| {
                let kind = groups.iter().find(|(g, _)| *g == w.group)?.1;
                let score = fuzzy_score(query, w.word)?;
                Some(Self::item(w, kind, score))
            })
            .collect();
        items.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then(a.label.len().cmp(&b.label.len()))
                .then(a.label.cmp(&b.label))
        });
        items
    }

    fn item(word: &Word, kind: CompletionKind, score: i32) -> CompletionItem {
        let detail = (!word.detail.is_empty()).then_some(word.detail);
        let mut item = CompletionItem::word(word.word, kind, detail);
        item.score = score;
        if let Some((before, after)) = word.insert.split_once(CURSOR) {
            item.insert = format!("{before}{after}");
            item.cursor_back = after.chars().count();
        } else {
            item.insert = word.insert.to_owned();
        }
        item
    }
}

#[cfg(test)]
#[path = "../tests/unit/vocabulary.rs"]
mod tests;
