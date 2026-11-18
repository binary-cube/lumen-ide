//! A file's symbols and name resolution (PHP rules: namespace, `use`, alias).

use std::sync::Arc;

/// The kind of a declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Class,
    Interface,
    Trait,
    Enum,
    Function,
    Method,
    Property,
    Constant,
    ClassConstant,
    EnumCase,
    /// CSS class selector (`.card`), without the dot.
    CssClass,
    /// CSS id selector (`#main`), without `#`.
    CssId,
    /// CSS variable (`--accent`) or SCSS variable (`$accent`), with the prefix.
    CssVariable,
    /// SCSS mixin or function.
    Mixin,
    /// Top-level variable (e.g. `const api = …` in JS).
    Variable,
    /// A titled group of a file's declarations (a `.env` file's section: its keys are in
    /// its range).
    Section,
}

impl SymbolKind {
    pub fn is_type(self) -> bool {
        matches!(self, Self::Class | Self::Interface | Self::Trait | Self::Enum)
    }

    pub fn is_member(self) -> bool {
        matches!(self, Self::Method | Self::Property | Self::ClassConstant | Self::EnumCase)
    }

    /// What its names may hold besides letters, digits and `_` (`-` in CSS's), for a new
    /// name to be valid.
    pub fn name_chars(self) -> &'static str {
        if crate::is_css_kind(self) { "-" } else { "" }
    }
}

/// A qualified name's own part (`User` of `App\Models\User`): after the last character no
/// name holds (the namespace's separator, whatever the language's).
pub fn short_name(fqn: &str) -> &str {
    fqn.rsplit(|c: char| !is_name_char(c)).next().unwrap_or(fqn)
}

/// A character of a plain name (letters, digits, `_`).
pub fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// A member's visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Visibility {
    #[default]
    Public,
    Protected,
    Private,
}

/// A declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The short name, as in the code (properties without `$`).
    pub name: Box<str>,
    pub kind: SymbolKind,
    /// The full name: `App\Models\User` for types and functions; for members, the class
    /// containing them is in `container`.
    pub fqn: Box<str>,
    /// The FQN of the type containing the member (shared by its members: one copy).
    pub container: Option<Arc<str>>,
    /// The parent types (extends, implements, used traits), as FQNs (shared). A fixed slice:
    /// a member has none, and it's 8 bytes less on each of the index's symbols (plan 73).
    pub parents: Box<[Arc<str>]>,
    /// The declaration's range (bytes): for types, the whole body (so we know the class at the
    /// cursor); `offset` is the start of the name.
    pub range: std::ops::Range<usize>,
    pub offset: usize,
    pub line: u32,
    /// The signature (parameters and return type), shown in completion.
    pub detail: Option<Box<str>>,
    pub is_static: bool,
    pub visibility: Visibility,
    /// What's written on the declaration, short (PHP's attributes: `AsCommand('app:x')`),
    /// shown beside it in the outline.
    pub note: Option<Box<str>>,
    /// What its doc comment says of its types that its code doesn't (PHPDoc's `@return
    /// Collection<int, User>`, `@template`, `@extends`, `@mixin`, a deprecation), in its
    /// language's own compact form: read by that language's intelligence only.
    pub types: Option<Box<str>>,
}

/// A `use A\B\C as D;` import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UseImport {
    pub alias: Arc<str>,
    pub fqn: Arc<str>,
}

/// What a file declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSymbols {
    /// The file's language (its id), so suggestions stay in the right language (shared).
    pub language: Arc<str>,
    pub namespace: Option<Arc<str>>,
    pub uses: Vec<UseImport>,
    pub symbols: Vec<Symbol>,
}

impl FileSymbols {
    /// A library's file as the project sees it: its private members can't be used from the
    /// project (its open file gets its own symbols anyway), so they aren't kept.
    pub fn keep_public(&mut self) {
        self.symbols.retain(|s| s.visibility != Visibility::Private);
    }

    /// The heap it takes (bytes, an estimate).
    pub fn memory(&self) -> u64 {
        use kernel::memory::vec_bytes;
        let symbols: u64 = self
            .symbols
            .iter()
            .map(|s| {
                (s.name.len() + s.fqn.len()) as u64
                    + s.detail.as_ref().map_or(0, |d| d.len() as u64)
                    + s.note.as_ref().map_or(0, |n| n.len() as u64)
                    + s.types.as_ref().map_or(0, |t| t.len() as u64)
                    + std::mem::size_of_val::<[Arc<str>]>(&s.parents) as u64
            })
            .sum();
        vec_bytes(&self.symbols) + vec_bytes(&self.uses) + symbols
    }

    /// The FQN of a type name written in the file, by its language's `naming`: absolute
    /// (`\X`), the type around `at` (`self`, `static`, the receiver) or its parent (`parent`),
    /// an import's alias as its first part, otherwise relative to the file's namespace.
    pub fn resolve_class(&self, name: &str, at: usize, naming: &crate::Naming) -> String {
        if let Some(absolute) = naming.absolute(name) {
            return absolute.to_owned();
        }
        let same = |a: &str, b: &str| {
            if naming.names_ignore_case { a.eq_ignore_ascii_case(b) } else { a == b }
        };
        let own = naming.own_type.iter().chain(&naming.receiver).any(|w| same(w, name));
        if own && let Some(class) = self.type_at(at) {
            return class.fqn.to_string();
        }
        if naming.parent_type.is_some_and(|w| same(w, name))
            && let Some(parent) = self.type_at(at).and_then(|c| c.parents.first())
        {
            return parent.to_string();
        }
        let separator = naming.namespace_separator;
        let (first, rest) = match separator.and_then(|s| name.split_once(s)) {
            Some((first, rest)) => (first, Some(rest)),
            None => (name, None),
        };
        if let Some(import) = self.uses.iter().find(|u| same(&u.alias, first)) {
            return match (rest, separator) {
                (Some(rest), Some(s)) => format!("{}{s}{rest}", import.fqn),
                _ => import.fqn.to_string(),
            };
        }
        match (&self.namespace, separator) {
            (Some(ns), Some(s)) => format!("{ns}{s}{name}"),
            _ => name.to_owned(),
        }
    }

    /// The type (class, interface…) in whose body `offset` lies.
    pub fn type_at(&self, offset: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.kind.is_type() && s.range.contains(&offset))
            .min_by_key(|s| s.range.len())
    }

    /// The innermost function / method around `offset` (its body includes both ends): a
    /// local variable's scope.
    pub fn function_at(&self, offset: usize) -> Option<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| {
                matches!(s.kind, SymbolKind::Function | SymbolKind::Method)
                    && s.range.start <= offset
                    && offset <= s.range.end
            })
            .min_by_key(|s| s.range.len())
    }
}

#[cfg(test)]
#[path = "../tests/unit/symbols.rs"]
mod tests;
