//! The project's search index (BM25 + vectors), kept alongside the symbol index:
//! same moments (startup, changes on disk), same rule — only files that actually
//! changed are reprocessed. Cached in `<project dir>/index/search.bin`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::UNIX_EPOCH;
use workspace::marks::{PathMarks, ProjectMarks};

use text_search::{Chunk, Embedder, SearchIndex};

/// A file read: its fingerprint (no hash yet) and its bytes.
type FileRead = (Fingerprint, Vec<u8>);

/// The searched text files (code, templates, configs, documentation).
pub const EXTENSIONS: &[&str] = &[
    "php", "phtml", "inc", "twig", "blade", "html", "htm", "js", "mjs", "cjs", "jsx", "ts", "tsx",
    "vue", "svelte", "css", "scss", "sass", "less", "json", "yml", "yaml", "xml", "md", "txt",
    "sql", "sh", "env", "ini", "toml", "py", "rb", "go", "rs", "java", "kt", "cs",
];
const MAX_FILE: u64 = 512 * 1024;

const MAGIC: &[u8; 4] = b"LMNS";
/// The layout of this file; bump it on any change (see `kernel::cache_stamp`).
/// 2: the vocabulary stores (term, id) pairs (ids may have gaps after pruning).
const FORMAT: u32 = 2;

/// A file's fingerprint: size, date, hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fingerprint {
    len: u64,
    modified: u64,
    hash: u64,
}

pub(crate) struct SearchState {
    cache: PathBuf,
    marks: ProjectMarks,
    /// What it reads and writes counts for the project's indexer.
    usage: Arc<crate::Usage>,
    pub index: Arc<RwLock<SearchIndex>>,
    files: HashMap<String, Fingerprint>,
    pub embedder: Option<Arc<dyn Embedder>>,
    /// Changed since it was read from its cache file or last written there.
    unsaved: bool,
}

/// The file is searchable: text extension, outside dependencies and minified files.
/// How many files a build prepares before applying them to the index (their prepared chunks
/// held meanwhile: a few MB).
const BUILD_BATCH: usize = 512;

pub(crate) fn searchable(relative: &str, marks: &PathMarks) -> bool {
    use workspace::marks::PathMark;
    let mark = marks.of(relative);
    if matches!(mark, Some(PathMark::Excluded | PathMark::Library | PathMark::Generated)) {
        return false;
    }
    let lower = relative.to_ascii_lowercase();
    // Dependencies and generated files aren't searched (large, rarely what's sought).
    let skipped = mark.is_none()
        && kernel::paths::skipped_from_search()
            .iter()
            .any(|d| lower.starts_with(&format!("{d}/")) || lower.contains(&format!("/{d}/")));
    let minified =
        lower.ends_with(".min.js") || lower.ends_with(".min.css") || lower.ends_with(".map");
    let ext = lower.rsplit_once('.').map(|(_, e)| e);
    !skipped && !minified && ext.is_some_and(|e| EXTENSIONS.contains(&e))
}

fn fingerprint(path: &Path) -> Option<(Fingerprint, Option<Vec<u8>>)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64);
    Some((Fingerprint { len: meta.len(), modified, hash: 0 }, None))
}

impl SearchState {
    pub fn new(usage: Arc<crate::Usage>, cache: PathBuf, marks: ProjectMarks) -> Self {
        Self {
            usage,
            cache,
            marks,
            index: Arc::default(),
            files: HashMap::new(),
            embedder: None,
            unsaved: false,
        }
    }

    /// The model to use from the next build on.
    pub fn keep_embedder(&mut self, embedder: Arc<dyn Embedder>) {
        self.embedder = Some(embedder);
    }

    /// What's in memory gone (its cache and its model stay; written first by whoever
    /// clears it: the cache is what's read back).
    pub fn clear(&mut self) {
        self.files = HashMap::new();
        self.unsaved = false;
        if let Ok(mut index) = self.index.write() {
            *index = SearchIndex::default();
        }
    }

    fn cache_file(&self) -> PathBuf {
        self.cache.join("search.bin")
    }

    fn dims(&self) -> u32 {
        self.embedder.as_ref().map_or(0, |e| e.dims() as u32)
    }

    /// Build: from the cache (unchanged files) plus the new / changed files.
    /// Returns `true` if anything changed compared to the cache.
    /// Stops early (returning `false`, state unchanged) once `stop` is set.
    pub fn build(
        &mut self,
        root: &Path,
        files: &[PathBuf],
        use_cache: bool,
        stop: &AtomicBool,
    ) -> bool {
        let changed = self.build_from(root, files, use_cache, stop);
        self.unsaved |= changed;
        changed
    }

    fn build_from(
        &mut self,
        root: &Path,
        files: &[PathBuf],
        use_cache: bool,
        stop: &AtomicBool,
    ) -> bool {
        let marks = self.marks.get();
        // Already built (a refresh): only what changed since, applied to the index in memory
        // (no second index beside it, nothing read back from disk).
        if use_cache && !self.files.is_empty() {
            return self.refresh(root, files, stop);
        }
        let cached = if use_cache { load(&self.cache_file(), self.dims()) } else { None };
        let (mut index, cached_files) = cached.unwrap_or_default();
        let mut fresh = HashMap::new();
        let mut changed = false;
        let embedder = self.embedder.clone();
        // Applied in batches: each application goes over the index once (one file at a time,
        // the build grew with the square of the files).
        let mut prepared = Vec::new();
        for path in files {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return false;
            }
            let relative = crate::relative(root, path);
            if !searchable(&relative, &marks) {
                continue;
            }
            let Some((mut print, _)) = fingerprint(path) else { continue };
            let previous = cached_files.get(&relative).copied();
            if previous.is_some_and(|p| p.len == print.len && p.modified == print.modified) {
                fresh.insert(relative, previous.unwrap_or(print));
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else { continue };
            self.usage.read_bytes(bytes.len());
            print.hash = crate::codec::hash(&bytes);
            if previous.is_some_and(|p| p.hash == print.hash) {
                fresh.insert(relative, print);
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            prepared.push(SearchIndex::prepare_file(&relative, &text, embedder.as_deref()));
            if prepared.len() >= BUILD_BATCH {
                index.apply(&[], std::mem::take(&mut prepared));
            }
            fresh.insert(relative, print);
            changed = true;
        }
        // Files deleted since the last session, with the last batch.
        let gone: Vec<&str> =
            cached_files.keys().filter(|p| !fresh.contains_key(*p)).map(String::as_str).collect();
        changed |= !gone.is_empty();
        if !gone.is_empty() || !prepared.is_empty() {
            index.apply(&gone, prepared);
        }
        self.files = fresh;
        if let Ok(mut shared) = self.index.write() {
            *shared = index;
        }
        changed
    }

    /// Reindexes the given files (created, modified or deleted). `true` if anything changed.
    /// One with the size and date already indexed isn't read (told twice: the save, then the
    /// watcher's word on it).
    pub fn update(&mut self, root: &Path, paths: &[PathBuf]) -> bool {
        let marks = self.marks.get();
        let files: Vec<_> = paths
            .iter()
            .map(|path| (crate::relative(root, path), path))
            .filter(|(relative, _)| searchable(relative, &marks))
            .filter_map(|(relative, path)| {
                let Some((print, _)) = fingerprint(path) else { return Some((relative, None)) };
                let known = self.files.get(&relative);
                if known.is_some_and(|p| p.len == print.len && p.modified == print.modified) {
                    return None;
                }
                let content = std::fs::read(path).ok().map(|bytes| (print, bytes));
                Some((relative, content))
            })
            .collect();
        let changed = self.apply(files, &[]);
        self.unsaved |= changed;
        changed
    }

    /// The walk's `files` against what's indexed: the new and changed ones (by size and date)
    /// reindexed, the ones gone removed.
    fn refresh(&mut self, root: &Path, files: &[PathBuf], stop: &AtomicBool) -> bool {
        let marks = self.marks.get();
        let mut seen = std::collections::HashSet::new();
        let mut changed = Vec::new();
        for path in files {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                return false;
            }
            let relative = crate::relative(root, path);
            if !searchable(&relative, &marks) {
                continue;
            }
            let Some((print, _)) = fingerprint(path) else { continue };
            let known = self.files.get(&relative);
            seen.insert(relative.clone());
            if known.is_some_and(|p| p.len == print.len && p.modified == print.modified) {
                continue;
            }
            let content = std::fs::read(path).ok().map(|bytes| (print, bytes));
            changed.push((relative, content));
        }
        let gone: Vec<String> = self.files.keys().filter(|p| !seen.contains(*p)).cloned().collect();
        self.apply(changed, &gone)
    }

    /// Indexes `files` (`None`: deleted) and removes `gone`: the files read and their chunks
    /// prepared (vectors included) before the write lock, then applied in one pass under it —
    /// searches wait only for that. `true` if anything changed.
    fn apply(&mut self, files: Vec<(String, Option<FileRead>)>, gone: &[String]) -> bool {
        let embedder = self.embedder.clone();
        let mut prepared = Vec::new();
        let mut removed: Vec<String> = Vec::new();
        for (relative, content) in files {
            match content {
                Some((mut print, bytes)) => {
                    self.usage.read_bytes(bytes.len());
                    print.hash = crate::codec::hash(&bytes);
                    if self.files.get(&relative).is_some_and(|p| p.hash == print.hash) {
                        // Touched, not changed: its new date remembered.
                        self.files.insert(relative, print);
                        continue;
                    }
                    let text = String::from_utf8_lossy(&bytes);
                    prepared.push(SearchIndex::prepare_file(&relative, &text, embedder.as_deref()));
                    self.files.insert(relative, print);
                }
                None => {
                    if self.files.remove(&relative).is_some() {
                        removed.push(relative);
                    }
                }
            }
        }
        for relative in gone {
            if self.files.remove(relative).is_some() {
                removed.push(relative.clone());
            }
        }
        if prepared.is_empty() && removed.is_empty() {
            return false;
        }
        let removed: Vec<&str> = removed.iter().map(String::as_str).collect();
        let Ok(mut index) = self.index.write() else { return false };
        index.apply(&removed, prepared);
        true
    }

    /// The model became available: vectors for all chunks are computed now. `false` if
    /// `stop` was raised meanwhile (the project closing): nothing is set.
    pub fn set_embedder(
        &mut self,
        root: &Path,
        embedder: Arc<dyn Embedder>,
        stop: &AtomicBool,
    ) -> bool {
        let usage = Arc::clone(&self.usage);
        let read = |relative: &str| {
            let text = std::fs::read_to_string(root.join(relative)).ok();
            usage.read_bytes(text.as_ref().map_or(0, String::len));
            text
        };
        // Computed under the read lock (searches go on meanwhile; this thread is the only
        // writer, so the chunks can't change), then set under a brief write lock.
        let stopped = || stop.load(Ordering::Relaxed);
        let vectors = match self.index.read() {
            Ok(index) => index.embeddings(embedder.as_ref(), &read, &stopped),
            Err(_) => return false,
        };
        let Some(vectors) = vectors else { return false };
        if let Ok(mut index) = self.index.write() {
            index.set_vectors(vectors);
        }
        self.embedder = Some(embedder);
        self.unsaved = true;
        true
    }

    /// Semantic search turned off: the model and the vectors go (about 1 KB per chunk);
    /// they are recomputed if it is turned on again.
    pub fn release_embedder(&mut self) {
        self.embedder = None;
        self.unsaved = true;
        if let Ok(mut index) = self.index.write() {
            for chunk in &mut index.chunks {
                chunk.vector = Vec::new();
            }
        }
    }

    /// Written to its cache file when it changed since it was read or last written (the
    /// whole index, tens of MB: not after an edit that changed only the symbols).
    pub fn save(&mut self) -> std::io::Result<()> {
        if !self.unsaved {
            return Ok(());
        }
        let Ok(index) = self.index.read() else { return Ok(()) };
        let path = self.cache_file();
        let encoded = encode(&index, &self.files, self.dims());
        drop(index);
        self.usage.written_bytes(encoded.len());
        files::write_atomic(&path, &encoded)?;
        self.unsaved = false;
        Ok(())
    }
}

fn encode(index: &SearchIndex, files: &HashMap<String, Fingerprint>, dims: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(index.len() * 512);
    let u32_ = |out: &mut Vec<u8>, v: u32| out.extend_from_slice(&v.to_le_bytes());
    let u64_ = |out: &mut Vec<u8>, v: u64| out.extend_from_slice(&v.to_le_bytes());
    let str_ = |out: &mut Vec<u8>, s: &str| {
        out.extend_from_slice(&(s.len() as u32).to_le_bytes());
        out.extend_from_slice(s.as_bytes());
    };
    out.extend_from_slice(MAGIC);
    u64_(&mut out, kernel::cache_stamp(FORMAT));
    u32_(&mut out, dims);
    u32_(&mut out, index.vocab.len() as u32);
    for (word, id) in &index.vocab {
        str_(&mut out, word);
        u32_(&mut out, *id);
    }
    u32_(&mut out, files.len() as u32);
    for (path, f) in files {
        str_(&mut out, path);
        u64_(&mut out, f.len);
        u64_(&mut out, f.modified);
        u64_(&mut out, f.hash);
    }
    u32_(&mut out, index.chunks.len() as u32);
    for c in &index.chunks {
        str_(&mut out, &c.path);
        u32_(&mut out, c.start_line);
        u32_(&mut out, c.end_line);
        str_(&mut out, &c.preview);
        u32_(&mut out, c.terms.len() as u32);
        for &(term, tf) in &c.terms {
            u32_(&mut out, term);
            out.extend_from_slice(&tf.to_le_bytes());
        }
        u32_(&mut out, c.length);
        u32_(&mut out, c.vector.len() as u32);
        for v in &c.vector {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

/// `None` if the file is missing, invalid or has vectors of another dimension.
fn load(path: &Path, dims: u32) -> Option<(SearchIndex, HashMap<String, Fingerprint>)> {
    let bytes = crate::map_file(path)?;
    let mut r = Reader(&bytes);
    if r.take(4)? != MAGIC || r.u64()? != kernel::cache_stamp(FORMAT) {
        return None;
    }
    let stored_dims = r.u32()?;
    let mut index = SearchIndex::default();
    for _ in 0..r.u32()? {
        let word = r.str()?;
        index.vocab.insert(word, r.u32()?);
    }
    let mut files = HashMap::new();
    for _ in 0..r.u32()? {
        let path = r.str()?;
        files.insert(path, Fingerprint { len: r.u64()?, modified: r.u64()?, hash: r.u64()? });
    }
    for _ in 0..r.u32()? {
        let path = r.str()?;
        let (start_line, end_line) = (r.u32()?, r.u32()?);
        let preview = r.str()?;
        let terms =
            (0..r.u32()?).map(|_| Some((r.u32()?, r.u16()?))).collect::<Option<Vec<_>>>()?;
        let length = r.u32()?;
        let vector = (0..r.u32()?).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        // Vectors from another model: discarded (recomputed when the model is available).
        let vector = if stored_dims == dims { vector } else { Vec::new() };
        index.chunks.push(Chunk { path, start_line, end_line, preview, terms, length, vector });
    }
    index.rebuild_stats();
    Some((index, files))
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u16(&mut self) -> Option<u16> {
        self.take(2)?.try_into().ok().map(u16::from_le_bytes)
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8)?.try_into().ok().map(u64::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        self.take(4)?.try_into().ok().map(f32::from_le_bytes)
    }
    fn str(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
}

#[cfg(test)]
#[path = "../tests/unit/search.rs"]
mod tests;
