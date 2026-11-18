//! Variables: their scope (the innermost function, method or closure; an arrow function
//! sees the scope it's written in, a closure only its `use`s), their last binding before a
//! place, and the `instanceof` checks narrowing them there.

use syntax_ts::tree_sitter::Node;

use super::{Infer, MemberKind, named_children, string_value};
use crate::types::Ty;

/// What gave a variable its value at a place.
#[derive(Debug, Clone, Copy)]
pub enum Binding<'t> {
    /// An assignment (the whole expression: `??=` keeps what was there).
    Assigned(Node<'t>),
    /// `[$a, 'k' => $b] = …`: the value, the key or the position.
    Destructured(Node<'t>, Option<Node<'t>>, usize),
    /// `foreach (… as $k => $v)`: the iterated value, and whether it's the key.
    Iterated(Node<'t>, bool),
    /// `catch (T $e)`: the type.
    Caught(Node<'t>),
    /// A parameter of its function (the parameter's node, the function's).
    Parameter(Node<'t>, Node<'t>),
    /// `/** @var T $x */` (the comment).
    Documented(Node<'t>),
    /// `$x instanceof T` around the place, or `assert($x instanceof T)` before it (the
    /// class's name).
    Narrowed(Node<'t>),
    /// `global $x`, `static $x`: not known.
    Unknown,
}

/// Where a scope starts its own variables.
fn is_scope(kind: &str) -> bool {
    matches!(
        kind,
        "function_definition" | "method_declaration" | "anonymous_function" | "arrow_function"
    )
}

/// What isn't part of the scope it's written in (its variables are its own).
fn is_nested(kind: &str) -> bool {
    is_scope(kind)
        || matches!(
            kind,
            "class_declaration"
                | "trait_declaration"
                | "interface_declaration"
                | "enum_declaration"
        )
}

impl<'a> Infer<'a> {
    /// Variable `$name`'s type where `at` is (a node of the document), and what bound it.
    pub fn variable<'t>(&self, name: &str, at: Node<'t>) -> Option<(Ty, Binding<'t>)> {
        let binding = self.binding(name, at, at.start_byte())?;
        let ty = self.deeper(|| self.bound_type(name, &binding));
        Some((ty, binding))
    }

    /// The binding of `$name` in force at `pos` (`at`: the node there).
    pub fn binding<'t>(&self, name: &str, at: Node<'t>, pos: usize) -> Option<Binding<'t>> {
        let scope = std::iter::successors(at.parent(), |n| n.parent()).find(|n| is_scope(n.kind()));
        let scope = match scope {
            Some(s) => s,
            None => return self.in_body(self.root_of(at), name, pos, at),
        };
        let params = scope.child_by_field_name("parameters");
        let param = params.and_then(|ps| {
            named_children(ps).find(|p| {
                p.child_by_field_name("name")
                    .is_some_and(|n| self.src(n).trim_start_matches('$') == name)
            })
        });
        match scope.kind() {
            "arrow_function" => match param {
                Some(p) => Some(Binding::Parameter(p, scope)),
                // Not its own: the scope it's written in, where it's written.
                None => self.binding(name, scope, scope.start_byte()),
            },
            "anonymous_function" => {
                if let Some(p) = param {
                    return Some(Binding::Parameter(p, scope));
                }
                let used = named_children(scope)
                    .filter(|c| c.kind() == "anonymous_function_use_clause")
                    .flat_map(named_children)
                    .any(|v| self.src(v).trim_start_matches('&').trim_start_matches('$') == name);
                let body = scope.child_by_field_name("body")?;
                let inside = self.in_body(body, name, pos, at);
                match inside {
                    Some(b) => Some(b),
                    None if used => self.binding(name, scope, scope.start_byte()),
                    None => None,
                }
            }
            _ => {
                let body = scope.child_by_field_name("body");
                let inside = body.and_then(|b| self.in_body(b, name, pos, at));
                inside.or_else(|| param.map(|p| Binding::Parameter(p, scope)))
            }
        }
    }

    fn root_of<'t>(&self, at: Node<'t>) -> Node<'t> {
        std::iter::successors(Some(at), |n| n.parent()).last().unwrap_or(at)
    }

    /// The last binding of `$name` in `body` before `pos` (nested scopes left out), or the
    /// `instanceof` narrowing `at`, whichever comes later.
    fn in_body<'t>(
        &self,
        body: Node<'t>,
        name: &str,
        pos: usize,
        at: Node<'t>,
    ) -> Option<Binding<'t>> {
        let mut last: Option<(usize, Binding<'t>)> = None;
        let mut record = |at: usize, b: Binding<'t>| {
            if last.as_ref().is_none_or(|(p, _)| at >= *p) {
                last = Some((at, b));
            }
        };
        let mut stack: Vec<Node<'t>> = named_children(body).collect();
        stack.reverse();
        while let Some(node) = stack.pop() {
            if node.start_byte() >= pos {
                continue;
            }
            let kind = node.kind();
            if is_nested(kind) {
                continue;
            }
            match kind {
                "assignment_expression"
                | "reference_assignment_expression"
                | "augmented_assignment_expression" => {
                    if let Some(left) = node.child_by_field_name("left") {
                        if self.is_variable(left, name) && node.end_byte() <= pos {
                            let documented = self.var_comment(node, name);
                            match documented {
                                Some(c) => record(node.start_byte(), Binding::Documented(c)),
                                None => record(node.start_byte(), Binding::Assigned(node)),
                            }
                        } else if matches!(
                            left.kind(),
                            "list_literal" | "array_creation_expression"
                        ) && node.end_byte() <= pos
                            && let Some(right) = node.child_by_field_name("right")
                            && let Some((key, position)) = self.destructured(left, name)
                        {
                            record(node.start_byte(), Binding::Destructured(right, key, position));
                        }
                    }
                }
                "foreach_statement" => {
                    let mut parts = named_children(node);
                    let iterable = parts.next();
                    let target = parts.next();
                    if let (Some(iterable), Some(target)) = (iterable, target)
                        && target.end_byte() <= pos
                    {
                        let (key, value) = match target.kind() {
                            "pair" => (target.named_child(0), target.named_child(1)),
                            _ => (None, Some(target)),
                        };
                        if key.is_some_and(|k| self.is_variable(k, name)) {
                            record(target.start_byte(), Binding::Iterated(iterable, true));
                        }
                        if let Some(value) = value {
                            if self.is_variable(value, name) {
                                record(target.start_byte(), Binding::Iterated(iterable, false));
                            } else if value.kind() == "list_literal"
                                && let Some((k, i)) = self.destructured(value, name)
                            {
                                let _ = (k, i);
                                record(target.start_byte(), Binding::Iterated(iterable, false));
                            }
                        }
                    }
                }
                "catch_clause" => {
                    if let (Some(var), Some(ty)) =
                        (node.child_by_field_name("name"), node.child_by_field_name("type"))
                        && self.is_variable(var, name)
                        && var.end_byte() <= pos
                    {
                        record(var.start_byte(), Binding::Caught(ty));
                    }
                }
                "comment" => {
                    let text = self.src(node);
                    if text.starts_with("/**") && text.contains("@var") {
                        let tags = crate::phpdoc::tags(text);
                        if let Some((Some(var), _)) = &tags.var
                            && var == name
                        {
                            record(node.start_byte(), Binding::Documented(node));
                        }
                    }
                }
                "global_declaration" | "function_static_declaration" => {
                    if named_children(node).any(|v| {
                        let v = if v.kind() == "static_variable_declaration" {
                            v.child_by_field_name("name").unwrap_or(v)
                        } else {
                            v
                        };
                        self.is_variable(v, name)
                    }) {
                        record(node.start_byte(), Binding::Unknown);
                    }
                }
                "function_call_expression" => {
                    // `assert($x instanceof T)`.
                    let function = node.child_by_field_name("function").map(|f| self.src(f));
                    if function
                        .is_some_and(|f| f.trim_start_matches('\\').eq_ignore_ascii_case("assert"))
                        && let Some(args) = node.child_by_field_name("arguments")
                        && let Some(arg) =
                            named_children(args).next().and_then(|a| a.named_child(0))
                        && let Some(class) = self.instanceof(arg, name)
                    {
                        record(node.start_byte(), Binding::Narrowed(class));
                    }
                }
                "if_statement" if node.end_byte() <= pos => {
                    // `if (!$x instanceof T) { return; }`: after it, `$x` is a T.
                    if let Some(condition) = node.child_by_field_name("condition")
                        && let Some(class) = self.negated_instanceof(condition, name)
                        && node.child_by_field_name("body").is_some_and(|b| exits(self, b))
                        && node.child_by_field_name("alternative").is_none()
                    {
                        record(node.end_byte(), Binding::Narrowed(class));
                    }
                }
                _ => {}
            }
            let mut children: Vec<Node<'t>> = named_children(node).collect();
            children.reverse();
            stack.extend(children);
        }
        // Narrowed around the place (`if ($x instanceof T) { … here … }`).
        if let Some((start, class)) = self.narrowing(name, at, body) {
            record(start, Binding::Narrowed(class));
        }
        last.map(|(_, b)| b)
    }

    /// Whether `node` is variable `$name`.
    fn is_variable(&self, node: Node<'_>, name: &str) -> bool {
        let node = if node.kind() == "by_ref" { node.named_child(0).unwrap_or(node) } else { node };
        node.kind() == "variable_name" && self.src(node).trim_start_matches('$') == name
    }

    /// Where `$name` is in a destructuring list: its key, or its position.
    fn destructured<'t>(&self, list: Node<'t>, name: &str) -> Option<(Option<Node<'t>>, usize)> {
        let mut position = 0;
        let mut pending_key: Option<Node<'t>> = None;
        let mut cursor = list.walk();
        for child in list.children(&mut cursor) {
            match child.kind() {
                "," => {
                    position += 1;
                    pending_key = None;
                }
                "=>" => {}
                _ if !child.is_named() => {}
                "variable_name" | "by_ref" => {
                    if self.is_variable(child, name) {
                        return Some((pending_key, position));
                    }
                }
                "array_element_initializer" => {
                    let parts: Vec<Node<'t>> = named_children(child).collect();
                    match parts.as_slice() {
                        [value] if self.is_variable(*value, name) => return Some((None, position)),
                        [key, value] if self.is_variable(*value, name) => {
                            return Some((Some(*key), position));
                        }
                        _ => {}
                    }
                }
                _ => pending_key = Some(child),
            }
        }
        None
    }

    /// A `/** @var T */` (or `@var T $name`) right before the statement of an assignment.
    fn var_comment<'t>(&self, assignment: Node<'t>, name: &str) -> Option<Node<'t>> {
        let statement = assignment.parent().filter(|p| p.kind() == "expression_statement")?;
        let comment = statement.prev_named_sibling().filter(|c| c.kind() == "comment")?;
        let text = self.src(comment);
        if !text.starts_with("/**") {
            return None;
        }
        match crate::phpdoc::tags(text).var {
            Some((None, _)) => Some(comment),
            Some((Some(var), _)) if var == name => Some(comment),
            _ => None,
        }
    }

    /// `$name instanceof T` in a condition (alone or joined with `&&`): `T`'s name.
    fn instanceof<'t>(&self, condition: Node<'t>, name: &str) -> Option<Node<'t>> {
        match condition.kind() {
            "parenthesized_expression" => self.instanceof(condition.named_child(0)?, name),
            "binary_expression" => {
                let operator = condition.child_by_field_name("operator").map(|o| self.src(o));
                let left = condition.child_by_field_name("left")?;
                let right = condition.child_by_field_name("right")?;
                match operator? {
                    "instanceof" if self.is_variable(left, name) => {
                        matches!(right.kind(), "name" | "qualified_name").then_some(right)
                    }
                    "&&" | "and" => {
                        self.instanceof(left, name).or_else(|| self.instanceof(right, name))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// `!$name instanceof T` / `!($name instanceof T)`: `T`'s name.
    fn negated_instanceof<'t>(&self, condition: Node<'t>, name: &str) -> Option<Node<'t>> {
        let inner = match condition.kind() {
            "parenthesized_expression" => condition.named_child(0)?,
            _ => condition,
        };
        if inner.kind() != "unary_op_expression" || !self.src(inner).starts_with('!') {
            return None;
        }
        let operand = inner.named_child(0)?;
        self.instanceof(operand, name)
    }

    /// The `instanceof` check around `at` (inside `body`): where its branch starts, and the
    /// class.
    fn narrowing<'t>(&self, name: &str, at: Node<'t>, body: Node<'t>) -> Option<(usize, Node<'t>)> {
        let mut child = at;
        while let Some(parent) = child.parent() {
            if parent.id() == body.id() || is_nested(parent.kind()) {
                return None;
            }
            let inside = |field: &str| {
                parent.child_by_field_name(field).is_some_and(|b| b.id() == child.id())
            };
            let found = match parent.kind() {
                "if_statement" | "while_statement" if inside("body") => {
                    parent.child_by_field_name("condition").and_then(|c| self.instanceof(c, name))
                }
                "conditional_expression" if inside("body") => {
                    parent.child_by_field_name("condition").and_then(|c| self.instanceof(c, name))
                }
                "binary_expression" if inside("right") => {
                    let operator = parent.child_by_field_name("operator").map(|o| self.src(o));
                    match operator {
                        Some("&&" | "and") => parent
                            .child_by_field_name("left")
                            .and_then(|l| self.instanceof(l, name)),
                        _ => None,
                    }
                }
                _ => None,
            };
            if let Some(class) = found {
                return Some((child.start_byte(), class));
            }
            child = parent;
        }
        None
    }

    /// The type a binding gives `$name`.
    pub(super) fn bound_type(&self, name: &str, binding: &Binding<'_>) -> Ty {
        match *binding {
            Binding::Assigned(node) => self.type_of(node),
            Binding::Destructured(value, key, position) => {
                let ty = self.type_of(value);
                let key = match key {
                    Some(k) => string_value(self.text, k),
                    None => Some(position.to_string()),
                };
                ty.at_key(key.as_deref())
            }
            Binding::Iterated(iterable, key) => {
                let ty = self.type_of(iterable);
                if key { ty.key() } else { self.element_of(&ty) }
            }
            Binding::Caught(ty) => self.written(self.src(ty), ty.start_byte()),
            Binding::Parameter(param, function) => self.parameter_type(name, param, function),
            Binding::Documented(comment) => match crate::phpdoc::tags(self.src(comment)).var {
                Some((_, ty)) => self.written(&ty, comment.start_byte()),
                None => Ty::Mixed,
            },
            Binding::Narrowed(class) => {
                Ty::named(&self.resolve(self.src(class), class.start_byte()))
            }
            Binding::Unknown => Ty::Mixed,
        }
    }

    /// What iterating over `ty` gives: an array's values, a generic collection's (its own
    /// arguments, or those it passes to `IteratorAggregate` / `Iterator` / `Traversable`),
    /// else what its `getIterator()` / `current()` give.
    pub fn element_of(&self, ty: &Ty) -> Ty {
        let direct = ty.element();
        if !direct.is_mixed() {
            return direct;
        }
        for (class, args) in ty.classes() {
            let owner = Ty::Named(class.into(), args.to_vec());
            for method in ["getIterator", "current"] {
                if let Some(found) = self.find_member(&owner, method, MemberKind::Method) {
                    let result = self.member_type(&found, &owner, None);
                    let element = if method == "current" { result } else { result.element() };
                    if !element.is_mixed() {
                        return element;
                    }
                }
            }
        }
        Ty::Mixed
    }

    /// A parameter's type: its doc comment's when that says more (a shape, a generic), else
    /// the written one; an untyped closure's parameter from the callable its call expects.
    fn parameter_type(&self, name: &str, param: Node<'_>, function: Node<'_>) -> Ty {
        let written =
            param.child_by_field_name("type").map(|t| self.written(self.src(t), t.start_byte()));
        let variadic = param.kind() == "variadic_parameter";
        let doc = self.doc_comment(function).and_then(|c| {
            let tags = crate::phpdoc::tags(self.src(c));
            tags.params.into_iter().find(|(n, _)| n == name).map(|(_, t)| (t, c.start_byte()))
        });
        let doc = doc.map(|(t, at)| self.written(&t, at));
        let ty = match (doc, written) {
            (Some(doc), Some(written)) if doc.is_mixed() => written,
            (Some(doc), _) if !doc.is_mixed() => doc,
            (_, Some(written)) => written,
            _ => {
                if matches!(function.kind(), "arrow_function" | "anonymous_function") {
                    self.closure_parameter(function, param)
                } else {
                    Ty::Mixed
                }
            }
        };
        if variadic { Ty::Array(Box::new(Ty::Mixed), Box::new(ty)) } else { ty }
    }

    /// The doc comment right before a declaration (`/** … */`), skipping its attributes.
    pub fn doc_comment<'t>(&self, node: Node<'t>) -> Option<Node<'t>> {
        let mut target = node;
        // An assignment of a closure: the statement's comment.
        if matches!(node.kind(), "anonymous_function" | "arrow_function") {
            target = std::iter::successors(node.parent(), |n| n.parent())
                .find(|n| n.kind() == "expression_statement")?;
        }
        let comment = target.prev_named_sibling().filter(|c| c.kind() == "comment")?;
        let between = self.text.get(comment.end_byte()..target.start_byte())?;
        (between.trim().is_empty() && self.src(comment).starts_with("/**")).then_some(comment)
    }

    /// An untyped parameter of a function passed as an argument: what the callee's
    /// `callable(…)` / `Closure(…)` parameter says at its position.
    fn closure_parameter(&self, function: Node<'_>, param: Node<'_>) -> Ty {
        let position = function
            .child_by_field_name("parameters")
            .map(|ps| named_children(ps).take_while(|p| p.id() != param.id()).count())
            .unwrap_or(0);
        let Some(argument) = function.parent().filter(|p| p.kind() == "argument") else {
            return Ty::Mixed;
        };
        let Some(arguments) = argument.parent() else { return Ty::Mixed };
        let index = named_children(arguments).take_while(|a| a.id() != argument.id()).count();
        let Some(call) = arguments.parent() else { return Ty::Mixed };
        let expected = self.expected_argument(call, index);
        match expected {
            Ty::Callable(params, _) => params.get(position).cloned().unwrap_or(Ty::Mixed),
            _ => Ty::Mixed,
        }
    }
}

/// Whether a block always leaves (its last statement returns, throws, continues, breaks).
fn exits(infer: &Infer<'_>, block: Node<'_>) -> bool {
    let last = if block.kind() == "compound_statement" {
        named_children(block).last()
    } else {
        Some(block)
    };
    last.is_some_and(|s| match s.kind() {
        "return_statement" | "throw_expression" | "continue_statement" | "break_statement" => true,
        "expression_statement" => {
            let text = infer.src(s);
            text.starts_with("throw") || text.starts_with("exit") || text.starts_with("die")
        }
        _ => false,
    })
}
