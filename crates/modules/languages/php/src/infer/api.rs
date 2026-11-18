//! What the editor asks of the inference: the receiver's type at a member access, the
//! members PHP itself gives a type, the hover of a variable or of a built-in member.

use std::collections::{HashSet, VecDeque};
use std::sync::LazyLock;

use code_intelligence::{
    CompletionKind, FileSymbols, HoverInfo, HoverNote, Symbol, SymbolIndex, SymbolKind, Types,
    Visibility,
};
use syntax_ts::tree_sitter::{Node, Tree};

use super::{Binding, Infer, MemberKind, Source};
use crate::types::Ty;

/// Runs `ask` with the inference over `text` (its tree cached).
pub fn with<R>(text: &str, index: &SymbolIndex, ask: impl FnOnce(&Infer<'_>) -> R) -> Option<R> {
    let parsed = crate::doctree::parsed(text)?;
    let own = parsed.symbols(text);
    let infer = Infer::new(text, parsed.tree.root_node(), index, own);
    Some(ask(&infer))
}

/// An inference over no document (the project's and PHP's declarations only).
fn detached<R>(index: &SymbolIndex, ask: impl FnOnce(&Infer<'_>) -> R) -> Option<R> {
    static EMPTY: LazyLock<Option<Tree>> =
        LazyLock::new(|| crate::intelligence::parse_from("<?php\n", None));
    static OWN: LazyLock<FileSymbols> = LazyLock::new(FileSymbols::default);
    let tree = EMPTY.as_ref()?;
    let infer = Infer::new("<?php\n", tree.root_node(), index, &OWN);
    Some(ask(&infer))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Expressions a member access can be made on.
fn is_expression(kind: &str) -> bool {
    matches!(
        kind,
        "variable_name"
            | "member_access_expression"
            | "nullsafe_member_access_expression"
            | "member_call_expression"
            | "nullsafe_member_call_expression"
            | "scoped_call_expression"
            | "scoped_property_access_expression"
            | "class_constant_access_expression"
            | "function_call_expression"
            | "object_creation_expression"
            | "parenthesized_expression"
            | "subscript_expression"
            | "clone_expression"
            | "match_expression"
    )
}

/// The object a member access at `offset` (in or right after the member's name) is made
/// on: the expression ending before its `->` / `?->`.
pub fn receiver_node<'t>(infer: &Infer<'t>, offset: usize) -> Option<Node<'t>> {
    let text = infer.text;
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let before = text[..start].trim_end();
    let arrow = if before.ends_with("?->") {
        before.len() - 3
    } else if before.ends_with("->") {
        before.len() - 2
    } else {
        return None;
    };
    let end = text[..arrow].trim_end().len();
    if end == 0 {
        return None;
    }
    let mut node = infer.root.descendant_for_byte_range(end - 1, end)?;
    let mut best = None;
    loop {
        if node.end_byte() != end {
            break;
        }
        if is_expression(node.kind()) {
            best = Some(node);
        }
        match node.parent() {
            Some(parent) => node = parent,
            None => break,
        }
    }
    best
}

/// The type of the receiver at `offset` (see [`receiver_node`]).
pub fn receiver(infer: &Infer<'_>, offset: usize) -> Option<Ty> {
    let node = receiver_node(infer, offset)?;
    let ty = infer.type_of(node).without_null();
    (!ty.is_mixed()).then_some(ty)
}

/// The type of the receiver at `offset` of `text`. Code being typed often doesn't parse
/// (`$x->` with nothing after it, a statement without its `;`): where the receiver ends up
/// in an error, it's asked again of the text repaired (a name after the `->`, the line
/// ended).
pub fn receiver_at(text: &str, offset: usize, index: &SymbolIndex) -> Option<Ty> {
    let (ty, broken) = with(text, index, |infer| {
        let broken = receiver_node(infer, offset).is_some_and(in_error);
        (receiver(infer, offset), broken)
    })?;
    if ty.is_some() || !broken {
        return ty;
    }
    let fixed = repaired(text, offset)?;
    with(&fixed, index, |infer| receiver(infer, offset)).flatten()
}

/// The class of the receiver at `offset`: the first class it can be (see [`receiver_at`]).
pub fn receiver_type(text: &str, offset: usize, types: &Types<'_>) -> Option<String> {
    receiver_at(text, offset, types.index)?.class().map(str::to_owned)
}

/// The class a callable's string at `offset` names a method of (`[$x, 'cart']`,
/// `'App\Shop::cart'`): the one declaring it (an ancestor, a mixin); with no name typed
/// yet, the class itself. `None` when it isn't certain: an object of unknown (or more than
/// one) type, a method the class doesn't have (one `__call` would take).
pub fn callable_owner(text: &str, offset: usize, types: &Types<'_>) -> Option<String> {
    use crate::callables::Class;
    crate::callables::with_callable(text, offset, |_, c| {
        let name = text.get(c.name.clone())?.to_owned();
        with(text, types.index, |infer| {
            let class = match &c.class {
                Class::Written(range) => {
                    let written = text.get(range.clone())?.trim_start_matches('\\');
                    // A string names a class by its full name; else as the file would.
                    if infer.class(written).is_some() {
                        Ty::named(written)
                    } else {
                        Ty::named(&infer.resolve(written, range.start))
                    }
                }
                Class::Constant(node) => infer.scope_class(*node),
                Class::Object(node) => {
                    let ty = infer.type_of(*node).without_null();
                    let classes = ty.classes();
                    let [(class, args)] = classes.as_slice() else { return None };
                    Ty::Named((*class).into(), args.to_vec())
                }
            };
            if name.is_empty() {
                return class.class().map(str::to_owned);
            }
            let found = infer.find_member(&class, &name, MemberKind::Method)?;
            owner_of(&found).or_else(|| class.class().map(str::to_owned))
        })
        .flatten()
    })
}

/// Every class the receiver at `offset` can be (a union's), the first first.
pub fn receiver_types(text: &str, offset: usize, types: &Types<'_>) -> Vec<String> {
    if let Some(owner) = callable_owner(text, offset, types) {
        return vec![owner];
    }
    let Some(ty) = receiver_at(text, offset, types.index) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for (class, _) in ty.classes() {
        if !out.iter().any(|c| c.eq_ignore_ascii_case(class)) {
            out.push(class.to_owned());
        }
    }
    out
}

/// Whether a node is inside a syntax error.
fn in_error(node: Node<'_>) -> bool {
    std::iter::successors(Some(node), |n| n.parent()).any(|n| n.is_error())
}

/// `text` with the member access at `offset` given a name and its line ended (`;`).
fn repaired(text: &str, offset: usize) -> Option<String> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let named = text[offset..].starts_with(is_ident);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let line = text[..line_end].trim_end();
    let ended = line.ends_with([';', '{', '}', ',']);
    let mut out = String::with_capacity(text.len() + 2);
    out.push_str(&text[..offset]);
    if !named {
        out.push('x');
    }
    out.push_str(&text[offset..line_end]);
    if !ended {
        out.push(';');
    }
    out.push_str(&text[line_end..]);
    // The blocks left open (the end of an unfinished file).
    let open = text[..offset].matches('{').count().saturating_sub(text.matches('}').count());
    out.push_str(&"}".repeat(open.min(64)));
    (out.len() != text.len()).then_some(out)
}

/// The class declaring the member named at `offset` (after `->`, `?->` or `::`): found
/// through the receiver's ancestors, its mixins, a static call's forwarding.
pub fn member_owner(text: &str, offset: usize, types: &Types<'_>) -> Option<String> {
    // A callable's string: its class, or nothing (never what's around the string).
    if crate::callables::with_callable(text, offset, |_, _| Some(())).is_some() {
        return callable_owner(text, offset, types);
    }
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = offset + text[offset..].find(|c: char| !is_ident(c)).unwrap_or(text.len() - offset);
    let name = text.get(start..end).filter(|n| !n.is_empty())?;
    let before = text[..start].trim_end();
    if before.ends_with("::") {
        with(text, types.index, |infer| {
            let node = infer.root.named_descendant_for_byte_range(start, end)?;
            let scoped = std::iter::successors(Some(node), |n| n.parent())
                .take(3)
                .find(|n| n.kind() == "scoped_call_expression")?;
            let receiver = infer.scope_class(scoped);
            let (found, _) = infer.static_method(&receiver, name)?;
            owner_of(&found)
        })
        .flatten()
    } else {
        let receiver = receiver_at(text, start, types.index)?;
        with(text, types.index, |infer| {
            let kind = if text[end..].trim_start().starts_with('(') {
                MemberKind::Method
            } else {
                MemberKind::Property
            };
            receiver.classes().into_iter().find_map(|(class, args)| {
                let owner = Ty::Named(class.into(), args.to_vec());
                infer.find_member(&owner, name, kind).as_ref().and_then(owner_of)
            })
        })
        .flatten()
    }
}

/// The class a found member's declaration is in.
fn owner_of(found: &super::Found<'_>) -> Option<String> {
    match &found.source {
        Source::Index(_, symbol) => symbol.container.as_deref().map(str::to_owned),
        _ => None,
    }
}

/// The members of `class` PHP gives it itself: a built-in ancestor's methods and constants
/// (the stubs), an enum's `name` / `value` (and `cases()` / `from()` / `tryFrom()`, its
/// built-in interfaces').
pub fn builtin_members(class: &str, index: &SymbolIndex) -> Vec<Symbol> {
    detached(index, |infer| {
        let mut out = Vec::new();
        let mut queue = VecDeque::from([class.to_owned()]);
        let mut seen = HashSet::new();
        while let Some(current) = queue.pop_front() {
            if !seen.insert(current.to_ascii_lowercase()) || seen.len() > 32 {
                continue;
            }
            let Some(c) = infer.class(&current) else { continue };
            if c.in_index {
                // A mixin's members are the class's (`@mixin Builder`).
                for mixin in &c.mixins {
                    if let Some(fqn) = mixin.class() {
                        out.extend(infer.index.members_of(fqn).into_iter().map(|(_, s)| s.clone()));
                        queue.push_back(fqn.to_owned());
                    }
                }
                if c.kind == SymbolKind::Enum {
                    out.push(builtin(&c.fqn, "name", SymbolKind::Property, "string", false));
                    if let Some(backed) = &c.backed {
                        out.push(builtin(
                            &c.fqn,
                            "value",
                            SymbolKind::Property,
                            &backed.show(),
                            false,
                        ));
                    }
                }
            } else {
                for m in infer.stubs.methods(&c.fqn) {
                    let mut s =
                        builtin(&c.fqn, m.name, SymbolKind::Method, &m.signature(), m.is_static());
                    if !m.is_public() {
                        s.visibility = Visibility::Protected;
                    }
                    out.push(s);
                }
                for p in infer.stubs.properties(&c.fqn) {
                    let mut s = builtin(&c.fqn, p.name, SymbolKind::Property, p.ty, p.is_static());
                    if !p.is_public() {
                        s.visibility = Visibility::Protected;
                    }
                    out.push(s);
                }
                for k in infer.stubs.class_constants(&c.fqn) {
                    out.push(builtin(&c.fqn, k, SymbolKind::ClassConstant, "", true));
                }
            }
            queue.extend(c.parents.iter().map(|(p, _)| p.clone()));
        }
        out
    })
    .unwrap_or_default()
}

fn builtin(class: &str, name: &str, kind: SymbolKind, detail: &str, is_static: bool) -> Symbol {
    Symbol {
        name: name.to_owned().into_boxed_str(),
        kind,
        fqn: name.to_owned().into_boxed_str(),
        container: Some(class.into()),
        parents: Box::default(),
        range: 0..0,
        offset: 0,
        line: 0,
        detail: ((!detail.is_empty()).then(|| detail.to_owned())).map(String::into_boxed_str),
        is_static,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

/// The hover of what isn't a project declaration: a variable (its type, where it comes
/// from), a built-in class's method reached through an inferred receiver.
pub fn hover(text: &str, offset: usize, types: &Types<'_>) -> Option<HoverInfo> {
    let range = code_intelligence::word_at(text, offset, "$\\")?;
    let word = &text[range.clone()];
    with(text, types.index, |infer| {
        if let Some(name) = word.strip_prefix('$') {
            return variable_hover(infer, name, range.clone());
        }
        member_hover(infer, word, range.clone())
    })
    .flatten()
}

fn variable_hover(
    infer: &Infer<'_>,
    name: &str,
    range: std::ops::Range<usize>,
) -> Option<HoverInfo> {
    let node = infer.root.named_descendant_for_byte_range(range.start, range.start + 1)?;
    let node =
        std::iter::successors(Some(node), |n| n.parent()).find(|n| n.kind() == "variable_name")?;
    let (ty, from) = if name == "this" {
        (infer.this_type(node), None)
    } else {
        let parent = node.parent();
        // Its own declaration: what it's given there.
        let own = parent.and_then(|p| match p.kind() {
            "assignment_expression" | "reference_assignment_expression"
                if p.child_by_field_name("left").is_some_and(|l| l.id() == node.id()) =>
            {
                Some(Binding::Assigned(p))
            }
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter" => {
                let function = std::iter::successors(p.parent(), |n| n.parent()).find(|n| {
                    matches!(
                        n.kind(),
                        "function_definition"
                            | "method_declaration"
                            | "anonymous_function"
                            | "arrow_function"
                    )
                })?;
                Some(Binding::Parameter(p, function))
            }
            _ => None,
        });
        match own {
            Some(binding) => (infer.bound(name, &binding), Some(binding)),
            None => {
                let (ty, binding) = infer.variable(name, node)?;
                (ty, Some(binding))
            }
        }
    };
    if ty.is_mixed() {
        return None;
    }
    let doc = from.and_then(|b| origin(infer, &b));
    Some(HoverInfo {
        range,
        code: format!("{} ${name}", ty.show()),
        doc,
        title: Some((i18n::t("hover.variable"), Some(CompletionKind::Variable))),
        ..Default::default()
    })
}

/// Where a variable's value comes from, in a line.
fn origin(infer: &Infer<'_>, binding: &Binding<'_>) -> Option<String> {
    let line = |node: Node<'_>| {
        let text = infer.src(node);
        let first = text.lines().next().unwrap_or("").trim();
        let mut short: String = first.chars().take(80).collect();
        if short.len() < first.len() || text.contains('\n') {
            short.push('…');
        }
        short
    };
    Some(match *binding {
        Binding::Assigned(node) => i18n::tr!("hover.variable_assigned", code = line(node)),
        Binding::Destructured(value, ..) => {
            i18n::tr!("hover.variable_assigned", code = line(value))
        }
        Binding::Iterated(iterable, _) => {
            i18n::tr!("hover.variable_iterated", code = line(iterable))
        }
        Binding::Caught(_) => i18n::t("hover.variable_caught"),
        Binding::Parameter(..) => i18n::t("hover.variable_parameter"),
        Binding::Documented(_) => i18n::t("hover.variable_documented"),
        Binding::Narrowed(class) => i18n::tr!("hover.variable_narrowed", code = infer.src(class)),
        Binding::Unknown => return None,
    })
}

/// A built-in class's method after `->` / `::` on an inferred receiver.
fn member_hover(infer: &Infer<'_>, word: &str, range: std::ops::Range<usize>) -> Option<HoverInfo> {
    let receiver = receiver_at(infer.text, range.end, infer.index)?;
    let (found, _) = infer.method_on(&receiver, word)?;
    let Source::Stub(class, method) = found.source else { return None };
    let mut notes = Vec::new();
    if method.since != infer.stubs.versions.first().copied().unwrap_or("")
        && !method.since.is_empty()
    {
        notes.push(HoverNote::Since(method.since));
    }
    if !method.removed.is_empty() {
        notes.push(HoverNote::Removed(method.removed));
    }
    let short = class.rsplit('\\').next().unwrap_or(class);
    let namespace = class.rsplit_once('\\').map(|(ns, _)| ns.to_owned());
    Some(HoverInfo {
        range,
        code: format!("{} function {}{}", method.modifiers, method.name, method.signature()),
        notes,
        title: Some((short.to_owned(), Some(CompletionKind::Class))),
        subtitle: namespace,
        ..Default::default()
    })
}

impl Infer<'_> {
    /// The type a binding gives `$name` (for the hover of its declaration).
    pub fn bound(&self, name: &str, binding: &Binding<'_>) -> Ty {
        match *binding {
            Binding::Assigned(node) => self.type_of(node),
            _ => self.deeper(|| self.bound_type(name, binding)),
        }
    }
}
