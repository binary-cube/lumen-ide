//! What a PHP file declares but never uses: `use` imports (a mention in a doc comment
//! counts, like `@param Device`), private methods and private properties, variables
//! assigned and never read in their function. Properties with attributes (e.g. Doctrine's
//! `#[ORM\Column]`) are left out: frameworks use them.

use std::collections::{HashMap, HashSet};

use code_intelligence::Unused;
use syntax_ts::tree_sitter::Node;

/// A name compared without its case (PHP's classes and functions), without a lowercased copy.
#[derive(Debug, Clone, Copy)]
struct Caseless<'a>(&'a str);

impl PartialEq for Caseless<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(other.0)
    }
}

impl Eq for Caseless<'_> {}

impl std::hash::Hash for Caseless<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for b in self.0.bytes() {
            state.write_u8(b.to_ascii_lowercase());
        }
        state.write_u8(0xff);
    }
}

pub fn find(root: Node<'_>, text: &str) -> Vec<Unused> {
    let src = |n: Node<'_>| &text[n.byte_range()];
    // Every name used (without its case, for classes / functions) and how often (members).
    let mut names: HashSet<Caseless<'_>> = HashSet::new();
    // Whether each class (by its node) uses a trait or includes a file: asked once.
    let mut traits: HashMap<usize, bool> = HashMap::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    let mut strings: HashSet<&str> = HashSet::new();
    let mut imports: Vec<(Node<'_>, Node<'_>, String)> = Vec::new();
    let mut members: Vec<(Node<'_>, &'static str, &str)> = Vec::new();
    let mut out: Vec<Unused> = Vec::new();
    // Each node with the class (trait, enum) it's in: asked of the walk, not by climbing to
    // it (`Node::parent` walks down from the root: slow in a file of many classes).
    let mut stack: Vec<(Node<'_>, Option<Node<'_>>)> = vec![(root, None)];
    while let Some((node, owner)) = stack.pop() {
        // A function's variables (its body walked once more, for its own scope).
        if matches!(
            node.kind(),
            "function_definition" | "method_declaration" | "anonymous_function"
        ) {
            function_variables(node, text, &mut out);
        }
        match node.kind() {
            "namespace_use_declaration" => {
                collect_imports(node, text, &mut imports);
                continue;
            }
            "namespace_definition" => {
                // Its name isn't a use; its body is.
                if let Some(body) = node.child_by_field_name("body") {
                    stack.push((body, owner));
                }
                continue;
            }
            "name" => {
                names.insert(Caseless(src(node)));
                *counts.entry(src(node)).or_default() += 1;
            }
            "comment" => {
                for word in src(node).split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                    if !word.is_empty() {
                        names.insert(Caseless(word));
                    }
                }
            }
            "string_content" | "string_value" => {
                strings.insert(src(node));
            }
            "method_declaration"
                if is_private(node, text) && !with_traits(owner, text, &mut traits) =>
            {
                if let Some(name) = node.child_by_field_name("name")
                    && !src(name).starts_with("__")
                {
                    members.push((name, "method", src(name)));
                }
            }
            "property_declaration"
                if is_private(node, text)
                    && !has_attributes(node)
                    && !with_traits(owner, text, &mut traits) =>
            {
                for element in children(node).filter(|c| c.kind() == "property_element") {
                    if let Some(name) = element.child(0).and_then(|v| v.child(1).or(v.child(0))) {
                        members.push((name, "property", src(name)));
                    }
                }
            }
            _ => {}
        }
        let inner = if matches!(
            node.kind(),
            "class_declaration" | "trait_declaration" | "enum_declaration"
        ) {
            Some(node)
        } else {
            owner
        };
        let at = stack.len();
        stack.extend(children(node).map(|c| (c, inner)));
        stack[at..].reverse();
    }
    out.extend(imports.into_iter().filter(|(_, _, alias)| !names.contains(&Caseless(alias))).map(
        |(range_node, _, alias)| Unused {
            range: range_node.byte_range(),
            kind: "import",
            name: alias,
        },
    ));
    for (name_node, kind, name) in members {
        // Its own declaration is the one occurrence.
        let used = counts.get(name).copied().unwrap_or(0) > 1 || strings.contains(name);
        if !used {
            let range = if kind == "property" {
                name_node.parent().map_or(name_node.byte_range(), |v| v.byte_range())
            } else {
                name_node.byte_range()
            };
            out.push(Unused { range, kind, name: name.to_owned() });
        }
    }
    out.sort_by_key(|u| u.range.start);
    out
}

/// What a function does that may read its variables without naming them: they're all
/// left alone there.
const READS_ALL: &[&str] =
    &["compact", "extract", "get_defined_vars", "parse_str", "func_get_args"];

/// The variables assigned in a function (method, closure) and never read in it — not a
/// parameter, a `foreach`'s, a reference, a `global` / `static` one; a function reading
/// variables by other means (`compact()`, `$$name`, `include`, `eval`) is left alone.
fn function_variables(node: Node<'_>, text: &str, out: &mut Vec<Unused>) {
    {
        if let Some(body) = node.child_by_field_name("body") {
            // Its parameters and a closure's `use`d variables aren't its own to report.
            let mut given: HashSet<&str> = HashSet::new();
            let mut sources: Vec<Node<'_>> =
                node.child_by_field_name("parameters").into_iter().collect();
            sources.extend(children(node).filter(|c| c.kind() == "anonymous_function_use_clause"));
            while let Some(n) = sources.pop() {
                if n.kind() == "variable_name" {
                    given.insert(text[n.byte_range()].trim_start_matches('$'));
                }
                sources.extend(children(n));
            }
            scope_variables(body, text, &given, out);
        }
    }
}

fn scope_variables<'t>(
    body: Node<'t>,
    text: &'t str,
    given: &HashSet<&str>,
    out: &mut Vec<Unused>,
) {
    let src = |n: Node<'_>| &text[n.byte_range()];
    let mut assigned: Vec<(&str, Node<'_>)> = Vec::new();
    let mut read: HashSet<&str> = HashSet::new();
    // Each node with whether it's a statement's own expression (its parent's kind, known
    // by the walk: `Node::parent` walks down from the root).
    let mut stack: Vec<(Node<'_>, bool)> = vec![(body, false)];
    while let Some((node, statement)) = stack.pop() {
        match node.kind() {
            // Their own scope (a closure's `use` reads ours: a variable inside it).
            "function_definition" | "method_declaration" | "class_declaration" => continue,
            "anonymous_function" => {
                for clause in children(node).filter(|c| c.kind() == "anonymous_function_use_clause")
                {
                    stack.push((clause, false));
                }
                continue;
            }
            "dynamic_variable_name"
            | "include_expression"
            | "include_once_expression"
            | "require_expression"
            | "require_once_expression"
            | "global_declaration"
            | "function_static_declaration"
            | "by_ref"
            | "reference_assignment_expression" => return,
            "function_call_expression" => {
                let name = node.child_by_field_name("function").map(src).unwrap_or("");
                let name = name.trim_start_matches('\\');
                if READS_ALL.iter().any(|f| f.eq_ignore_ascii_case(name))
                    || name.eq_ignore_ascii_case("eval")
                {
                    return;
                }
            }
            "assignment_expression" => {
                if let Some(left) =
                    node.child_by_field_name("left").filter(|l| l.kind() == "variable_name")
                {
                    // A statement of its own (in an argument, `f($create = true)` names a value).
                    if statement {
                        assigned.push((src(left).trim_start_matches('$'), left));
                    }
                    if let Some(right) = node.child_by_field_name("right") {
                        stack.push((right, false));
                    }
                    continue;
                }
            }
            "variable_name" => {
                read.insert(src(node).trim_start_matches('$'));
                continue;
            }
            _ => {}
        }
        let own = node.kind() == "expression_statement";
        let at = stack.len();
        stack.extend(children(node).map(|c| (c, own)));
        stack[at..].reverse();
    }
    let mut reported = HashSet::new();
    for (name, node) in assigned {
        // `$_SESSION = []`: a superglobal is everyone's.
        let global = name.starts_with('_') && name.chars().skip(1).all(|c| c.is_ascii_uppercase())
            || name == "GLOBALS";
        if name == "this"
            || global
            || read.contains(name)
            || given.contains(name)
            || !reported.insert(name)
        {
            continue;
        }
        out.push(Unused { range: node.byte_range(), kind: "variable", name: format!("${name}") });
    }
}

/// The imports of a `use` statement: (what to dim, the clause, the alias). A statement
/// with one clause dims whole.
fn collect_imports<'a>(node: Node<'a>, text: &str, out: &mut Vec<(Node<'a>, Node<'a>, String)>) {
    let mut clauses = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if n.kind() == "namespace_use_clause" {
            clauses.push(n);
            continue;
        }
        stack.extend(syntax_ts::children(n));
    }
    let single = clauses.len() == 1;
    for clause in clauses {
        let alias = clause
            .child_by_field_name("alias")
            .map(|a| text[a.byte_range()].to_owned())
            .unwrap_or_else(|| {
                let full = text[clause.byte_range()].trim();
                // `use function count;`, `use const E_ALL;`: the kind isn't part of the name.
                let full = full
                    .strip_prefix("function ")
                    .or_else(|| full.strip_prefix("const "))
                    .unwrap_or(full)
                    .trim();
                let path = full.split(" as ").next().unwrap_or(full).trim();
                path.rsplit('\\').next().unwrap_or(path).to_owned()
            });
        out.push((if single { node } else { clause }, clause, alias));
    }
}

fn children<'a>(node: Node<'a>) -> impl Iterator<Item = Node<'a>> {
    syntax_ts::children(node)
}

/// Whether a member of `owner` (its class, trait or enum) is a trait's (its users' to use)
/// or a class's using a trait (whose code may use it); worked out once per class (`known`:
/// by its node), not per member (each a walk over the whole class).
fn with_traits(owner: Option<Node<'_>>, text: &str, known: &mut HashMap<usize, bool>) -> bool {
    owner.is_some_and(|o| {
        *known.entry(o.id()).or_insert_with(|| {
            o.kind() == "trait_declaration"
                || o.child_by_field_name("body")
                    .is_some_and(|body| children(body).any(|c| c.kind() == "use_declaration"))
                || includes(o, text)
        })
    })
}

/// Whether a declaration includes another file (a template using `$this`): its tree walked
/// only if its text has the words at all.
fn includes(node: Node<'_>, text: &str) -> bool {
    // PHP's keywords in any case (`INCLUDE`).
    let own = text.get(node.byte_range()).unwrap_or_default().as_bytes();
    let has = |word: &[u8]| own.windows(word.len()).any(|w| w.eq_ignore_ascii_case(word));
    if !has(b"include") && !has(b"require") {
        return false;
    }
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        if matches!(
            n.kind(),
            "include_expression"
                | "include_once_expression"
                | "require_expression"
                | "require_once_expression"
        ) {
            return true;
        }
        stack.extend(children(n));
    }
    false
}

fn is_private(node: Node<'_>, text: &str) -> bool {
    children(node).any(|c| c.kind() == "visibility_modifier" && &text[c.byte_range()] == "private")
}

fn has_attributes(node: Node<'_>) -> bool {
    children(node).any(|c| c.kind() == "attribute_list")
}

#[cfg(test)]
#[path = "../tests/unit/unused.rs"]
mod tests;
