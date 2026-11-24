//! "Go to definition": where what's under the cursor is declared (the project before the
//! libraries), a class's member by how it's named.

use super::completion::receiver_class;
use super::{Project, in_language, is_vendor, location};
use crate::LanguageIntelligence;
use crate::Naming;
use crate::Target;
use crate::index::Location;
use crate::index::SymbolIndex;
use crate::symbols::FileSymbols;
use crate::symbols::Symbol;
use crate::symbols::SymbolKind;
use std::path::Path;

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
