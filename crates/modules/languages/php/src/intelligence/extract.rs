//! The symbols of a PHP file, read from its tree: types, members, functions, constants, imports,
//! with their visibility and signatures.

use super::*;

impl Extractor<'_> {
    pub(super) fn text(&self, node: Node<'_>) -> String {
        self.text.get(node.byte_range()).unwrap_or_default().to_owned()
    }

    pub(super) fn fqn(&self, name: &str) -> String {
        match &self.file.namespace {
            Some(ns) => format!("{ns}\\{name}"),
            None => name.to_owned(),
        }
    }

    pub(super) fn walk(&mut self, node: Node<'_>, class: Option<&str>) {
        match node.kind() {
            "namespace_definition" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.file.namespace = Some(self.text(name).into());
                }
            }
            "namespace_use_declaration" => self.uses(node),
            "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration" => {
                self.type_decl(node);
                return;
            }
            "function_definition" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let name = self.text(name);
                    let fqn = self.fqn(&name);
                    let index = self.push(node, name, SymbolKind::Function, fqn, None, false);
                    self.file.symbols[index].detail = Some(self.signature(node).into_boxed_str());
                    self.file.symbols[index].note =
                        self.attributes_note(node).map(String::into_boxed_str);
                    self.file.symbols[index].types = self.callable_facts(node).encode();
                }
                return;
            }
            "const_declaration" if class.is_none() => {
                for element in children(node).filter(|c| c.kind() == "const_element") {
                    if let Some(name) = children(element).find(|c| c.kind() == "name") {
                        let name = self.text(name);
                        let fqn = self.fqn(&name);
                        let index =
                            self.push(element, name, SymbolKind::Constant, fqn, None, false);
                        self.file.symbols[index].detail =
                            self.const_value(element).map(String::into_boxed_str);
                    }
                }
                return;
            }
            // `define('App\LIMIT', 10)`: a constant of that (full) name.
            "function_call_expression" if class.is_none() && self.defined(node).is_some() => {
                return;
            }
            _ => {}
        }
        // Each named child's doc comment noted on the way (see `Extractor::docs`).
        let mut previous: Option<Node<'_>> = None;
        for child in children(node) {
            if child.is_named() {
                let doc = self.doc_before(child, previous);
                self.docs.insert(child.id(), doc);
                previous = Some(child);
            }
            self.walk(child, class);
        }
    }

    pub(super) fn uses(&mut self, node: Node<'_>) {
        // `use A\{B, C as D};`: the common prefix is a `namespace_name` directly under it.
        let prefix = children(node).find(|c| c.kind() == "namespace_name").map(|n| self.text(n));
        let clauses = children(node)
            .flat_map(|c| {
                if c.kind() == "namespace_use_group" { children(c).collect() } else { vec![c] }
            })
            .filter(|c| c.kind() == "namespace_use_clause");
        for clause in clauses {
            let Some(name) =
                children(clause).find(|c| matches!(c.kind(), "qualified_name" | "name"))
            else {
                continue;
            };
            let name = self.text(name);
            let fqn = match &prefix {
                Some(prefix) => format!("{prefix}\\{name}"),
                None => name.trim_start_matches('\\').to_owned(),
            };
            let alias = clause
                .child_by_field_name("alias")
                .map(|a| self.text(a))
                .unwrap_or_else(|| fqn.rsplit('\\').next().unwrap_or(&fqn).to_owned());
            self.file.uses.push(UseImport { alias: alias.into(), fqn: fqn.into() });
        }
    }

    pub(super) fn type_decl(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else { return };
        let name = self.text(name);
        let kind = match node.kind() {
            "interface_declaration" => SymbolKind::Interface,
            "trait_declaration" => SymbolKind::Trait,
            "enum_declaration" => SymbolKind::Enum,
            _ => SymbolKind::Class,
        };
        let fqn = self.fqn(&name);
        let index = self.push(node, name, kind, fqn.clone(), None, false);
        self.file.symbols[index].note = self.attributes_note(node).map(String::into_boxed_str);
        self.type_facts(node, index, &fqn);
        let mut parents = Vec::new();
        for clause in
            children(node).filter(|c| matches!(c.kind(), "base_clause" | "class_interface_clause"))
        {
            parents.extend(
                children(clause)
                    .filter(|c| matches!(c.kind(), "name" | "qualified_name"))
                    .map(|c| self.text(c)),
            );
        }
        let Some(body) = node.child_by_field_name("body") else {
            self.parents.push((index, parents));
            return;
        };
        let mut trait_args: Vec<String> = Vec::new();
        for member in children(body) {
            match member.kind() {
                "use_declaration" => {
                    parents.extend(
                        children(member)
                            .filter(|c| matches!(c.kind(), "name" | "qualified_name"))
                            .map(|c| self.text(c)),
                    );
                    // `/** @use HasItems<TModel> */ use HasItems;`: the trait's arguments.
                    let (_, tags) = self.doc_tags(member);
                    trait_args.extend(tags.uses);
                    // `use T { many as private manyAlias; }`: the alias is a method too.
                    for list in children(member).filter(|c| c.kind() == "use_list") {
                        for clause in children(list).filter(|c| c.kind() == "use_as_clause") {
                            let names: Vec<Node<'_>> =
                                children(clause).filter(|c| c.kind() == "name").collect();
                            if let [_, alias] = names.as_slice() {
                                let name = self.text(*alias);
                                let at = self.push(
                                    *alias,
                                    name.clone(),
                                    SymbolKind::Method,
                                    name,
                                    Some(&fqn),
                                    false,
                                );
                                self.file.symbols[at].visibility = visibility(clause, self.text);
                            }
                        }
                    }
                }
                "method_declaration" => self.method(member, &fqn),
                "property_declaration" => {
                    let is_static = has_child(member, "static_modifier");
                    for element in children(member).filter(|c| c.kind() == "property_element") {
                        if let Some(var) = element.child_by_field_name("name") {
                            let name = self.text(var).trim_start_matches('$').to_owned();
                            let ty = member.child_by_field_name("type").map(|t| self.text(t));
                            let facts = self.property_facts(member, ty.as_deref());
                            let index = self.push(
                                element,
                                name.clone(),
                                SymbolKind::Property,
                                name,
                                Some(&fqn),
                                is_static,
                            );
                            self.file.symbols[index].detail = ty.map(String::into_boxed_str);
                            self.file.symbols[index].types = facts;
                        }
                    }
                }
                "const_declaration" => {
                    for element in children(member).filter(|c| c.kind() == "const_element") {
                        if let Some(name) = children(element).find(|c| c.kind() == "name") {
                            let name = self.text(name);
                            let index = self.push(
                                element,
                                name.clone(),
                                SymbolKind::ClassConstant,
                                name,
                                Some(&fqn),
                                true,
                            );
                            self.file.symbols[index].detail =
                                self.const_value(element).map(String::into_boxed_str);
                        }
                    }
                }
                "enum_case" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        let name = self.text(name);
                        let index = self.push(
                            member,
                            name.clone(),
                            SymbolKind::EnumCase,
                            name,
                            Some(&fqn),
                            true,
                        );
                        self.file.symbols[index].detail =
                            (member.child_by_field_name("value").and_then(|v| self.value_text(v)))
                                .map(String::into_boxed_str);
                    }
                }
                _ => {}
            }
        }
        if !trait_args.is_empty() {
            let mut facts = crate::phpdoc::Facts::decode(self.file.symbols[index].types.as_deref());
            facts.extends.extend(trait_args);
            self.file.symbols[index].types = facts.encode();
        }
        self.parents.push((index, parents));
    }

    /// `(int $a, string $b): bool` of a function or method declaration.
    pub(super) fn signature(&self, node: Node<'_>) -> String {
        let params = node
            .child_by_field_name("parameters")
            .map(|p| {
                // Comments among the parameters (a promoted property's doc) left out.
                if children(p).any(|c| c.kind() == "comment") {
                    let parts: Vec<String> = children(p)
                        .filter(|c| c.is_named() && c.kind() != "comment")
                        .map(|c| compact(&self.text(c)))
                        .collect();
                    format!("({})", parts.join(", "))
                } else {
                    compact(&self.text(p))
                }
            })
            .unwrap_or_else(|| "()".into());
        let ret = node.child_by_field_name("return_type").map(|r| format!(": {}", self.text(r)));
        format!("{params}{}", ret.unwrap_or_default())
    }

    /// The declaration's attributes, short, for the outline: each one's name and its first
    /// argument when it's a string (`AsCommand('app:auth:flush')`, `Route('/blog')`).
    fn attributes_note(&self, node: Node<'_>) -> Option<String> {
        const MAX: usize = 60;
        let mut parts = Vec::new();
        for list in children(node).filter(|c| c.kind() == "attribute_list") {
            for group in children(list).filter(|c| c.kind() == "attribute_group") {
                for attribute in children(group).filter(|c| c.kind() == "attribute") {
                    let Some(name) = children(attribute)
                        .find(|c| matches!(c.kind(), "name" | "qualified_name" | "relative_name"))
                    else {
                        continue;
                    };
                    let name = self.text(name);
                    let short = name.rsplit('\\').next().unwrap_or(&name).to_owned();
                    let first = attribute
                        .child_by_field_name("parameters")
                        .and_then(|args| children(args).find(|c| c.kind() == "argument"))
                        .and_then(|arg| {
                            children(arg).find(|c| matches!(c.kind(), "string" | "encapsed_string"))
                        })
                        .map(|s| self.text(s));
                    parts.push(match first {
                        Some(value) => format!("{short}({value})"),
                        None => short,
                    });
                }
            }
        }
        let note = parts.join(" ");
        if note.is_empty() {
            return None;
        }
        Some(match note.char_indices().nth(MAX) {
            Some((cut, _)) => format!("{}…", &note[..cut]),
            None => note,
        })
    }

    pub(super) fn method(&mut self, node: Node<'_>, class: &str) {
        let Some(name) = node.child_by_field_name("name") else { return };
        let name = self.text(name);
        let is_static = has_child(node, "static_modifier");
        let index =
            self.push(node, name.clone(), SymbolKind::Method, name.clone(), Some(class), is_static);
        self.file.symbols[index].detail = Some(self.signature(node).into_boxed_str());
        self.file.symbols[index].note = self.attributes_note(node).map(String::into_boxed_str);
        let facts = self.callable_facts(node);
        self.file.symbols[index].types = facts.encode();
        // Constructor-promoted properties: `public function __construct(private X $x)`.
        if name.eq_ignore_ascii_case("__construct")
            && let Some(params) = node.child_by_field_name("parameters")
        {
            for param in children(params).filter(|c| c.kind() == "property_promotion_parameter") {
                if let Some(var) = param.child_by_field_name("name") {
                    let prop = self.text(var).trim_start_matches('$').to_owned();
                    let ty = param.child_by_field_name("type").map(|t| self.text(t));
                    // Its `@param` in the constructor's doc comment, when it says more.
                    let doc = facts.params.iter().find(|(n, _)| *n == prop).map(|(_, t)| t);
                    let var = doc.filter(|d| differs(d, ty.as_deref())).cloned();
                    let index = self.push(
                        param,
                        prop.clone(),
                        SymbolKind::Property,
                        prop,
                        Some(class),
                        false,
                    );
                    self.file.symbols[index].detail = ty.map(String::into_boxed_str);
                    self.file.symbols[index].types =
                        crate::phpdoc::Facts { var, ..Default::default() }.encode();
                }
            }
        }
    }

    /// A constant's value as its declaration writes it (` = 50`), for its card; long or
    /// multi-line values left out.
    fn const_value(&self, element: Node<'_>) -> Option<String> {
        let value = children(element).filter(|c| c.is_named() && c.kind() != "name").last()?;
        self.value_text(value)
    }

    /// ` = <value>` for a value's node, when it's short and on one line.
    fn value_text(&self, value: Node<'_>) -> Option<String> {
        const MAX: usize = 80;
        let text = self.text(value);
        let text = text.trim();
        (!text.is_empty() && text.len() <= MAX && !text.contains('\n'))
            .then(|| format!(" = {text}"))
    }

    /// `define('NAME', value)` with a literal name: the constant declared (its index).
    fn defined(&mut self, call: Node<'_>) -> Option<usize> {
        let function = call.child_by_field_name("function")?;
        if !self.text(function).trim_start_matches('\\').eq_ignore_ascii_case("define") {
            return None;
        }
        let arguments = call.child_by_field_name("arguments")?;
        let mut args = children(arguments).filter(|c| c.kind() == "argument");
        let first = args.next()?;
        let literal =
            first.named_child(0).filter(|n| matches!(n.kind(), "string" | "encapsed_string"))?;
        let quoted = self.text(literal);
        let fqn = quoted.trim_matches(['\'', '"']).replace("\\\\", "\\");
        let fqn = fqn.trim_start_matches('\\').to_owned();
        let valid = !fqn.is_empty()
            && fqn.split('\\').all(|part| {
                part.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                    && part.chars().all(|c| c.is_alphanumeric() || c == '_')
            });
        if !valid {
            return None;
        }
        let name = fqn.rsplit('\\').next().unwrap_or(&fqn).to_owned();
        let index = self.push(literal, name, SymbolKind::Constant, fqn, None, false);
        self.file.symbols[index].detail =
            (args.next().and_then(|a| a.named_child(0)).and_then(|v| self.value_text(v)))
                .map(String::into_boxed_str);
        Some(index)
    }

    pub(super) fn push(
        &mut self,
        node: Node<'_>,
        name: String,
        kind: SymbolKind,
        fqn: String,
        container: Option<&str>,
        is_static: bool,
    ) -> usize {
        let at = node.child_by_field_name("name").unwrap_or(node);
        self.file.symbols.push(Symbol {
            name: name.into_boxed_str(),
            kind,
            fqn: fqn.into_boxed_str(),
            container: container.map(std::sync::Arc::from),
            parents: Box::default(),
            range: node.byte_range(),
            offset: at.start_byte(),
            line: at.start_position().row as u32,
            detail: None,
            is_static,
            visibility: visibility(node, self.text),
            note: None,
            types: None,
        });
        self.file.symbols.len() - 1
    }

    /// The doc comment (`/** … */`) right before a declaration: as the walk noted it, else
    /// its previous sibling's (a class's members: a few).
    pub(super) fn doc_comment(&self, node: Node<'_>) -> Option<DocComment> {
        match self.docs.get(&node.id()) {
            Some(noted) => *noted,
            None => self.doc_before(node, node.prev_named_sibling()),
        }
    }

    /// `previous` (`node`'s previous named sibling) as `node`'s doc comment, if it is one.
    fn doc_before(&self, node: Node<'_>, previous: Option<Node<'_>>) -> Option<DocComment> {
        let comment = previous.filter(|c| c.kind() == "comment")?;
        let between = self.text.get(comment.end_byte()..node.start_byte())?;
        (between.trim().is_empty() && self.text[comment.byte_range()].starts_with("/**")).then(
            || DocComment {
                start: comment.start_byte(),
                end: comment.end_byte(),
                row: comment.start_position().row,
            },
        )
    }

    /// The doc comment's tags before a declaration (empty without one).
    fn doc_tags(&self, node: Node<'_>) -> (Option<DocComment>, crate::phpdoc::Tags) {
        let comment = self.doc_comment(node);
        let tags = comment.map(|c| crate::phpdoc::tags(&self.text[c.start..c.end]));
        (comment, tags.unwrap_or_default())
    }

    /// What a function's or method's doc comment says of its types beyond its code: its
    /// return type, its templates, its parameters' generic types, a deprecation.
    fn callable_facts(&self, node: Node<'_>) -> crate::phpdoc::Facts {
        let (_, tags) = self.doc_tags(node);
        let native = node.child_by_field_name("return_type").map(|r| self.text(r));
        let native_params = self.native_params(node);
        let templates: Vec<&str> = tags.templates.iter().map(|(n, _)| n.as_str()).collect();
        let params = tags
            .params
            .iter()
            .filter(|(name, ty)| {
                let native =
                    native_params.iter().find(|(n, _)| n == name).and_then(|(_, t)| t.as_deref());
                informative(ty, &templates) || (native.is_none() && differs(ty, native))
            })
            .cloned()
            .collect();
        crate::phpdoc::Facts {
            returns: tags.returns.filter(|r| differs(r, native.as_deref())),
            params,
            templates: tags.templates,
            deprecated: tags.deprecated || self.has_attribute(node, "Deprecated"),
            ..Default::default()
        }
    }

    /// The parameters' names (without `$`) and their written types.
    fn native_params(&self, node: Node<'_>) -> Vec<(String, Option<String>)> {
        let Some(params) = node.child_by_field_name("parameters") else { return Vec::new() };
        children(params)
            .filter_map(|p| {
                let name = p.child_by_field_name("name")?;
                let name = self.text(name).trim_start_matches('$').to_owned();
                Some((name, p.child_by_field_name("type").map(|t| self.text(t))))
            })
            .collect()
    }

    /// A property's `@var`, when it says more than its written type.
    fn property_facts(&self, declaration: Node<'_>, native: Option<&str>) -> Option<Box<str>> {
        let (_, tags) = self.doc_tags(declaration);
        let var = tags.var.map(|(_, ty)| ty).filter(|v| differs(v, native));
        let deprecated = tags.deprecated || self.has_attribute(declaration, "Deprecated");
        crate::phpdoc::Facts { var, deprecated, ..Default::default() }.encode()
    }

    /// Whether the declaration carries attribute `name` (`#[\Deprecated]`, by its short name).
    fn has_attribute(&self, node: Node<'_>, name: &str) -> bool {
        children(node).filter(|c| c.kind() == "attribute_list").any(|list| {
            let text = &self.text[list.byte_range()];
            text.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == name)
        })
    }

    /// A type's doc comment: its templates, the generic arguments of what it extends, its
    /// mixins, a deprecation, an enum's backing type — and the members it has by magic
    /// (`@property`, `@method`), as symbols at their tags.
    fn type_facts(&mut self, node: Node<'_>, index: usize, fqn: &str) {
        let (comment, tags) = self.doc_tags(node);
        let backed = (node.kind() == "enum_declaration")
            .then(|| children(node).find(|c| c.kind() == "primitive_type").map(|t| self.text(t)))
            .flatten();
        let mut extends = tags.extends;
        extends.extend(tags.implements);
        extends.extend(tags.uses);
        let facts = crate::phpdoc::Facts {
            templates: tags.templates,
            extends,
            mixins: tags.mixins,
            deprecated: tags.deprecated || self.has_attribute(node, "Deprecated"),
            backed,
            ..Default::default()
        };
        self.file.symbols[index].types = facts.encode();
        let Some(comment) = comment.filter(|_| !tags.magic.is_empty()) else { return };
        // The type's range takes its doc comment in: its magic members are inside it.
        let start = comment.start;
        self.file.symbols[index].range.start = start;
        let row = comment.row;
        let doc = &self.text[comment.start..comment.end];
        for magic in tags.magic {
            let offset = start + magic.offset;
            let line = (row + doc[..magic.offset].matches('\n').count()) as u32;
            let end = self.text[offset..].find('\n').map_or(self.text.len(), |i| offset + i);
            let kind = if magic.is_method { SymbolKind::Method } else { SymbolKind::Property };
            self.file.symbols.push(Symbol {
                name: magic.name.clone().into_boxed_str(),
                kind,
                fqn: magic.name.into_boxed_str(),
                container: Some(std::sync::Arc::from(fqn)),
                parents: Box::default(),
                range: offset..end.min(comment.end),
                offset,
                line,
                detail: (!magic.detail.is_empty()).then_some(magic.detail.into_boxed_str()),
                is_static: magic.is_static,
                visibility: Visibility::Public,
                note: Some(magic.tag.to_owned().into_boxed_str()),
                types: None,
            });
        }
    }

    pub(super) fn finish(mut self) -> FileSymbols {
        for (index, names) in std::mem::take(&mut self.parents) {
            let at = self.file.symbols[index].offset;
            let resolved =
                names.iter().map(|n| self.file.resolve_class(n, at, &NAMING).into()).collect();
            self.file.symbols[index].parents = resolved;
        }
        self.file
    }
}

/// Visibility of a declaration: its own modifier or the parent declaration's (a
/// property is a `property_element` under `property_declaration`); public by default.
pub(super) fn visibility<'t>(node: Node<'t>, text: &str) -> Visibility {
    pub(super) fn modifier(n: Node<'_>) -> Option<Node<'_>> {
        children(n).find(|c| c.kind() == "visibility_modifier")
    }
    // A member's modifier may be its declaration's (`private $a, $b;`); a type or a
    // function of the file has none, and its parent is the whole file (thousands of nodes).
    let own_only = matches!(
        node.kind(),
        "class_declaration"
            | "interface_declaration"
            | "trait_declaration"
            | "enum_declaration"
            | "function_definition"
    );
    let found =
        modifier(node).or_else(|| if own_only { None } else { node.parent().and_then(modifier) });
    match found.and_then(|m| text.get(m.byte_range())).map(str::to_ascii_lowercase).as_deref() {
        Some("private") => Visibility::Private,
        Some("protected") => Visibility::Protected,
        _ => Visibility::Public,
    }
}

pub(super) fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    syntax_ts::children(node)
}

pub(super) fn has_child(node: Node<'_>, kind: &str) -> bool {
    children(node).any(|c| c.kind() == kind)
}

/// Whether a doc type says something its written type doesn't (spaces, a leading `\`,
/// case and `?T` / `T|null` aside).
///
/// A doc type naming no class (`void`, `int|string`, `array`) helps no inference: it isn't
/// kept (most of a library's methods have one).
pub(super) fn differs(doc: &str, native: Option<&str>) -> bool {
    let useful = crate::types::parse(doc, &crate::types::NameScope::GLOBAL)
        .is_some_and(|t| t.mentions_class());
    if !useful {
        return false;
    }
    let Some(native) = native else { return true };
    let norm = |t: &str| {
        let t: String =
            t.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
        let t = t.replace('\\', "");
        match t.strip_prefix('?') {
            Some(inner) => format!("{inner}|null"),
            None => t,
        }
    };
    norm(doc) != norm(native)
}

/// Whether a parameter's doc type is worth keeping for the inference: generic, a shape, a
/// callable's signature, a `class-string`, or a template.
fn informative(ty: &str, templates: &[&str]) -> bool {
    ty.contains(['<', '{', '('])
        || ty.contains("[]")
        || templates
            .iter()
            .any(|t| ty.split(|c: char| !(c.is_alphanumeric() || c == '_')).any(|w| w == *t))
}

/// Multiple spaces (from multi-line signatures) become a single one.
pub(super) fn compact(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")")
}
