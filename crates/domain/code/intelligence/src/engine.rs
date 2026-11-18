//! Completion and "go to definition", over the context given by the language and the index.

use std::path::Path;

use kernel::LanguageId;

use crate::fuzzy::fuzzy_score;
use crate::index::{Location, SymbolIndex};
use crate::symbols::{FileSymbols, Symbol, SymbolKind, Visibility};
use crate::{
    CompletionContext, CompletionItem, CompletionKind, ContextKind, LanguageIntelligence, Naming,
    Receiver, Target,
};

/// Project access for completion and definitions: the root, reading files and
/// listing directories (the domain does no I/O; the application provides the functions).
pub struct Project<'a> {
    pub root: Option<&'a Path>,
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    /// A directory's entries: (name, is directory).
    pub list: &'a dyn Fn(&Path) -> Vec<(String, bool)>,
    /// When a file was last changed on disk (to re-read only what changed).
    pub modified: &'a dyn Fn(&Path) -> Option<std::time::SystemTime>,
    /// The language level chosen for the project, per language (e.g. `php` → `8.2`).
    pub levels: &'a [(LanguageId, String)],
    /// Every language's intelligence: what a file of another language says of itself (its
    /// doc comments, a CSS rule) is read by its own.
    pub languages: &'a [std::sync::Arc<dyn LanguageIntelligence>],
}

impl std::fmt::Debug for Project<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Project").field("root", &self.root).finish_non_exhaustive()
    }
}

impl Project<'_> {
    /// A project without files (tests, unsaved documents).
    pub fn none() -> Project<'static> {
        Project {
            root: None,
            read: &|_| None,
            list: &|_| Vec::new(),
            modified: &|_| None,
            levels: &[],
            languages: &[],
        }
    }

    /// The intelligence of `path`'s language (by its extension), if one is known.
    pub fn intelligence_for(&self, path: &Path) -> Option<&dyn LanguageIntelligence> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        self.languages
            .iter()
            .find(|i| i.extensions().iter().any(|e| e.eq_ignore_ascii_case(ext)))
            .map(AsRef::as_ref)
    }

    /// The level chosen for the language, if any.
    pub fn level(&self, language: &LanguageId) -> Option<&str> {
        self.levels.iter().find(|(l, _)| l == language).map(|(_, v)| v.as_str())
    }

    /// The language word exists at the project's chosen level (no level: yes).
    pub fn available(&self, intelligence: &dyn LanguageIntelligence, word: &str) -> bool {
        let (Some(level), Some(since)) =
            (self.level(&intelligence.language()), intelligence.since(word))
        else {
            return true;
        };
        let levels = intelligence.levels();
        let position = |v: &str| levels.iter().position(|l| *l == v);
        match (position(level), position(since)) {
            (Some(level), Some(since)) => since <= level,
            _ => true,
        }
    }

    /// The file exists (from its metadata; without it, e.g. in tests, by reading it).
    pub fn exists(&self, path: &Path) -> bool {
        (self.modified)(path).is_some() || (self.read)(path).is_some()
    }

    /// The path of a file included from `from`: relative to `from`'s directory (with `/…` after
    /// `__DIR__ .`), then to the project root.
    pub fn resolve_include(&self, from: &Path, include: &str) -> Option<std::path::PathBuf> {
        let relative = include.trim_start_matches('/');
        let dir = from.parent()?;
        std::iter::once(dir.join(relative))
            .chain(self.root.map(|r| r.join(relative)))
            .find(|p| self.exists(p))
    }

    /// The entries of directory `dir` (as in the code), relative to file `from`.
    fn entries(&self, from: &Path, dir: &str) -> Vec<(String, bool)> {
        let relative = dir.trim_start_matches('/');
        let Some(base) = from.parent() else { return Vec::new() };
        let entries = (self.list)(&base.join(relative));
        if !entries.is_empty() {
            return entries;
        }
        self.root.map(|r| (self.list)(&r.join(relative))).unwrap_or_default()
    }
}

/// At most how many suggestions are returned.
const LIMIT: usize = 80;
/// At most how many of a type's members are returned (after `->` / `::`): a type is offered
/// whole, scrolling through its members being the point (a query builder has hundreds).
const MEMBER_LIMIT: usize = 400;

fn location(path: &Path, s: &Symbol) -> Location {
    Location { path: path.to_path_buf(), offset: s.offset, line: s.line }
}

/// Files in `vendor/` (libraries) are less relevant than the project's own.
fn is_vendor(path: &Path) -> bool {
    kernel::in_dependencies(path)
}

fn kind_of(s: &Symbol) -> CompletionKind {
    match s.kind {
        SymbolKind::Class | SymbolKind::Trait | SymbolKind::Enum => CompletionKind::Class,
        SymbolKind::Interface => CompletionKind::Interface,
        SymbolKind::Function => CompletionKind::Function,
        SymbolKind::Method => CompletionKind::Method,
        SymbolKind::Property => CompletionKind::Property,
        SymbolKind::Constant | SymbolKind::ClassConstant | SymbolKind::EnumCase => {
            CompletionKind::Constant
        }
        SymbolKind::CssClass | SymbolKind::CssId | SymbolKind::CssVariable => {
            CompletionKind::Selector
        }
        SymbolKind::Mixin => CompletionKind::Function,
        SymbolKind::Variable => CompletionKind::Variable,
        SymbolKind::Section => CompletionKind::Namespace,
    }
}

/// The symbol is from a file of the given language (nothing unpacked: plan 74).
fn in_language(index: &SymbolIndex, path: &Path, language: &str) -> bool {
    index.language_of(path).is_some_and(|l| l.is_empty() || l == language)
}

/// The file is in `language`, or in none (suggested everywhere).
fn of_language(file: &FileSymbols, language: &str) -> bool {
    file.language.is_empty() || &*file.language == language
}

/// What a word's matches among the libraries' lists are kept for (each its own: they're
/// places in different lists, or the same list with another filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Narrowing {
    Names,
    Types,
    Members,
}

/// A word's matches among the libraries' lists: what for, the libraries' version and the
/// language they were found in, the word, the places matched.
type Narrowed = (Narrowing, u64, String, String, Vec<u32>);

thread_local! {
    /// The last words' matches (one for each [`Narrowing`]), kept for the next key: a word
    /// typed on (`ge`, then `get`) matches only among what the one before it did — those
    /// alone scored again, not the libraries' tens of thousands of names.
    static NARROWED: std::cell::RefCell<Vec<Narrowed>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The places to look among for `query`: those the word before it matched, when `query`
/// goes on from it (the same libraries, language and list); `None`: all of them.
fn narrowed(what: Narrowing, version: u64, language: &str, query: &str) -> Option<Vec<u32>> {
    let query = query.to_lowercase();
    NARROWED.with_borrow(|kept| {
        kept.iter()
            .find(|(w, v, l, q, _)| {
                *w == what
                    && *v == version
                    && l == language
                    && !q.is_empty()
                    && query.starts_with(q.as_str())
            })
            .map(|(.., places)| places.clone())
    })
}

/// `query`'s matches kept for the next key (see [`narrowed`]).
fn keep_narrowed(what: Narrowing, version: u64, language: &str, query: &str, places: Vec<u32>) {
    let query = query.to_lowercase();
    NARROWED.with_borrow_mut(|kept| {
        kept.retain(|(w, ..)| *w != what);
        kept.push((what, version, language.to_owned(), query, places));
    });
}

/// A suggestion for a symbol: methods and functions get `()`, with the cursor between
/// the parens if they have parameters.
fn item(
    s: &Symbol,
    static_access: bool,
    query: &str,
    path: &Path,
    naming: &Naming,
) -> Option<CompletionItem> {
    // A static property is written with the language's sigil (`self::$count`). Scored before
    // anything is copied: most candidates don't match.
    let sigiled = (static_access && s.kind == SymbolKind::Property)
        .then_some(naming.sigil)
        .flatten()
        .map(|sigil| format!("{sigil}{}", s.name));
    let mut score = fuzzy_score(query, sigiled.as_deref().unwrap_or(&s.name))?;
    let label = sigiled.unwrap_or_else(|| s.name.to_string());
    if is_vendor(path) {
        score -= 15;
    }
    let callable = matches!(s.kind, SymbolKind::Method | SymbolKind::Function)
        && !s.detail.as_deref().is_some_and(|d| d.starts_with("=>"));
    let has_params = s.detail.as_deref().is_some_and(|d| !d.starts_with("()"));
    let (insert, cursor_back) =
        if callable { (format!("{label}()"), usize::from(has_params)) } else { (label.clone(), 0) };
    let detail = match s.kind {
        k if k.is_type() => naming.namespace_of(&s.fqn).map(str::to_owned),
        _ => s.detail.clone().map(String::from),
    };
    Some(CompletionItem {
        label,
        kind: kind_of(s),
        detail,
        insert,
        cursor_back,
        score,
        // Types can be imported (the engine decides if needed); members show their visibility.
        import: (s.kind.is_type().then(|| s.fqn.clone())).map(String::from),
        visibility: s.kind.is_member().then_some(s.visibility),
        ..Default::default()
    })
}

/// A suggestion for a member the language gives a type itself: after the type's own and
/// its libraries' (a mixin's members, a built-in ancestor's).
fn builtin_item(
    s: &Symbol,
    static_access: bool,
    query: &str,
    naming: &Naming,
) -> Option<CompletionItem> {
    let mut found = item(s, static_access, query, Path::new(""), naming)?;
    found.score -= 16;
    Some(found)
}

/// The member is accessible from the current position: public always; protected from inside
/// the class (or via `$this`); private only from the declaring class.
fn accessible(s: &Symbol, current: Option<&str>, receiver_is_self: bool) -> bool {
    match s.visibility {
        Visibility::Public => true,
        Visibility::Protected => current.is_some() && receiver_is_self,
        Visibility::Private => current
            .zip(s.container.as_deref())
            .is_some_and(|(c, owner)| c.eq_ignore_ascii_case(owner)),
    }
}

/// The type must be imported in the file: it's in another namespace and not yet imported (nor
/// is its short name taken by another import).
fn needs_import(file: &FileSymbols, fqn: &str, naming: &Naming) -> bool {
    // A global class (`Exception`) needs one only inside a namespace.
    let (namespace, short) = (naming.namespace_of(fqn).unwrap_or(""), naming.short_name(fqn));
    if namespace.is_empty() && file.namespace.is_none() {
        return false;
    }
    if file.namespace.as_deref().is_some_and(|ns| ns.eq_ignore_ascii_case(namespace)) {
        return false;
    }
    !file
        .uses
        .iter()
        .any(|u| u.fqn.eq_ignore_ascii_case(fqn) || u.alias.eq_ignore_ascii_case(short))
}

fn finish(mut items: Vec<CompletionItem>, limit: usize) -> Vec<CompletionItem> {
    items.sort_by(|a, b| {
        b.score.cmp(&a.score).then(a.label.len().cmp(&b.label.len())).then(a.label.cmp(&b.label))
    });
    // Same name, kind and import: the same suggestion (classes with the same short name in
    // different namespaces all stay).
    items.dedup_by(|a, b| a.label == b.label && a.kind == b.kind && a.import == b.import);
    items.truncate(limit);
    items
}

/// The class of the object to the left of `->`.
fn receiver_class(
    receiver: &Receiver,
    file: &FileSymbols,
    offset: usize,
    naming: &Naming,
) -> Option<String> {
    match receiver {
        Receiver::This => (file.type_at(offset).map(|t| t.fqn.clone())).map(String::from),
        Receiver::Class(name) => Some(file.resolve_class(name, offset, naming)),
        Receiver::Unknown => None,
    }
}

/// What completion at one position works with: the language, the index, the file and the
/// prefix being completed (`query`).
struct Completer<'a, 'p> {
    intelligence: &'a dyn LanguageIntelligence,
    index: &'a SymbolIndex,
    path: &'a Path,
    text: &'a str,
    offset: usize,
    project: &'a Project<'p>,
    file: &'a FileSymbols,
    query: String,
    language: LanguageId,
    naming: Naming,
}

impl Completer<'_, '_> {
    /// The suggestions for the context's kind.
    fn items(&self, kind: &ContextKind) -> Vec<CompletionItem> {
        match kind {
            ContextKind::None | ContextKind::Vocabulary(_) => Vec::new(),
            ContextKind::Member(receiver) => self.members(receiver),
            ContextKind::Methods(receiver) => self
                .members(receiver)
                .into_iter()
                .filter(|i| i.kind == CompletionKind::Method)
                .map(|i| CompletionItem { insert: i.label.clone(), cursor_back: 0, ..i })
                .collect(),
            ContextKind::Static(class) => self.statics(class),
            ContextKind::Variable => self.variables(),
            ContextKind::Name | ContextKind::TypeName if self.query.is_empty() => self.namespaces(),
            ContextKind::TypeName => {
                let mut items = self.type_names();
                items.extend(self.namespaces());
                items
            }
            ContextKind::Name => {
                let mut items = self.names();
                items.extend(self.namespaces());
                items
            }
            ContextKind::Symbols(kinds) => self.symbols(kinds),
            ContextKind::Path(dir) => self.paths(dir),
        }
    }

    /// The members after `->`: those of the receiver's class, accessible from here (its
    /// type as the language works it out knowing the project, else the context's).
    fn members(&self, receiver: &Receiver) -> Vec<CompletionItem> {
        let (file, offset, query) = (self.file, self.offset, &self.query);
        let types = crate::Types { index: self.index, path: self.path };
        // Every class it can be (`$flag ? new A() : new B()`: both), each member once.
        let mut classes = self.intelligence.receiver_types(self.text, offset, &types);
        if classes.is_empty() {
            classes.extend(receiver_class(receiver, file, offset, &self.naming));
        }
        match classes.is_empty() {
            false => {
                let current = file.type_at(offset).map(|t| &*t.fqn);
                let mut seen = std::collections::HashSet::new();
                let mut items = Vec::new();
                for class in &classes {
                    let own = *receiver == Receiver::This
                        || current.is_some_and(|c| c.eq_ignore_ascii_case(class));
                    let instance = |s: &Symbol| {
                        accessible(s, current, own)
                            && (!s.is_static || s.kind == SymbolKind::Method)
                            && s.kind != SymbolKind::ClassConstant
                            && s.kind != SymbolKind::EnumCase
                    };
                    for item in self.class_members(class, instance) {
                        if seen.insert((item.label.clone(), item.kind)) {
                            items.push(item);
                        }
                    }
                }
                items
            }
            // Unknown type: methods and properties with a matching name, from the whole index
            // (a library file looked at only if one of its members' names matches).
            true if !query.is_empty() => {
                let mut items = Vec::new();
                let score = |name: &str| fuzzy_score(query, name);
                let language = self.language.as_str();
                let version = self.index.library_version();
                let within = narrowed(Narrowing::Members, version, language, query);
                // Twice the suggestions shown: room for the names whose members aren't
                // methods or properties, or of another language.
                let taken = self.index.for_each_best_member(
                    &score,
                    LIMIT * 2,
                    within.as_deref(),
                    |p, s| {
                        if matches!(s.kind, SymbolKind::Method | SymbolKind::Property)
                            && in_language(self.index, p, language)
                            && let Some(mut i) = item(s, false, query, p, &self.naming)
                        {
                            i.score -= 10;
                            items.push(i);
                        }
                    },
                );
                keep_narrowed(Narrowing::Members, version, language, query, taken);
                items
            }
            true => Vec::new(),
        }
    }

    /// The suggestions for `class`'s members `keep` accepts: the index's (inherited ones
    /// included), then those its language provides itself (built-in types, an enum's own),
    /// each name once.
    fn class_members(&self, class: &str, keep: impl Fn(&Symbol) -> bool) -> Vec<CompletionItem> {
        let static_access = false;
        let members = self.index.members_of(class);
        let mut seen: std::collections::HashSet<(String, SymbolKind)> =
            members.iter().map(|(_, s)| (s.name.to_lowercase(), s.kind)).collect();
        let mut items: Vec<CompletionItem> = members
            .into_iter()
            .filter(|(_, s)| keep(s))
            .filter_map(|(p, s)| item(s, static_access, &self.query, p, &self.naming))
            .collect();
        let types = crate::Types { index: self.index, path: self.path };
        for s in self.intelligence.builtin_members(class, &types) {
            if keep(&s) && seen.insert((s.name.to_lowercase(), s.kind)) {
                items.extend(builtin_item(&s, static_access, &self.query, &self.naming));
            }
        }
        items
    }

    /// The members after `Class::`: static ones, constants and cases, then `class`.
    fn statics(&self, class: &str) -> Vec<CompletionItem> {
        let (index, query) = (self.index, &self.query);
        let class = self.file.resolve_class(class, self.offset, &self.naming);
        let current = self.file.type_at(self.offset).map(|t| t.fqn.clone());
        let own = current.as_deref().is_some_and(|c| {
            c.eq_ignore_ascii_case(&class)
                || index
                    .type_by_fqn(c)
                    .is_some_and(|(_, t)| t.parents.iter().any(|p| p.eq_ignore_ascii_case(&class)))
        });
        let mut items: Vec<CompletionItem> = index
            .members_of(&class)
            .into_iter()
            .filter(|(_, s)| accessible(s, current.as_deref(), own))
            .filter(|(_, s)| {
                s.is_static || matches!(s.kind, SymbolKind::ClassConstant | SymbolKind::EnumCase)
            })
            .filter_map(|(p, s)| item(s, true, query, p, &self.naming))
            .collect();
        // What the language provides itself (an enum's `cases()`, a built-in class's statics).
        let types = crate::Types { index, path: self.path };
        let builtin = self.intelligence.builtin_members(&class, &types);
        for s in builtin.iter().filter(|s| s.is_static || s.kind == SymbolKind::ClassConstant) {
            if !items.iter().any(|i| i.label.eq_ignore_ascii_case(&s.name)) {
                items.extend(builtin_item(s, true, query, &self.naming));
            }
        }
        if let Some(keyword) = self.naming.type_name_member
            && let Some(score) = fuzzy_score(query, keyword)
        {
            items.push(CompletionItem {
                label: keyword.into(),
                kind: CompletionKind::Keyword,
                detail: Some(class),
                insert: keyword.into(),
                cursor_back: 0,
                score: score - 5,
                ..Default::default()
            });
        }
        items
    }

    /// The variables in scope (and in the included files), then the receiver (`$this`), each
    /// written with the language's sigil.
    fn variables(&self) -> Vec<CompletionItem> {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        let mut names = intelligence.locals(self.text, self.offset);
        // Variables defined in included files (`include 'config.php'`).
        for include in intelligence.includes(self.text) {
            if let Some(file) = project.resolve_include(self.path, &include)
                && let Some(source) = (project.read)(&file)
            {
                for name in intelligence.locals(&source, source.len()) {
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
            }
        }
        let sigil = self.naming.sigil.map(String::from).unwrap_or_default();
        if let Some(receiver) = self.naming.receiver {
            names.push(receiver.trim_start_matches(sigil.as_str()).to_owned());
        }
        names
            .into_iter()
            .filter(|n| n != query)
            .filter_map(|n| {
                let score = fuzzy_score(query, &n)?;
                Some(CompletionItem {
                    label: format!("{sigil}{n}"),
                    kind: CompletionKind::Variable,
                    detail: None,
                    insert: format!("{sigil}{n}"),
                    cursor_back: 0,
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The namespace written before the name being typed (`App\Models` of `App\Models\Us|`,
    /// a leading separator left out), if any.
    fn qualifier(&self) -> Option<&str> {
        let separator = self.naming.namespace_separator?;
        let start = self.offset - self.query.len();
        let head = self.text.get(..start)?.strip_suffix(separator)?;
        let from = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == separator)
            .last()
            .map_or(head.len(), |(i, _)| i);
        let qualifier = head[from..].trim_start_matches(separator);
        (!qualifier.is_empty()).then_some(qualifier)
    }

    /// After a namespace and its separator (`use App\Mo|`): the namespaces in it the project
    /// declares things in, inserted with their separator (the next part comes right after).
    fn namespaces(&self) -> Vec<CompletionItem> {
        let (Some(qualifier), Some(separator)) =
            (self.qualifier(), self.naming.namespace_separator)
        else {
            return Vec::new();
        };
        let prefix = format!("{qualifier}{separator}").to_lowercase();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut items = Vec::new();
        for (_, s) in self.index.top_level_where(|f| of_language(f, self.language.as_str())) {
            let Some(rest) = s.fqn.get(prefix.len()..) else { continue };
            if !s.fqn[..prefix.len()].eq_ignore_ascii_case(&prefix) {
                continue;
            }
            let Some((segment, _)) = rest.split_once(separator) else { continue };
            if segment.is_empty() || !seen.insert(segment.to_lowercase()) {
                continue;
            }
            let Some(score) = fuzzy_score(&self.query, segment) else { continue };
            items.push(CompletionItem {
                label: segment.to_owned(),
                kind: CompletionKind::Namespace,
                detail: Some(format!("{qualifier}{separator}{segment}")),
                insert: format!("{segment}{separator}"),
                score: score + 1,
                ..Default::default()
            });
        }
        items
    }

    /// Where only a type fits: the language's types.
    fn type_names(&self) -> Vec<CompletionItem> {
        self.best_items(Narrowing::Types, |s| s.kind.is_type())
    }

    /// The language's types and functions `keep` takes that can be among the suggestions
    /// shown: each scored first (its name, as [`item`] scores it, its file's being a
    /// dependency's known with the index), only the best made into suggestions — not every
    /// type and function of a project and its libraries (tens of thousands match a letter)
    /// made, copied and sorted at each key. A few times the most shown: room for those the
    /// list then merges (the same name and kind).
    fn best_items(&self, what: Narrowing, keep: impl Fn(&Symbol) -> bool) -> Vec<CompletionItem> {
        let (index, language) = (self.index, self.language.as_str());
        let query = self.query.as_str();
        // The project's, all of them (they change between keys: the file typed in).
        let mut scored: Vec<(i32, &Path, &Symbol)> = index
            .project_top_level(|f| of_language(f, language))
            .filter(|(_, s)| keep(s))
            .filter_map(|(p, s)| {
                let score = fuzzy_score(query, &s.name)?;
                Some((if is_vendor(p) { score - 15 } else { score }, p, s))
            })
            .collect();
        // The libraries': those the word before this one matched, when it goes on from it.
        let version = index.library_version();
        let within = narrowed(what, version, language, query);
        let mut places = Vec::new();
        let mut take = |place: u32| {
            let (p, s, dependency, file) = index.library_top_at(place)?;
            if !of_language(file, language) || !keep(s) {
                return None;
            }
            let score = fuzzy_score(query, &s.name)?;
            places.push(place);
            Some((if dependency { score - 15 } else { score }, p, s))
        };
        match &within {
            Some(within) => scored.extend(within.iter().filter_map(|&place| take(place))),
            None => scored.extend((0..index.library_top_count() as u32).filter_map(take)),
        }
        keep_narrowed(what, version, language, query, places);
        let most = LIMIT * 3;
        if scored.len() > most {
            scored.select_nth_unstable_by(most, |a, b| {
                (b.0)
                    .cmp(&a.0)
                    .then(a.2.name.len().cmp(&b.2.name.len()))
                    .then(a.2.name.cmp(&b.2.name))
            });
            scored.truncate(most);
        }
        scored
            .into_iter()
            .filter_map(|(_, p, s)| item(s, false, &self.query, p, &self.naming))
            .collect()
    }

    /// A name: types, functions, constants and globals, then the keywords.
    fn names(&self) -> Vec<CompletionItem> {
        let mut items = self.best_items(Narrowing::Names, |s| {
            s.kind.is_type()
                || matches!(
                    s.kind,
                    SymbolKind::Function | SymbolKind::Constant | SymbolKind::Variable
                )
        });
        items.extend(self.keywords());
        items
    }

    /// The language's keywords available at the project's level.
    fn keywords(&self) -> Vec<CompletionItem> {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        intelligence
            .keywords_for(self.path)
            .iter()
            .filter(|k| project.available(intelligence, k))
            .filter_map(|k| {
                let score = fuzzy_score(query, k)?;
                let (insert, cursor_back) = keyword_insert(k, intelligence.keyword_insert(k));
                Some(CompletionItem {
                    label: (*k).into(),
                    kind: CompletionKind::Keyword,
                    detail: None,
                    insert,
                    cursor_back,
                    // A capitalized prefix is almost certainly looking for a class.
                    score: if query.starts_with(char::is_uppercase) {
                        score - 12
                    } else {
                        score + 2
                    },
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The declarations of the given kinds (selectors, mixins…).
    fn symbols(&self, kinds: &[SymbolKind]) -> Vec<CompletionItem> {
        self.index
            .top_level()
            .filter(|(_, s)| kinds.contains(&s.kind))
            .filter_map(|(p, s)| item(s, false, &self.query, p, &self.naming))
            .collect()
    }

    /// The entries of directory `dir` (a path being written).
    fn paths(&self, dir: &str) -> Vec<CompletionItem> {
        let query = &self.query;
        self.project
            .entries(self.path, dir)
            .into_iter()
            .filter(|(name, _)| !name.starts_with('.') || query.starts_with('.'))
            .filter_map(|(name, is_dir)| {
                let score = fuzzy_score(query, &name)? + if is_dir { 2 } else { 0 };
                let insert = if is_dir { format!("{name}/") } else { name };
                Some(CompletionItem {
                    label: insert.clone(),
                    kind: if is_dir { CompletionKind::Selector } else { CompletionKind::Value },
                    detail: Some(i18n::t(if is_dir {
                        "completion.directory"
                    } else {
                        "completion.file"
                    })),
                    insert,
                    cursor_back: 0,
                    score,
                    ..Default::default()
                })
            })
            .collect()
    }

    /// The language's own words for the context (CSS values, HTML attributes…), available at
    /// the project's level and matching the prefix.
    fn vocabulary(&self, context: &CompletionContext) -> impl Iterator<Item = CompletionItem> + '_ {
        let (intelligence, project, query) = (self.intelligence, self.project, &self.query);
        intelligence
            .vocabulary(context, self.text)
            .into_iter()
            .filter(move |w| project.available(intelligence, &w.label))
            .filter_map(move |mut word| {
                word.score += fuzzy_score(query, &word.label)?;
                Some(word)
            })
    }
}

/// The suggestions for position `offset` in `text` (file `path`). Returns the context
/// (what is replaced) and the ordered list.
pub fn complete(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    project: &Project<'_>,
) -> (CompletionContext, Vec<CompletionItem>) {
    let context = intelligence.context(text, offset);
    let empty = FileSymbols::default();
    let file = index.file(path).unwrap_or(&empty);
    let completer = Completer {
        intelligence,
        index,
        path,
        text,
        offset,
        project,
        file,
        query: context.prefix.clone(),
        language: intelligence.language(),
        naming: intelligence.naming(),
    };
    let mut items = completer.items(&context.kind);
    if !matches!(context.kind, ContextKind::None | ContextKind::Methods(_)) {
        items.extend(completer.vocabulary(&context));
        // Where an argument starts: the callee's parameters, by name.
        let named = crate::signature::named_arguments(
            intelligence,
            index,
            path,
            text,
            context.replace.clone(),
            project,
        );
        items.extend(named);
    }
    // Imports: only for types from another namespace, not yet imported.
    for item in &mut items {
        if item.import.as_deref().is_some_and(|fqn| !needs_import(file, fqn, &completer.naming)) {
            item.import = None;
        }
    }
    let limit = match context.kind {
        ContextKind::Member(_) | ContextKind::Static(_) | ContextKind::Methods(_) => MEMBER_LIMIT,
        _ => LIMIT,
    };
    (context, finish(items, limit))
}

/// What is inserted for a keyword (as its language says): with a space after (`function `,
/// `return `), like calls (`isset()`) or exactly (`true`).
fn keyword_insert(keyword: &str, insert: crate::KeywordInsert) -> (String, usize) {
    match insert {
        crate::KeywordInsert::Call => (format!("{keyword}()"), 1),
        crate::KeywordInsert::Exact => (keyword.to_owned(), 0),
        crate::KeywordInsert::Spaced => (format!("{keyword} "), 0),
    }
}

/// Where what is under the cursor is declared. Multiple results when the name is ambiguous (the
/// project before libraries).
pub fn definition(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    project: &Project<'_>,
) -> Vec<Location> {
    let here = |at: usize| {
        let line = kernel::text::line_of(text, at);
        vec![Location { path: path.to_path_buf(), offset: at, line }]
    };
    let target = match intelligence.comment_place(text, offset) {
        // In a comment: only what's written there as code leads somewhere.
        Some(crate::CommentPlace::Text) => return Vec::new(),
        Some(crate::CommentPlace::Declared(at)) => return here(at),
        Some(crate::CommentPlace::Code(target)) => target,
        None => {
            // Declared in this text, in an enclosing scope: that declaration wins (a local
            // variable has no target of its own: PHP's `$x`).
            if let Some(at) = intelligence.local_definition(text, offset) {
                return here(at);
            }
            let Some(target) = intelligence.target(text, offset) else { return Vec::new() };
            target
        }
    };
    definition_of(intelligence, index, path, text, offset, &target, project)
}

/// Where `target`, named at `offset` of `text`, is declared, as `intelligence`'s language
/// finds it: what [`definition`] does once it knows what's there, also for a name a module
/// found in another language's text (a PHP class in a Twig comment).
pub fn definition_of(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    target: &Target,
    project: &Project<'_>,
) -> Vec<Location> {
    let empty = FileSymbols::default();
    let file = index.file(path).unwrap_or(&empty);
    let naming = intelligence.naming();
    let called = called_at(text, offset, intelligence.word_chars());
    // A class, a function or a member found only by its short name: this language's (a
    // script's `window.addEventListener` is never a PHP class's method of that name).
    let own = intelligence.language();
    let language = Some(own.as_str());
    let mut found: Vec<Location> = match target {
        Target::Class(name) => {
            let fqn = file.resolve_class(name, offset, &naming);
            // Written whole (`\App\Shop`, PHP's strings name classes so): that class or
            // nothing — no other one by its short name; a global one (`\Cart`) by its case
            // too (`'cart'` isn't it).
            let absolute = naming.absolute(name).is_some();
            let global = absolute && naming.namespace_separator.is_none_or(|s| !fqn.contains(s));
            match index.type_by_fqn(&fqn).filter(|(_, s)| !global || *s.fqn == *fqn) {
                Some((p, s)) => vec![location(p, s)],
                None if global => Vec::new(),
                None => {
                    // The autoloader (e.g. PSR-4): the file in which the class should be.
                    let located: Vec<Location> = intelligence
                        .locate(project, &fqn)
                        .into_iter()
                        .filter(|p| project.exists(p))
                        .map(|path| Location { path, offset: 0, line: 0 })
                        .collect();
                    if located.is_empty() && !absolute {
                        by_short_name(index, naming.short_name(name), language, |k| k.is_type())
                    } else {
                        located
                    }
                }
            }
        }
        Target::Function(name) => {
            let local = file.namespace.as_ref().zip(naming.namespace_separator);
            let local = local.map(|(ns, separator)| format!("{ns}{separator}{name}"));
            let global = naming.absolute(name).unwrap_or(name);
            local
                .and_then(|fqn| index.function_by_fqn(&fqn))
                .or_else(|| index.function_by_fqn(global))
                .map(|(p, s)| vec![location(p, s)])
                .unwrap_or_else(|| {
                    by_short_name(index, global, language, |k| k == SymbolKind::Function)
                })
        }
        Target::Member { receiver, name } => {
            let types = crate::Types { index, path };
            let owned = intelligence
                .member_owner(text, offset, &types)
                .map(|owner| member(index, &owner, name, &naming, called))
                .filter(|found| !found.is_empty());
            let inferred = || intelligence.receiver_type(text, offset, &types);
            match owned {
                Some(found) => found,
                None => {
                    match inferred().or_else(|| receiver_class(receiver, file, offset, &naming)) {
                        Some(class) => member(index, &class, name, &naming, called),
                        None => by_short_name(index, name, language, SymbolKind::is_member),
                    }
                }
            }
        }
        Target::Static { class, name } => {
            // No class written (PHP's `[$obj, 'name']`): the language's answer alone.
            let found = match class.is_empty() {
                true => Vec::new(),
                false => {
                    let class = file.resolve_class(class, offset, &naming);
                    member(index, &class, name, &naming, called)
                }
            };
            let types = crate::Types { index, path };
            match intelligence.member_owner(text, offset, &types) {
                Some(owner) if found.is_empty() => member(index, &owner, name, &naming, called),
                _ => found,
            }
        }
        // A name of the kinds given, wherever it's declared (a page's `class="…"`: a
        // stylesheet's).
        Target::Named { name, kinds } => by_short_name(index, name, None, |k| kinds.contains(&k)),
        Target::File(include) => project
            .resolve_include(path, include)
            .map(|path| vec![Location { path, offset: 0, line: 0 }])
            .unwrap_or_default(),
    };
    found.sort_by_key(|l| is_vendor(&l.path));
    found
}

/// What is under the mouse, explained: a project declaration (its signature) or, failing
/// that, what the language itself provides (see [`LanguageIntelligence::hover`]).
pub fn hover(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    project: &Project<'_>,
) -> Option<crate::HoverInfo> {
    let word = crate::word_at(text, offset, intelligence.word_chars())?;
    let place = intelligence.comment_place(text, offset);
    if place == Some(crate::CommentPlace::Text) {
        return None;
    }
    let declared =
        definition(intelligence, index, path, text, offset, project).into_iter().find_map(|l| {
            let file = index.file(&l.path)?;
            let symbol = file.symbols.iter().find(|s| s.offset == l.offset).cloned()?;
            Some((l, symbol))
        });
    if let Some((at, symbol)) = declared {
        return Some(describe(intelligence, index, (path, text), word, &at, &symbol, project));
    }
    // In a comment, no variable's type: only what the language itself has (a built-in class).
    if place.is_some() {
        return intelligence.hover(text, offset);
    }
    let types = crate::Types { index, path };
    intelligence.typed_hover(text, offset, &types).or_else(|| intelligence.hover(text, offset))
}

/// The hover card of `target`, named at `offset` of `text` (a module found it there: a PHP
/// class in a Twig comment): its declaration, as `intelligence`'s language finds it.
pub fn hover_of(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    path: &Path,
    text: &str,
    offset: usize,
    target: &Target,
    project: &Project<'_>,
) -> Option<crate::HoverInfo> {
    let word = crate::word_at(text, offset, intelligence.word_chars())?;
    let found = definition_of(intelligence, index, path, text, offset, target, project);
    let (at, symbol) = found.into_iter().find_map(|l| {
        let file = index.file(&l.path)?;
        let symbol = file.symbols.iter().find(|s| s.offset == l.offset).cloned()?;
        Some((l, symbol))
    })?;
    Some(describe(intelligence, index, (path, text), word, &at, &symbol, project))
}

/// A project declaration on the hover card: its signature (a constructor's for
/// `new X(`), the declaring type, the doc comment's summary and parameters, its file.
fn describe(
    intelligence: &dyn LanguageIntelligence,
    index: &SymbolIndex,
    (path, text): (&Path, &str),
    word: std::ops::Range<usize>,
    at: &Location,
    symbol: &Symbol,
    project: &Project<'_>,
) -> crate::HoverInfo {
    // The declaration's language writes its signature: this text's, when it's declared in
    // it (another's, e.g. a script's function in an HTML page, is written plainly).
    let language = index.file(&at.path).map_or("", |f| &*f.language);
    let own = language.is_empty() || language == intelligence.language().as_str();
    let naming = if own { intelligence.naming() } else { Naming::default() };
    // `new X(`: the constructor, under the class.
    let constructing = symbol.kind.is_type() && intelligence.constructs_at(text, word.start);
    let constructor = constructing
        .then(|| {
            // `new` is written in this text: its language names the constructor.
            let naming = intelligence.naming();
            index.members_of(&symbol.fqn).into_iter().find(|(_, s)| naming.is_constructor(&s.name))
        })
        .flatten()
        .map(|(p, s)| {
            (Location { path: p.to_path_buf(), offset: s.offset, line: s.line }, s.clone())
        });
    let read =
        |file: &Path| if file == path { Some(text.to_owned()) } else { (project.read)(file) };
    let class_at = at;
    let (at, shown) = match &constructor {
        Some((l, s)) => (l, s),
        None => (at, symbol),
    };
    let short = |fqn: &str| naming.short_name(fqn).to_owned();
    let namespace = |fqn: &str| naming.namespace_of(fqn).map(str::to_owned);
    // The header: the declaring type (or the type itself), its namespace dimmed.
    let (title, subtitle) = match &shown.container {
        Some(class) => {
            let kind = index.type_by_fqn(class).map_or(CompletionKind::Class, |(_, t)| kind_of(t));
            (Some((short(class), Some(kind))), namespace(class))
        }
        None if shown.kind.is_type() => {
            (Some((shown.name.to_string(), Some(kind_of(shown)))), namespace(&shown.fqn))
        }
        None => (None, None),
    };
    let modifiers = naming.parameter_modifiers;
    let detail = shown.detail.as_deref().map(|d| strip_modifiers(d, modifiers));
    // The declaring file's language reads it (a CSS rule from a stylesheet, hovered in a
    // page); without the project's languages, this text's.
    let language_of = |file: &Path| project.intelligence_for(file).unwrap_or(intelligence);
    // What the declaration says whole, when its language shows it so (a CSS rule).
    let rule = read(&at.path)
        .and_then(|t| language_of(&at.path).declaration_code(&t, at.offset, shown.kind));
    // The declaring file's language writes its signature; one the project doesn't know
    // (none given) is written plainly.
    let code = rule.unwrap_or_else(|| {
        let detail = detail.as_deref();
        match project.intelligence_for(&at.path) {
            Some(declaring) => declaring.signature(shown, detail),
            None if own => intelligence.signature(shown, detail),
            None => signature_plainly(shown, detail),
        }
    });
    let doc_of = |file: &Path, text: &str, offset: usize| {
        let rules = language_of(file).doc_rules()?;
        crate::doc_before(text, offset, &rules)
    };
    let docs = read(&at.path).and_then(|t| doc_of(&at.path, &t, at.offset));
    // A constructor without its own doc: the class's.
    let summary =
        docs.as_ref().map(|d| d.summary.clone()).filter(|s| !s.is_empty()).or_else(|| {
            constructor.as_ref()?;
            let class_text = read(&class_at.path)?;
            doc_of(&class_at.path, &class_text, class_at.offset)
                .map(|d| d.summary)
                .filter(|s| !s.is_empty())
        });
    // A summary starting with the declaration's own name (a common doc convention)
    // repeats the header: that line goes.
    let summary = summary.map(|s| {
        let names = [&*shown.name, &*symbol.name];
        match s.split_once('\n') {
            Some((first, rest)) if names.contains(&first.trim()) => rest.to_owned(),
            None if names.contains(&s.trim()) => String::new(),
            _ => s,
        }
    });
    let callable = matches!(shown.kind, SymbolKind::Function | SymbolKind::Method);
    let params = callable
        .then(|| crate::parameters(&code))
        .flatten()
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let param = &code[r];
            let (ty, name) =
                if own { intelligence.split_param(param) } else { split_param_plainly(param) };
            let doc = docs.as_ref().and_then(|d| d.param(&name)).map(str::to_owned);
            // No type in the code (JavaScript): the doc comment's.
            let ty = ty.or_else(|| docs.as_ref()?.param_type(&name).map(str::to_owned));
            crate::HoverParam { ty, name, doc }
        })
        .collect();
    // Deprecated: its doc comment says so, or what its language writes on it.
    let deprecated = docs.as_ref().is_some_and(|d| d.deprecated)
        || language_of(&at.path).is_deprecated(shown)
        || (own && intelligence.is_deprecated(shown));
    let notes = if deprecated { vec![crate::HoverNote::Deprecated] } else { Vec::new() };
    // `new X(`: the class's card, its constructor's signature in it.
    let code = match &constructor {
        Some(_) => {
            let class = match project.intelligence_for(&class_at.path) {
                Some(declaring) => declaring.signature(symbol, None),
                None => signature_plainly(symbol, None),
            };
            format!("{class}\n{code}")
        }
        None => code,
    };
    let source = project
        .root
        .and_then(|root| at.path.strip_prefix(root).ok())
        .unwrap_or(&at.path)
        .display()
        .to_string();
    crate::HoverInfo {
        range: word,
        code,
        notes,
        doc: summary.filter(|s| !s.is_empty()).map(|s| shorten(&s, 600)),
        title,
        subtitle,
        params,
        params_label: None,
        source: Some(source),
    }
}

/// A declaration's signature on one line as most languages write it: `name(…)` for a
/// method, `function name(…)`, `class Name`, `const NAME` (see
/// [`LanguageIntelligence::signature`]).
pub fn signature_plainly(s: &Symbol, detail: Option<&str>) -> String {
    let detail = detail.unwrap_or("");
    match s.kind {
        SymbolKind::Method => format!("{}{detail}", s.name),
        SymbolKind::Function if detail.starts_with("=>") => format!("{} {detail}", s.name),
        SymbolKind::Function => format!("function {}{detail}", s.name),
        SymbolKind::Class => format!("class {}", s.name),
        SymbolKind::Interface => format!("interface {}", s.name),
        SymbolKind::Trait => format!("trait {}", s.name),
        SymbolKind::Enum => format!("enum {}", s.name),
        // With its value, when the declaration's language gave it (` = 50`).
        SymbolKind::Constant | SymbolKind::ClassConstant => {
            format!("const {}{}", s.name, value_of(detail))
        }
        SymbolKind::EnumCase => format!("case {}{}", s.name, value_of(detail)),
        _ => format!("{}{detail}", crate::label(s, None)),
    }
}

/// A constant's value as a symbol's detail gives it (` = 50`); anything else: nothing.
fn value_of(detail: &str) -> &str {
    if detail.starts_with(" = ") { detail } else { "" }
}

/// `(public Device $device, private readonly bool $x = true)` → without the parameters'
/// `modifiers` (PHP's promotion: `Device $device, bool $x = true`).
fn strip_modifiers(detail: &str, modifiers: &[&str]) -> String {
    let mut out = String::with_capacity(detail.len());
    let mut rest = detail;
    while !rest.is_empty() {
        let at_word_start = out.is_empty() || out.ends_with(['(', ' ', ',']);
        let modifier = modifiers
            .iter()
            .find(|m| rest.strip_prefix(**m).is_some_and(|after| after.starts_with(' ')));
        if at_word_start && let Some(m) = modifier {
            rest = &rest[m.len() + 1..];
            continue;
        }
        let c = rest.chars().next().unwrap_or(' ');
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A parameter's type and name as most languages write it: `name: Type = 1` → (`Type`,
/// `name`) (see [`LanguageIntelligence::split_param`]).
pub fn split_param_plainly(param: &str) -> (Option<String>, String) {
    let param = param.split_once(" = ").map_or(param, |(p, _)| p).trim();
    match param.split_once(':') {
        Some((name, ty)) => (Some(ty.trim().to_owned()), name.trim().to_owned()),
        None => (None, param.to_owned()),
    }
}

/// At most `max` characters, cut at a word.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max).collect();
    format!("{}…", cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head))
}

/// The member `name` of `class`: a method where it's `called` (`->cart(`), else a property,
/// a constant or a case (`->cart`) — a class may have both, by the same name — or, failing
/// that, whichever it has.
fn member(
    index: &SymbolIndex,
    class: &str,
    name: &str,
    naming: &Naming,
    called: bool,
) -> Vec<Location> {
    let name = naming.sigil.map_or(name, |s| name.trim_start_matches(s));
    let members: Vec<_> = index
        .members_of(class)
        .into_iter()
        .filter(|(_, s)| s.name.eq_ignore_ascii_case(name))
        .collect();
    let fits = |s: &Symbol| (s.kind == SymbolKind::Method) == called;
    members
        .iter()
        .find(|(_, s)| fits(s))
        .or_else(|| members.first())
        .map(|(p, s)| location(p, s))
        .into_iter()
        .collect()
}

/// Whether the name at `offset` is called (`(` after it), or named in quotes as a callable
/// (`[$this, 'cart']`: only a callable's string reaches a member target).
fn called_at(text: &str, offset: usize, word_chars: &str) -> bool {
    crate::word_at(text, offset, word_chars).is_some_and(|w| {
        let after = &text[w.end..];
        after.trim_start().starts_with('(') || after.starts_with(['\'', '"'])
    })
}

/// The symbols named `name` of a kind `kind` takes, in `language`'s files (and those of
/// none) when one's given.
fn by_short_name(
    index: &SymbolIndex,
    name: &str,
    language: Option<&str>,
    kind: impl Fn(SymbolKind) -> bool,
) -> Vec<Location> {
    index
        .by_name(name)
        .into_iter()
        .filter(|(_, s)| kind(s.kind))
        .filter(|(p, _)| language.is_none_or(|l| in_language(index, p, l)))
        .map(|(p, s)| location(p, s))
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/engine.rs"]
mod tests;
