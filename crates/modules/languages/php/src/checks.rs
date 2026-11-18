//! What's wrong in a PHP file knowing the project — only what's certain (a doubt is never
//! shown): a class made or used statically that nothing declares, a call with too few
//! arguments (too many for PHP's own functions), a method called that its class (known,
//! with no magic, none of its subclasses having it either) doesn't have, a call of
//! something deprecated. A library's files (`vendor/`) and an IDE's meta files
//! (`.phpstorm.meta.php`, not code that runs) aren't checked.

use code_intelligence::{Checked, Deprecated, Problem, SymbolKind, Types};
use syntax_ts::tree_sitter::Node;

use crate::infer::{Binding, Found, Infer, MemberKind, Source, named_children};
use crate::types::Ty;

pub fn check(text: &str, types: &Types<'_>) -> Checked {
    // A library's file isn't the project's to fix (its optional dependencies aren't
    // installed, its private members aren't indexed): nothing said there.
    let meta =
        types.path.file_name().is_some_and(|n| n.to_string_lossy().starts_with(".phpstorm.meta"));
    if kernel::in_dependencies(types.path) || meta {
        return Checked::default();
    }
    crate::infer::api::with(text, types.index, |infer| {
        let mut out = Checked::default();
        // A text that doesn't parse (being typed) is read in pieces: what's missing may be
        // there once it does.
        if infer.root.has_error() {
            return out;
        }
        let mut stack = vec![infer.root];
        while let Some(node) = stack.pop() {
            match node.kind() {
                "object_creation_expression" => {
                    if let Some(name) = class_name(node) {
                        unknown_class(infer, name, &mut out);
                        deprecated_class(infer, name, &mut out);
                    }
                    call(infer, node, &mut out);
                }
                "scoped_call_expression" | "class_constant_access_expression" => {
                    // `X::class` is a name, not a use of the class (often of an optional one).
                    let names_class = node.kind() == "class_constant_access_expression"
                        && named_children(node)
                            .last()
                            .is_some_and(|c| infer.src(c).eq_ignore_ascii_case("class"));
                    if let Some(scope) =
                        node.child_by_field_name("scope").or_else(|| node.named_child(0))
                        && matches!(scope.kind(), "name" | "qualified_name")
                        && !names_class
                    {
                        unknown_class(infer, scope, &mut out);
                    }
                    if node.kind() == "scoped_call_expression" {
                        call(infer, node, &mut out);
                    }
                }
                "member_call_expression" | "nullsafe_member_call_expression" => {
                    undefined_method(infer, node, &mut out);
                    call(infer, node, &mut out);
                }
                "function_call_expression" => call(infer, node, &mut out),
                "binary_expression" => {
                    let operator = node.child_by_field_name("operator").map(|o| infer.src(o));
                    if operator == Some("instanceof")
                        && let Some(right) = node.child_by_field_name("right")
                        && matches!(right.kind(), "name" | "qualified_name")
                    {
                        unknown_class(infer, right, &mut out);
                    }
                }
                _ => {}
            }
            syntax_ts::push_children(&mut stack, node);
        }
        out
    })
    .unwrap_or_default()
}

/// The class named by `new X(…)` (not `new static`, `new $name`, an anonymous class).
fn class_name(node: Node<'_>) -> Option<Node<'_>> {
    named_children(node).find(|c| matches!(c.kind(), "name" | "qualified_name"))
}

/// Whether a class (interface, trait, enum) of that full name exists: the project's or PHP's.
fn known(infer: &Infer<'_>, fqn: &str) -> bool {
    infer.index.type_by_fqn(fqn).is_some() || infer.stubs.class(fqn).is_some()
}

/// `new X`, `X::y()`, `$a instanceof X` where nothing declares `X`.
fn unknown_class(infer: &Infer<'_>, name: Node<'_>, out: &mut Checked) {
    let written = infer.src(name);
    if matches!(written.to_ascii_lowercase().as_str(), "self" | "static" | "parent") {
        return;
    }
    let fqn = infer.resolve(written, name.start_byte());
    if known(infer, &fqn) {
        return;
    }
    // An unqualified name not imported and in no namespace's index: PHP's fallback for
    // classes doesn't exist, but a name the project has elsewhere (a class alias, a
    // generated class) is a doubt: not said.
    let short = written.trim_start_matches('\\').rsplit('\\').next().unwrap_or(written);
    if infer.index.by_name(short).iter().any(|(_, s)| s.kind.is_type()) {
        return;
    }
    out.problems.push(Problem {
        range: name.byte_range(),
        kind: "undefined_class",
        name: written.trim_start_matches('\\').to_owned(),
        arguments: None,
    });
}

fn deprecated_class(infer: &Infer<'_>, name: Node<'_>, out: &mut Checked) {
    let fqn = infer.resolve(infer.src(name), name.start_byte());
    if let Some((_, symbol)) = infer.index.type_by_fqn(&fqn)
        && crate::phpdoc::Facts::says_deprecated(symbol.types.as_deref())
    {
        out.deprecated.push(Deprecated {
            range: name.byte_range(),
            kind: "class",
            name: symbol.name.to_string(),
        });
    }
}

/// `$x->name()` on a class known whole (it and its ancestors, all declared) that has no
/// such method and no way to answer it anyway (`__call`, `@mixin`, `@method`).
fn undefined_method(infer: &Infer<'_>, call: Node<'_>, out: &mut Checked) {
    let Some(name) = call.child_by_field_name("name").filter(|n| n.kind() == "name") else {
        return;
    };
    let Some(object) = call.child_by_field_name("object") else { return };
    // `$this->` in an abstract class: what its subclasses declare may be meant.
    if infer.src(object) == "$this" && in_abstract_class(infer, call) {
        return;
    }
    if !certain(infer, object) {
        return;
    }
    let receiver = infer.type_of(object).without_null();
    let Ty::Named(fqn, _) = &receiver else { return };
    // PHP's own classes may answer by themselves (`RecursiveIteratorIterator` forwards to
    // its iterator, `FFI` calls C): only the project's.
    if !infer.class(fqn).is_some_and(|c| c.in_index) {
        return;
    }
    let method = infer.src(name);
    if infer.find_member(&receiver, method, MemberKind::Method).is_some() {
        return;
    }
    if !closed(infer, fqn) || in_descendant(infer, fqn, method) || probed(infer, call, method) {
        return;
    }
    out.problems.push(Problem {
        range: name.byte_range(),
        kind: "undefined_method",
        name: format!("{}::{method}()", fqn.rsplit('\\').next().unwrap_or(fqn)),
        arguments: None,
    });
}

/// Whether what a method is called on is certainly of its inferred type: `$this` (not in a
/// closure: one may be bound to another object), a parameter declared with a type, a
/// variable narrowed by `instanceof` or assigned in its function's own block (not in a
/// branch: another branch may give it something else), `$this->property` declared with a
/// type.
fn certain(infer: &Infer<'_>, object: Node<'_>) -> bool {
    match object.kind() {
        "variable_name" => {
            let name = infer.src(object).trim_start_matches('$');
            if name == "this" {
                return !in_closure(object);
            }
            match infer.binding(name, object, object.start_byte()) {
                Some(Binding::Parameter(param, _)) => param.child_by_field_name("type").is_some(),
                Some(Binding::Narrowed(_)) => true,
                Some(Binding::Assigned(assignment)) => {
                    let statement =
                        assignment.parent().filter(|p| p.kind() == "expression_statement");
                    let block = statement.and_then(|s| s.parent());
                    let function = block.and_then(|b| b.parent());
                    function.is_some_and(|f| {
                        matches!(f.kind(), "method_declaration" | "function_definition")
                            && block.is_some_and(|b| {
                                f.child_by_field_name("body")
                                    .is_some_and(|body| body.id() == b.id())
                            })
                    })
                }
                _ => false,
            }
        }
        "member_access_expression" => {
            let Some(inner) = object.child_by_field_name("object") else { return false };
            let Some(name) = object.child_by_field_name("name") else { return false };
            if infer.src(inner) != "$this" || in_closure(inner) {
                return false;
            }
            let this = infer.type_of(inner);
            matches!(
                infer.find_member(&this, infer.src(name), MemberKind::Property),
                Some(Found { source: Source::Index(_, s), .. }) if s.detail.is_some()
            )
        }
        _ => false,
    }
}

/// Whether `node` is inside a closure or an arrow function (whose `$this` may be bound to
/// another object).
fn in_closure(node: Node<'_>) -> bool {
    std::iter::successors(node.parent(), |n| n.parent())
        .take_while(|n| !matches!(n.kind(), "method_declaration" | "function_definition"))
        .any(|n| matches!(n.kind(), "anonymous_function" | "arrow_function"))
}

/// Whether the function around a call names its method in a string (`method_exists($x,
/// 'name')`, `is_callable([$x, 'name'])`): it's checked for before being called.
fn probed(infer: &Infer<'_>, call: Node<'_>, method: &str) -> bool {
    let function = std::iter::successors(call.parent(), |n| n.parent())
        .find(|n| {
            matches!(
                n.kind(),
                "method_declaration"
                    | "function_definition"
                    | "anonymous_function"
                    | "arrow_function"
            )
        })
        .unwrap_or(infer.root);
    let text = infer.src(function);
    text.contains(&format!("'{method}'")) || text.contains(&format!("\"{method}\""))
}

/// Whether a class extending `fqn` declares `method` (the object may be one of those: a
/// `ReflectionType` that's a `ReflectionNamedType`).
fn in_descendant(infer: &Infer<'_>, fqn: &str, method: &str) -> bool {
    let project = infer.index.by_name(method).into_iter().any(|(_, s)| {
        let Some(container) = s.container.as_deref().filter(|_| s.kind == SymbolKind::Method)
        else {
            return false;
        };
        if descends(infer, container, fqn) {
            return true;
        }
        // A trait's: a class using it may extend `fqn`.
        infer.class(container).is_some_and(|c| c.kind == SymbolKind::Trait)
            && infer.index.top_level().any(|(_, t)| {
                t.kind.is_type()
                    && t.parents.iter().any(|p| p.eq_ignore_ascii_case(container))
                    && descends(infer, &t.fqn, fqn)
            })
    });
    project
        || infer.stubs.classes_with_prefix("").any(|c| {
            infer.stubs.methods(c.name).iter().any(|m| m.name.eq_ignore_ascii_case(method))
                && descends(infer, c.name, fqn)
        })
}

/// Whether `class` has `ancestor` among its ancestors (the project's or PHP's).
fn descends(infer: &Infer<'_>, class: &str, ancestor: &str) -> bool {
    let mut queue = vec![class.to_owned()];
    let mut seen = std::collections::HashSet::new();
    while let Some(current) = queue.pop() {
        if !seen.insert(current.to_ascii_lowercase()) || seen.len() > 32 {
            continue;
        }
        let Some(c) = infer.class(&current) else { continue };
        for (parent, _) in &c.parents {
            if parent.eq_ignore_ascii_case(ancestor) {
                return true;
            }
            queue.push(parent.clone());
        }
    }
    false
}

/// Whether `node` is in an abstract class's body.
fn in_abstract_class(infer: &Infer<'_>, node: Node<'_>) -> bool {
    std::iter::successors(node.parent(), |n| n.parent())
        .find(|n| n.kind() == "class_declaration")
        .is_some_and(|class| {
            named_children(class).any(|c| c.kind() == "abstract_modifier")
                || infer.src(class).trim_start().starts_with("abstract")
        })
}

/// Whether a class is known whole, with no magic: a class (not an interface or a trait),
/// it and every ancestor declared (the project's or PHP's), none answering unknown calls
/// (`__call`), no `@mixin`, no `@method` / `@property` of its own.
fn closed(infer: &Infer<'_>, fqn: &str) -> bool {
    let mut queue = vec![fqn.to_owned()];
    let mut seen = std::collections::HashSet::new();
    let mut first = true;
    while let Some(current) = queue.pop() {
        if !seen.insert(current.to_ascii_lowercase()) {
            continue;
        }
        if seen.len() > 32 {
            return false;
        }
        let Some(class) = infer.class(&current) else { return false };
        if first && !matches!(class.kind, SymbolKind::Class | SymbolKind::Enum) {
            return false;
        }
        first = false;
        if !class.mixins.is_empty() {
            return false;
        }
        if class.in_index {
            // A library's trait: its private methods (callable by the class using it) aren't
            // indexed.
            let library = infer
                .index
                .type_by_fqn(&class.fqn)
                .is_some_and(|(path, _)| kernel::in_dependencies(path));
            if library && class.kind == SymbolKind::Trait {
                return false;
            }
            let magic = infer.index.declared_members(&class.fqn).any(|(_, s)| {
                s.name.eq_ignore_ascii_case("__call")
                    || s.note.as_deref().is_some_and(|n| n.starts_with('@'))
            });
            if magic {
                return false;
            }
        } else if infer
            .stubs
            .methods(&class.fqn)
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case("__call"))
        {
            return false;
        }
        queue.extend(class.parents.iter().map(|(p, _)| p.clone()));
    }
    true
}

/// A call's arguments against its callee's parameters, and its callee's deprecation.
fn call(infer: &Infer<'_>, node: Node<'_>, out: &mut Checked) {
    let Some(args) = node
        .child_by_field_name("arguments")
        .or_else(|| named_children(node).find(|c| c.kind() == "arguments"))
    else {
        return;
    };
    let given: Vec<Node<'_>> = named_children(args).collect();
    // `f(...)` (a first-class callable), `f(...$args)`, named arguments: not counted.
    let countable = given.iter().all(|a| {
        a.kind() == "argument"
            && a.child_by_field_name("name").is_none()
            && !infer.src(*a).trim_start().starts_with("...")
    });
    let at = callee_name(node).unwrap_or(node);
    // A receiver that may be one of several classes: whose method it is isn't certain.
    let receiver = match node.kind() {
        "member_call_expression" | "nullsafe_member_call_expression" => {
            node.child_by_field_name("object")
        }
        _ => None,
    };
    if receiver.is_some_and(|o| {
        infer.type_of(o).without_null().classes().len() > 1
            || (infer.src(o) == "$this" && in_closure(o))
    }) {
        return;
    }
    let Some((signature, name, deprecated, builtin)) = callee(infer, node) else { return };
    if deprecated {
        let kind = if node.kind() == "function_call_expression" { "function" } else { "method" };
        out.deprecated.push(Deprecated { range: at.byte_range(), kind, name: name.clone() });
    }
    if !countable {
        return;
    }
    let Some((required, max)) = arity(&signature) else { return };
    let count = given.len();
    if count < required {
        out.problems.push(Problem {
            range: at.byte_range(),
            kind: "too_few_arguments",
            name: format!("{name}()"),
            arguments: Some((required, count)),
        });
    } else if builtin && node.kind() == "function_call_expression" && max.is_some_and(|m| count > m)
    {
        out.problems.push(Problem {
            range: at.byte_range(),
            kind: "too_many_arguments",
            name: format!("{name}()"),
            arguments: max.map(|m| (m, count)),
        });
    }
}

/// The name a call is marked at (the function's or method's, the class's after `new`).
fn callee_name(node: Node<'_>) -> Option<Node<'_>> {
    match node.kind() {
        "function_call_expression" => node.child_by_field_name("function"),
        "object_creation_expression" => class_name(node),
        _ => node.child_by_field_name("name"),
    }
}

/// What a call calls: its parameters (`(int $a, …)`), its name, whether it's deprecated
/// and whether it's PHP's own (whose extra arguments are an error, not ignored).
fn callee(infer: &Infer<'_>, node: Node<'_>) -> Option<(String, String, bool, bool)> {
    if node.kind() == "function_call_expression" {
        let function = node.child_by_field_name("function")?;
        if !matches!(function.kind(), "name" | "qualified_name") {
            return None;
        }
        let written = infer.src(function);
        if let Some((_, symbol)) = infer.function(written) {
            let deprecated = crate::phpdoc::Facts::says_deprecated(symbol.types.as_deref());
            return Some((
                symbol.detail.clone()?.to_string(),
                symbol.name.to_string(),
                deprecated,
                false,
            ));
        }
        // PHP's fallback: an unqualified function not found in the namespace is the global
        // one (a qualified one isn't PHP's).
        let global = match written.strip_prefix('\\') {
            Some(absolute) => absolute,
            None if !written.contains('\\') => written,
            None => return None,
        };
        let f = infer.stubs.function(global)?;
        let deprecated = infer.stubs.deprecated(f.name).is_some();
        return Some((f.signature(), f.name.to_owned(), deprecated, true));
    }
    let (found, _) = infer.callee(node)?;
    let Found { source, .. } = found;
    match source {
        Source::Index(_, symbol) => {
            // A method by magic (`@method`): its documented parameters may be loose.
            if symbol.note.as_deref().is_some_and(|n| n.starts_with('@')) {
                return None;
            }
            let deprecated = crate::phpdoc::Facts::says_deprecated(symbol.types.as_deref());
            Some((symbol.detail.clone()?.to_string(), symbol.name.to_string(), deprecated, false))
        }
        Source::Stub(class, method) => {
            let deprecated = infer.stubs.deprecated(&format!("{class}::{}", method.name)).is_some();
            Some((method.signature(), method.name.to_owned(), deprecated, true))
        }
        _ => None,
    }
}

/// How many arguments a signature needs and takes at most (`None`: any more, a variadic).
pub fn arity(signature: &str) -> Option<(usize, Option<usize>)> {
    let params = code_intelligence::parameters(signature)?;
    let mut required = 0;
    let mut variadic = false;
    for (i, range) in params.iter().enumerate() {
        let param = &signature[range.clone()];
        if param.contains("...") {
            variadic = true;
            continue;
        }
        // A required parameter after optional ones makes them all needed (PHP's rule:
        // arguments are positional).
        if !param.contains('=') {
            required = i + 1;
        }
    }
    Some((required, (!variadic).then_some(params.len())))
}

#[cfg(test)]
#[path = "../tests/unit/checks.rs"]
mod tests;
