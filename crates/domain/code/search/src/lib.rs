//! Code search (like semble): code is split into chunks, and a query finds
//! the matching chunks through two combined methods:
//!
//! - **lexical (BM25)** on terms, with identifiers split (`getUserName` →
//!   `get`, `user`, `name`, plus the whole) — precise for names;
//! - **semantic** on vectors (static embeddings, if the model is available) — also finds
//!   different phrasings ("where is the confirmation email sent").
//!
//! The rankings are combined via Reciprocal Rank Fusion. Vectors come from outside (the
//! [`Embedder`] port); the domain knows nothing about the model.

use std::collections::{HashMap, HashSet};

/// At most how many lines a code chunk has.
pub const CHUNK_LINES: usize = 40;

/// The port to the embeddings model.
pub trait Embedder: Send + Sync {
    /// The text's normalized vector.
    fn embed(&self, text: &str) -> Vec<f32>;
    fn dims(&self) -> usize;
}

/// A code chunk in the index.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// The path relative to the project, with `/`.
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    /// The first lines (for displaying the result).
    pub preview: String,
    /// The terms (ids in the index vocabulary) and their frequency.
    pub terms: Vec<(u32, u16)>,
    pub length: u32,
    /// The semantic vector (empty if the model is not available).
    pub vector: Vec<f32>,
}

/// The project search API, for anyone who needs it: the UI panel, commands, an AI agent
/// exploring the project. The implementation (the index kept up to date in the background)
/// can be cloned and used from any thread; calls are fast (in-memory reads only).
pub trait CodeSearch: Send + Sync {
    fn search(&self, query: &SearchQuery) -> Vec<Hit>;
    /// Code similar to the chunk containing line `line` (0-based) in `path`.
    fn related(&self, path: &str, line: u32, limit: usize) -> Vec<Hit>;
    /// Search can also use the semantic model.
    fn is_semantic(&self) -> bool;
}

/// A search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    /// A name, a few words or a question.
    pub text: String,
    pub limit: usize,
    /// Only files whose paths (relative, with `/`) start with one of the prefixes (empty:
    /// the whole project).
    pub paths: Vec<String>,
    /// Only files with these extensions, without the dot (empty: all).
    pub extensions: Vec<String>,
    pub strategy: Strategy,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            limit,
            paths: Vec::new(),
            extensions: Vec::new(),
            strategy: Strategy::Hybrid,
        }
    }

    fn accepts(&self, path: &str) -> bool {
        (self.paths.is_empty() || self.paths.iter().any(|p| path.starts_with(p.as_str())))
            && (self.extensions.is_empty()
                || path.rsplit_once('.').is_some_and(|(_, ext)| {
                    self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
                }))
    }
}

/// How to search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strategy {
    /// Lexical and semantic (if the model is available), combined.
    #[default]
    Hybrid,
    /// Terms only (precise for names).
    Lexical,
    /// Vectors only (different phrasings); empty without a model.
    Semantic,
}

/// A search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub preview: String,
    pub score: f32,
}

/// A text's chunks: blocks separated by blank lines, merged up to
/// [`CHUNK_LINES`] lines. `(first line, last line, text)`.
pub fn chunks(text: &str) -> Vec<(u32, u32, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        // Skip the leading blank lines.
        while start < lines.len() && lines[start].trim().is_empty() {
            start += 1;
        }
        if start >= lines.len() {
            break;
        }
        let mut end = (start + CHUNK_LINES).min(lines.len());
        // Cut at the last blank line in the second half, so the chunk is a whole block.
        if end < lines.len()
            && let Some(blank) =
                (start + CHUNK_LINES / 2..end).rev().find(|&i| lines[i].trim().is_empty())
        {
            end = blank;
        }
        let body = lines[start..end].join("\n");
        if body.trim().len() > 2 {
            out.push((start as u32, (end - 1) as u32, body));
        }
        start = end;
    }
    out
}

/// A text's terms: the words, with identifiers split on camelCase / snake_case
/// (and also kept whole), lowercase; without one-letter words.
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in text.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
        if word.len() < 2 {
            continue;
        }
        let lower = word.to_lowercase();
        let mut parts = Vec::new();
        let mut current = String::new();
        let chars: Vec<char> = word.chars().collect();
        for (i, &c) in chars.iter().enumerate() {
            let boundary = c == '_'
                || (c.is_uppercase()
                    && i > 0
                    && (chars[i - 1].is_lowercase()
                        || chars.get(i + 1).is_some_and(|n| n.is_lowercase())))
                || (c.is_ascii_digit()
                    != chars.get(i.wrapping_sub(1)).is_some_and(char::is_ascii_digit)
                    && i > 0);
            if boundary && !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            if c != '_' {
                current.extend(c.to_lowercase());
            }
        }
        if !current.is_empty() {
            parts.push(current);
        }
        if parts.len() > 1 {
            out.extend(parts.into_iter().filter(|p| p.len() > 1));
        }
        out.push(lower);
    }
    out
}

/// A file's chunks made ready for the index without it ([`SearchIndex::prepare_file`]): the
/// costly part (splitting, terms, vectors) done while searches go on; adding it
/// ([`SearchIndex::apply`]) only gives its terms their ids.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedFile {
    pub path: String,
    chunks: Vec<PreparedChunk>,
}

#[derive(Debug, Clone, PartialEq)]
struct PreparedChunk {
    start_line: u32,
    end_line: u32,
    preview: String,
    /// The terms and their frequency, in the order first seen.
    terms: Vec<(String, u16)>,
    length: u32,
    vector: Vec<f32>,
}

/// A project's search index.
#[derive(Debug, Default, Clone)]
pub struct SearchIndex {
    pub chunks: Vec<Chunk>,
    pub vocab: HashMap<String, u32>,
    /// How many chunks contain each term (terms no chunk contains are removed).
    pub df: HashMap<u32, u32>,
    total_length: u64,
    /// The next term id (ids are never reused while their term is in `vocab`).
    next_id: u32,
    /// Terms in `vocab` that no chunk contains any more (pruned in batches).
    dead: usize,
}

impl SearchIndex {
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Rebuilds the statistics after loading from the cache.
    pub fn rebuild_stats(&mut self) {
        self.df.clear();
        self.total_length = 0;
        self.next_id = self.vocab.values().max().map_or(0, |id| id + 1);
        for chunk in &self.chunks {
            self.total_length += u64::from(chunk.length);
            for (term, _) in &chunk.terms {
                *self.df.entry(*term).or_default() += 1;
            }
        }
    }

    pub fn remove_file(&mut self, path: &str) {
        self.remove_files(&HashSet::from([path]));
    }

    /// Removes the chunks of all `paths` in one pass over the index (a checkout's hundreds of
    /// files don't each go over it).
    pub fn remove_files(&mut self, paths: &HashSet<&str>) {
        if paths.is_empty() {
            return;
        }
        let mut removed = Vec::new();
        self.chunks.retain_mut(|c| {
            let gone = paths.contains(c.path.as_str());
            if gone {
                removed.push((c.length, std::mem::take(&mut c.terms)));
            }
            !gone
        });
        if removed.is_empty() {
            return;
        }
        for (length, terms) in removed {
            self.total_length -= u64::from(length);
            for (term, _) in terms {
                if let Some(df) = self.df.get_mut(&term) {
                    *df = df.saturating_sub(1);
                    if *df == 0 {
                        self.df.remove(&term);
                        self.dead += 1;
                    }
                }
            }
        }
        self.prune_vocab();
    }

    /// Drops the terms no chunk contains any more, once they are many (long sessions,
    /// branch switches: otherwise the vocabulary only grows).
    fn prune_vocab(&mut self) {
        if self.dead > 4096 && self.dead * 4 > self.vocab.len() {
            let df = &self.df;
            self.vocab.retain(|_, id| df.contains_key(id));
            self.dead = 0;
        }
    }

    /// Replaces a file's chunks (with vectors, if there is a model).
    pub fn set_file(&mut self, path: &str, text: &str, embedder: Option<&dyn Embedder>) {
        self.apply(&[], vec![Self::prepare_file(path, text, embedder)]);
    }

    /// `path`'s chunks, ready to be added (see [`PreparedFile`]); needs no index.
    pub fn prepare_file(path: &str, text: &str, embedder: Option<&dyn Embedder>) -> PreparedFile {
        let chunks = chunks(text)
            .into_iter()
            .map(|(start, end, body)| {
                // The path is part of the text: "controller", "migration" etc. matter.
                let text = format!("{path}\n{body}");
                let words = terms(&text);
                let mut seen: HashMap<&str, usize> = HashMap::new();
                let mut tf: Vec<(String, u16)> = Vec::new();
                for word in &words {
                    match seen.get(word.as_str()) {
                        Some(&i) => tf[i].1 = tf[i].1.saturating_add(1),
                        None => {
                            seen.insert(word, tf.len());
                            tf.push((word.clone(), 1));
                        }
                    }
                }
                PreparedChunk {
                    start_line: start,
                    end_line: end,
                    preview: body.lines().take(6).collect::<Vec<_>>().join("\n"),
                    terms: tf,
                    length: words.len() as u32,
                    vector: embedder.map(|e| e.embed(&text)).unwrap_or_default(),
                }
            })
            .collect();
        PreparedFile { path: path.to_owned(), chunks }
    }

    /// Removes `removed` and the prepared files' old chunks (one pass), then adds theirs.
    pub fn apply(&mut self, removed: &[&str], prepared: Vec<PreparedFile>) {
        let mut paths: HashSet<&str> = removed.iter().copied().collect();
        paths.extend(prepared.iter().map(|f| f.path.as_str()));
        self.remove_files(&paths);
        for file in prepared {
            for chunk in file.chunks {
                let mut terms = Vec::with_capacity(chunk.terms.len());
                for (word, tf) in chunk.terms {
                    let id = match self.vocab.get(&word) {
                        Some(id) => *id,
                        None => {
                            let id = self.next_id;
                            self.next_id += 1;
                            self.vocab.insert(word, id);
                            id
                        }
                    };
                    *self.df.entry(id).or_default() += 1;
                    terms.push((id, tf));
                }
                self.total_length += u64::from(chunk.length);
                self.chunks.push(Chunk {
                    path: file.path.clone(),
                    start_line: chunk.start_line,
                    end_line: chunk.end_line,
                    preview: chunk.preview,
                    terms,
                    length: chunk.length,
                    vector: chunk.vector,
                });
            }
        }
    }

    /// The vectors of all chunks, in chunk order, without changing the index (so it can be
    /// computed while others only read it). A file's chunks are consecutive: one file is
    /// read at a time. `None` if `stopped` said so first (checked between files).
    pub fn embeddings(
        &self,
        embedder: &dyn Embedder,
        read: &dyn Fn(&str) -> Option<String>,
        stopped: &dyn Fn() -> bool,
    ) -> Option<Vec<Vec<f32>>> {
        let mut file: Option<(&str, Vec<String>)> = None;
        let mut vectors = Vec::with_capacity(self.chunks.len());
        for chunk in &self.chunks {
            if file.as_ref().is_none_or(|(path, _)| *path != chunk.path) {
                // Between files: a project closing doesn't wait for the rest.
                if stopped() {
                    return None;
                }
                let lines = read(&chunk.path)
                    .map(|t| t.lines().map(str::to_owned).collect())
                    .unwrap_or_default();
                file = Some((&chunk.path, lines));
            }
            let lines = file.as_ref().map_or(&[][..], |(_, l)| l.as_slice());
            let (start, end) =
                (chunk.start_line as usize, (chunk.end_line as usize + 1).min(lines.len()));
            let body = lines.get(start..end).map(|l| l.join("\n")).unwrap_or_default();
            vectors.push(embedder.embed(&format!("{}\n{body}", chunk.path)));
        }
        Some(vectors)
    }

    /// Sets the vectors computed by [`Self::embeddings`] (in chunk order).
    pub fn set_vectors(&mut self, vectors: Vec<Vec<f32>>) {
        for (chunk, vector) in self.chunks.iter_mut().zip(vectors) {
            chunk.vector = vector;
        }
    }

    /// The best `limit` chunks for the query (BM25 + vectors, via RRF).
    pub fn search(&self, query: &str, query_vector: Option<&[f32]>, limit: usize) -> Vec<Hit> {
        self.query(&SearchQuery::new(query, limit), query_vector)
    }

    /// The full search: filters, strategy; `query_vector` is the query's vector.
    pub fn query(&self, query: &SearchQuery, query_vector: Option<&[f32]>) -> Vec<Hit> {
        if self.chunks.is_empty() {
            return Vec::new();
        }
        let (limit, text) = (query.limit, query.text.as_str());
        let lexical_on = query.strategy != Strategy::Semantic;
        let query_vector = query_vector.filter(|_| query.strategy != Strategy::Lexical);
        let n = self.chunks.len() as f32;
        let average = (self.total_length as f32 / n).max(1.0);
        let query_terms: Vec<u32> = {
            let mut ids: Vec<u32> =
                terms(text).iter().filter_map(|t| self.vocab.get(t).copied()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        // BM25.
        let (k1, b) = (1.2f32, 0.75f32);
        let mut lexical: Vec<(usize, f32)> = self
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| lexical_on && query.accepts(&c.path))
            .filter_map(|(i, chunk)| {
                let mut score = 0.0;
                for &(term, tf) in &chunk.terms {
                    if query_terms.binary_search(&term).is_err() {
                        continue;
                    }
                    let df = *self.df.get(&term).unwrap_or(&1) as f32;
                    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                    let tf = f32::from(tf);
                    score += idf * tf * (k1 + 1.0)
                        / (tf + k1 * (1.0 - b + b * chunk.length as f32 / average));
                }
                (score > 0.0).then_some((i, score))
            })
            .collect();
        top(&mut lexical, FUSED);
        // Semantic: cosine similarity (the vectors are normalized).
        let mut semantic: Vec<(usize, f32)> = match query_vector {
            Some(q) if !q.is_empty() => self
                .chunks
                .iter()
                .enumerate()
                .filter(|(_, c)| c.vector.len() == q.len() && query.accepts(&c.path))
                .map(|(i, c)| (i, c.vector.iter().zip(q).map(|(a, b)| a * b).sum::<f32>()))
                .collect(),
            _ => Vec::new(),
        };
        top(&mut semantic, FUSED);
        // Reciprocal Rank Fusion.
        const K: f32 = 60.0;
        let mut fused: HashMap<usize, f32> = HashMap::new();
        for (rank, (i, _)) in lexical.iter().enumerate() {
            *fused.entry(*i).or_default() += 1.0 / (K + rank as f32 + 1.0);
        }
        for (rank, (i, _)) in semantic.iter().enumerate() {
            *fused.entry(*i).or_default() += 1.0 / (K + rank as f32 + 1.0);
        }
        let mut ranked: Vec<(usize, f32)> = fused.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        ranked
            .into_iter()
            .take(limit)
            .map(|(i, score)| {
                let c = &self.chunks[i];
                Hit {
                    path: c.path.clone(),
                    start_line: c.start_line,
                    end_line: c.end_line,
                    preview: c.preview.clone(),
                    score,
                }
            })
            .collect()
    }

    /// The chunk containing line `line` in `path`.
    pub fn chunk_at(&self, path: &str, line: u32) -> Option<&Chunk> {
        self.chunks.iter().find(|c| c.path == path && (c.start_line..=c.end_line).contains(&line))
    }

    /// Code similar to the chunk at `path:line` (excluding it): by vectors if present,
    /// otherwise by its terms.
    pub fn related(&self, path: &str, line: u32, limit: usize) -> Vec<Hit> {
        let Some(chunk) = self.chunk_at(path, line) else {
            return Vec::new();
        };
        let (start, own) = (chunk.start_line, chunk.path.clone());
        let vector = (!chunk.vector.is_empty()).then(|| chunk.vector.clone());
        let mut query = SearchQuery::new(chunk.preview.clone(), limit + 1);
        if vector.is_some() {
            query.strategy = Strategy::Semantic;
        }
        let mut hits = self.query(&query, vector.as_deref());
        hits.retain(|h| !(h.path == own && h.start_line == start));
        hits.truncate(limit);
        hits
    }
}

/// How many of each ranking go into the fusion.
const FUSED: usize = 200;

/// The best `n` of `ranked` (highest score first; ties by position), without sorting the
/// rest: a query over a large project ranks thousands of chunks.
fn top(ranked: &mut Vec<(usize, f32)>, n: usize) {
    let order = |a: &(usize, f32), b: &(usize, f32)| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0));
    if ranked.len() > n {
        ranked.select_nth_unstable_by(n - 1, order);
        ranked.truncate(n);
    }
    ranked.sort_unstable_by(order);
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
