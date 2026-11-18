//! A data source's query history (plan 39i): every statement a console ran on it, kept with
//! the project (never shared between projects), one history per source.
//!
//! An entry ([`HistoryEntry`]) is the statement as the user wrote it — its secrets masked
//! ([`crate::sql::mask_secrets`]) and its parameters left as written (their names only,
//! never a value: values may be personal data) —, where it ran (the database and schema),
//! when (given by the caller: the domain has no clock), how long it took, the rows it
//! brought or changed and whether it ended well.
//!
//! The rules ([`QueryHistory`]): the same statement run again moves up (one entry, its
//! figures the last run's), at most `limit` entries per source (the oldest leave), a search
//! by the words of the text. The store ([`HistoryStore`]) appends each run at the end of the
//! source's file (written once, never rewritten for a run) and compacts it — rewritten
//! whole, the duplicates and the oldest left out — only once it holds clearly more blocks
//! than the limit ([`needs_compaction`]), or after a deletion.

use std::fmt;
use std::sync::{Mutex, PoisonError};

use crate::DataSourceId;
use crate::console::ConsoleId;
use crate::sql::{self, Flavor, ParamStyles};

/// The most entries a source's history keeps by default (the setting's default).
pub const DEFAULT_LIMIT: usize = 1000;

/// An entry's identity: a UUID v4 (the same statement run again keeps its entry's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HistoryId(u128);

impl HistoryId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }

    pub const fn from_u128(id: u128) -> Self {
        Self(id)
    }

    pub const fn get(self) -> u128 {
        self.0
    }

    /// From its saved text; `None` if it isn't a UUID.
    pub fn parse(text: &str) -> Option<Self> {
        kernel::id::parse_uuid(text).map(Self)
    }
}

impl Default for HistoryId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for HistoryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&kernel::id::uuid_text(self.0))
    }
}

/// How a statement's run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Outcome {
    #[default]
    Ok,
    /// The engine refused it (or the connection failed).
    Failed,
    /// Stopped by the user.
    Cancelled,
}

impl Outcome {
    pub fn id(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Failed => "error",
            Outcome::Cancelled => "cancelled",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "ok" => Outcome::Ok,
            "error" => Outcome::Failed,
            "cancelled" => Outcome::Cancelled,
            _ => return None,
        })
    }
}

/// A statement run, as the history keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub id: HistoryId,
    pub source: DataSourceId,
    /// The console that ran it last (it may be gone since).
    pub console: Option<ConsoleId>,
    pub database: Option<String>,
    pub schema: Option<String>,
    /// The statement as written, its secrets masked, its parameters as written.
    pub text: String,
    /// Its parameters' names, as written (`:id`, `$1`, `?`), never their values.
    pub params: Vec<String>,
    /// When it ran (milliseconds since the Unix epoch, given by the caller).
    pub at_ms: i64,
    /// How long it took.
    pub duration_ms: Option<u64>,
    /// The rows it brought (a query) or changed (a write).
    pub rows: Option<u64>,
    pub outcome: Outcome,
}

impl HistoryEntry {
    /// The entry of `text` (the statement as written in the console, its parameters not
    /// given values) run on `source` at `at_ms`: its secrets masked, its parameters' names
    /// read with `styles`.
    pub fn new(
        source: DataSourceId,
        text: &str,
        flavor: Flavor,
        styles: &ParamStyles,
        at_ms: i64,
    ) -> Self {
        let masked = sql::mask_secrets(text.trim(), flavor).into_owned();
        let tokens = sql::lex(&masked, flavor);
        let params = sql::params(&masked, &tokens, styles)
            .into_iter()
            .filter_map(|p| p.places.first().and_then(|r| masked.get(r.clone())).map(str::to_owned))
            .collect();
        Self {
            id: HistoryId::new(),
            source,
            console: None,
            database: None,
            schema: None,
            text: masked,
            params,
            at_ms,
            duration_ms: None,
            rows: None,
            outcome: Outcome::Ok,
        }
    }

    /// What tells two runs of the same statement apart from two statements: the text with
    /// its spaces collapsed and a final `;` dropped (`SELECT 1;` and `select  1` aren't the
    /// same, `SELECT 1;` and `SELECT 1` are).
    pub fn key(&self) -> String {
        key_of(&self.text)
    }

    /// `database.schema` as the Output's prefix writes it (what's known of them).
    pub fn place(&self) -> Option<String> {
        match (&self.database, &self.schema) {
            (Some(d), Some(s)) => Some(format!("{d}.{s}")),
            (Some(x), None) | (None, Some(x)) => Some(x.clone()),
            (None, None) => None,
        }
    }
}

/// [`HistoryEntry::key`] of `text`.
pub fn key_of(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches(';').trim_end().to_owned()
}

/// Whether `text`'s key is `key` (a [`key_of`]), without making it: its words, the
/// semicolons and spaces at its end left out, are `key`'s.
pub fn has_key(text: &str, key: &str) -> bool {
    let text = text.trim_end().trim_end_matches(';').trim_end();
    text.split_whitespace().eq(key.split(' ').filter(|w| !w.is_empty()))
}

/// A file holding `written` blocks for a history of at most `limit` entries is rewritten:
/// clearly past the limit (a quarter more, at least 16 blocks: not at every run once full).
pub fn needs_compaction(written: usize, limit: usize) -> bool {
    written > limit + (limit / 4).max(16)
}

/// What [`QueryHistory::record`] asks the store to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    /// Append this entry at the end of the file.
    Append(HistoryEntry),
    /// Rewrite the file with every entry kept (oldest first).
    Compact(Vec<HistoryEntry>),
}

/// A source's history: its entries, each statement once, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueryHistory {
    entries: Vec<HistoryEntry>,
    /// The blocks its file holds (duplicates and entries past the limit included).
    written: usize,
}

impl QueryHistory {
    /// The history of a file's blocks (in the file's order, oldest first): a statement
    /// appended again counts as its last block (moved up), the oldest past `limit` leave.
    pub fn from_blocks(blocks: Vec<HistoryEntry>, limit: usize) -> Self {
        let written = blocks.len();
        // From the newest: the first block of each statement is its last run.
        let mut seen = std::collections::HashSet::new();
        let mut entries: Vec<HistoryEntry> =
            blocks.into_iter().rev().filter(|e| seen.insert(e.key())).collect();
        entries.reverse();
        let mut history = Self { entries, written };
        history.trim(limit);
        history
    }

    fn trim(&mut self, limit: usize) {
        let excess = self.entries.len().saturating_sub(limit.max(1));
        self.entries.drain(..excess);
    }

    /// `entry` run: the same statement's entry moves up (it keeps its identity, its figures
    /// are this run's), the oldest past `limit` leave. What the file needs is returned.
    pub fn record(&mut self, mut entry: HistoryEntry, limit: usize) -> Write {
        let key = entry.key();
        // Compared word by word: no key made per kept entry (up to a thousand, per run).
        if let Some(at) = self.entries.iter().position(|e| has_key(&e.text, &key)) {
            entry.id = self.entries.remove(at).id;
        }
        self.entries.push(entry.clone());
        self.trim(limit);
        self.written += 1;
        if needs_compaction(self.written, limit) {
            self.written = self.entries.len();
            Write::Compact(self.entries.clone())
        } else {
            Write::Append(entry)
        }
    }

    /// Entry `id` deleted: the entries to write the file with, or `None` if it had none.
    pub fn remove(&mut self, id: HistoryId) -> Option<Vec<HistoryEntry>> {
        let at = self.entries.iter().position(|e| e.id == id)?;
        self.entries.remove(at);
        self.written = self.entries.len();
        Some(self.entries.clone())
    }

    /// Every entry deleted (the file emptied).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.written = 0;
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The blocks its file holds.
    pub fn written(&self) -> usize {
        self.written
    }

    /// The entries, oldest first.
    pub fn entries(&self) -> &[HistoryEntry] {
        &self.entries
    }

    /// The entries, newest first.
    pub fn newest_first(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter().rev()
    }

    pub fn get(&self, id: HistoryId) -> Option<&HistoryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// The entries whose text has every word of `query` (any case), newest first; an empty
    /// query: all of them.
    pub fn search(&self, query: &str) -> Vec<&HistoryEntry> {
        let search = Search::new(query);
        self.newest_first().filter(|e| search.matches(e)).collect()
    }
}

/// A history's search: the words typed (lowercased); an entry matches when its text has
/// each of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    words: Vec<String>,
}

impl Search {
    pub fn new(query: &str) -> Self {
        Self { words: query.split_whitespace().map(str::to_lowercase).collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn matches(&self, entry: &HistoryEntry) -> bool {
        if self.words.is_empty() {
            return true;
        }
        let text = entry.text.to_lowercase();
        self.words.iter().all(|w| text.contains(w.as_str()))
    }

    /// Where its words are in `text` (byte ranges, in order, not overlapping): what's lit.
    pub fn places(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        let lower = text.to_lowercase();
        // Lowercasing may change a character's length: only then is nothing lit.
        if lower.len() != text.len() {
            return Vec::new();
        }
        let mut out: Vec<std::ops::Range<usize>> = Vec::new();
        for word in self.words.iter().filter(|w| !w.is_empty()) {
            let mut from = 0;
            while let Some(at) = lower[from..].find(word.as_str()) {
                let start = from + at;
                out.push(start..start + word.len());
                from = start + word.len();
            }
        }
        out.sort_by_key(|r| (r.start, r.end));
        let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
        for r in out {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        merged
    }
}

/// A source's file as read: its blocks in order, and the file kept aside when it couldn't
/// all be read (what could be is in `entries`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Loaded {
    pub entries: Vec<HistoryEntry>,
    /// Where a broken file was kept (never written over), when it was.
    pub broken: Option<std::path::PathBuf>,
}

/// Where the histories are kept (the project's files: `storage::TomlQueryHistory`).
pub trait HistoryStore: Send + Sync + fmt::Debug {
    /// Source `source`'s blocks, oldest first (none yet: empty).
    fn load(&self, source: DataSourceId) -> Result<Loaded, String>;
    /// `entry` added at the end of source `source`'s file.
    fn append(&self, source: DataSourceId, entry: &HistoryEntry) -> Result<(), String>;
    /// Source `source`'s file written again with `entries` (oldest first), atomically.
    fn rewrite(&self, source: DataSourceId, entries: &[HistoryEntry]) -> Result<(), String>;
}

/// No history kept (a host without a project).
#[derive(Debug, Default)]
pub struct NoHistory;

impl HistoryStore for NoHistory {
    fn load(&self, _source: DataSourceId) -> Result<Loaded, String> {
        Ok(Loaded::default())
    }

    fn append(&self, _source: DataSourceId, _entry: &HistoryEntry) -> Result<(), String> {
        Ok(())
    }

    fn rewrite(&self, _source: DataSourceId, _entries: &[HistoryEntry]) -> Result<(), String> {
        Ok(())
    }
}

/// A history kept in memory (the tests' and the snapshots'): the blocks of each source as
/// a file would hold them.
#[derive(Debug, Default)]
pub struct MemoryHistory {
    files: Mutex<Vec<(DataSourceId, Vec<HistoryEntry>)>>,
}

impl MemoryHistory {
    /// The blocks source `source`'s file holds.
    pub fn blocks(&self, source: DataSourceId) -> Vec<HistoryEntry> {
        let files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files.iter().find(|(s, _)| *s == source).map(|(_, b)| b.clone()).unwrap_or_default()
    }
}

impl HistoryStore for MemoryHistory {
    fn load(&self, source: DataSourceId) -> Result<Loaded, String> {
        Ok(Loaded { entries: self.blocks(source), broken: None })
    }

    fn append(&self, source: DataSourceId, entry: &HistoryEntry) -> Result<(), String> {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        match files.iter_mut().find(|(s, _)| *s == source) {
            Some((_, blocks)) => blocks.push(entry.clone()),
            None => files.push((source, vec![entry.clone()])),
        }
        Ok(())
    }

    fn rewrite(&self, source: DataSourceId, entries: &[HistoryEntry]) -> Result<(), String> {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files.retain(|(s, _)| *s != source);
        files.push((source, entries.to_vec()));
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/history.rs"]
mod tests;
