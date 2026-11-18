//! Language level checks: the PHP features used in a file that are newer than the chosen
//! version (e.g. a return type with PHP 5.6, `match` with 7.4), found on the syntax tree.
//! Each becomes an error "… requires PHP x.y".

use code_intelligence::LevelIssue;
use syntax_ts::tree_sitter::Node;

/// `"8.1"` → `(8, 1)`.
fn parse(level: &str) -> (u32, u32) {
    let mut parts = level.split('.').map(|p| p.trim().parse::<u32>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

/// The features of `root` (the whole file) newer than `level`.
pub fn check(root: Node<'_>, text: &str, level: &str) -> Vec<LevelIssue> {
    let mut checker = Checker { text, level: parse(level), out: Vec::new() };
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        checker.visit(node);
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    let mut out = checker.out;
    out.sort_by_key(|i| i.range.start);
    out
}

/// The keyword token of a construct (e.g. `match` in a whole match expression), so only
/// it is underlined; the node itself if there is none.
fn keyword<'a>(node: Node<'a>, word: &str) -> Node<'a> {
    let mut cursor = node.walk();
    let found = node.children(&mut cursor).find(|c| c.kind() == word);
    found.unwrap_or(node)
}

/// The file's text, the chosen level and the issues found so far.
struct Checker<'a> {
    text: &'a str,
    level: (u32, u32),
    out: Vec<LevelIssue>,
}

impl<'a> Checker<'a> {
    fn src(&self, n: Node<'_>) -> &'a str {
        n.utf8_text(self.text.as_bytes()).unwrap_or_default()
    }

    /// `feature` (at `at`) needs PHP `since`: an issue if that's newer than the level.
    fn report(
        &mut self,
        at: Node<'_>,
        feature: &'static str,
        name: Option<String>,
        since: &'static str,
    ) {
        if parse(since) > self.level {
            let code = code_of(feature, name.as_deref());
            self.out.push(LevelIssue { range: at.byte_range(), feature, name, since, code });
        }
    }

    /// `name` was removed in a version at or before the level: "… was removed in PHP x.y".
    fn removed(&mut self, at: Node<'_>, feature: &'static str, name: &str) {
        if let Some(removed) = crate::stubs::get().removed(name)
            && parse(removed) <= self.level
        {
            self.out.push(LevelIssue {
                range: at.byte_range(),
                feature,
                name: Some(name.to_owned()),
                since: removed,
                code: code_of(feature, Some(name)),
            });
        }
    }

    fn visit(&mut self, node: Node<'_>) {
        match node.kind() {
            "function_definition"
            | "method_declaration"
            | "anonymous_function"
            | "arrow_function" => {
                self.function(node);
            }
            "simple_parameter" | "variadic_parameter" => self.parameter(node),
            "formal_parameters" => self.optional_before_required(node),
            "property_promotion_parameter" => self.report(node, "promotion", None, "8.0"),
            "optional_type" => self.report(node, "nullable_type", None, "7.1"),
            "union_type" => self.report(node, "union_type", None, "8.0"),
            "intersection_type" => self.report(node, "intersection_type", None, "8.1"),
            "disjunctive_normal_form_type" => self.report(node, "dnf_type", None, "8.2"),
            "primitive_type" => self.primitive_type(node),
            "bottom_type" => self.report(node, "never", None, "8.1"),
            "property_declaration" => self.property(node),
            "const_declaration" => {
                if let Some(ty) = node.child_by_field_name("type") {
                    self.report(ty, "typed_constant", None, "8.3");
                }
            }
            "visibility_modifier" if self.src(node).contains("(set)") => {
                self.report(node, "asymmetric_visibility", None, "8.4");
            }
            "readonly_modifier" => self.readonly(node),
            "binary_expression" => self.binary(node),
            "augmented_assignment_expression" => {
                if let Some(op) = node.child_by_field_name("operator")
                    && self.src(op) == "??="
                {
                    self.report(op, "null_coalescing_assignment", None, "7.4");
                }
            }
            "match_expression" => self.report(keyword(node, "match"), "match", None, "8.0"),
            "enum_declaration" => self.report(keyword(node, "enum"), "enum", None, "8.1"),
            "attribute_list" => self.report(node, "attributes", None, "8.0"),
            "nullsafe_member_access_expression" | "nullsafe_member_call_expression" => {
                self.report(node, "nullsafe", None, "8.0");
            }
            "throw_expression" => {
                // `throw` as a statement is old; as an expression (`?? throw …`) it's 8.0.
                if node.parent().is_some_and(|p| p.kind() != "expression_statement") {
                    self.report(keyword(node, "throw"), "throw_expression", None, "8.0");
                }
            }
            "anonymous_class" => {
                self.report(keyword(node, "class"), "anonymous_class", None, "7.0");
            }
            "namespace_use_group" => self.report(node, "group_use", None, "7.0"),
            "variadic_placeholder" => self.report(node, "first_class_callable", None, "8.1"),
            "argument" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.report(name, "named_arguments", None, "8.0");
                }
            }
            "array_creation_expression" => self.array(node),
            "integer" | "float" if self.src(node).contains('_') => {
                self.report(node, "numeric_separator", None, "7.4");
            }
            "function_call_expression" => self.function_call(node),
            // `new X`, `X::method()`, `X::CONST`, `catch (X $e)`…: PHP's own classes.
            "object_creation_expression"
            | "scoped_call_expression"
            | "class_constant_access_expression" => self.class_use(node),
            _ => {}
        }
    }

    /// Return types (`void`, `never`, `static`…) and the arrow function itself.
    fn function(&mut self, node: Node<'_>) {
        if let Some(ret) = node.child_by_field_name("return_type") {
            let ty = self.src(ret).trim_start_matches(':').trim().to_ascii_lowercase();
            let (feature, since) = match ty.as_str() {
                "void" => ("void", "7.1"),
                "never" => ("never", "8.1"),
                "static" => ("static_return", "8.0"),
                _ => ("return_type", "7.0"),
            };
            self.report(ret, feature, None, since);
        }
        if node.kind() == "arrow_function" {
            self.report(keyword(node, "fn"), "arrow_function", None, "7.4");
        }
    }

    /// Scalar parameter types.
    fn parameter(&mut self, node: Node<'_>) {
        if let Some(ty) = node.child_by_field_name("type")
            && ty.kind() == "primitive_type"
            && matches!(
                self.src(ty).to_ascii_lowercase().as_str(),
                "int" | "float" | "string" | "bool"
            )
        {
            self.report(ty, "scalar_type", None, "7.0");
        }
    }

    /// An optional parameter before a required one: from PHP 8.0 it's required (and
    /// deprecated). A typed one defaulting to `null` is spared (that's how a nullable type was
    /// written before `?T`).
    fn optional_before_required(&mut self, node: Node<'_>) {
        if self.level < (8, 0) {
            return;
        }
        let mut cursor = node.walk();
        let params: Vec<Node<'_>> =
            node.named_children(&mut cursor).filter(|p| p.kind().ends_with("_parameter")).collect();
        let required = |p: &Node<'_>| {
            p.kind() != "variadic_parameter" && p.child_by_field_name("default_value").is_none()
        };
        let Some(last) = params.iter().rposition(required) else { return };
        for param in &params[..last] {
            let Some(default) = param.child_by_field_name("default_value") else { continue };
            let typed = param.child_by_field_name("type").is_some();
            if typed && self.src(default).eq_ignore_ascii_case("null") {
                continue;
            }
            let name = param.child_by_field_name("name").unwrap_or(*param);
            self.out.push(LevelIssue {
                range: name.byte_range(),
                feature: "deprecated_optional_parameter",
                // On one line: a parameter may be split over lines (`$\n    a = 1`).
                name: Some(kernel::text::one_line(self.src(name)).into_owned()),
                since: "8.0",
                code: Some(kernel::text::one_line(self.src(*param)).into_owned()),
            });
        }
    }

    fn primitive_type(&mut self, node: Node<'_>) {
        match self.src(node).to_ascii_lowercase().as_str() {
            "mixed" => self.report(node, "mixed", None, "8.0"),
            "iterable" => self.report(node, "iterable", None, "7.1"),
            "object" => self.report(node, "object_type", None, "7.2"),
            _ => {}
        }
    }

    /// Typed properties and property hooks.
    fn property(&mut self, node: Node<'_>) {
        if let Some(ty) = node.child_by_field_name("type") {
            self.report(ty, "typed_property", None, "7.4");
        }
        let mut cursor = node.walk();
        if let Some(hooks) = node.children(&mut cursor).find(|c| c.kind() == "property_hook_list") {
            self.report(hooks, "property_hooks", None, "8.4");
        }
    }

    fn readonly(&mut self, node: Node<'_>) {
        let in_class = node.parent().is_some_and(|p| p.kind() == "class_declaration");
        if in_class {
            self.report(node, "readonly_class", None, "8.2");
        } else {
            self.report(node, "readonly", None, "8.1");
        }
    }

    /// `??`, `<=>`, `|>`.
    fn binary(&mut self, node: Node<'_>) {
        let Some(op) = node.child_by_field_name("operator") else { return };
        match self.src(op) {
            "??" => self.report(op, "null_coalescing", None, "7.0"),
            "<=>" => self.report(op, "spaceship", None, "7.0"),
            "|>" => self.report(op, "pipe", None, "8.5"),
            _ => {}
        }
    }

    /// Spreading into an array (`[...$a]`).
    fn array(&mut self, node: Node<'_>) {
        let mut cursor = node.walk();
        let spreads = node.named_children(&mut cursor).any(|el| {
            let mut c = el.walk();
            el.named_children(&mut c).any(|n| n.kind() == "variadic_unpacking")
        });
        if spreads {
            self.report(node, "array_spread", None, "7.4");
        }
    }

    /// PHP's own functions, newer than the level or removed by it.
    fn function_call(&mut self, node: Node<'_>) {
        let Some(function) = node.child_by_field_name("function") else { return };
        let name = self.src(function).trim_start_matches('\\');
        if let Some(since) = crate::version::since(name) {
            self.report(function, "function", Some(name.to_owned()), since);
        }
        self.removed(function, "removed_function", name);
    }

    /// PHP's own classes, newer than the level or removed by it.
    fn class_use(&mut self, node: Node<'_>) {
        let class = if node.kind() == "object_creation_expression" {
            syntax_ts::named_children(node).find(|c| matches!(c.kind(), "name" | "qualified_name"))
        } else {
            node.child_by_field_name("scope")
        };
        let Some(class) = class.filter(|c| matches!(c.kind(), "name" | "qualified_name")) else {
            return;
        };
        let name = self.src(class).trim_start_matches('\\');
        if let Some(since) = crate::stubs::get().class(name).map(|c| c.since)
            && crate::stubs::get().since(name).is_some()
        {
            self.report(class, "class", Some(name.to_owned()), since);
        }
        self.removed(class, "removed_class", name);
    }
}

/// What feature `feature` (about `name`, if any) is written as: a class by its name, a
/// function as `name()`, a keyword or an operator as itself.
fn code_of(feature: &str, name: Option<&str>) -> Option<String> {
    match (name, feature) {
        (Some(name), "class" | "removed_class") => return Some(name.to_owned()),
        (Some(name), _) => return Some(format!("{name}()")),
        (None, _) => {}
    }
    let code = match feature {
        "match" => "match",
        "enum" => "enum",
        "readonly" | "readonly_class" => "readonly",
        "null_coalescing" => "??",
        "null_coalescing_assignment" => "??=",
        "spaceship" => "<=>",
        "pipe" => "|>",
        "nullsafe" => "?->",
        "arrow_function" => "fn",
        "void" => "void",
        "never" => "never",
        "mixed" => "mixed",
        "iterable" => "iterable",
        "object_type" => "object",
        "static_return" => "static",
        "throw_expression" => "throw",
        "first_class_callable" => "f(...)",
        "array_spread" => "[...$a]",
        "numeric_separator" => "1_000",
        "nullable_type" => "?T",
        _ => return None,
    };
    Some(code.to_owned())
}

#[cfg(test)]
#[path = "../tests/unit/level_check.rs"]
mod tests;
