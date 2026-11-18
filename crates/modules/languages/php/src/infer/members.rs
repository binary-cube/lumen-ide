//! Classes and their members as the inference sees them: the index's (with what their doc
//! comments say: templates, the generic arguments of their parents, mixins) and PHP's own
//! (the stubs), a member found through the ancestors with the template parameters it was
//! declared with replaced by the receiver's arguments.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::rc::Rc;

use code_intelligence::{Symbol, SymbolKind};
use syntax_ts::tree_sitter::Node;

use super::{Infer, named_children};
use crate::phpdoc::Facts;
use crate::types::{NameScope, Ty};

/// At most how many ancestors are walked.
const MAX_ANCESTORS: usize = 32;

/// A class (interface, trait, enum) as the inference reads it.
#[derive(Debug)]
pub struct Class {
    pub fqn: String,
    pub kind: SymbolKind,
    /// Its `@template`s, with their bounds.
    pub templates: Vec<(Box<str>, Option<Ty>)>,
    /// What it extends, implements and uses, each with its generic arguments (in terms of
    /// this class's templates).
    pub parents: Vec<(String, Vec<Ty>)>,
    pub mixins: Vec<Ty>,
    /// An enum's backing type (`string` / `int`); `None` for a pure enum or another kind.
    pub backed: Option<Ty>,
    /// Declared in the project (the index), or by PHP (the stubs).
    pub in_index: bool,
}

/// What kind of member is looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    Method,
    Property,
    /// A class constant or an enum's case.
    Constant,
}

/// Where a member is declared.
#[derive(Debug, Clone)]
pub enum Source<'a> {
    Index(&'a Path, &'a Symbol),
    /// A built-in class's method (its class, as the stubs name it).
    Stub(&'static str, crate::stubs::Method),
    /// A built-in class's constant.
    StubConstant,
    /// A built-in class's property.
    StubProperty(crate::stubs::Property),
    /// An enum's own `name` / `value`.
    EnumProperty(Ty),
}

/// A member found on a receiver.
#[derive(Debug, Clone)]
pub struct Found<'a> {
    pub source: Source<'a>,
    /// The declaring class with its arguments (as reached from the receiver).
    pub owner: Ty,
    /// The declaring class's templates and what they are for this receiver.
    pub substitution: Vec<(Box<str>, Ty)>,
}

impl<'a> Infer<'a> {
    /// Class `fqn` as read from the index or the stubs (once per question).
    pub fn class(&self, fqn: &str) -> Option<Rc<Class>> {
        let key = fqn.trim_start_matches('\\').to_ascii_lowercase();
        if let Some(found) = self.classes.borrow().get(&key) {
            return found.clone();
        }
        let class = self.load(fqn.trim_start_matches('\\')).map(Rc::new);
        self.classes.borrow_mut().insert(key, class.clone());
        class
    }

    fn load(&self, fqn: &str) -> Option<Class> {
        if let Some((path, symbol)) = self.index.type_by_fqn(fqn) {
            let file = self.index.file(path);
            let facts = Facts::decode(symbol.types.as_deref());
            let names: Vec<Box<str>> =
                facts.templates.iter().map(|(n, _)| n.clone().into_boxed_str()).collect();
            let scope = NameScope { file, at: symbol.offset, templates: &names };
            let parse = |t: &str| crate::types::parse(t, &scope).unwrap_or(Ty::Mixed);
            let templates = facts
                .templates
                .iter()
                .map(|(n, bound)| (n.clone().into_boxed_str(), bound.as_deref().map(parse)))
                .collect();
            let generic: Vec<Ty> = facts.extends.iter().map(|e| parse(e)).collect();
            let mut parents: Vec<(String, Vec<Ty>)> = symbol
                .parents
                .iter()
                .map(|p| {
                    let args = generic
                        .iter()
                        .find_map(|g| match g {
                            Ty::Named(name, args) if name.eq_ignore_ascii_case(p) => {
                                Some(args.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    (p.to_string(), args)
                })
                .collect();
            let backed = facts.backed.as_deref().map(parse);
            if symbol.kind == SymbolKind::Enum {
                // What every enum implements.
                parents.push(("UnitEnum".into(), Vec::new()));
                if backed.is_some() {
                    parents.push(("BackedEnum".into(), Vec::new()));
                }
            }
            return Some(Class {
                fqn: symbol.fqn.to_string(),
                kind: symbol.kind,
                templates,
                parents,
                mixins: facts.mixins.iter().map(|m| parse(m)).collect(),
                backed,
                in_index: true,
            });
        }
        let class = self.stubs.class(fqn)?;
        let kind = match class.kind {
            "interface" => SymbolKind::Interface,
            "trait" => SymbolKind::Trait,
            "enum" => SymbolKind::Enum,
            _ => SymbolKind::Class,
        };
        Some(Class {
            fqn: class.name.to_owned(),
            kind,
            templates: Vec::new(),
            parents: class.ancestors().map(|a| (a.to_owned(), Vec::new())).collect(),
            mixins: Vec::new(),
            backed: None,
            in_index: false,
        })
    }

    /// Member `name` of `receiver` (a class with its arguments): declared in it or in one
    /// of its ancestors (nearest first), then in its mixins.
    pub fn find_member(&self, receiver: &Ty, name: &str, kind: MemberKind) -> Option<Found<'a>> {
        let Ty::Named(fqn, args) = receiver else { return None };
        let mut queue: VecDeque<(String, Vec<Ty>)> =
            VecDeque::from([(fqn.to_string(), args.clone())]);
        // The mixins met, searched once the ancestors are (each type once in all).
        let mut mixins: VecDeque<(String, Vec<Ty>)> = VecDeque::new();
        let mut seen: HashSet<String> = HashSet::new();
        loop {
            let (current, current_args) = queue.pop_front().or_else(|| mixins.pop_front())?;
            if seen.len() > 2 * MAX_ANCESTORS {
                return None;
            }
            if !seen.insert(current.to_ascii_lowercase()) {
                continue;
            }
            let Some(class) = self.class(&current) else { continue };
            let substitution = substitution(&class, &current_args);
            let owner = Ty::Named(class.fqn.as_str().into(), current_args.clone());
            if let Some(source) = self.declared(&class, name, kind) {
                return Some(Found { source, owner, substitution });
            }
            let apply = |t: &Ty| substitute(t, &substitution, None, &owner, &owner);
            // PHP's order: the traits it uses (their methods win), then its parent, then the
            // interfaces.
            let mut parents: Vec<(u8, String, Vec<Ty>)> = class
                .parents
                .iter()
                .map(|(p, a)| {
                    let rank = match self.class(p).map(|c| c.kind) {
                        Some(SymbolKind::Trait) => 0,
                        Some(SymbolKind::Interface) => 2,
                        _ => 1,
                    };
                    (rank, p.clone(), a.iter().map(apply).collect())
                })
                .collect();
            parents.sort_by_key(|(rank, _, _)| *rank);
            // Depth-first along traits and parents (a trait's method overrides the parent's).
            for (_, parent, parent_args) in parents.into_iter().rev() {
                queue.push_front((parent, parent_args));
            }
            for mixin in class.mixins.iter().map(apply) {
                if let Ty::Named(m, margs) = mixin {
                    mixins.push_back((m.to_string(), margs));
                }
            }
        }
    }

    /// Member `name` declared in `class` itself.
    fn declared(&self, class: &Class, name: &str, kind: MemberKind) -> Option<Source<'a>> {
        if class.in_index {
            let found = self.index.declared_members(&class.fqn).find(|(_, s)| match kind {
                MemberKind::Method => {
                    s.kind == SymbolKind::Method && s.name.eq_ignore_ascii_case(name)
                }
                MemberKind::Property => s.kind == SymbolKind::Property && s.name == name.into(),
                MemberKind::Constant => {
                    matches!(s.kind, SymbolKind::ClassConstant | SymbolKind::EnumCase)
                        && s.name == name.into()
                }
            });
            if let Some((path, symbol)) = found {
                return Some(Source::Index(path, symbol));
            }
            if kind == MemberKind::Property && class.kind == SymbolKind::Enum {
                match name {
                    "name" => return Some(Source::EnumProperty(Ty::Scalar("string".into()))),
                    "value" => return class.backed.clone().map(Source::EnumProperty),
                    _ => {}
                }
            }
            return None;
        }
        let stub_class = self.stubs.class(&class.fqn)?.name;
        match kind {
            MemberKind::Method => self
                .stubs
                .methods(&class.fqn)
                .iter()
                .find(|m| m.name.eq_ignore_ascii_case(name))
                .map(|m| Source::Stub(stub_class, *m)),
            MemberKind::Constant => self
                .stubs
                .class_constants(&class.fqn)
                .iter()
                .find(|c| **c == name)
                .map(|_| Source::StubConstant),
            MemberKind::Property => self
                .stubs
                .properties(&class.fqn)
                .iter()
                .find(|p| p.name == name)
                .map(|p| Source::StubProperty(*p)),
        }
    }

    /// The type a found member gives (a method's result, a property's value), for
    /// `receiver`; `args`: a call's arguments (for the method's own templates).
    pub fn member_type(&self, found: &Found<'_>, receiver: &Ty, args: Option<Node<'_>>) -> Ty {
        let declared = match &found.source {
            Source::Index(path, symbol) => {
                let file = self.index.file(path);
                let facts = Facts::decode(symbol.types.as_deref());
                let class_templates: Vec<Box<str>> =
                    found.substitution.iter().map(|(n, _)| n.clone()).collect();
                let mut templates = class_templates;
                templates.extend(facts.templates.iter().map(|(n, _)| n.clone().into_boxed_str()));
                let scope = NameScope { file, at: symbol.offset, templates: &templates };
                let native = match symbol.kind {
                    SymbolKind::Method => symbol.detail.as_deref().and_then(return_of),
                    _ => symbol.detail.as_deref().map(str::trim),
                };
                let doc =
                    if symbol.kind == SymbolKind::Method { &facts.returns } else { &facts.var };
                let ty = choose(doc.as_deref(), native, &scope);
                let own = self.method_templates(&facts, &scope, symbol, args);
                let receiver = receiver_or(receiver, &found.owner);
                return substitute(&ty, &found.substitution, Some(&own), receiver, &found.owner)
                    .settled();
            }
            Source::Stub(_, method) => {
                crate::types::parse(method.returns, &NameScope::GLOBAL).unwrap_or(Ty::Mixed)
            }
            Source::StubConstant => Ty::Mixed,
            Source::StubProperty(p) => {
                crate::types::parse(p.ty, &NameScope::GLOBAL).unwrap_or(Ty::Mixed)
            }
            Source::EnumProperty(t) => t.clone(),
        };
        let receiver = receiver_or(receiver, &found.owner);
        substitute(&declared, &found.substitution, None, receiver, &found.owner).settled()
    }

    /// The method's own templates, worked out from the call's arguments: `class-string<T>`
    /// given `X::class`, `T $value` given a typed value, `callable(…): T` given a function
    /// with a return type.
    fn method_templates(
        &self,
        facts: &Facts,
        scope: &NameScope<'_>,
        symbol: &Symbol,
        args: Option<Node<'_>>,
    ) -> Vec<(Box<str>, Ty)> {
        let Some(args) = args.filter(|_| !facts.templates.is_empty()) else { return Vec::new() };
        let params = parameter_names(symbol.detail.as_deref().unwrap_or(""));
        let given: Vec<Node<'_>> =
            named_children(args).filter(|a| a.kind() == "argument").collect();
        let mut out: Vec<(Box<str>, Ty)> = Vec::new();
        for (name, doc) in &facts.params {
            let Some(position) = params.iter().position(|p| p == name) else { continue };
            // By name (`class: X::class`), else by position.
            let arg = given
                .iter()
                .find(|a| a.child_by_field_name("name").is_some_and(|n| self.src(n) == name))
                .or_else(|| {
                    given.get(position).filter(|a| a.child_by_field_name("name").is_none())
                });
            let Some(value) = arg.and_then(|a| named_children(*a).last()) else { continue };
            let Some(ty) = crate::types::parse(doc, scope) else { continue };
            let given = if matches!(value.kind(), "arrow_function" | "anonymous_function") {
                self.closure_signature(value)
            } else {
                self.type_of(value)
            };
            unify(&ty, &given, &facts.templates, &mut out);
        }
        out
    }

    /// What calling function `name` (as written at `at`) gives: the project's (its namespace,
    /// then the global one), then PHP's.
    pub fn function_type(&self, name: &str, at: usize, args: Option<Node<'_>>) -> Ty {
        let _ = at;
        let Some((path, symbol)) = self.function(name) else {
            let bare = name.trim_start_matches('\\');
            return match self.stubs.function(bare) {
                Some(f) => crate::types::parse(f.returns, &NameScope::GLOBAL).unwrap_or(Ty::Mixed),
                None => Ty::Mixed,
            };
        };
        let file = self.index.file(path);
        let facts = Facts::decode(symbol.types.as_deref());
        let templates: Vec<Box<str>> =
            facts.templates.iter().map(|(n, _)| n.clone().into_boxed_str()).collect();
        let scope = NameScope { file, at: symbol.offset, templates: &templates };
        let native = symbol.detail.as_deref().and_then(return_of);
        let ty = choose(facts.returns.as_deref(), native, &scope);
        let own = self.method_templates(&facts, &scope, symbol, args);
        substitute(&ty, &[], Some(&own), &Ty::Mixed, &Ty::Mixed).settled()
    }

    /// The project's function `name` (as written in the document): `use function`d, in the
    /// document's namespace, else global.
    pub fn function(&self, name: &str) -> Option<(&'a Path, &'a Symbol)> {
        if let Some(absolute) = name.strip_prefix('\\') {
            return self.index.function_by_fqn(absolute);
        }
        let imported =
            self.own.uses.iter().find(|u| u.alias.eq_ignore_ascii_case(name) && !u.fqn.is_empty());
        if let Some(import) = imported
            && let Some(found) = self.index.function_by_fqn(&import.fqn)
        {
            return Some(found);
        }
        if let Some(ns) = &self.own.namespace
            && let Some(found) = self.index.function_by_fqn(&format!("{ns}\\{name}"))
        {
            return Some(found);
        }
        self.index.function_by_fqn(name)
    }
}

impl<'a> Infer<'a> {
    /// The callee of a call node and the receiver it's called on: a method (or a
    /// constructor) found through its receiver's type, or a function.
    pub fn callee(&self, call: Node<'_>) -> Option<(Found<'a>, Ty)> {
        match call.kind() {
            "member_call_expression" | "nullsafe_member_call_expression" => {
                let object = call.child_by_field_name("object")?;
                let name = call.child_by_field_name("name").filter(|n| n.kind() == "name")?;
                let receiver = self.type_of(object).without_null();
                self.method_on(&receiver, self.src(name))
            }
            "scoped_call_expression" => {
                let name = call.child_by_field_name("name").filter(|n| n.kind() == "name")?;
                let receiver = self.scope_class(call);
                self.static_method(&receiver, self.src(name))
            }
            "object_creation_expression" => {
                let receiver = self.creation(call);
                self.method_on(&receiver, "__construct")
            }
            "function_call_expression" => {
                let function = call.child_by_field_name("function")?;
                let (path, symbol) = self.function(self.src(function))?;
                let found = Found {
                    source: Source::Index(path, symbol),
                    owner: Ty::Mixed,
                    substitution: Vec::new(),
                };
                Some((found, Ty::Mixed))
            }
            _ => None,
        }
    }

    /// Method `name` called statically on `receiver` (`X::name()`): its own, else the one
    /// PHP's `__callStatic` forwards to — a class with `__callStatic` and a static `query()`
    /// (Laravel's models: `User::where()` is `User::query()->where()`) gets the method of
    /// what `query()` gives.
    pub fn static_method(&self, receiver: &Ty, name: &str) -> Option<(Found<'a>, Ty)> {
        if let Some(found) = self.method_on(receiver, name) {
            return Some(found);
        }
        self.method_on(receiver, "__callStatic")?;
        let (query, owner) = self.method_on(receiver, "query")?;
        if !matches!(query.source, Source::Index(_, s) if s.is_static) {
            return None;
        }
        let builder = self.member_type(&query, &owner, None).without_null();
        self.method_on(&builder, name)
    }

    /// Method `name` on any class `receiver` can be.
    pub fn method_on(&self, receiver: &Ty, name: &str) -> Option<(Found<'a>, Ty)> {
        receiver.classes().into_iter().find_map(|(class, args)| {
            let owner = Ty::Named(class.into(), args.to_vec());
            self.find_member(&owner, name, MemberKind::Method).map(|f| (f, owner))
        })
    }

    /// The type a call expects for its argument at `index` (its doc comment's, else its
    /// code's), for the receiver it's called on.
    pub fn expected_argument(&self, call: Node<'_>, index: usize) -> Ty {
        let Some((found, receiver)) = self.callee(call) else { return Ty::Mixed };
        let Source::Index(path, symbol) = found.source else { return Ty::Mixed };
        let detail = symbol.detail.as_deref().unwrap_or("");
        let names = parameter_names(detail);
        let Some(name) = names.get(index) else { return Ty::Mixed };
        let facts = Facts::decode(symbol.types.as_deref());
        let mut templates: Vec<Box<str>> =
            found.substitution.iter().map(|(n, _)| n.clone()).collect();
        templates.extend(facts.templates.iter().map(|(n, _)| n.clone().into_boxed_str()));
        let scope =
            NameScope { file: self.index.file(path), at: symbol.offset, templates: &templates };
        let doc = facts.params.iter().find(|(n, _)| n == name).map(|(_, t)| t.as_str());
        let native = code_intelligence::parameters(detail).and_then(|ranges| {
            let param = &detail[ranges.get(index)?.clone()];
            let (ty, _) = crate::intelligence::split_param_pub(param);
            ty
        });
        let ty = choose(doc, native.as_deref(), &scope);
        let receiver = receiver_or(&receiver, &found.owner);
        substitute(&ty, &found.substitution, None, receiver, &found.owner)
    }
}

/// The method's templates `param` (a parameter's declared type) binds, given `arg` (the
/// argument's type): `T` ← anything known, `array<K, V>` ← an array's keys and values,
/// `X<T>` ← the same class's arguments, `class-string<T>` ← `X::class`, `callable(): T` ← a
/// function's result.
fn unify(
    param: &Ty,
    arg: &Ty,
    templates: &[(String, Option<String>)],
    out: &mut Vec<(Box<str>, Ty)>,
) {
    match (param, arg) {
        (Ty::Template(name), arg) => {
            let own = templates.iter().any(|(t, _)| **t == **name);
            if own && !arg.is_mixed() && !out.iter().any(|(n, _)| n == name) {
                out.push((name.clone(), arg.clone()));
            }
        }
        (Ty::Union(parts), arg) => {
            for p in parts {
                unify(p, arg, templates, out);
            }
        }
        (Ty::Array(k, v), Ty::Array(k2, v2)) => {
            unify(k, k2, templates, out);
            unify(v, v2, templates, out);
        }
        (Ty::Array(_, v), Ty::Shape(fields)) => {
            if let Some(values) = fields.iter().map(|(_, t)| t.clone()).reduce(Ty::union) {
                unify(v, &values, templates, out);
            }
        }
        (Ty::Named(a, args), Ty::Named(b, args2)) if a.eq_ignore_ascii_case(b) => {
            for (p, g) in args.iter().zip(args2) {
                unify(p, g, templates, out);
            }
        }
        // `Arrayable<TKey, TValue>` given an array.
        (Ty::Named(_, args), Ty::Array(k, v)) if args.len() == 2 => {
            unify(&args[0], k, templates, out);
            unify(&args[1], v, templates, out);
        }
        (Ty::ClassString(inner), Ty::ClassString(class)) => unify(inner, class, templates, out),
        (Ty::Callable(_, ret), Ty::Callable(_, ret2)) => unify(ret, ret2, templates, out),
        _ => {}
    }
}

/// The receiver `static` stands for: the one asked about, else the declaring class.
fn receiver_or<'t>(receiver: &'t Ty, owner: &'t Ty) -> &'t Ty {
    if matches!(receiver, Ty::Named(..)) { receiver } else { owner }
}

/// The type to use from a doc comment's and the code's: the doc's when it says something
/// (not a conditional the code settles, not `mixed`), else the code's.
fn choose(doc: Option<&str>, native: Option<&str>, scope: &NameScope<'_>) -> Ty {
    let native = native.and_then(|n| crate::types::parse(n, scope));
    let doc = doc.and_then(|d| crate::types::parse(d, scope));
    match (doc, native) {
        (Some(Ty::Conditional(..)), Some(native)) => native,
        (Some(Ty::Mixed), Some(native)) => native,
        (Some(doc), _) => doc,
        (None, Some(native)) => native,
        (None, None) => Ty::Mixed,
    }
}

/// A class's templates paired with the arguments it was reached with (a missing one: its
/// bound, else `mixed`).
fn substitution(class: &Class, args: &[Ty]) -> Vec<(Box<str>, Ty)> {
    class
        .templates
        .iter()
        .enumerate()
        .map(|(i, (name, bound))| {
            let ty = args.get(i).cloned().or_else(|| bound.clone()).unwrap_or(Ty::Mixed);
            (name.clone(), ty)
        })
        .collect()
}

/// `ty` with its templates replaced (the method's own, then the class's), `static` /
/// `$this` by the receiver (with the arguments written after `static<…>`), `self` by the
/// declaring class (`owner`).
pub fn substitute(
    ty: &Ty,
    class: &[(Box<str>, Ty)],
    method: Option<&[(Box<str>, Ty)]>,
    receiver: &Ty,
    owner: &Ty,
) -> Ty {
    ty.map(&|t| match t {
        Ty::Template(name) => Some(
            method
                .and_then(|m| m.iter().find(|(n, _)| n == name))
                .or_else(|| class.iter().find(|(n, _)| n == name))
                .map_or(Ty::Mixed, |(_, t)| t.clone()),
        ),
        Ty::Static(args) => Some(match (receiver, args.is_empty()) {
            (_, true) => receiver.clone(),
            (Ty::Named(fqn, _), false) => Ty::Named(
                fqn.clone(),
                args.iter().map(|a| substitute(a, class, method, receiver, owner)).collect(),
            ),
            _ => receiver.clone(),
        }),
        Ty::SelfType => Some(owner.clone()),
        _ => None,
    })
}

/// A signature's return type (`(int $a): ?User` → `?User`).
pub fn return_of(detail: &str) -> Option<&str> {
    let close = matching_close(detail)?;
    let rest = detail[close + 1..].trim_start();
    rest.strip_prefix(':').map(str::trim).filter(|r| !r.is_empty())
}

/// Where the `(` a signature starts with closes.
fn matching_close(detail: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote = None;
    for (i, c) in detail.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' => quote = Some(c),
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 && c == ')' {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The parameters' names (without `$`) of a signature.
pub fn parameter_names(detail: &str) -> Vec<String> {
    let Some(ranges) = code_intelligence::parameters(detail) else { return Vec::new() };
    ranges
        .into_iter()
        .filter_map(|r| {
            let param = &detail[r];
            let at = param.find('$')?;
            let name: String =
                param[at + 1..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}
