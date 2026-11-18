//! The type inference: what a PHP expression is, from the document's tree and the project's
//! declarations (the index, with what their doc comments say) and PHP's own (the stubs).
//!
//! Asked per question (completion, navigation, hover, a check), on the document's cached
//! tree ([`crate::doctree`]); what one question reads (the classes, the variables) is kept
//! for it alone. Variables are scoped like PHP's: the innermost function, method or closure
//! (an arrow function sees the scope it's written in, a closure its `use`s); a variable at
//! a place is its last binding before it there (an assignment, a `foreach`, a `catch`, a
//! parameter, a `@var`), narrowed by an `instanceof` around it.

pub mod api;
mod members;
mod scope;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use code_intelligence::{FileSymbols, SymbolIndex};
use syntax_ts::tree_sitter::Node;

use crate::types::{NameScope, Ty};

pub use members::{Class, Found, MemberKind, Source};
pub use scope::Binding;

/// How deep an expression is followed (a chain, variables defined from variables).
const MAX_DEPTH: usize = 24;

/// One question's inference over a document.
pub struct Infer<'a> {
    pub text: &'a str,
    pub root: Node<'a>,
    pub index: &'a SymbolIndex,
    /// The document's own declarations (its namespace, imports, types), from its tree.
    pub own: &'a FileSymbols,
    pub stubs: Arc<crate::stubs::Stubs>,
    classes: RefCell<HashMap<String, Option<Rc<Class>>>>,
    depth: Cell<usize>,
}

impl<'a> Infer<'a> {
    pub fn new(
        text: &'a str,
        root: Node<'a>,
        index: &'a SymbolIndex,
        own: &'a FileSymbols,
    ) -> Self {
        Self {
            text,
            root,
            index,
            own,
            stubs: crate::stubs::get(),
            classes: RefCell::new(HashMap::new()),
            depth: Cell::new(0),
        }
    }

    pub fn src(&self, node: Node<'_>) -> &'a str {
        self.text.get(node.byte_range()).unwrap_or("")
    }

    /// A class name written in the document at `at`, resolved (its imports, namespace,
    /// `self` / `static` / `parent`).
    pub fn resolve(&self, name: &str, at: usize) -> String {
        self.own.resolve_class(name.trim(), at, &crate::intelligence::naming())
    }

    /// A type written in the document (a type hint node or PHPDoc text) at `at`.
    pub fn written(&self, text: &str, at: usize) -> Ty {
        let templates = self.templates_at(at);
        let scope = NameScope { file: Some(self.own), at, templates: &templates };
        crate::types::parse(text, &scope).unwrap_or(Ty::Mixed)
    }

    /// The template parameters in scope at `at` of the document (its class's and its
    /// method's `@template`s).
    fn templates_at(&self, at: usize) -> Vec<Box<str>> {
        let mut out = Vec::new();
        for s in self.own.symbols.iter().filter(|s| s.range.start <= at && at <= s.range.end) {
            let facts = crate::phpdoc::Facts::decode(s.types.as_deref());
            out.extend(facts.templates.into_iter().map(|(n, _)| n.into_boxed_str()));
        }
        out
    }

    /// Runs `f` one level deeper (`Mixed` past the limit).
    fn deeper(&self, f: impl FnOnce() -> Ty) -> Ty {
        let depth = self.depth.get();
        if depth >= MAX_DEPTH {
            return Ty::Mixed;
        }
        self.depth.set(depth + 1);
        let ty = f();
        self.depth.set(depth);
        ty
    }

    /// The class around `node` (its body), as a type.
    pub fn this_type(&self, node: Node<'_>) -> Ty {
        let mut current = Some(node);
        while let Some(n) = current {
            if matches!(
                n.kind(),
                "class_declaration"
                    | "trait_declaration"
                    | "enum_declaration"
                    | "interface_declaration"
            ) {
                if let Some(t) = self.own.type_at(n.start_byte().max(1)).or_else(|| {
                    n.child_by_field_name("name")
                        .and_then(|name| self.own.type_at(name.start_byte()))
                }) {
                    return Ty::named(&t.fqn);
                }
                return Ty::Mixed;
            }
            // An anonymous class's `$this`: not the class around it.
            if n.kind() == "anonymous_class" {
                return Ty::Mixed;
            }
            current = n.parent();
        }
        Ty::Mixed
    }

    /// The type of an expression.
    pub fn type_of(&self, node: Node<'_>) -> Ty {
        self.deeper(|| self.expression(node))
    }

    fn expression(&self, node: Node<'_>) -> Ty {
        match node.kind() {
            "variable_name" => {
                let name = self.src(node).trim_start_matches('$');
                if name == "this" {
                    return self.this_type(node);
                }
                self.variable(name, node).map_or(Ty::Mixed, |(t, _)| t)
            }
            "parenthesized_expression" | "error_suppression_expression" | "clone_expression" => {
                named_children(node).last().map_or(Ty::Mixed, |n| self.type_of(n))
            }
            "assignment_expression" | "reference_assignment_expression" => {
                node.child_by_field_name("right").map_or(Ty::Mixed, |r| self.type_of(r))
            }
            "augmented_assignment_expression" => {
                let right =
                    node.child_by_field_name("right").map_or(Ty::Mixed, |r| self.type_of(r));
                let left = node.child_by_field_name("left").map_or(Ty::Mixed, |l| self.type_of(l));
                Ty::union(left.without_null(), right)
            }
            "object_creation_expression" => self.creation(node),
            "member_call_expression" | "nullsafe_member_call_expression" => self.method_call(node),
            "scoped_call_expression" => self.static_call(node),
            "function_call_expression" => self.function_call(node),
            "member_access_expression" | "nullsafe_member_access_expression" => {
                self.property_access(node)
            }
            "scoped_property_access_expression" => self.static_property(node),
            "class_constant_access_expression" => self.constant_access(node),
            "conditional_expression" => {
                let alternative =
                    node.child_by_field_name("alternative").map_or(Ty::Mixed, |a| self.type_of(a));
                match node.child_by_field_name("body") {
                    Some(body) => Ty::union(self.type_of(body), alternative),
                    None => {
                        let condition = node
                            .child_by_field_name("condition")
                            .map_or(Ty::Mixed, |c| self.type_of(c));
                        Ty::union(condition.without_null(), alternative)
                    }
                }
            }
            "binary_expression" => {
                let operator = node.child_by_field_name("operator").map(|o| self.src(o));
                match operator {
                    Some("??") => {
                        let left =
                            node.child_by_field_name("left").map_or(Ty::Mixed, |l| self.type_of(l));
                        let right = node
                            .child_by_field_name("right")
                            .map_or(Ty::Mixed, |r| self.type_of(r));
                        Ty::union(left.without_null(), right)
                    }
                    Some(
                        "instanceof" | "==" | "===" | "!=" | "!==" | "<" | ">" | "<=" | ">=" | "&&"
                        | "||" | "and" | "or" | "xor",
                    ) => Ty::Scalar("bool".into()),
                    Some(".") => Ty::Scalar("string".into()),
                    _ => Ty::Mixed,
                }
            }
            "match_expression" => {
                let mut ty: Option<Ty> = None;
                let Some(body) = node.child_by_field_name("body") else { return Ty::Mixed };
                for arm in named_children(body) {
                    if let Some(value) = arm.child_by_field_name("return_expression") {
                        let t = self.type_of(value);
                        ty = Some(match ty {
                            Some(prev) => Ty::union(prev, t),
                            None => t,
                        });
                    }
                }
                ty.unwrap_or(Ty::Mixed)
            }
            "subscript_expression" => {
                let mut parts = named_children(node);
                let Some(object) = parts.next() else { return Ty::Mixed };
                let key = parts.next().and_then(|k| string_value(self.text, k));
                self.type_of(object).at_key(key.as_deref())
            }
            "cast_expression" => {
                let ty = node.child_by_field_name("type").map(|t| self.src(t).to_ascii_lowercase());
                match ty.as_deref() {
                    Some("int" | "integer") => Ty::Scalar("int".into()),
                    Some("bool" | "boolean") => Ty::Scalar("bool".into()),
                    Some("float" | "double" | "real") => Ty::Scalar("float".into()),
                    Some("string" | "binary") => Ty::Scalar("string".into()),
                    Some("array") => Ty::Array(Box::new(Ty::Mixed), Box::new(Ty::Mixed)),
                    Some("object") => Ty::Scalar("object".into()),
                    _ => Ty::Mixed,
                }
            }
            "string" | "encapsed_string" | "heredoc" | "nowdoc" => Ty::Scalar("string".into()),
            "integer" => Ty::Scalar("int".into()),
            "float" => Ty::Scalar("float".into()),
            "boolean" => Ty::Scalar("bool".into()),
            "null" => Ty::null(),
            "array_creation_expression" => self.array_literal(node),
            "anonymous_function" | "arrow_function" => Ty::named("Closure"),
            "unary_op_expression" if self.src(node).starts_with('!') => Ty::Scalar("bool".into()),
            _ => Ty::Mixed,
        }
    }

    /// `[$a, 'k' => $b]`: a shape when its keys are written (or positions), else an array of
    /// its values' type.
    fn array_literal(&self, node: Node<'_>) -> Ty {
        const MAX_FIELDS: usize = 32;
        let elements: Vec<Node<'_>> =
            named_children(node).filter(|c| c.kind() == "array_element_initializer").collect();
        if elements.is_empty() {
            return Ty::Array(Box::new(Ty::Mixed), Box::new(Ty::Mixed));
        }
        let mut fields: Vec<(Box<str>, Ty)> = Vec::new();
        let mut position = 0usize;
        let mut shaped = elements.len() <= MAX_FIELDS;
        let mut values: Option<Ty> = None;
        for element in elements {
            let parts: Vec<Node<'_>> = named_children(element).collect();
            let (key, value) = match parts.as_slice() {
                [value] => (None, *value),
                [key, value] => (Some(*key), *value),
                _ => {
                    shaped = false;
                    continue;
                }
            };
            let ty = self.type_of(value);
            values = Some(match values {
                Some(prev) if prev == ty => prev,
                Some(prev) => Ty::union(prev, ty.clone()),
                None => ty.clone(),
            });
            if !shaped {
                continue;
            }
            let key = match key {
                None => {
                    position += 1;
                    Some((position - 1).to_string())
                }
                Some(k) => string_value(self.text, k),
            };
            match key {
                Some(key) => fields.push((key.into(), ty)),
                None => shaped = false,
            }
        }
        if shaped {
            Ty::Shape(fields)
        } else {
            Ty::Array(Box::new(Ty::Mixed), Box::new(values.unwrap_or(Ty::Mixed)))
        }
    }

    /// A function written as a value: its parameters' and its result's types (the result
    /// written, else an arrow function's body).
    pub fn closure_signature(&self, node: Node<'_>) -> Ty {
        let params = node
            .child_by_field_name("parameters")
            .map(|ps| {
                named_children(ps)
                    .map(|p| {
                        p.child_by_field_name("type")
                            .map_or(Ty::Mixed, |t| self.written(self.src(t), t.start_byte()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ret = match node.child_by_field_name("return_type") {
            Some(r) => self.written(self.src(r), r.start_byte()),
            None if node.kind() == "arrow_function" => {
                node.child_by_field_name("body").map_or(Ty::Mixed, |b| self.type_of(b))
            }
            None => Ty::Mixed,
        };
        Ty::Callable(params, Box::new(ret))
    }

    /// `new X(…)` (`static` / `self`: the class around it).
    fn creation(&self, node: Node<'_>) -> Ty {
        let Some(name) = named_children(node)
            .find(|c| matches!(c.kind(), "name" | "qualified_name" | "relative_scope"))
        else {
            return Ty::Mixed;
        };
        let written = self.src(name);
        if written.eq_ignore_ascii_case("static") || written.eq_ignore_ascii_case("self") {
            return self.this_type(node);
        }
        Ty::named(&self.resolve(written, name.start_byte()))
    }

    /// The class named by a scoped access's scope (`X::`, `static::`, `parent::`, `$x::`).
    fn scope_class(&self, node: Node<'_>) -> Ty {
        let Some(scope) = node.child_by_field_name("scope").or_else(|| node.named_child(0)) else {
            return Ty::Mixed;
        };
        match scope.kind() {
            "name" | "qualified_name" => {
                Ty::named(&self.resolve(self.src(scope), scope.start_byte()))
            }
            "relative_scope" => {
                let word = self.src(scope).to_ascii_lowercase();
                let this = self.this_type(node);
                if word == "parent" {
                    let Some(class) = this.class() else { return Ty::Mixed };
                    return self
                        .class(class)
                        .and_then(|c| {
                            c.parents
                                .first()
                                .map(|(p, args)| Ty::Named(p.as_str().into(), args.clone()))
                        })
                        .unwrap_or(Ty::Mixed);
                }
                this
            }
            _ => self.type_of(scope),
        }
    }

    fn method_call(&self, node: Node<'_>) -> Ty {
        let Some(object) = node.child_by_field_name("object") else { return Ty::Mixed };
        let Some(name) = node.child_by_field_name("name").filter(|n| n.kind() == "name") else {
            return Ty::Mixed;
        };
        let receiver = self.type_of(object).without_null();
        let args = node.child_by_field_name("arguments");
        self.call_on(&receiver, self.src(name), args)
    }

    fn static_call(&self, node: Node<'_>) -> Ty {
        let Some(name) = node.child_by_field_name("name").filter(|n| n.kind() == "name") else {
            return Ty::Mixed;
        };
        let receiver = self.scope_class(node);
        let args = node.child_by_field_name("arguments");
        match self.static_method(&receiver, self.src(name)) {
            Some((found, owner)) => self.member_type(&found, &owner, args),
            None => Ty::Mixed,
        }
    }

    /// What calling method `name` on `receiver` gives: the class its argument names when a
    /// framework says so (`$container->get(Mailer::class)`, or a service's id the project's
    /// framework knows), else its declared type.
    pub fn call_on(&self, receiver: &Ty, name: &str, args: Option<Node<'_>>) -> Ty {
        let Some((found, owner)) = self.method_on(receiver, name) else { return Ty::Mixed };
        if let Some(class) = found.owner.class()
            && let Some(rule) = code_intelligence::calls::class_argument(class, name)
            && let Some(named) = self.class_argument(args, rule)
        {
            return named;
        }
        self.member_type(&found, &owner, args)
    }

    /// The class the call's argument names, as `rule` says: `X::class`, or a name (a quoted
    /// one, nothing interpolated) the project's table has.
    fn class_argument(
        &self,
        args: Option<Node<'_>>,
        rule: code_intelligence::calls::ClassArgumentCall,
    ) -> Option<Ty> {
        let arg = named_children(args?).filter(|a| a.kind() == "argument").nth(rule.argument)?;
        let value = named_children(arg).last()?;
        if let Ty::ClassString(class) = self.type_of(value) {
            return Some(*class);
        }
        let table = rule.names?;
        let written = self.src(value);
        let quote = written.chars().next().filter(|q| *q == '\'' || *q == '"')?;
        let name = written.strip_prefix(quote)?.strip_suffix(quote)?;
        if name.contains(['$', '{', '\\']) {
            return None;
        }
        Some(Ty::named(self.index.named_type(table, name)?))
    }

    fn property_access(&self, node: Node<'_>) -> Ty {
        let Some(object) = node.child_by_field_name("object") else { return Ty::Mixed };
        let Some(name) = node.child_by_field_name("name").filter(|n| n.kind() == "name") else {
            return Ty::Mixed;
        };
        let receiver = self.type_of(object).without_null();
        self.property_of(&receiver, self.src(name))
    }

    /// The type of property `name` of `receiver`.
    pub fn property_of(&self, receiver: &Ty, name: &str) -> Ty {
        for (class, class_args) in receiver.classes() {
            let owner = Ty::Named(class.into(), class_args.to_vec());
            if let Some(found) = self.find_member(&owner, name, MemberKind::Property) {
                return self.member_type(&found, &owner, None);
            }
        }
        Ty::Mixed
    }

    fn static_property(&self, node: Node<'_>) -> Ty {
        let Some(name) = node.child_by_field_name("name") else { return Ty::Mixed };
        let receiver = self.scope_class(node);
        self.property_of(&receiver, self.src(name).trim_start_matches('$'))
    }

    /// `X::CASE` (an enum's case: the enum), `X::class` (its name).
    fn constant_access(&self, node: Node<'_>) -> Ty {
        let Some(name) = named_children(node).last() else { return Ty::Mixed };
        let receiver = self.scope_class(node);
        let constant = self.src(name);
        if constant.eq_ignore_ascii_case("class") {
            return match receiver {
                Ty::Named(..) => Ty::ClassString(Box::new(receiver)),
                _ => Ty::Scalar("class-string".into()),
            };
        }
        let Some(class) = receiver.class() else { return Ty::Mixed };
        match self.find_member(&receiver, constant, MemberKind::Constant) {
            Some(Found { source: Source::Index(_, s), .. })
                if s.kind == code_intelligence::SymbolKind::EnumCase =>
            {
                Ty::named(s.container.as_deref().unwrap_or(class))
            }
            _ => Ty::Mixed,
        }
    }

    fn function_call(&self, node: Node<'_>) -> Ty {
        let Some(function) = node.child_by_field_name("function") else { return Ty::Mixed };
        if !matches!(function.kind(), "name" | "qualified_name") {
            return Ty::Mixed;
        }
        let args = node.child_by_field_name("arguments");
        let name = self.src(function);
        self.function_type(name, function.start_byte(), args)
    }
}

/// The named children of `node`.
pub fn named_children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    syntax_ts::named_children(node)
}

/// A string literal's value (`'user'` → `user`), if it's a plain one.
pub fn string_value(text: &str, node: Node<'_>) -> Option<String> {
    match node.kind() {
        "string" | "encapsed_string" => {
            let inner: Vec<_> = named_children(node).collect();
            match inner.as_slice() {
                [content] if content.kind() == "string_content" => {
                    text.get(content.byte_range()).map(str::to_owned)
                }
                [] => Some(String::new()),
                _ => None,
            }
        }
        "integer" => text.get(node.byte_range()).map(str::to_owned),
        _ => None,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/infer.rs"]
mod tests;
