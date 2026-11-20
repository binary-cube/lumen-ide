//! Grammars (contributed by language modules) and their queries, compiled lazily.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use languages::LanguageId;
use tree_sitter::{Language, Query};

/// A language module's contribution to the syntax engine.
///
/// Captures in `highlights` are theme scopes (`keyword`, `string`,
/// `function.method`...); as in tree-sitter, the first pattern wins on the same node.
///
/// Captures in `folds` mark collapsible regions: `@fold` (generic),
/// `@fold.object` ("12 keys"), `@fold.array` ("3 items"). The label can count
/// the children of another node in the same pattern: `@count.object` / `@count.array`.
#[derive(Clone)]
pub struct Grammar {
    pub language: LanguageId,
    pub ts: Language,
    pub highlights: Cow<'static, str>,
    pub injections: Cow<'static, str>,
    pub folds: Cow<'static, str>,
    /// What indents (`@indent`: the nodes whose inner lines go one level in); empty: none.
    pub indents: Cow<'static, str>,
    /// Nodes whose text is the content of a string (without quotes). Embedded languages
    /// are looked for inside them (`language=...` markers, content sniffers).
    pub string_content_nodes: &'static [&'static str],
    /// Its parse errors are reported when it's embedded in another language. Off for
    /// grammars that only ever see fragments there (JSON in a string, the PHP of a Blade
    /// echo, `$users as $user`), whose "errors" are just the fragment's edges.
    pub embedded_errors: bool,
    /// The parse errors of what's embedded in it are reported. Off for templates mixing
    /// their own syntax into the embedded code (Blade's `{{ }}` inside a `<script>`).
    pub injected_errors: bool,
    /// Its parse errors are reported in its own files. Off for a permissive grammar
    /// covering several dialects (SQL), whose "errors" are often valid code of a dialect it
    /// doesn't know: false alarms break trust; the real errors come from elsewhere.
    pub own_errors: bool,
}

impl std::fmt::Debug for Grammar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grammar").field("language", &self.language).finish_non_exhaustive()
    }
}

/// A grammar's compiled queries.
#[derive(Debug)]
pub(crate) struct Queries {
    pub highlights: Option<Query>,
    pub injections: Option<Query>,
    pub folds: Option<Query>,
    pub indents: Option<Query>,
    /// Languages injected with `#set! injection.combined` (the fragments form a single
    /// document, e.g. the HTML in a PHP file).
    pub combined: Vec<LanguageId>,
    /// Its doc comments' parts, if its module reads them.
    pub doc: Option<Arc<dyn crate::DocComments>>,
}

#[derive(Debug)]
pub(crate) struct GrammarEntry {
    pub grammar: Grammar,
    /// The language's index in the scope table.
    pub index: u16,
    held: Mutex<Held>,
    /// Its doc comments' reader (set once, at startup).
    doc: std::sync::OnceLock<Arc<dyn crate::DocComments>>,
}

/// The compiled queries while they're kept, and when they were last asked for.
#[derive(Debug)]
struct Held {
    queries: Option<Arc<Queries>>,
    used: Instant,
}

impl GrammarEntry {
    fn new(grammar: Grammar, index: u16) -> Self {
        let held = Mutex::new(Held { queries: None, used: Instant::now() });
        Self { grammar, index, held, doc: std::sync::OnceLock::new() }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queries are compiled on first use (compiling can cost tens of ms, so we don't
    /// pay for it at startup for unused languages), and again after they were let go
    /// ([`Grammars::release_idle`]). The layers parsed with them hold them: while a file
    /// of the language is open, they stay. An invalid query is reported and ignored,
    /// without stopping the rest.
    pub fn queries(&self) -> Arc<Queries> {
        let mut held = self.held();
        held.used = Instant::now();
        let queries = held.queries.get_or_insert_with(|| Arc::new(self.compile()));
        Arc::clone(queries)
    }

    fn compile(&self) -> Queries {
        let compile = |kind: &str, source: &str| {
            if source.trim().is_empty() {
                return None;
            }
            Query::new(&self.grammar.ts, source)
                .inspect_err(|e| {
                    tracing::error!(language = %self.grammar.language, %kind, error = %e, "a grammar's query is invalid");
                })
                .ok()
        };
        let injections = compile("injections", &self.grammar.injections);
        let combined = injections.as_ref().map_or_else(Vec::new, |q| {
            (0..q.pattern_count())
                .filter(|&i| q.property_settings(i).iter().any(|p| &*p.key == "injection.combined"))
                .filter_map(|i| {
                    q.property_settings(i)
                        .iter()
                        .find(|p| &*p.key == "injection.language")
                        .and_then(|p| p.value.as_deref())
                        .map(crate::injection::normalize_language)
                })
                .collect()
        });
        Queries {
            highlights: compile("highlights", &self.grammar.highlights),
            injections,
            folds: compile("folds", &self.grammar.folds),
            indents: compile("indents", &self.grammar.indents),
            combined,
            doc: self.doc.get().cloned(),
        }
    }
}

/// All registered grammars.
#[derive(Debug, Default, Clone)]
pub struct Grammars {
    entries: HashMap<LanguageId, Arc<GrammarEntry>>,
    by_index: Vec<Arc<GrammarEntry>>,
}

impl Grammars {
    pub fn register(&mut self, grammar: Grammar) {
        let index = self.by_index.len() as u16;
        let entry = Arc::new(GrammarEntry::new(grammar, index));
        self.entries.insert(entry.grammar.language.clone(), Arc::clone(&entry));
        self.by_index.push(entry);
    }

    /// A language's doc comments read for their parts (its grammar registered first).
    pub fn add_doc_comments(&mut self, reader: Arc<dyn crate::DocComments>) {
        if let Some(entry) = self.entries.get(&reader.language()) {
            let _ = entry.doc.set(reader);
        }
    }

    pub fn contains(&self, language: &LanguageId) -> bool {
        self.entries.contains_key(language)
    }

    pub(crate) fn get(&self, language: &LanguageId) -> Option<&Arc<GrammarEntry>> {
        self.entries.get(language)
    }

    pub(crate) fn by_index(&self, index: u16) -> Option<&Arc<GrammarEntry>> {
        self.by_index.get(index as usize)
    }

    /// The language's queries are compiled (a file of it was opened, or it was warmed up).
    pub fn is_compiled(&self, language: &LanguageId) -> bool {
        self.get(language).is_some_and(|entry| entry.held().queries.is_some())
    }

    /// Lets go of the compiled queries no open file uses (nothing but the grammar holds
    /// them) and unused for `idle`, except the languages `keep`s. Returns how many.
    pub fn release_idle(&self, idle: Duration, keep: impl Fn(&LanguageId) -> bool) -> usize {
        let mut released = 0;
        for entry in &self.by_index {
            let mut held = entry.held();
            let unused = held.queries.as_ref().is_some_and(|q| Arc::strong_count(q) == 1);
            if unused && held.used.elapsed() >= idle && !keep(&entry.grammar.language) {
                held.queries = None;
                released += 1;
            }
        }
        released
    }

    /// The memory the language's compiled queries hold (bytes, an estimate: tree-sitter
    /// allocates them itself; measured at 13–31× their source, PHP's the largest); 0 while
    /// they're not compiled.
    pub fn memory(&self, language: &LanguageId) -> u64 {
        const PER_SOURCE_BYTE: u64 = 20;
        let Some(entry) = self.get(language) else { return 0 };
        if entry.held().queries.is_none() {
            return 0;
        }
        let g = &entry.grammar;
        let source = [&g.highlights, &g.injections, &g.folds, &g.indents]
            .iter()
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.len() as u64)
            .sum::<u64>();
        source * PER_SOURCE_BYTE
    }

    /// Compiles the queries ahead of time (e.g. on a background thread, at startup).
    pub fn warm_up(&self, language: &LanguageId) {
        if let Some(entry) = self.get(language) {
            entry.queries();
        }
    }

    /// Checks all queries; returns the languages with problems (for tests and CI).
    /// The conditions the queries use that nothing evaluates (`language: condition`): the
    /// engine leaves them to its caller, so a pattern with one would match everywhere.
    pub fn unevaluated_conditions(&self) -> Vec<String> {
        let mut unknown = Vec::new();
        for entry in &self.by_index {
            let g = &entry.grammar;
            for source in [&g.highlights, &g.injections, &g.folds, &g.indents] {
                let Ok(query) = tree_sitter::Query::new(&g.ts, source) else { continue };
                for pattern in 0..query.pattern_count() {
                    unknown.extend(
                        query
                            .general_predicates(pattern)
                            .iter()
                            .filter(|p| !crate::parse::EVALUATED.contains(&&*p.operator))
                            .map(|p| format!("{}: {}", g.language, p.operator)),
                    );
                }
            }
        }
        unknown.sort();
        unknown.dedup();
        unknown
    }

    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for entry in &self.by_index {
            for (kind, source) in [
                ("highlights", &entry.grammar.highlights),
                ("injections", &entry.grammar.injections),
                ("folds", &entry.grammar.folds),
                ("indents", &entry.grammar.indents),
            ] {
                if !source.trim().is_empty()
                    && let Err(e) = Query::new(&entry.grammar.ts, source)
                {
                    problems.push(format!("{} / {kind}: {e}", entry.grammar.language));
                }
            }
        }
        problems
    }
}

#[cfg(test)]
#[path = "../tests/unit/grammar.rs"]
mod tests;
