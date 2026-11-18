//! What a rename at the cursor renames: a declaration there, the declaration of what's used
//! there, or a local variable (declared in the file but not in the index, or — in a language
//! declaring variables by assigning them — any name with its sigil, scoped to its function).
//! Pure: the index, the language's naming and the definition found are given.

use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::{Location, Naming, SymbolIndex, SymbolKind};

/// Where the renamed symbol is declared, and what kind it is.
#[derive(Debug, Clone)]
pub struct Declared {
    pub at: Location,
    /// `None` for a local variable (not in the index).
    pub kind: Option<SymbolKind>,
    /// Its full name (`App\Models\User`, a member's own name).
    pub fqn: String,
    /// The type containing a member.
    pub container: Option<Arc<str>>,
}

/// The symbol a rename at the cursor renames.
#[derive(Debug, Clone)]
pub struct Subject {
    pub name: String,
    pub declared: Declared,
    /// A local variable's scope (the enclosing function, or the file).
    pub local: Option<Range<usize>>,
    /// The language of its declaring file.
    pub language: Arc<str>,
}

/// The name at `offset` of `text`: letters, digits, `_` and `extra` (its kind's own:
/// [`SymbolKind::name_chars`](crate::SymbolKind::name_chars), `-` for CSS's), with the
/// language's `sigil` before it (`$total`, the cursor on the `$` too) — not for a kind with
/// characters of its own.
pub fn name_at(
    text: &str,
    offset: usize,
    extra: &str,
    sigil: Option<char>,
) -> Option<Range<usize>> {
    let is_name = |c: char| c.is_alphanumeric() || c == '_' || extra.contains(c);
    let offset = offset.min(text.len());
    if !text.is_char_boundary(offset) {
        return None;
    }
    let mut start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = offset + text[offset..].find(|c: char| !is_name(c)).unwrap_or(text.len() - offset);
    let Some(sigil) = sigil.filter(|_| extra.is_empty()) else {
        return (start < end).then_some(start..end);
    };
    // The cursor on a variable's sigil.
    if start == end && text[offset..].starts_with(sigil) {
        let rest = &text[offset + sigil.len_utf8()..];
        let len = rest.find(|c: char| !is_name(c)).unwrap_or(rest.len());
        return (len > 0).then_some(offset..offset + sigil.len_utf8() + len);
    }
    if text[..start].ends_with(sigil) {
        start -= sigil.len_utf8();
    }
    (start < end).then_some(start..end)
}

/// A local variable's scope: the function around `head`, else the whole file.
pub fn local_scope(index: &SymbolIndex, path: &Path, source: &str, head: usize) -> Range<usize> {
    let function = index.file(path).and_then(|f| f.function_at(head));
    function.map_or(0..source.len(), |s| s.range.clone())
}

/// The symbol at `head` of `path` (its text `source`, its language `language` when it isn't
/// indexed) that a rename renames. `definition`: where what's at `head` is declared (the
/// navigation's answer); `naming_of`: a language's naming.
pub fn subject_at(
    index: &SymbolIndex,
    path: &Path,
    source: &str,
    head: usize,
    language: Arc<str>,
    definition: Option<Location>,
    naming_of: &dyn Fn(&str) -> Naming,
) -> Option<Subject> {
    let file = index.file(path);
    let language = file.map_or(language, |f| f.language.clone());
    let naming = naming_of(&language);
    // On a declaration's name (a property's may start at its sigil, `$`).
    let declaration = file.and_then(|f| {
        f.symbols.iter().find(|s| {
            let Some(at) = source.get(s.offset..) else { return false };
            let sigil = naming.sigil.filter(|&c| at.starts_with(c)).map_or(0, char::len_utf8);
            s.offset <= head
                && head <= s.offset + sigil + s.name.len()
                && at[sigil..].starts_with(&*s.name)
        })
    });
    if let Some(s) = declaration {
        return Some(Subject {
            name: s.name.to_string(),
            declared: Declared {
                at: Location { path: path.to_path_buf(), offset: s.offset, line: s.line },
                kind: Some(s.kind),
                fqn: s.fqn.to_string(),
                container: s.container.clone(),
            },
            local: None,
            language,
        });
    }
    let word = name_at(source, head, "", naming.sigil)?;
    let name = &source[word.clone()];
    // The object a method runs on (`$this`) is never renamed.
    if naming.receiver == Some(name) {
        return None;
    }
    if let Some(at) = definition {
        let symbol = index.file(&at.path).and_then(|f| {
            f.symbols
                .iter()
                .find(|s| s.offset == at.offset)
                .map(|s| (s.clone(), f.language.clone()))
        });
        if let Some((symbol, language)) = symbol {
            return Some(Subject {
                name: symbol.name.to_string(),
                declared: Declared {
                    at,
                    kind: Some(symbol.kind),
                    fqn: symbol.fqn.to_string(),
                    container: symbol.container,
                },
                local: None,
                language,
            });
        }
        // Declared in this file, not in the index: a local variable / parameter.
        if at.path == path {
            return Some(Subject {
                name: name.to_owned(),
                declared: Declared { at, kind: None, fqn: name.to_owned(), container: None },
                local: Some(local_scope(index, path, source, head)),
                language,
            });
        }
        return None;
    }
    // A variable declared by assigning it (PHP's `$x`): its function is its scope.
    if naming.implicit_locals && naming.sigil.is_some_and(|c| name.starts_with(c)) {
        let line = kernel::text::line_of(source, word.start);
        return Some(Subject {
            name: name.to_owned(),
            declared: Declared {
                at: Location { path: path.to_path_buf(), offset: word.start, line },
                kind: None,
                fqn: name.to_owned(),
                container: None,
            },
            local: Some(local_scope(index, path, source, head)),
            language,
        });
    }
    None
}

/// How sure a place found by name is to be the renamed symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Target,
    /// The user decides (not found, or one of several with the name).
    Unknown,
    Other,
}

impl Declared {
    /// `location` is this declaration.
    pub fn is(&self, location: &Location) -> bool {
        location.path == self.at.path
            && (location.offset == self.at.offset || location.line == self.at.line)
    }

    /// `range` of `path` is the declaration itself (a property's name may start after the
    /// language's `sigil`).
    pub fn declares(&self, path: &Path, range: &Range<usize>, sigil: Option<char>) -> bool {
        let after_sigil = sigil.map_or(0, char::len_utf8);
        path == self.at.path
            && (range.contains(&self.at.offset) || range.start == self.at.offset + after_sigil)
    }

    /// A place whose name the navigation took to `found` (its declarations): the target when
    /// all are this one, unsure when none or only some are.
    pub fn by_definitions(&self, found: &[Location]) -> Match {
        let targets = found.iter().filter(|l| self.is(l)).count();
        if found.is_empty() || (targets > 0 && targets < found.len()) {
            Match::Unknown
        } else if targets > 0 {
            Match::Target
        } else {
            Match::Other
        }
    }
}

impl Declared {
    /// [`by_definitions`](Self::by_definitions), with the same method declared in the
    /// target's ancestors or descendants (the abstract method it implements, an override)
    /// counting as the target: renaming one renames the family.
    pub fn by_definitions_in(&self, found: &[Location], index: &SymbolIndex) -> Match {
        let family = |l: &Location| self.is(l) || self.overrides(l, index);
        let targets = found.iter().filter(|l| family(l)).count();
        if found.is_empty() || (targets > 0 && targets < found.len()) {
            Match::Unknown
        } else if targets > 0 {
            Match::Target
        } else {
            Match::Other
        }
    }

    /// Whether `location` declares this method in a type related to its own (an ancestor
    /// or a descendant).
    fn overrides(&self, location: &Location, index: &SymbolIndex) -> bool {
        if self.kind != Some(SymbolKind::Method) {
            return false;
        }
        let Some(own) = self.container.as_deref() else { return false };
        let Some(symbol) = index
            .file(&location.path)
            .and_then(|f| f.symbols.iter().find(|s| s.offset == location.offset))
        else {
            return false;
        };
        let Some(other) = symbol.container.as_deref() else { return false };
        symbol.kind == SymbolKind::Method
            && symbol.name.eq_ignore_ascii_case(&self.fqn)
            && !other.eq_ignore_ascii_case(own)
            && (descends(index, own, other) || descends(index, other, own))
    }
}

/// Whether type `class` has `ancestor` among its parents (any depth, at most 32 types).
fn descends(index: &SymbolIndex, class: &str, ancestor: &str) -> bool {
    let mut queue = vec![class.to_owned()];
    let mut seen: Vec<String> = Vec::new();
    while let Some(current) = queue.pop() {
        let key = current.to_lowercase();
        if seen.contains(&key) || seen.len() > 32 {
            continue;
        }
        seen.push(key);
        let Some((_, ty)) = index.type_by_fqn(&current) else { continue };
        for parent in &ty.parents {
            if parent.eq_ignore_ascii_case(ancestor) {
                return true;
            }
            queue.push(parent.to_string());
        }
    }
    false
}

/// What a declaration's new name `new_name` would clash with in `index` (a type or function
/// in its namespace, a member of its type in the same group, a CSS name anywhere, another
/// symbol of its file); `naming`: whether case matters, the namespace separator.
pub fn clashes<'a>(
    index: &'a SymbolIndex,
    target: &Declared,
    kind: SymbolKind,
    new_name: &str,
    naming: &Naming,
) -> Vec<(&'a Path, &'a crate::Symbol)> {
    let same = |a: &str, b: &str| {
        if naming.names_ignore_case { a.eq_ignore_ascii_case(b) } else { a == b }
    };
    // The namespace the symbol is declared in (`App\Models` for `App\Models\User`).
    let namespace = naming.namespace_of(&target.fqn);
    match kind {
        // A type or a function: another one with the name in the same namespace.
        k if k.is_type() || k == SymbolKind::Function => index
            .by_name(new_name)
            .into_iter()
            .filter(|(_, s)| {
                let kind_clash = if k.is_type() { s.kind.is_type() } else { s.kind == k };
                kind_clash && same(&s.name, new_name) && naming.namespace_of(&s.fqn) == namespace
            })
            .collect(),
        k if k.is_member() => {
            let Some(class) = &target.container else { return Vec::new() };
            // Methods, properties and constants have names of their own (`$x->a` and
            // `$x->a()` don't clash); constants and enum cases share theirs.
            let group = |k: SymbolKind| match k {
                SymbolKind::EnumCase => SymbolKind::ClassConstant,
                other => other,
            };
            index
                .members_of(class)
                .into_iter()
                .filter(|(_, s)| group(s.kind) == group(kind) && same(&s.name, new_name))
                .collect()
        }
        _ => index
            .by_name(new_name)
            .into_iter()
            .filter(|(p, s)| {
                s.kind == kind && (crate::is_css_kind(kind) || *p == target.at.path.as_path())
            })
            .collect(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/rename.rs"]
mod tests;
