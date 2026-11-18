//! Types' families from the symbol index: what implements or extends a type (directly or
//! further down), the methods overriding one, a type's supertypes and subtypes as trees,
//! and the type a declaration has (a property's, what a method returns) — for "Go to
//! Implementation", "Go to Type Declaration", the type hierarchy and the code lenses.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{Naming, Symbol, SymbolIndex, SymbolKind};

/// How many levels a hierarchy follows (up and down).
const MAX_DEPTH: usize = 6;
/// At most this many types in a list (a base class with thousands of children stays
/// usable).
const MAX_TYPES: usize = 500;

/// A declaration found: its name, kind, the type it's in (a member's), where it is
/// (`None`: a type the index doesn't know — a built-in one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub fqn: String,
    pub name: String,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub path: Option<PathBuf>,
    pub offset: usize,
    pub line: u32,
}

impl Place {
    pub fn of(path: &Path, s: &Symbol) -> Self {
        Self {
            fqn: s.fqn.to_string(),
            name: s.name.to_string(),
            kind: s.kind,
            container: s.container.as_deref().map(str::to_owned),
            path: Some(path.to_path_buf()),
            offset: s.offset,
            line: s.line,
        }
    }

    /// A type known only by its name (not in the index).
    fn unknown(fqn: &str, naming: &Naming) -> Self {
        Self {
            fqn: fqn.to_owned(),
            name: naming.short_name(fqn).to_owned(),
            kind: SymbolKind::Class,
            container: None,
            path: None,
            offset: 0,
            line: 0,
        }
    }
}

/// The index's types by the types they name as parents (extends, implements, uses): who
/// is under whom, worked out once for many questions.
#[derive(Debug)]
pub struct Subtypes<'a> {
    children: HashMap<String, Vec<(&'a Path, &'a Symbol)>>,
}

impl<'a> Subtypes<'a> {
    pub fn of(index: &'a SymbolIndex) -> Self {
        let mut children: HashMap<String, Vec<(&'a Path, &'a Symbol)>> = HashMap::new();
        for (path, s) in index.top_level().filter(|(_, s)| s.kind.is_type()) {
            for parent in &s.parents {
                let list = children.entry(parent.to_lowercase()).or_default();
                if !list.iter().any(|(_, c)| c.fqn.eq_ignore_ascii_case(&s.fqn)) {
                    list.push((path, s));
                }
            }
        }
        for list in children.values_mut() {
            list.sort_by(|(pa, a), (pb, b)| {
                is_vendor(pa).cmp(&is_vendor(pb)).then_with(|| a.fqn.cmp(&b.fqn))
            });
        }
        Self { children }
    }

    /// The types naming `fqn` as a parent.
    pub fn direct(&self, fqn: &str) -> &[(&'a Path, &'a Symbol)] {
        self.children.get(&fqn.to_lowercase()).map_or(&[], Vec::as_slice)
    }

    /// Every type under `fqn`, nearest first (each once).
    pub fn all(&self, fqn: &str) -> Vec<(&'a Path, &'a Symbol)> {
        let mut out: Vec<(&'a Path, &'a Symbol)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        seen.insert(fqn.to_lowercase());
        let mut level = vec![fqn.to_owned()];
        for _ in 0..MAX_DEPTH * 2 {
            let mut next = Vec::new();
            for parent in &level {
                for &(path, s) in self.direct(parent) {
                    if seen.insert(s.fqn.to_lowercase()) && out.len() < MAX_TYPES {
                        out.push((path, s));
                        next.push(s.fqn.to_string());
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            level = next;
        }
        out
    }

    /// What implements or extends type `fqn`, further down too: the classes, enums and
    /// traits (not the interfaces: an interface implements nothing).
    pub fn implementations(&self, fqn: &str) -> Vec<Place> {
        self.all(fqn)
            .into_iter()
            .filter(|(_, s)| s.kind != SymbolKind::Interface)
            .map(|(p, s)| Place::of(p, s))
            .collect()
    }

    /// The methods named `name` that the types under `container` declare themselves (they
    /// override or implement its own).
    pub fn overrides(
        &self,
        index: &SymbolIndex,
        container: &str,
        name: &str,
        naming: &Naming,
    ) -> Vec<Place> {
        let same = |a: &str, b: &str| {
            if naming.methods_ignore_case { a.eq_ignore_ascii_case(b) } else { a == b }
        };
        let mut out = Vec::new();
        for (path, ty) in self.all(container) {
            let Some(file) = index.file(path) else { continue };
            let own = file.symbols.iter().find(|m| {
                m.kind == SymbolKind::Method
                    && same(&m.name, name)
                    && m.container.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&ty.fqn))
            });
            if let Some(m) = own {
                out.push(Place::of(path, m));
            }
        }
        out
    }
}

/// A type in a hierarchy, with the types above it (a supertypes tree) or below it (a
/// subtypes tree).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyNode {
    pub place: Place,
    pub children: Vec<HierarchyNode>,
}

/// A type's family: what it extends / implements / uses up the chain, and what's under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeHierarchy {
    pub focus: Place,
    pub supertypes: Vec<HierarchyNode>,
    pub subtypes: Vec<HierarchyNode>,
}

impl TypeHierarchy {
    /// Its rows as listed: the supertypes (each indented by how far up it is, the farthest
    /// deepest), the type itself (depth 0), the subtypes (indented by how far down) — with
    /// their depth and which part they're in (-1 above, 0 the type, 1 below).
    pub fn rows(&self) -> Vec<(i8, usize, &Place)> {
        fn walk<'a>(
            nodes: &'a [HierarchyNode],
            depth: usize,
            side: i8,
            out: &mut Vec<(i8, usize, &'a Place)>,
        ) {
            for node in nodes {
                out.push((side, depth, &node.place));
                walk(&node.children, depth + 1, side, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.supertypes, 1, -1, &mut out);
        out.push((0, 0, &self.focus));
        walk(&self.subtypes, 1, 1, &mut out);
        out
    }
}

/// The hierarchy of type `fqn`; `None` if the index doesn't know it.
pub fn type_hierarchy(index: &SymbolIndex, fqn: &str, naming: &Naming) -> Option<TypeHierarchy> {
    let (path, focus) = index.type_by_fqn(fqn)?;
    let subtypes = Subtypes::of(index);
    let mut count = 0;
    let supertypes = up(index, &focus.parents, naming, 1, &mut count);
    let mut seen = vec![focus.fqn.to_lowercase()];
    let subtypes = down(&subtypes, &focus.fqn, 1, &mut count, &mut seen);
    Some(TypeHierarchy { focus: Place::of(path, focus), supertypes, subtypes })
}

fn up(
    index: &SymbolIndex,
    parents: &[std::sync::Arc<str>],
    naming: &Naming,
    depth: usize,
    count: &mut usize,
) -> Vec<HierarchyNode> {
    let mut out = Vec::new();
    for parent in parents {
        if *count >= MAX_TYPES {
            break;
        }
        *count += 1;
        let node = match index.type_by_fqn(parent) {
            Some((path, ty)) => {
                let children = if depth < MAX_DEPTH {
                    up(index, &ty.parents, naming, depth + 1, count)
                } else {
                    Vec::new()
                };
                HierarchyNode { place: Place::of(path, ty), children }
            }
            None => HierarchyNode { place: Place::unknown(parent, naming), children: Vec::new() },
        };
        out.push(node);
    }
    out
}

fn down(
    subtypes: &Subtypes<'_>,
    fqn: &str,
    depth: usize,
    count: &mut usize,
    seen: &mut Vec<String>,
) -> Vec<HierarchyNode> {
    let mut out = Vec::new();
    for &(path, child) in subtypes.direct(fqn) {
        let key = child.fqn.to_lowercase();
        if *count >= MAX_TYPES || seen.contains(&key) {
            continue;
        }
        *count += 1;
        seen.push(key);
        let children = if depth < MAX_DEPTH {
            down(subtypes, &child.fqn, depth + 1, count, seen)
        } else {
            Vec::new()
        };
        out.push(HierarchyNode { place: Place::of(path, child), children });
    }
    out
}

/// The type written for a declaration: a property's (or a constant's, a variable's), what
/// a method or a function returns (after the parameters' `)`, `:` or `=>`). As written.
pub fn declared_type(symbol: &Symbol) -> Option<&str> {
    let detail = symbol.detail.as_deref()?.trim();
    match symbol.kind {
        SymbolKind::Method | SymbolKind::Function => {
            let close = matching_paren_end(detail)?;
            let rest = detail[close..].trim_start();
            let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix("=>"))?.trim();
            (!rest.is_empty()).then_some(rest)
        }
        SymbolKind::Property
        | SymbolKind::Constant
        | SymbolKind::ClassConstant
        | SymbolKind::Variable => (!detail.is_empty()).then_some(detail),
        _ => None,
    }
}

/// Past the parameters' closing parenthesis (the first `(`'s match).
fn matching_paren_end(detail: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in detail.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The first type in `written` naming a class (`?User`, `User|null`, `array<User>`…
/// read as `User`), not a built-in one (`int`, `string`, `void`…); `self` / `static` /
/// `this` as written (the caller resolves them to the declaring type).
pub fn class_in_type(written: &str) -> Option<&str> {
    const BUILT_IN: &[&str] = &[
        "int",
        "integer",
        "float",
        "double",
        "string",
        "bool",
        "boolean",
        "array",
        "mixed",
        "void",
        "null",
        "callable",
        "iterable",
        "object",
        "never",
        "false",
        "true",
        "resource",
        "number",
        "any",
        "unknown",
        "undefined",
        "symbol",
        "bigint",
    ];
    written
        .split(['|', '&', '<', '>', ',', '(', ')', ' ', '[', ']'])
        .map(|part| part.trim().trim_start_matches('?'))
        .filter(|part| !part.is_empty())
        .find(|part| {
            part.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '\\')
                && !BUILT_IN.iter().any(|b| b.eq_ignore_ascii_case(part))
        })
}

/// The declaration of `file`'s symbols whose name is at `offset` (the cursor on the name of
/// a class, a method being declared).
pub fn declaration_at(
    file: &crate::FileSymbols,
    offset: usize,
    sigil: Option<char>,
) -> Option<&Symbol> {
    file.symbols.iter().find(|s| {
        let start = s.offset;
        let len = s.name.len() + usize::from(sigil.is_some() && s.kind == SymbolKind::Property);
        start <= offset && offset <= start + len
    })
}

fn is_vendor(path: &Path) -> bool {
    kernel::in_dependencies(path)
}

#[cfg(test)]
#[path = "../tests/unit/hierarchy.rs"]
mod tests;
