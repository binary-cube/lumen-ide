//! A type's diagram: the class (interface, trait, enum) with its members, what it inherits
//! (parents, interfaces, traits — up the chain), its direct children, and the types its
//! constructor depends on. Built from the symbol index, as a graph a view lays out.

use std::path::PathBuf;

use crate::{Symbol, SymbolIndex, SymbolKind, Visibility};

/// How many ancestor levels are followed.
const MAX_DEPTH: u8 = 4;
/// At most this many nodes per role (a base class with hundreds of children stays readable).
const MAX_PER_ROLE: usize = 24;

/// Where a node sits relative to the type the diagram is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagramRole {
    Focus,
    /// A parent, an interface or a trait, `n` levels up.
    Ancestor(u8),
    /// A direct child (extends or implements the focus).
    Child,
    /// A type the focus's constructor takes.
    Dependency,
}

/// How two nodes are related (`from` → `to`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    Extends,
    Implements,
    UsesTrait,
    /// A constructor parameter's type.
    Depends,
}

/// A member shown in a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramMember {
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub is_static: bool,
    /// The signature or the type (as in the code).
    pub detail: Option<String>,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramNode {
    pub fqn: String,
    pub name: String,
    pub kind: SymbolKind,
    pub role: DiagramRole,
    /// Where it's declared (`None`: not in the index — a built-in or unindexed type).
    pub path: Option<PathBuf>,
    pub line: u32,
    /// Its own members (not inherited), properties and constants before methods.
    pub members: Vec<DiagramMember>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagramEdge {
    pub from: usize,
    pub to: usize,
    pub relation: Relation,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClassDiagram {
    pub nodes: Vec<DiagramNode>,
    pub edges: Vec<DiagramEdge>,
}

impl ClassDiagram {
    /// The node the diagram is about (always the first).
    pub fn focus(&self) -> Option<&DiagramNode> {
        self.nodes.first()
    }

    fn find(&self, fqn: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.fqn.eq_ignore_ascii_case(fqn))
    }

    fn add(&mut self, node: DiagramNode) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    fn link(&mut self, from: usize, to: usize, relation: Relation) {
        let edge = DiagramEdge { from, to, relation };
        if from != to && !self.edges.contains(&edge) {
            self.edges.push(edge);
        }
    }

    fn count(&self, role: impl Fn(DiagramRole) -> bool) -> usize {
        self.nodes.iter().filter(|n| role(n.role)).count()
    }
}

/// The diagram of type `fqn`; `None` if the index doesn't know it. `naming`: a language's
/// naming rules (its constructor's name), by its id; `parameter_types`: the types a
/// language's signature names (its id, the signature), as written
/// ([`crate::LanguageIntelligence::parameter_types`]).
pub fn class_diagram(
    index: &SymbolIndex,
    fqn: &str,
    naming: &dyn Fn(&str) -> crate::Naming,
    parameter_types: &dyn Fn(&str, &str) -> Vec<String>,
) -> Option<ClassDiagram> {
    let (path, focus) = index.type_by_fqn(fqn)?;
    // The focus's language names the types the index doesn't know.
    let naming = index.file(path).map(|f| naming(&f.language)).unwrap_or_default();
    let node = |fqn: &str, role| node(index, fqn, role, &naming);
    let mut diagram = ClassDiagram::default();
    diagram.add(node(&focus.fqn, DiagramRole::Focus));

    // Up: parents, interfaces and traits, level by level.
    let mut level = vec![0usize];
    for depth in 1..=MAX_DEPTH {
        let mut next = Vec::new();
        for &child in &level {
            let Some((_, ty)) = index.type_by_fqn(&diagram.nodes[child].fqn) else { continue };
            let child_kind = ty.kind;
            for parent in &ty.parents {
                let at = match diagram.find(parent) {
                    Some(at) => at,
                    None if diagram.count(|r| matches!(r, DiagramRole::Ancestor(_)))
                        < MAX_PER_ROLE =>
                    {
                        let at = diagram.add(node(parent, DiagramRole::Ancestor(depth)));
                        next.push(at);
                        at
                    }
                    None => continue,
                };
                let relation = relation(child_kind, diagram.nodes[at].kind);
                diagram.link(child, at, relation);
            }
        }
        if next.is_empty() {
            break;
        }
        level = next;
    }

    // Down: the types that extend / implement / use it directly.
    let mut children: Vec<&Symbol> = index
        .top_level()
        .map(|(_, s)| s)
        .filter(|s| s.kind.is_type() && s.parents.iter().any(|p| p.eq_ignore_ascii_case(fqn)))
        .collect();
    children.sort_by(|a, b| a.fqn.cmp(&b.fqn));
    children.dedup_by(|a, b| a.fqn.eq_ignore_ascii_case(&b.fqn));
    for child in children.into_iter().take(MAX_PER_ROLE) {
        if diagram.find(&child.fqn).is_some() {
            continue;
        }
        let at = diagram.add(node(&child.fqn, DiagramRole::Child));
        diagram.link(at, 0, relation(child.kind, focus.kind));
    }

    // Right: what its constructor takes.
    if let Some(file) = index.file(path) {
        let constructor = file.symbols.iter().find(|s| {
            s.kind == SymbolKind::Method
                && naming.is_constructor(&s.name)
                && s.container.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&focus.fqn))
        });
        if let Some(constructor) = constructor {
            let detail = constructor.detail.as_deref().unwrap_or("");
            let mut types: Vec<String> = Vec::new();
            for written in parameter_types(&file.language, detail) {
                let fqn = file.resolve_class(&written, constructor.offset, &naming);
                if !types.iter().any(|t| t.eq_ignore_ascii_case(&fqn)) {
                    types.push(fqn);
                }
            }
            for ty in types {
                let at = match diagram.find(&ty) {
                    Some(at) => at,
                    None if diagram.count(|r| r == DiagramRole::Dependency) < MAX_PER_ROLE => {
                        diagram.add(node(&ty, DiagramRole::Dependency))
                    }
                    None => continue,
                };
                diagram.link(0, at, Relation::Depends);
            }
        }
    }
    Some(diagram)
}

/// `child` → `parent`: a class extends a class, implements an interface, uses a trait; an
/// interface extends interfaces.
fn relation(child: SymbolKind, parent: SymbolKind) -> Relation {
    match parent {
        SymbolKind::Trait => Relation::UsesTrait,
        SymbolKind::Interface if child != SymbolKind::Interface => Relation::Implements,
        _ => Relation::Extends,
    }
}

/// A node for `fqn`: from the index if it knows it, otherwise just the name.
fn node(index: &SymbolIndex, fqn: &str, role: DiagramRole, naming: &crate::Naming) -> DiagramNode {
    let Some((path, ty)) = index.type_by_fqn(fqn) else {
        return DiagramNode {
            fqn: fqn.to_owned(),
            name: naming.short_name(fqn).to_owned(),
            kind: SymbolKind::Class,
            role,
            path: None,
            line: 0,
            members: Vec::new(),
        };
    };
    let mut members: Vec<DiagramMember> = index
        .file(path)
        .into_iter()
        .flat_map(|f| &f.symbols)
        .filter(|s| s.container.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(&ty.fqn)))
        .map(|s| DiagramMember {
            name: s.name.to_string(),
            kind: s.kind,
            visibility: s.visibility,
            is_static: s.is_static,
            detail: s.detail.clone().map(String::from),
            line: s.line,
        })
        .collect();
    // Cases and constants, then properties, then methods (each in the code's order).
    members.sort_by_key(|m| match m.kind {
        SymbolKind::EnumCase | SymbolKind::ClassConstant => 0,
        SymbolKind::Property => 1,
        _ => 2,
    });
    DiagramNode {
        fqn: ty.fqn.to_string(),
        name: ty.name.to_string(),
        kind: ty.kind,
        role,
        path: Some(path.to_path_buf()),
        line: ty.line,
        members,
    }
}

#[cfg(test)]
#[path = "../tests/unit/diagram.rs"]
mod tests;
