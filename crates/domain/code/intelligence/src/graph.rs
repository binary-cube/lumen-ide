//! The dependency graph around a file (the canvas): what it includes and uses
//! (files, classes, functions, methods) and who uses it. Built from the symbol
//! index and from the files' text (read through [`Project`]), scanned by the rules each
//! language gives ([`SourceRules`]); the domain does no I/O.

use std::path::{Path, PathBuf};

use crate::Naming;
use crate::engine::Project;
use crate::index::SymbolIndex;
use crate::symbols::{FileSymbols, Symbol, SymbolKind, Visibility};

/// How a language's code points at other files and declarations: the words and operators
/// the dependency graph looks for in its text. Given by the language
/// ([`crate::LanguageIntelligence::source_rules`]); a file without them shows only its own
/// declarations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRules {
    /// The extensions of the files that can use each other (PHP with `phtml`, JS with TS).
    pub extensions: &'static [&'static str],
    /// Words followed by a quoted path that brings another file in (`require`, `from`).
    pub includes: &'static [&'static str],
    /// What an include's path may leave out (`.js`, `/index.js`), tried after it as written.
    pub include_suffixes: &'static [&'static str],
    /// Words before a type's name (`new`, `extends`, `implements`, `instanceof`).
    pub before_type: &'static [&'static str],
    /// After a type's name, its static members (`::`).
    pub static_access: Option<&'static str>,
    /// Before a member's name (`->`, `::`, `.`).
    pub member_access: &'static [&'static str],
    /// Words before a function's name where it's declared, not called (`function`).
    pub declares_function: &'static [&'static str],
    /// Words before a type's name where it's declared (`class`, `interface`, `trait`).
    pub declares_type: &'static [&'static str],
    /// Modifiers left out of a signature (`public`, `static`, `function`, `async`).
    pub modifiers: &'static [&'static str],
    /// How it writes names (the namespace separator, the sigil, the case).
    pub naming: Naming,
    /// How it reads a place of its comments (plan 75): a name written there as text isn't a
    /// usage. `None`: its comments count as code.
    pub comments: Option<CommentReader>,
}

/// A language's [`crate::LanguageIntelligence::comment_place`], as a plain function: what a
/// place of a text is, when it's in a comment.
#[derive(Clone, Copy)]
pub struct CommentReader(pub fn(&str, usize) -> Option<crate::CommentPlace>);

impl CommentReader {
    /// Whether `at` of `text` is a comment's text (not code written in it).
    pub fn is_text(self, text: &str, at: usize) -> bool {
        (self.0)(text, at) == Some(crate::CommentPlace::Text)
    }
}

impl PartialEq for CommentReader {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::fn_addr_eq(self.0, other.0)
    }
}

impl Eq for CommentReader {}

impl std::fmt::Debug for CommentReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CommentReader")
    }
}

impl SourceRules {
    /// Files of `extensions` that are code (shown beside a file of their folder) but whose
    /// text points nowhere the graph follows (CSS, HTML).
    pub const fn plain(extensions: &'static [&'static str]) -> Self {
        Self {
            extensions,
            includes: &[],
            include_suffixes: &[],
            before_type: &[],
            static_access: None,
            member_access: &[],
            declares_function: &[],
            declares_type: &[],
            modifiers: &[],
            naming: Naming::PLAIN,
            comments: None,
        }
    }

    /// The rules of a file with extension `ext`, among `rules`.
    fn of<'r>(rules: &'r [SourceRules], path: &Path) -> Option<&'r SourceRules> {
        let ext = path.extension().and_then(|e| e.to_str())?;
        rules.iter().find(|r| r.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)))
    }

    fn is_ident(&self, c: char) -> bool {
        c.is_alphanumeric() || c == '_' || Some(c) == self.naming.namespace_separator
    }

    /// `head` ends with one of `words` (`new`, `->`).
    fn ends_with_any(head: &str, words: &[&str]) -> bool {
        words.iter().any(|w| head.ends_with(w))
    }
}

/// At most how many neighboring files are added (in each direction).
const MAX_NEIGHBOURS: usize = 24;
/// How many lines of code a preview has.
const PREVIEW_LINES: usize = 14;
/// At most how many lines of code a declaration has (for editing on the canvas).
const MAX_CODE_LINES: usize = 400;
/// How many unconnected files (from the same directory) are shown.
const MAX_UNCONNECTED: usize = 6;

/// The kind of a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// `require` / `include` / `import`.
    Include,
    /// Uses a class (`new X`, `X::`, `extends X`, type).
    Type,
    /// Calls a function or a method.
    Call,
}

/// A declaration in a graph file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSymbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The class containing it (short name).
    pub container: Option<String>,
    pub line: u32,
    pub end_line: u32,
    /// The signature, on one line (`stop(int $a = 1)`).
    pub signature: String,
    pub private: bool,
    /// The declaration's code (for the center file), with the number of the first line.
    pub code: Vec<String>,
}

/// A file in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphFile {
    /// Relative to the project root (or absolute, outside it).
    pub path: PathBuf,
    pub absolute: PathBuf,
    pub language: String,
    pub lines: u32,
    /// The first lines of code (without the leading blank ones)…
    pub preview: Vec<String>,
    /// …starting at this line (0-based).
    pub preview_start: u32,
    pub symbols: Vec<GraphSymbol>,
    /// In the same directory, but with no links to the center file.
    pub unconnected: bool,
}

/// A link: from file `from` (at line `line`) to file `to` (and, if
/// known, the declaration `symbol` in it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: usize,
    pub line: u32,
    pub to: usize,
    pub symbol: Option<usize>,
    pub kind: EdgeKind,
}

/// A place you can jump to from the canvas (⌘K): a file or a declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub label: String,
    /// The relative path (shown next to the label).
    pub detail: String,
    pub path: PathBuf,
    pub line: Option<u32>,
    pub kind: Option<SymbolKind>,
}

/// At most how many declarations go into the catalog.
const MAX_CATALOG_SYMBOLS: usize = 20_000;

/// The project's files and the declarations in the index, for canvas search (members
/// named after their class's short name, by its language's `rules`).
pub fn catalog(
    index: &SymbolIndex,
    root: &Path,
    files: &[String],
    rules: &[SourceRules],
) -> Vec<CatalogEntry> {
    let mut out: Vec<CatalogEntry> = files
        .iter()
        .map(|f| {
            let path = root.join(f);
            CatalogEntry {
                label: Path::new(f)
                    .file_name()
                    .map_or_else(|| f.clone(), |n| n.to_string_lossy().into_owned()),
                detail: f.clone(),
                path,
                line: None,
                kind: None,
            }
        })
        .collect();
    let relative = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    for (path, s) in index.top_level().chain(index.all_members()).take(MAX_CATALOG_SYMBOLS) {
        if !(s.kind.is_type() || matches!(s.kind, SymbolKind::Function | SymbolKind::Method)) {
            continue;
        }
        let naming = SourceRules::of(rules, path).map(|r| r.naming).unwrap_or_default();
        let label = match (&s.container, s.kind) {
            (Some(c), _) => format!("{}::{}()", naming.short_name(c), s.name),
            (None, SymbolKind::Function) => format!("{}()", s.name),
            _ => s.name.to_string(),
        };
        out.push(CatalogEntry {
            label,
            detail: relative(path),
            path: path.to_path_buf(),
            line: Some(s.line),
            kind: Some(s.kind),
        });
    }
    out
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepGraph {
    /// The project's files and declarations (⌘K search), shared between graphs of the
    /// same project state.
    pub catalog: std::sync::Arc<Vec<CatalogEntry>>,
    /// The index of the center file.
    pub focus: usize,
    /// The cursor line in the center file (the declaration there is selected).
    pub cursor_line: Option<u32>,
    pub files: Vec<GraphFile>,
    pub edges: Vec<GraphEdge>,
}

/// A reference found in text: where it is and what it points to (file, declaration in it).
struct Reference {
    offset: usize,
    target: PathBuf,
    symbol: Option<String>,
    kind: EdgeKind,
}

/// The word (identifier, with its namespace's separators) that starts at `at`.
fn word_at<'t>(text: &'t str, at: usize, rules: &SourceRules) -> &'t str {
    let end = text[at..].find(|c: char| !rules.is_ident(c)).map_or(text.len(), |i| at + i);
    &text[at..end]
}

/// The quoted string after `at` (on the same line), e.g. `require 'a.php';`.
fn quoted_after(text: &str, at: usize) -> Option<&str> {
    let rest = &text[at..];
    let line_end = rest.find('\n').unwrap_or(rest.len());
    let rest = &rest[..line_end];
    let open = rest.find(['\'', '"', '`'])?;
    let quote = rest[open..].chars().next()?;
    let close = rest[open + 1..].find(quote)?;
    Some(&rest[open + 1..open + 1 + close])
}

/// The references in `text` (file `path`, with symbols `file`) to other files, found by
/// its language's `rules`.
fn references(
    index: &SymbolIndex,
    project: &Project<'_>,
    path: &Path,
    text: &str,
    file: Option<&FileSymbols>,
    rules: &SourceRules,
) -> Vec<Reference> {
    let naming = &rules.naming;
    let separator = naming.namespace_separator;
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let Some(c) = text[i..].chars().next() else { break };
        if !(c.is_ascii_alphabetic() || c == '_' || Some(c) == separator) {
            i += c.len_utf8();
            continue;
        }
        let prev = text[..i].chars().next_back();
        if prev.is_some_and(|p| rules.is_ident(p) || Some(p) == naming.sigil) {
            i += c.len_utf8();
            continue;
        }
        let word = word_at(text, i, rules);
        let after = &text[i + word.len()..];
        let head = text[..i].trim_end();
        // Includes: `require 'x.php'`, `include_once __DIR__ . '/x.php'`, `import … from './x'`.
        if rules.includes.iter().any(|w| w.eq_ignore_ascii_case(word))
            && let Some(literal) = quoted_after(text, i + word.len())
        {
            let candidates = std::iter::once(literal.to_owned())
                .chain(rules.include_suffixes.iter().map(|s| format!("{literal}{s}")));
            if let Some(target) =
                candidates.into_iter().find_map(|c| project.resolve_include(path, &c))
            {
                out.push(Reference { offset: i, target, symbol: None, kind: EdgeKind::Include });
            }
            i += word.len();
            continue;
        }
        let after_trim = after.trim_start();
        let after_member = SourceRules::ends_with_any(head, rules.member_access);
        let is_type_use = SourceRules::ends_with_any(head, rules.before_type)
            || rules.static_access.is_some_and(|op| after_trim.starts_with(op))
            // A typed parameter or property (`Cart $cart`).
            || naming.sigil.is_some_and(|s| after_trim.starts_with(s) && !after_member);
        let is_call = after_trim.starts_with('(')
            && !after_member
            && !SourceRules::ends_with_any(head, rules.declares_function)
            && !SourceRules::ends_with_any(head, rules.before_type);
        if (is_type_use || is_call) && !word.is_empty() {
            let resolved = file.map_or_else(
                || naming.absolute(word).unwrap_or(word).to_owned(),
                |f| f.resolve_class(word, i, naming),
            );
            let found = if is_type_use {
                index.type_by_fqn(&resolved).map(|(p, s)| (p, s.name.clone(), EdgeKind::Type))
            } else {
                index
                    .function_by_fqn(&resolved)
                    .or_else(|| {
                        index
                            .by_name(word)
                            .into_iter()
                            .find(|(_, s)| s.kind == SymbolKind::Function)
                    })
                    .map(|(p, s)| (p, s.name.clone(), EdgeKind::Call))
            };
            if let Some((target, symbol, kind)) = found
                && target != path
            {
                out.push(Reference {
                    offset: i,
                    target: target.to_path_buf(),
                    symbol: Some(symbol.to_string()),
                    kind,
                });
            }
        }
        i += word.len().max(1);
    }
    out
}

/// The method calls (`->name(`, `.name(`: the language's member access) in `text` that are
/// methods of the center class (by name): (offset, method name).
fn method_calls<'a>(
    text: &str,
    methods: &'a [String],
    rules: &SourceRules,
) -> Vec<(usize, &'a str)> {
    let mut out = Vec::new();
    let same = |a: &str, b: &str| {
        if rules.naming.methods_ignore_case { a.eq_ignore_ascii_case(b) } else { a == b }
    };
    for arrow in rules.member_access {
        let mut from = 0;
        while let Some(i) = text[from..].find(arrow) {
            let at = from + i + arrow.len();
            from = at;
            let word = word_at(text, at, rules);
            if text[at + word.len()..].trim_start().starts_with('(')
                && let Some(m) = methods.iter().find(|m| same(m, word))
            {
                out.push((at, m.as_str()));
            }
        }
    }
    out.sort_unstable();
    out
}

fn line_of(text: &str, offset: usize) -> u32 {
    kernel::text::line_of(text, offset)
}

fn preview(text: &str) -> Vec<String> {
    text.lines()
        .skip_while(|l| l.trim().is_empty())
        .take(PREVIEW_LINES)
        .map(|l| l.trim_end().replace('\t', "    "))
        .collect()
}

/// The signature of a declaration from its header: without its language's `modifiers`, on
/// one line.
fn signature_of(header: &str, modifiers: &[&str]) -> String {
    header.split_whitespace().skip_while(|w| modifiers.contains(w)).collect::<Vec<_>>().join(" ")
}

/// A declaration of file `text` as shown in the graph (with its code if `with_code`), by
/// its language's `rules` (none: as written).
fn graph_symbol(
    s: &Symbol,
    text: &str,
    with_code: bool,
    rules: Option<&SourceRules>,
) -> GraphSymbol {
    let start = s.range.start.min(text.len());
    let end = s.range.end.min(text.len());
    let body = text.get(start..end).unwrap_or("");
    let header = body.split(['{', ';']).next().unwrap_or("");
    let signature = signature_of(header, rules.map_or(&[], |r| r.modifiers));
    let naming = rules.map(|r| r.naming).unwrap_or_default();
    let first = line_of(text, start);
    // The code starts with the docblock above it, if it has one.
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[..line_start];
    let doc_start = before
        .trim_end()
        .ends_with("*/")
        .then(|| before.rfind("/**"))
        .flatten()
        .map_or(line_start, |i| text[..i].rfind('\n').map_or(0, |n| n + 1));
    let code = if with_code {
        text[doc_start..end].lines().take(MAX_CODE_LINES).map(str::to_owned).collect()
    } else {
        Vec::new()
    };
    GraphSymbol {
        name: s.name.to_string(),
        kind: s.kind,
        container: s.container.as_deref().map(|c| naming.short_name(c).to_owned()),
        line: line_of(text, doc_start).min(first),
        end_line: line_of(text, end),
        signature: if signature.is_empty() { s.name.to_string() } else { signature },
        private: s.visibility == Visibility::Private,
        code,
    }
}

/// The names of the declarations in `file` whose kind passes `keep`.
fn names_where(file: Option<&FileSymbols>, keep: impl Fn(SymbolKind) -> bool) -> Vec<String> {
    file.map(|f| f.symbols.iter().filter(|s| keep(s.kind)).map(|s| s.name.to_string()).collect())
        .unwrap_or_default()
}

/// The graph being built around the center file, with what its phases share.
struct GraphBuilder<'a, 'p> {
    index: &'a SymbolIndex,
    project: &'a Project<'p>,
    language_of: &'a dyn Fn(&Path) -> String,
    /// Every language's rules (a file's, by its extension).
    rules: &'a [SourceRules],
    focus: &'a Path,
    focus_text: &'a str,
    graph: DepGraph,
}

impl GraphBuilder<'_, '_> {
    fn relative(&self, p: &Path) -> PathBuf {
        self.project.root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).to_path_buf()
    }

    /// The absolute path of project file `rel`.
    fn absolute(&self, rel: &Path) -> PathBuf {
        self.project.root.map_or_else(|| rel.to_path_buf(), |r| r.join(rel))
    }

    /// Adds file `path` (once) and returns its index in the graph.
    fn add_file(&mut self, path: &Path, text: &str, with_code: bool) -> usize {
        if let Some(i) = self.graph.files.iter().position(|f| f.absolute == path) {
            return i;
        }
        let rules = SourceRules::of(self.rules, path);
        let symbols = self
            .index
            .file(path)
            .map(|f| {
                f.symbols
                    .iter()
                    .filter(|s| !s.name.is_empty() && !s.range.is_empty())
                    .map(|s| graph_symbol(s, text, with_code, rules))
                    .collect()
            })
            .unwrap_or_default();
        self.graph.files.push(GraphFile {
            path: self.relative(path),
            absolute: path.to_path_buf(),
            language: (self.language_of)(path),
            lines: text.lines().count() as u32,
            preview: preview(text),
            preview_start: text.lines().take_while(|l| l.trim().is_empty()).count() as u32,
            unconnected: false,
            symbols,
        });
        self.graph.files.len() - 1
    }

    /// The index of the center file's declaration named `name`.
    fn focus_symbol(&self, name: &str) -> Option<usize> {
        self.graph.files[self.graph.focus].symbols.iter().position(|s| s.name == name)
    }

    /// What the center file uses.
    fn add_outgoing(&mut self) {
        let Some(rules) = SourceRules::of(self.rules, self.focus) else { return };
        let focus_symbols = self.index.file(self.focus);
        let (project, focus, text) = (self.project, self.focus, self.focus_text);
        let refs = references(self.index, project, focus, text, focus_symbols, rules);
        for r in refs {
            if self.graph.files.len() > MAX_NEIGHBOURS {
                break;
            }
            let Some(text) = (self.project.read)(&r.target) else { continue };
            let to = self.add_file(&r.target, &text, false);
            let symbol = r
                .symbol
                .and_then(|n| self.graph.files[to].symbols.iter().position(|s| s.name == n));
            let line = line_of(self.focus_text, r.offset);
            let edge = GraphEdge { from: self.graph.focus, line, to, symbol, kind: r.kind };
            if !self
                .graph
                .edges
                .iter()
                .any(|e| e.from == edge.from && e.to == edge.to && e.symbol == edge.symbol)
            {
                self.graph.edges.push(edge);
            }
        }
    }

    /// Who uses it: files in which its names appear (or that include it).
    fn add_incoming(&mut self, files: &[String]) {
        let focus = self.focus;
        // Files that can use it: of its language's family.
        let Some(rules) = SourceRules::of(self.rules, focus) else { return };
        let focus_symbols = self.index.file(focus);
        let names = names_where(focus_symbols, |k| k.is_type() || k == SymbolKind::Function);
        let methods = names_where(focus_symbols, |k| k == SymbolKind::Method);
        let file_name =
            focus.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut incoming = 0;
        for file in files {
            if incoming >= MAX_NEIGHBOURS {
                break;
            }
            let rel = Path::new(file);
            if kernel::in_dependencies_or_git(rel) {
                continue;
            }
            if SourceRules::of(std::slice::from_ref(rules), rel).is_none() {
                continue;
            }
            let path = self.absolute(rel);
            if path == focus {
                continue;
            }
            let Some(text) = (self.project.read)(&path) else { continue };
            let mentions = names.iter().any(|n| text.contains(n.as_str()))
                || text.contains(file_name.as_str());
            if !mentions {
                continue;
            }
            let symbols = self.index.file(&path);
            let refs: Vec<Reference> =
                references(self.index, self.project, &path, &text, symbols, rules)
                    .into_iter()
                    .filter(|r| r.target == focus)
                    .collect();
            if refs.is_empty() {
                continue;
            }
            incoming += 1;
            let from = self.add_file(&path, &text, false);
            self.add_uses(from, &text, &refs, &methods, rules);
        }
    }

    /// The links from file `from` (with `text`) to the center file: its references
    /// `refs`, then the calls of the center class's `methods`.
    fn add_uses(
        &mut self,
        from: usize,
        text: &str,
        refs: &[Reference],
        methods: &[String],
        rules: &SourceRules,
    ) {
        let to = self.graph.focus;
        for r in refs {
            let symbol = r.symbol.as_ref().and_then(|n| self.focus_symbol(n));
            let line = line_of(text, r.offset);
            self.graph.edges.push(GraphEdge { from, line, to, symbol, kind: r.kind });
        }
        // Methods of the center class called from there (`$car->stop()`).
        for (offset, method) in method_calls(text, methods, rules) {
            let symbol = self.focus_symbol(method);
            let line = line_of(text, offset);
            if !self.graph.edges.iter().any(|e| {
                e.from == from && e.symbol == symbol && e.to == to && e.kind == EdgeKind::Call
            }) {
                self.graph.edges.push(GraphEdge { from, line, to, symbol, kind: EdgeKind::Call });
            }
        }
    }

    /// Code files from the same directory, without links: "unconnected".
    fn add_unconnected(&mut self, files: &[String]) {
        let dir = self.relative(self.focus).parent().map(Path::to_path_buf).unwrap_or_default();
        let mut unconnected = 0;
        for file in files {
            if unconnected >= MAX_UNCONNECTED {
                break;
            }
            let rel = Path::new(file);
            if rel.parent().map(Path::to_path_buf).unwrap_or_default() != dir {
                continue;
            }
            // Code: a file some language has rules for.
            if SourceRules::of(self.rules, rel).is_none() {
                continue;
            }
            let path = self.absolute(rel);
            if self.graph.files.iter().any(|f| f.absolute == path) {
                continue;
            }
            let Some(text) = (self.project.read)(&path) else { continue };
            let i = self.add_file(&path, &text, false);
            self.graph.files[i].unconnected = true;
            unconnected += 1;
        }
    }
}

/// Builds the graph around file `focus` (absolute path). `files` are all
/// the project's files (relative to `root`), for usages by others; `rules`, every
/// language's ([`SourceRules`]).
pub fn build_graph(
    index: &SymbolIndex,
    project: &Project<'_>,
    files: &[String],
    focus: &Path,
    language_of: &dyn Fn(&Path) -> String,
    rules: &[SourceRules],
) -> Option<DepGraph> {
    let focus_text = (project.read)(focus)?;
    let mut builder = GraphBuilder {
        index,
        project,
        language_of,
        rules,
        focus,
        focus_text: &focus_text,
        graph: DepGraph::default(),
    };
    builder.graph.focus = builder.add_file(focus, &focus_text, true);
    builder.add_outgoing();
    builder.add_incoming(files);
    builder.add_unconnected(files);
    Some(builder.graph)
}

#[cfg(test)]
#[path = "../tests/unit/graph.rs"]
mod tests;
