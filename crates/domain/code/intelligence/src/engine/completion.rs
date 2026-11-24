//! Completion at a position: the context the language gives, the suggestions made for it
//! (a symbol's, a type's own member's, a keyword's insert), ordered and cut, a word's
//! matches among the libraries kept for the next key.

use super::completer::Completer;
use super::{Project, is_vendor, kind_of};
use crate::CompletionContext;
use crate::CompletionItem;
use crate::ContextKind;
use crate::LanguageIntelligence;
use crate::Naming;
use crate::Receiver;
use crate::fuzzy::fuzzy_score;
use crate::index::SymbolIndex;
use crate::symbols::FileSymbols;
use crate::symbols::Symbol;
use crate::symbols::SymbolKind;
use crate::symbols::Visibility;
use std::path::Path;

/// At most how many suggestions are returned.
pub(super) const LIMIT: usize = 80;
/// At most how many of a type's members are returned (after `->` / `::`): a type is offered
/// whole, scrolling through its members being the point (a query builder has hundreds).
const MEMBER_LIMIT: usize = 400;

/// What a word's matches among the libraries' lists are kept for (each its own: they're
/// places in different lists, or the same list with another filter).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Narrowing {
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
    pub(super) static NARROWED: std::cell::RefCell<Vec<Narrowed>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The places to look among for `query`: those the word before it matched, when `query`
/// goes on from it (the same libraries, language and list); `None`: all of them.
pub(super) fn narrowed(
    what: Narrowing,
    version: u64,
    language: &str,
    query: &str,
) -> Option<Vec<u32>> {
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
pub(super) fn keep_narrowed(
    what: Narrowing,
    version: u64,
    language: &str,
    query: &str,
    places: Vec<u32>,
) {
    let query = query.to_lowercase();
    NARROWED.with_borrow_mut(|kept| {
        kept.retain(|(w, ..)| *w != what);
        kept.push((what, version, language.to_owned(), query, places));
    });
}

/// A suggestion for a symbol: methods and functions get `()`, with the cursor between
/// the parens if they have parameters.
pub(super) fn item(
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
pub(super) fn builtin_item(
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
pub(super) fn accessible(s: &Symbol, current: Option<&str>, receiver_is_self: bool) -> bool {
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
pub(super) fn receiver_class(
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
pub(super) fn keyword_insert(keyword: &str, insert: crate::KeywordInsert) -> (String, usize) {
    match insert {
        crate::KeywordInsert::Call => (format!("{keyword}()"), 1),
        crate::KeywordInsert::Exact => (keyword.to_owned(), 0),
        crate::KeywordInsert::Spaced => (format!("{keyword} "), 0),
    }
}
