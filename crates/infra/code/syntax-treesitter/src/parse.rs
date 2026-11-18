//! Parsing a document into language layers (PHP → HTML → JS / CSS / JSON...).
//!
//! The first parse analyzes the whole document. The following ones are incremental: tree-sitter
//! reports which zones changed structurally, and injections and folding regions are
//! recomputed only there. Untouched layers (along with their descendants) are
//! reused, and combined ones (e.g. the HTML in a PHP file) are reparsed
//! incrementally. So a keystroke in a huge file costs milliseconds, not seconds.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use document::{TextChange, TextSnapshot};
use languages::{Confidence, ContentSniffer, FoldCount, FoldRange, LanguageId};
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

use crate::grammar::{GrammarEntry, Grammars, Queries};
use crate::injection::{
    Injection, InjectionContext, InjectionProvider, InjectionSource, comment_marker,
    normalize_language,
};
use crate::regions::FoldProvider;
use crate::text::SnapshotText;

/// How many levels of embedded languages we track (PHP → HTML → JS → JSON).
const MAX_DEPTH: u8 = 4;
/// How many layers a document can have (a file with thousands of JSON strings must not
/// produce thousands of layers).
const MAX_LAYERS: usize = 256;
/// Maximum length of a string analyzed by content sniffers.
const SNIFF_MAX: usize = 256 * 1024;
/// Minimum confidence for an injection detected from content.
const SNIFF_THRESHOLD: Confidence = Confidence(70);

/// Engine configuration: grammars, injection rules, content sniffers.
#[derive(Debug, Default, Clone)]
pub struct SyntaxConfig {
    pub(crate) grammars: Grammars,
    providers: Vec<Arc<dyn InjectionProvider>>,
    sniffers: Vec<Arc<dyn ContentSniffer>>,
    folders: Vec<Arc<dyn FoldProvider>>,
}

impl SyntaxConfig {
    pub fn new(grammars: Grammars) -> Self {
        Self { grammars, providers: Vec::new(), sniffers: Vec::new(), folders: Vec::new() }
    }

    /// A language's own folding regions, beyond its tree's.
    pub fn add_fold_provider(&mut self, provider: Arc<dyn FoldProvider>) {
        self.folders.push(provider);
    }

    pub fn grammars(&self) -> &Grammars {
        &self.grammars
    }

    pub fn add_provider(&mut self, provider: Arc<dyn InjectionProvider>) {
        self.providers.push(provider);
    }

    /// A language's doc comments read for their parts (colored as code).
    pub fn add_doc_comments(&mut self, reader: Arc<dyn crate::DocComments>) {
        self.grammars.add_doc_comments(reader);
    }

    pub fn add_sniffer(&mut self, sniffer: Arc<dyn ContentSniffer>) {
        self.sniffers.push(sniffer);
    }

    /// The scope name of a highlight capture: (language, capture).
    pub fn scope(&self, key: ScopeKey) -> Option<(&LanguageId, String)> {
        let entry = self.grammars.by_index(key.grammar)?;
        if let Some(scope) = crate::doc::scope(key.capture) {
            return Some((&entry.grammar.language, scope.to_owned()));
        }
        let queries = entry.queries();
        let name = queries.highlights.as_ref()?.capture_names().get(key.capture as usize)?;
        Some((&entry.grammar.language, (*name).to_owned()))
    }
}

/// Identifies a highlight capture in a grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeKey {
    pub(crate) grammar: u16,
    pub(crate) capture: u16,
}

/// A layer: a tree for one language, over the whole document or over ranges.
#[derive(Debug, Clone)]
pub struct Layer {
    pub(crate) language: LanguageId,
    pub(crate) grammar: u16,
    /// Its grammar's queries (held: they stay compiled while the layer lives).
    pub(crate) queries: Arc<Queries>,
    pub(crate) tree: Tree,
    /// Empty for the root layer (the whole document).
    pub(crate) ranges: Vec<Range<usize>>,
    pub(crate) depth: u8,
    pub(crate) parent: Option<usize>,
    /// The fragments are parsed together (e.g. the HTML between PHP blocks).
    pub(crate) combined: bool,
    /// Its errors are reported (see [`crate::Grammar::embedded_errors`]).
    pub(crate) report_errors: bool,
    pub(crate) folds: Vec<FoldRange>,
}

impl Layer {
    pub(crate) fn contains(&self, offset: usize) -> bool {
        self.ranges.is_empty() || self.ranges.iter().any(|r| r.start <= offset && offset <= r.end)
    }

    pub(crate) fn intersects(&self, range: &Range<usize>) -> bool {
        self.ranges.is_empty() || self.ranges.iter().any(|r| overlaps(r, range))
    }
}

/// A syntax error: its zone and kind (the UI builds the message, in its own language).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub range: Range<usize>,
    pub kind: SyntaxErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    /// A missing element (e.g. `;`, `)`), with its name from the grammar.
    Missing(String),
    /// Code that doesn't match the grammar (the text is in `range`).
    Unexpected,
}

/// At most how many errors are reported (a broken file doesn't flood the UI).
const MAX_ERRORS: usize = 200;

/// The result of parsing a document at a given version.
#[derive(Debug, Clone)]
pub struct SyntaxSnapshot {
    pub(crate) version: u64,
    pub(crate) layers: Vec<Layer>,
    pub(crate) folds: Vec<FoldRange>,
}

impl SyntaxSnapshot {
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn folds(&self) -> &[FoldRange] {
        &self.folds
    }

    /// Syntax errors (`ERROR` and `MISSING` nodes), in position order. Only branches
    /// containing errors (`has_error`) are searched, so it's fast even on large files.
    ///
    /// The main layer and the "real" embedded layers are reported (CSS from
    /// `<style>`, JS from `<script>`); the combined HTML between PHP blocks and the JSON
    /// guessed from strings would raise false alarms.
    pub fn errors(&self) -> Vec<SyntaxError> {
        self.collect_errors(true)
    }

    /// The errors [`Self::errors`] leaves out: those of the layers whose grammar isn't
    /// trusted to report them (a permissive SQL grammar's, a fragment's edges), for a
    /// document that asks to see them anyway, faint.
    pub fn quiet_errors(&self) -> Vec<SyntaxError> {
        self.collect_errors(false)
    }

    /// The errors of the layers reporting theirs (`reported`), or of the others.
    fn collect_errors(&self, reported: bool) -> Vec<SyntaxError> {
        let mut out = Vec::new();
        for layer in &self.layers {
            if layer.report_errors != reported || !layer.tree.root_node().has_error() {
                continue;
            }
            // Where the host's code was cut out (PHP printed into a script:
            // `var x = <?= $y ?>;`), the fragments alone don't make sense: not an error.
            // Sorted; none for a layer of one piece (its neighbours never looked for).
            let cuts: Vec<usize> = cuts(&layer.ranges).collect();
            let mut cursor = layer.tree.walk();
            'walk: loop {
                let node = cursor.node();
                let report = node.is_error() || node.is_missing();
                let at_cut = report && !cuts.is_empty() && {
                    // Only for what's reported: finding a node's neighbours walks its parent's
                    // children from the first (tree-sitter), too dear for every node visited.
                    let range = between_neighbours(node);
                    let first = cuts.partition_point(|c| *c < range.start);
                    cuts.get(first).is_some_and(|c| range.contains(c))
                };
                if report && !at_cut && out.len() < MAX_ERRORS {
                    let kind = if node.is_missing() {
                        SyntaxErrorKind::Missing(node.kind().to_owned())
                    } else {
                        SyntaxErrorKind::Unexpected
                    };
                    out.push(SyntaxError { range: node.byte_range(), kind });
                }
                if !report && node.has_error() && cursor.goto_first_child() {
                    continue;
                }
                loop {
                    if cursor.goto_next_sibling() {
                        continue 'walk;
                    }
                    if !cursor.goto_parent() {
                        break 'walk;
                    }
                }
            }
        }
        out.sort_by_key(|e| e.range.start);
        out.dedup_by(|a, b| a.range == b.range);
        out
    }

    pub fn layer_count(&self) -> usize {
        self.layers.len()
    }

    /// Description of the layers (language, depth, number of fragments), for diagnostics.
    pub fn describe_layers(&self) -> Vec<(String, u8, usize, bool)> {
        self.layers
            .iter()
            .map(|l| (l.language.to_string(), l.depth, l.ranges.len(), l.combined))
            .collect()
    }

    /// The language at `offset`: the deepest layer that contains it.
    pub fn language_at(&self, offset: usize) -> &LanguageId {
        self.layers
            .iter()
            .filter(|l| l.contains(offset))
            .max_by_key(|l| l.depth)
            .map_or(&self.layers[0].language, |l| &l.language)
    }

    /// The ranges around `range` ("Extend Selection"), the smallest first: the nodes of
    /// the deepest layer holding it (an embedded script's), then its parent layers' (the
    /// page's), up to the whole document.
    pub fn selection_ranges(&self, range: Range<usize>) -> Vec<Range<usize>> {
        let deepest = self
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.contains(range.start) && l.contains(range.end))
            .max_by_key(|(_, l)| l.depth)
            .map(|(i, _)| i);
        let mut out: Vec<Range<usize>> = Vec::new();
        let mut at = deepest;
        while let Some(i) = at {
            let layer = &self.layers[i];
            for r in crate::selection::selection_ranges(&layer.tree, range.clone()) {
                let grows =
                    out.last().is_none_or(|l| r.start <= l.start && l.end <= r.end && r != *l);
                if grows {
                    out.push(r);
                }
            }
            at = layer.parent;
        }
        out
    }

    /// Adjusts trees and positions after not-yet-parsed changes (while the new parse
    /// runs in the background), so highlighting stays aligned with the text.
    pub fn apply_changes(&mut self, changes: &[TextChange], version: u64) {
        // Many changes (a multi-cursor edit): the trees edited by each, the ranges and folds
        // placed among them at once (not moved by each change in turn).
        if changes.len() > MANY_CHANGES {
            let map = document::ChangeMap::new(changes);
            for layer in &mut self.layers {
                for change in changes {
                    layer.tree.edit(&input_edit(change));
                }
                for r in &mut layer.ranges {
                    let start = map.map(r.start).0;
                    *r = start..map.map(r.end).0.max(start);
                }
                map_folds(&mut layer.folds, &map);
            }
            map_folds(&mut self.folds, &map);
            self.version = version;
            return;
        }
        for change in changes {
            let edit = input_edit(change);
            for layer in &mut self.layers {
                layer.tree.edit(&edit);
                for r in &mut layer.ranges {
                    let start = change.map(r.start);
                    *r = start..change.map(r.end).max(start);
                }
                shift_folds(&mut layer.folds, change);
            }
            shift_folds(&mut self.folds, change);
        }
        self.version = version;
    }
}

/// More changes than this are mapped with a [`document::ChangeMap`].
const MANY_CHANGES: usize = 16;

/// [`shift_folds`] for all of a map's changes at once.
fn map_folds(folds: &mut [FoldRange], map: &document::ChangeMap) {
    for fold in folds {
        let (start, before_start) = map.map(fold.start_byte);
        let (end, before_end) = map.map(fold.end_byte);
        fold.start_line = (i64::from(fold.start_line) + before_start).max(0) as u32;
        fold.end_line =
            (i64::from(fold.end_line) + before_end).max(i64::from(fold.start_line)) as u32;
        fold.start_byte = start;
        fold.end_byte = end.max(start);
    }
}

fn shift_folds(folds: &mut [FoldRange], change: &TextChange) {
    let lines = change.new_end_point.line.0 as i64 - change.old_end_point.line.0 as i64;
    for fold in folds {
        if fold.start_byte >= change.old_end {
            fold.start_line = (fold.start_line as i64 + lines).max(0) as u32;
            fold.end_line = (fold.end_line as i64 + lines).max(0) as u32;
        } else if fold.end_byte >= change.old_end {
            fold.end_line = (fold.end_line as i64 + lines).max(fold.start_line as i64) as u32;
        }
        fold.start_byte = change.map(fold.start_byte);
        fold.end_byte = change.map(fold.end_byte).max(fold.start_byte);
    }
}

pub(crate) fn input_edit(change: &TextChange) -> InputEdit {
    let point = |p: kernel::TextPosition| Point::new(p.line.0 as usize, p.column.0 as usize);
    InputEdit {
        start_byte: change.start,
        old_end_byte: change.old_end,
        new_end_byte: change.new_end,
        start_position: point(change.start_point),
        old_end_position: point(change.old_end_point),
        new_end_position: point(change.new_end_point),
    }
}

/// Parses the document. With `previous` (the previous snapshot, already updated with
/// [`SyntaxSnapshot::apply_changes`]) and `changes` (the applied changes), parsing is
/// incremental.
pub fn parse_document(
    config: &SyntaxConfig,
    language: &LanguageId,
    text: &TextSnapshot,
    previous: Option<&SyntaxSnapshot>,
    changes: &[TextChange],
    version: u64,
) -> Option<SyntaxSnapshot> {
    let started = std::time::Instant::now();
    let entry = config.grammars.get(language)?;
    let previous = previous.filter(|p| p.layers.first().is_some_and(|l| &l.language == language));
    let mut parser = Parser::new();
    parser.set_language(&entry.grammar.ts).ok()?;
    let root = parse_text(&mut parser, text, previous.map(|p| &p.layers[0].tree))?;

    // Affected zones: structural changes reported by tree-sitter + the edited text.
    let changed = previous.map(|p| changed_ranges(&p.layers[0].tree, &root, changes, text));
    let scope = changed.as_deref();
    let root_time = started.elapsed();

    let mut builder = Builder { config, text, parser, previous, scope, layers: Vec::new() };
    let root_layer = Layer {
        language: language.clone(),
        grammar: entry.index,
        queries: entry.queries(),
        tree: root,
        ranges: Vec::new(),
        depth: 0,
        parent: None,
        combined: false,
        report_errors: entry.grammar.own_errors,
        folds: Vec::new(),
    };
    builder.layers.push(root_layer);
    builder.build(0, previous.map(|_| 0));

    let mut layers = builder.layers;
    // The language's own regions (a `.env` file's sections) beside its tree's.
    let own: Vec<FoldRange> = config
        .folders
        .iter()
        .filter(|f| f.language() == *language)
        .flat_map(|f| f.folds(&text.slice(0..text.len())))
        .collect();
    let folds = merge_folds(layers.iter().flat_map(|l| &l.folds).chain(&own));
    for layer in &mut layers {
        layer.folds.shrink_to_fit();
    }
    tracing::debug!(
        %language,
        layers = layers.len(),
        root = ?root_time,
        total = ?started.elapsed(),
        incremental = previous.is_some(),
        changed = scope.map_or(0, <[_]>::len),
        "faze parsare"
    );
    Some(SyntaxSnapshot { version, layers, folds })
}

struct Builder<'a> {
    config: &'a SyntaxConfig,
    text: &'a TextSnapshot,
    parser: Parser,
    previous: Option<&'a SyntaxSnapshot>,
    /// The changed zones; `None` = full parse.
    scope: Option<&'a [Range<usize>]>,
    layers: Vec<Layer>,
}

impl Builder<'_> {
    /// Builds the layers starting from `index` (already parsed). `prev` is the
    /// corresponding layer from the previous snapshot, if any.
    fn build(&mut self, index: usize, prev: Option<usize>) {
        let mut queue = vec![(index, prev)];
        while let Some((index, prev)) = queue.pop() {
            // A "fresh" layer (with no counterpart) is analyzed in full.
            let scope = if prev.is_some() { self.scope } else { None };
            let Some(host) =
                self.config.grammars.by_index(self.layers[index].grammar).map(Arc::clone)
            else {
                continue;
            };
            self.layers[index].folds = compute_folds(
                &self.layers[index],
                self.text,
                scope,
                prev.and_then(|p| self.previous.map(|s| s.layers[p].folds.as_slice())),
            );
            if self.layers[index].depth >= MAX_DEPTH {
                continue;
            }

            let found = find_injections(self.config, &host, &self.layers[index], self.text, scope);
            let mut combined: HashMap<LanguageId, Vec<Range<usize>>> = HashMap::new();
            let mut separate = Vec::new();
            for injection in found {
                if injection.ranges.len() > 1 || self.is_combined_language(index, &injection) {
                    combined.entry(injection.language).or_default().extend(injection.ranges);
                } else {
                    separate.push(injection);
                }
            }

            // Previous children: untouched ones are kept (with descendants), combined ones are
            // rebuilt from the untouched old fragments + the new fragments.
            let mut combined_prev: HashMap<LanguageId, usize> = HashMap::new();
            if let (Some(prev), Some(previous)) = (prev, self.previous) {
                for (child_index, child) in
                    previous.layers.iter().enumerate().filter(|(_, l)| l.parent == Some(prev))
                {
                    // A layer whose text was deleted completely (empty ranges) disappears.
                    if child.ranges.iter().all(|r| r.is_empty()) {
                        continue;
                    }
                    let touched = scope.is_some_and(|s| s.iter().any(|r| child.intersects(r)));
                    if child.combined {
                        let kept = child
                            .ranges
                            .iter()
                            .filter(|r| !scope.is_some_and(|s| s.iter().any(|c| overlaps(r, c))));
                        combined.entry(child.language.clone()).or_default().extend(kept.cloned());
                        combined_prev.insert(child.language.clone(), child_index);
                    } else if !touched
                        && !separate.iter().any(|i| i.ranges.iter().any(|r| child.intersects(r)))
                    {
                        self.copy_subtree(previous, child_index, index);
                    }
                }
            }

            for (language, ranges) in combined {
                if self.layers.len() >= MAX_LAYERS {
                    break;
                }
                let old = combined_prev.get(&language).copied();
                let old_tree = old.and_then(|i| self.previous.map(|p| &p.layers[i].tree));
                if let Some(child) = self.parse_child(index, language, ranges, true, old_tree) {
                    queue.push((child, old));
                }
            }
            for injection in separate {
                if self.layers.len() >= MAX_LAYERS {
                    break;
                }
                if let Some(child) =
                    self.parse_child(index, injection.language, injection.ranges, false, None)
                {
                    queue.push((child, None));
                }
            }
        }
    }

    /// Languages injected combined by the host (e.g. `text` → HTML in PHP) stay combined
    /// even when the changed zone contains a single fragment.
    fn is_combined_language(&self, host: usize, injection: &Injection) -> bool {
        injection.source == InjectionSource::Declarative
            && self.layers[host].queries.combined.contains(&injection.language)
    }

    fn parse_child(
        &mut self,
        parent: usize,
        language: LanguageId,
        mut ranges: Vec<Range<usize>>,
        combined: bool,
        old_tree: Option<&Tree>,
    ) -> Option<usize> {
        let entry = self.config.grammars.get(&language)?;
        ranges.sort_by_key(|r| r.start);
        ranges.dedup();
        // Never past its host's own text: a node of a combined host (HTML between PHP
        // blocks) spans the blocks it skipped — a `<script>` around PHP would hand the
        // script the PHP too.
        let ranges = clip(ranges, &self.layers[parent].ranges);
        let ts = ts_ranges(self.text, &ranges);
        if ts.is_empty() {
            return None;
        }
        self.parser.set_language(&entry.grammar.ts).ok()?;
        self.parser.set_included_ranges(&ts).ok()?;
        let tree = parse_text(&mut self.parser, self.text, old_tree);
        let _ = self.parser.set_included_ranges(&[]);
        self.layers.push(Layer {
            language,
            grammar: entry.index,
            queries: entry.queries(),
            tree: tree?,
            ranges: ts.iter().map(|r| r.start_byte..r.end_byte).collect(),
            depth: self.layers[parent].depth + 1,
            parent: Some(parent),
            combined,
            report_errors: !combined
                && entry.grammar.embedded_errors
                && self
                    .config
                    .grammars
                    .get(&self.layers[parent].language)
                    .is_none_or(|host| host.grammar.injected_errors),
            folds: Vec::new(),
        });
        Some(self.layers.len() - 1)
    }

    /// Copies an untouched layer and all its descendants from the previous snapshot.
    fn copy_subtree(&mut self, previous: &SyntaxSnapshot, from: usize, parent: usize) {
        if self.layers.len() >= MAX_LAYERS {
            return;
        }
        let mut layer = previous.layers[from].clone();
        layer.parent = Some(parent);
        self.layers.push(layer);
        let new_index = self.layers.len() - 1;
        for (child, _) in previous.layers.iter().enumerate().filter(|(_, l)| l.parent == Some(from))
        {
            self.copy_subtree(previous, child, new_index);
        }
    }
}

/// `ranges` (sorted) cut to `within` (sorted; empty: the whole text).
fn clip(ranges: Vec<Range<usize>>, within: &[Range<usize>]) -> Vec<Range<usize>> {
    if within.is_empty() {
        return ranges;
    }
    let mut out = Vec::with_capacity(ranges.len());
    for r in ranges {
        let first = within.partition_point(|w| w.end <= r.start);
        for w in within[first..].iter().take_while(|w| w.start < r.end) {
            let (start, end) = (r.start.max(w.start), r.end.min(w.end));
            if start < end {
                out.push(start..end);
            }
        }
    }
    out
}

/// Where a layer made of fragments has its text cut (the ends and starts between them).
fn cuts(ranges: &[Range<usize>]) -> impl Iterator<Item = usize> + '_ {
    ranges.windows(2).filter(|w| w[0].end < w[1].start).flat_map(|w| [w[0].end, w[1].start])
}

/// From the end of what's before `node` to the start of what's after it (its own range
/// when it has no neighbours): a cut there is where it stands.
fn between_neighbours(node: tree_sitter::Node<'_>) -> Range<usize> {
    let mut start = None;
    let mut end = None;
    let mut at = Some(node);
    while let Some(n) = at {
        if start.is_none() {
            start = n.prev_sibling().map(|s| s.end_byte());
        }
        if end.is_none() {
            end = n.next_sibling().map(|s| s.start_byte());
        }
        if start.is_some() && end.is_some() {
            break;
        }
        at = n.parent();
    }
    let start = start.unwrap_or(node.start_byte()).min(node.start_byte());
    let end = end.unwrap_or(node.end_byte()).max(node.end_byte());
    start..end + 1
}

fn parse_text(parser: &mut Parser, text: &TextSnapshot, old: Option<&Tree>) -> Option<Tree> {
    parser.parse_with_options(&mut |byte, _| text.chunk_from(byte), old, None)
}

/// Intersection of half-open ranges, as in `QueryCursor::set_byte_range`: two
/// ranges that only touch don't overlap (the changed zones are whole lines anyway).
fn overlaps(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// The affected zones, extended to whole lines and merged.
fn changed_ranges(
    old: &Tree,
    new: &Tree,
    changes: &[TextChange],
    text: &TextSnapshot,
) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> =
        old.changed_ranges(new).map(|r| r.start_byte..r.end_byte).collect();
    // The edited zones, brought into final coordinates (the changes are sequential): a
    // change is followed only by those of its run (a transaction's, after it) that touch
    // it, then placed among the later runs at once — not mapped by every later change
    // (quadratic with thousands of cursors).
    let mut run_starts = vec![0];
    for (i, pair) in changes.windows(2).enumerate() {
        if pair[1].start < pair[0].new_end {
            run_starts.push(i + 1);
        }
    }
    for (r, &from) in run_starts.iter().enumerate() {
        let to = run_starts.get(r + 1).copied().unwrap_or(changes.len());
        let later_runs = document::ChangeMap::new(&changes[to..]);
        for (i, change) in changes.iter().enumerate().take(to).skip(from) {
            let (mut start, mut end) = (change.start, change.new_end);
            for later in &changes[i + 1..to] {
                if end < later.start {
                    break;
                }
                start = later.map(start);
                end = later.map(end).max(start);
            }
            let start = later_runs.map(start).0;
            ranges.push(start..later_runs.map(end).0.max(start));
        }
    }
    let len = text.len();
    let mut lines: Vec<Range<usize>> = ranges
        .into_iter()
        .map(|r| {
            let start = text.line_start(text.line_of(r.start.min(len)));
            let end_line = text.line_of(r.end.min(len));
            let end = text.line_start(end_line + 1).max(r.end).min(len);
            start..end
        })
        .collect();
    lines.sort_by_key(|r| r.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(lines.len());
    for r in lines {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

fn ts_ranges(text: &TextSnapshot, ranges: &[Range<usize>]) -> Vec<tree_sitter::Range> {
    let point = |offset| {
        let p = text.position(offset);
        Point::new(p.line.0 as usize, p.column.0 as usize)
    };
    let mut out: Vec<tree_sitter::Range> = Vec::with_capacity(ranges.len());
    for r in ranges.iter().filter(|r| r.start < r.end) {
        if out.last().is_some_and(|last| r.start < last.end_byte) {
            continue;
        }
        out.push(tree_sitter::Range {
            start_byte: r.start,
            end_byte: r.end,
            start_point: point(r.start),
            end_point: point(r.end),
        });
    }
    out
}

/// The injections in a layer (only in `scope`, if given), from all sources, without
/// overlaps (the highest-priority source wins).
/// The conditions evaluated here, beyond those the query engine does itself.
pub(crate) const EVALUATED: &[&str] = &["has-ancestor?", "offset!"];

/// Whether `found` meets its `#has-ancestor? @capture "kind" …` (the query engine leaves
/// such predicates to whoever asks: unchecked, the pattern would match anywhere).
fn has_its_ancestors(query: &tree_sitter::Query, found: &tree_sitter::QueryMatch<'_, '_>) -> bool {
    use tree_sitter::QueryPredicateArg::{Capture, String as Text};
    query.general_predicates(found.pattern_index).iter().all(|predicate| {
        if &*predicate.operator != "has-ancestor?" {
            return true;
        }
        let Some(Capture(index)) = predicate.args.first() else { return true };
        let kinds: Vec<&str> = predicate.args[1..]
            .iter()
            .filter_map(|arg| if let Text(kind) = arg { Some(&**kind) } else { None })
            .collect();
        found.captures().iter().filter(|c| c.index == *index).all(|c| {
            std::iter::successors(c.node.parent(), tree_sitter::Node::parent)
                .any(|node| kinds.contains(&node.kind()))
        })
    })
}

/// `range` of capture `index` after its `#offset! @capture _ start _ end` (columns, on the
/// capture's own lines: a template literal without its backticks).
fn narrowed(query: &Query, pattern: usize, index: u32, range: Range<usize>) -> Range<usize> {
    use tree_sitter::QueryPredicateArg::{Capture, String as Text};
    let column = |arg: Option<&tree_sitter::QueryPredicateArg>| match arg {
        Some(Text(number)) => number.parse::<isize>().unwrap_or(0),
        _ => 0,
    };
    let offset = query.general_predicates(pattern).iter().find(|p| {
        &*p.operator == "offset!" && matches!(p.args.first(), Some(Capture(c)) if *c == index)
    });
    let Some(offset) = offset else { return range };
    let start = range.start.saturating_add_signed(column(offset.args.get(2)));
    let end = range.end.saturating_add_signed(column(offset.args.get(4)));
    if start <= end { start..end } else { range }
}

fn find_injections(
    config: &SyntaxConfig,
    host: &GrammarEntry,
    layer: &Layer,
    text: &TextSnapshot,
    scope: Option<&[Range<usize>]>,
) -> Vec<Injection> {
    let mut found = Vec::new();
    let root = layer.tree.root_node();
    let whole = [root.byte_range()];
    let ranges = scope.unwrap_or(&whole);

    // 1. Declarative: injections.scm.
    if let Some(query) = &layer.queries.injections {
        let names = query.capture_names();
        let content = names.iter().position(|n| *n == "injection.content");
        let language_capture = names.iter().position(|n| *n == "injection.language");
        let mut combined: HashMap<LanguageId, Vec<Range<usize>>> = HashMap::new();
        let mut cursor = QueryCursor::new();
        for range in ranges {
            cursor.set_byte_range(range.clone());
            let mut matches = cursor.matches(query, root, SnapshotText(text));
            while let Some(m) = matches.next() {
                if !has_its_ancestors(query, m) {
                    continue;
                }
                let mut language = None;
                let mut is_combined = false;
                for prop in query.property_settings(m.pattern_index) {
                    match (&*prop.key, prop.value.as_deref()) {
                        ("injection.language", Some(v)) => language = Some(normalize_language(v)),
                        ("injection.combined", _) => is_combined = true,
                        _ => {}
                    }
                }
                if let Some(i) = language_capture
                    && let Some(c) = m.captures().iter().find(|c| c.index as usize == i)
                {
                    language = Some(normalize_language(&text.slice(c.node.byte_range())));
                }
                let (Some(language), Some(content)) = (language, content) else { continue };
                for c in m.captures().iter().filter(|c| c.index as usize == content) {
                    let range = narrowed(query, m.pattern_index, c.index, c.node.byte_range());
                    if is_combined {
                        combined.entry(language.clone()).or_default().push(range);
                    } else {
                        found.push(Injection {
                            ranges: vec![range],
                            language: language.clone(),
                            source: InjectionSource::Declarative,
                        });
                    }
                }
            }
        }
        found.extend(combined.into_iter().map(|(language, mut ranges)| {
            ranges.sort_by_key(|r| r.start);
            ranges.dedup();
            Injection { ranges, language, source: InjectionSource::Declarative }
        }));
    }

    // 2. Rules from modules.
    let ctx = InjectionContext {
        host: &layer.language,
        language: &host.grammar.ts,
        root,
        text,
        changed: scope,
    };
    for provider in &config.providers {
        if provider.hosts().is_empty() || provider.hosts().contains(&layer.language) {
            provider.detect(&ctx, &mut found);
        }
    }

    // 3. Markers in comments and 4. content sniffers, in a single pass.
    scan_strings(config, host, root, text, scope, &mut found);

    // No overlaps and no duplicates; languages without a grammar are ignored.
    found.retain(|i| config.grammars.contains(&i.language) && i.language != layer.language);
    found.sort_by_key(|i| std::cmp::Reverse(i.source));
    let mut accepted: Vec<Injection> = Vec::new();
    for injection in found {
        let clash = accepted.iter().any(|a| {
            a.ranges
                .iter()
                .any(|x| injection.ranges.iter().any(|y| x.start < y.end && y.start < x.end))
        });
        if !clash {
            accepted.push(injection);
        }
    }
    accepted
}

/// Walks the tree once: `language=...` markers in comments (applied to the next
/// string) and strings that look like another language. Subtrees outside `scope` are skipped.
fn scan_strings(
    config: &SyntaxConfig,
    host: &GrammarEntry,
    root: Node<'_>,
    text: &TextSnapshot,
    scope: Option<&[Range<usize>]>,
    out: &mut Vec<Injection>,
) {
    let content_kinds = host.grammar.string_content_nodes;
    if content_kinds.is_empty() {
        return;
    }
    let sniffers: Vec<&Arc<dyn ContentSniffer>> =
        config.sniffers.iter().filter(|s| config.grammars.contains(&s.language())).collect();
    let in_scope = |r: &Range<usize>| scope.is_none_or(|s| s.iter().any(|c| overlaps(r, c)));
    let mut pending_marker: Option<(LanguageId, usize)> = None;
    let mut cursor = root.walk();
    'walk: loop {
        let node = cursor.node();
        let range = node.byte_range();
        let visible = in_scope(&range);
        if visible && node.kind().contains("comment") {
            if range.len() < 1024
                && let Some(name) = comment_marker(&text.slice(range.clone()))
            {
                pending_marker = Some((normalize_language(&name), node.end_position().row));
            }
        } else if visible && content_kinds.contains(&node.kind()) {
            // The marker applies to the first string on the same line or on the next one.
            match pending_marker.take() {
                Some((language, row)) if node.start_position().row <= row + 1 => {
                    out.push(Injection {
                        ranges: vec![range],
                        language,
                        source: InjectionSource::CommentMarker,
                    });
                }
                _ if !sniffers.is_empty() && range.len() >= 2 && range.len() <= SNIFF_MAX => {
                    // The quick filter on the first visible character avoids copying the text.
                    let first = text
                        .chunks_in(range.clone())
                        .flat_map(str::chars)
                        .find(|c| !c.is_whitespace());
                    if let Some(first) =
                        first.filter(|c| sniffers.iter().any(|s| s.may_start_with(*c)))
                    {
                        let content = text.slice(range.clone());
                        let best = sniffers
                            .iter()
                            .filter(|s| s.may_start_with(first))
                            .filter_map(|s| s.sniff(&content).map(|c| (c, s.language())))
                            .filter(|(c, _)| *c >= SNIFF_THRESHOLD)
                            .max_by_key(|(c, _)| *c);
                        if let Some((_, language)) = best {
                            out.push(Injection {
                                ranges: vec![range],
                                language,
                                source: InjectionSource::Sniffed,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        // Descend only if the subtree touches the zones of interest.
        if visible && cursor.goto_first_child() {
            continue;
        }
        loop {
            if cursor.goto_next_sibling() {
                continue 'walk;
            }
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
}

/// A layer's folding regions. Incremental: old regions outside the changed
/// zones are kept, and the changed zones are queried again.
fn compute_folds(
    layer: &Layer,
    text: &TextSnapshot,
    scope: Option<&[Range<usize>]>,
    previous: Option<&[FoldRange]>,
) -> Vec<FoldRange> {
    let mut folds: Vec<FoldRange> = match (scope, previous) {
        // The changed zones are sorted and apart: the first one ending after a region's start
        // is the only one it may overlap.
        (Some(scope), Some(prev)) => prev
            .iter()
            .filter(|f| {
                let next = scope.partition_point(|c| c.end <= f.start_byte);
                !scope.get(next).is_some_and(|c| overlaps(&(f.start_byte..f.end_byte), c))
            })
            .cloned()
            .collect(),
        _ => Vec::new(),
    };
    let Some(query) = &layer.queries.folds else { return folds };
    let names = query.capture_names();
    let root = layer.tree.root_node();
    if names.contains(&"fold.pair") {
        paired_folds(query, root, text, &mut folds);
    }
    // The regions' starts, looked up for each match (thousands in a large file).
    let mut starts: std::collections::HashSet<usize> = folds.iter().map(|f| f.start_byte).collect();
    let whole = [root.byte_range()];
    let mut cursor = QueryCursor::new();
    for range in scope.unwrap_or(&whole) {
        cursor.set_byte_range(range.clone());
        let mut matches = cursor.matches(query, root, SnapshotText(text));
        while let Some(m) = matches.next() {
            let mut region = None;
            let mut count = None;
            for c in m.captures() {
                match names[c.index as usize] {
                    "fold" => region = Some((c.node, None)),
                    "fold.object" => region = Some((c.node, Some(true))),
                    "fold.array" => region = Some((c.node, Some(false))),
                    "count.object" => count = Some((c.node, true)),
                    "count.array" => count = Some((c.node, false)),
                    _ => {}
                }
            }
            let Some((node, own_count)) = region else { continue };
            let (start_byte, start_line) = trimmed_start(node, text);
            let end = node.end_position();
            let end_line =
                if end.column == 0 && end.row > 0 { end.row - 1 } else { end.row } as u32;
            if end_line <= start_line || !starts.insert(start_byte) {
                continue;
            }
            let label = |n: Node<'_>, object: bool| {
                let entries = entries(n);
                if object { FoldCount::Keys(entries) } else { FoldCount::Items(entries) }
            };
            let placeholder = count
                .map(|(n, object)| label(n, object))
                .or_else(|| own_count.map(|object| label(node, object)));
            folds.push(FoldRange {
                start_byte,
                end_byte: node.end_byte(),
                start_line,
                end_line,
                placeholder,
            });
        }
    }
    folds
}

/// Regions from pairs of flat directives (siblings in the tree, not an enclosing node),
/// e.g. Twig: `{% if %}` … `{% endif %}`. In the query, `@fold.pair` is the directive and
/// `@fold.key` its keyword; `X` is closed by `endX` (the nearest opening
/// with the same name). Unpaired openings (`{% set x = 1 %}`) are ignored.
///
/// Always computed over the whole layer: a pair can span unchanged zones.
fn paired_folds(query: &Query, root: Node<'_>, text: &TextSnapshot, folds: &mut Vec<FoldRange>) {
    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, SnapshotText(text));
    let mut open: HashMap<String, Vec<Node<'_>>> = HashMap::new();
    let mut pairs = Vec::new();
    while let Some(m) = matches.next() {
        let (mut directive, mut key) = (None, None);
        for c in m.captures() {
            match names[c.index as usize] {
                "fold.pair" => directive = Some(c.node),
                "fold.key" => key = Some(c.node),
                _ => {}
            }
        }
        let (Some(directive), Some(key)) = (directive, key) else { continue };
        let key = text.slice(key.byte_range()).trim().to_ascii_lowercase();
        match key.strip_prefix("end") {
            Some(name) if !name.is_empty() => {
                if let Some(start) = open.get_mut(name).and_then(Vec::pop) {
                    pairs.push((start, directive));
                }
            }
            _ => open.entry(key).or_default().push(directive),
        }
    }
    for (start, end) in pairs {
        let (start_byte, start_line) = trimmed_start(start, text);
        let end_line = end.end_position().row as u32;
        if end_line <= start_line {
            continue;
        }
        folds.retain(|f| f.start_byte != start_byte);
        folds.push(FoldRange {
            start_byte,
            end_byte: end.end_byte(),
            start_line,
            end_line,
            placeholder: None,
        });
    }
}

/// The real start of a node (offset, line), without leading whitespace: some
/// grammars (e.g. Twig) include in the token the whitespace and blank lines before it.
fn trimmed_start(node: Node<'_>, text: &TextSnapshot) -> (usize, u32) {
    let start = node.start_byte();
    let row = node.start_position().row as u32;
    let first_line_end = (start + 256).min(node.end_byte());
    let head = text.slice(start..first_line_end);
    let skipped = head.len() - head.trim_start().len();
    if skipped == 0 {
        return (start, row);
    }
    let newlines = head[..skipped].matches('\n').count() as u32;
    (start + skipped, row + newlines)
}

/// The regions of all layers; on the same line, the largest region remains.
fn merge_folds<'a>(all: impl Iterator<Item = &'a FoldRange>) -> Vec<FoldRange> {
    let mut by_line: HashMap<u32, FoldRange> = HashMap::new();
    for fold in all {
        match by_line.get(&fold.start_line) {
            Some(existing) if existing.end_line >= fold.end_line => {}
            _ => {
                by_line.insert(fold.start_line, fold.clone());
            }
        }
    }
    let mut folds: Vec<FoldRange> = by_line.into_values().collect();
    folds.sort_by_key(|f| f.start_line);
    folds
}

/// "12 keys", "1 item": counts the named children (excluding comments).
/// The entries of an object or a list (its named children, comments left out).
fn entries(node: Node<'_>) -> usize {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).filter(|c| !c.kind().contains("comment")).count()
}
