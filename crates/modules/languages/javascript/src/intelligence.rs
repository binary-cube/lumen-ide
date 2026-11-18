//! JavaScript intelligence: top-level functions, classes (with methods and fields) and
//! variables from the project, members of global objects (`console.`, `document.`…) and
//! local variables.

use std::sync::LazyLock;

use code_intelligence::{
    CompletionContext, CompletionItem, CompletionKind, ContextKind, FileSymbols,
    LanguageIntelligence, Naming, Receiver, Symbol, SymbolKind, Target, Visibility,
};
use languages::LanguageId;
use regex::Regex;
use syntax_ts::tree_sitter::{Node, Parser};

use crate::LANGUAGE;

/// The completion of a doc comment's tags (`@param`…).
const DOC_TAG: &str = "jsdoc-tag";

/// What code shown without a parser colors as keywords (TypeScript's types too).
const CODE_KEYWORDS: &[&str] = &[
    "abstract",
    "any",
    "as",
    "async",
    "await",
    "boolean",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "number",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "string",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

const KEYWORDS: &[&str] = &[
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "let",
    "new",
    "null",
    "of",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];

/// Which ECMAScript the intelligence reads: JavaScript, TypeScript, TSX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dialect {
    #[default]
    JavaScript,
    TypeScript,
    Tsx,
}

impl Dialect {
    fn language(self) -> LanguageId {
        match self {
            Self::JavaScript => LANGUAGE,
            Self::TypeScript => crate::TYPESCRIPT,
            Self::Tsx => crate::TSX,
        }
    }

    fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::JavaScript => &["js", "mjs", "cjs", "jsx"],
            Self::TypeScript => &["ts", "mts", "cts"],
            Self::Tsx => &["tsx"],
        }
    }

    fn grammar(self) -> syntax_ts::tree_sitter::Language {
        match self {
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }
}

/// The JavaScript family's intelligence: declarations, local definitions, completion
/// context, hover, unused, deprecated — for each dialect (TypeScript's declarations too).
#[derive(Default)]
pub struct JsIntelligence {
    dialect: Dialect,
}

thread_local! {
    /// Each thread's parser per dialect (creating one is costly): the indexers, the symbols'
    /// worker and the UI parse at the same time, none waiting for another. They go with
    /// their thread (a pool's threads end when idle).
    static PARSERS: std::cell::RefCell<Vec<(Dialect, Parser)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// The text this thread parsed last for a hover or ⌘ over a word (its dialect, length
    /// and hash), and its tree: every word of the same text found without parsing it again.
    static LAST: std::cell::RefCell<Option<(Dialect, usize, u64, syntax_ts::tree_sitter::Tree)>> =
        const { std::cell::RefCell::new(None) };
}

/// `text`'s hash (the same text, the same hash; read in one pass, far cheaper than a parse).
fn text_hash(text: &str) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl std::fmt::Debug for JsIntelligence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "JsIntelligence({:?})", self.dialect)
    }
}

impl JsIntelligence {
    pub fn new(dialect: Dialect) -> Self {
        Self { dialect }
    }

    /// [`Self::parse`], the tree kept for the next ask about the same text (the words
    /// hovered one after another in a file not changed meanwhile).
    fn parse_again(&self, text: &str) -> Option<syntax_ts::tree_sitter::Tree> {
        let (len, hash) = (text.len(), text_hash(text));
        let kept = LAST.with_borrow(|last| {
            last.as_ref()
                .filter(|(d, l, h, _)| *d == self.dialect && *l == len && *h == hash)
                .map(|(.., tree)| tree.clone())
        });
        if kept.is_some() {
            return kept;
        }
        let tree = self.parse(text)?;
        LAST.with_borrow_mut(|last| *last = Some((self.dialect, len, hash, tree.clone())));
        Some(tree)
    }

    /// Parses `text` with this dialect's grammar (this thread's parser, reused).
    fn parse(&self, text: &str) -> Option<syntax_ts::tree_sitter::Tree> {
        PARSERS.with_borrow_mut(|parsers| {
            let i = match parsers.iter().position(|(d, _)| *d == self.dialect) {
                Some(i) => i,
                None => {
                    let mut parser = Parser::new();
                    let _ = parser.set_language(&self.dialect.grammar());
                    parsers.push((self.dialect, parser));
                    parsers.len() - 1
                }
            };
            parsers[i].1.parse(text, None)
        })
    }
}

/// What "Move Statement" moves whole in JS / TS: statements, declarations, class members,
/// `switch` cases.
const MOVED_WHOLE: &[&str] = &[
    "expression_statement",
    "lexical_declaration",
    "variable_declaration",
    "return_statement",
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "try_statement",
    "switch_statement",
    "throw_statement",
    "break_statement",
    "continue_statement",
    "function_declaration",
    "generator_function_declaration",
    "class_declaration",
    "method_definition",
    "field_definition",
    "import_statement",
    "export_statement",
    "switch_case",
    "switch_default",
];

impl LanguageIntelligence for JsIntelligence {
    fn completion_triggers(&self) -> &'static str {
        ".\"'/@# <-:$"
    }

    /// JSX's and Vue's attributes, the class list's calls; any string may be a class.
    fn css_references(&self) -> Option<code_intelligence::CssReferences> {
        Some(code_intelligence::CssReferences {
            class_attributes: &["class=\"", "class='", "className=\"", "className='"],
            id_attributes: &["id=\"", "id='"],
            class_calls: &[
                "classList.add(",
                "classList.remove(",
                "classList.toggle(",
                "classList.contains(",
            ],
            script: true,
            ..Default::default()
        })
    }

    fn language(&self) -> LanguageId {
        self.dialect.language()
    }

    fn extensions(&self) -> &[&'static str] {
        self.dialect.extensions()
    }

    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        code_intelligence::LanguageFeature::ALL
    }

    /// Scripts, and the pages / components / templates embedding them.
    fn usage_extensions(&self, _kind: SymbolKind) -> &'static [&'static str] {
        &[
            "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue", "svelte", "html", "php",
            "twig",
        ]
    }

    fn word_chars(&self) -> &'static str {
        "$"
    }

    fn naming(&self) -> Naming {
        Naming { constructor: Some("constructor"), construct: Some("new"), ..Naming::default() }
    }

    fn keyword_insert(&self, keyword: &str) -> code_intelligence::KeywordInsert {
        // Values and jumps are complete as written.
        const EXACT: &[&str] =
            &["true", "false", "null", "undefined", "this", "break", "continue", "default"];
        if EXACT.contains(&keyword) {
            code_intelligence::KeywordInsert::Exact
        } else {
            code_intelligence::KeywordInsert::Spaced
        }
    }

    fn code_style(&self) -> Option<code_intelligence::code_style::CodeStyle> {
        Some(code_intelligence::code_style::CodeStyle {
            extensions: &["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue"],
            keywords: CODE_KEYWORDS,
            sigil: None,
            comments: &["//", "/*", "*"],
        })
    }

    /// JSDoc, with TypeScript's decorators (`@Component(…)`) between it and the declaration.
    fn doc_rules(&self) -> Option<code_intelligence::DocRules> {
        Some(code_intelligence::DocRules {
            opener: "/**",
            closer: "*/",
            attribute: None,
            decorator: Some('@'),
            sigil: None,
        })
    }

    fn source_rules(&self) -> Option<code_intelligence::SourceRules> {
        Some(code_intelligence::SourceRules {
            // Script files use each other whatever their dialect (and Vue components them).
            extensions: &["js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue"],
            includes: &["from", "import", "require"],
            include_suffixes: &[".js", ".ts", "/index.js"],
            before_type: &["new", "extends", "implements", "instanceof"],
            static_access: None,
            member_access: &["."],
            declares_function: &["function"],
            declares_type: &["class", "interface", "enum", "type"],
            modifiers: &[
                "export",
                "default",
                "async",
                "static",
                "function",
                "public",
                "private",
                "protected",
                "abstract",
                "readonly",
            ],
            naming: self.naming(),
            comments: Some(code_intelligence::CommentReader(crate::comment_places::place)),
        })
    }

    fn extract(&self, text: &str) -> FileSymbols {
        let Some(tree) = self.parse(text) else { return FileSymbols::default() };
        let mut out = Extractor { text, file: FileSymbols::default() };
        out.walk(tree.root_node(), 0);
        out.file
    }

    fn statement_move(
        &self,
        text: &str,
        lines: std::ops::Range<usize>,
        up: bool,
    ) -> code_intelligence::StatementMove {
        match self.parse(text) {
            Some(tree) => syntax_ts::statement_move(&tree, text, lines, up, MOVED_WHOLE),
            None => code_intelligence::StatementMove::Lines,
        }
    }

    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        context(text, offset)
    }

    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        target(text, offset)
    }

    /// In a JSDoc / TSDoc comment, the `{types}` and the names; every other comment's text.
    fn comment_place(&self, text: &str, offset: usize) -> Option<code_intelligence::CommentPlace> {
        crate::comment_places::place(text, offset)
    }

    fn local_definition(&self, text: &str, offset: usize) -> Option<usize> {
        // JavaScript inside HTML (`<script>`) is not parsed as a whole file here.
        if text.trim_start().starts_with('<') {
            return None;
        }
        let tree = self.parse_again(text)?;
        crate::definition::local_definition(tree.root_node(), text, offset)
    }

    fn locals(&self, text: &str, offset: usize) -> Vec<String> {
        locals(text, offset)
    }

    fn keywords(&self) -> &[&'static str] {
        KEYWORDS
    }

    fn deprecated(&self, text: &str) -> Vec<code_intelligence::Deprecated> {
        self.parse(text).map(|tree| crate::deprecated::find(&tree, text)).unwrap_or_default()
    }

    fn hover(&self, text: &str, offset: usize) -> Option<code_intelligence::HoverInfo> {
        crate::hover::hover(text, offset)
    }

    fn unused(&self, text: &str) -> Vec<code_intelligence::Unused> {
        self.parse(text).map(|tree| crate::unused::find(&tree, text)).unwrap_or_default()
    }

    /// One parse for all of it (the declarations, then each analysis on the same tree).
    fn analyse(&self, text: &str, _level: Option<&str>) -> code_intelligence::DocumentAnalysis {
        let Some(tree) = self.parse(text) else { return Default::default() };
        let mut out = Extractor { text, file: FileSymbols::default() };
        out.walk(tree.root_node(), 0);
        code_intelligence::DocumentAnalysis {
            symbols: out.file,
            deprecated: crate::deprecated::find(&tree, text),
            unused: crate::unused::find(&tree, text),
            ..Default::default()
        }
    }

    fn vocabulary(&self, context: &CompletionContext, text: &str) -> Vec<CompletionItem> {
        match &context.kind {
            // In a doc comment's braces: the types JSDoc names itself (the project's come
            // from the index).
            ContextKind::TypeName
                if crate::comment_places::in_doc_comment(text, context.replace.start) =>
            {
                crate::comment_places::DOC_TYPES
                    .iter()
                    .map(|t| CompletionItem::word(t, CompletionKind::Keyword, Some("type")))
                    .collect()
            }
            ContextKind::Vocabulary(DOC_TAG) => code_intelligence::comments::tag_items(
                crate::comment_places::TAGS,
                "jsdoc",
                crate::comment_places::SPACED,
            ),
            // The built-in objects and DOM interfaces starting like the typed text (there
            // are over a thousand), the global instances (`document`…), then the locals.
            ContextKind::Name => crate::data::get()
                .globals
                .iter()
                .filter(|(g, _)| starts_like(g, &context.prefix))
                .map(|(g, flags)| {
                    let mut item =
                        CompletionItem::word(g, CompletionKind::Value, Some(status(flags)));
                    item.score -= if flags.is_empty() { 0 } else { 4 };
                    item
                })
                .chain(
                    crate::data::get()
                        .instances()
                        .map(|g| CompletionItem::word(g, CompletionKind::Value, Some("global"))),
                )
                .chain(locals(text, context.replace.start).into_iter().map(|l| {
                    let mut item = CompletionItem::word(&l, CompletionKind::Variable, None);
                    item.score += 4;
                    item
                }))
                .collect(),
            ContextKind::Vocabulary(owner) => crate::data::get()
                .members(owner)
                .iter()
                .map(|(m, flags)| {
                    let detail = if flags.is_empty() { owner } else { status(flags) };
                    let mut item = CompletionItem::word(m, CompletionKind::Method, Some(detail));
                    item.score -= if flags.is_empty() { 0 } else { 4 };
                    item
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

struct Extractor<'a> {
    text: &'a str,
    file: FileSymbols,
}

fn children<'t>(node: Node<'t>) -> impl Iterator<Item = Node<'t>> {
    syntax_ts::children(node)
}

impl Extractor<'_> {
    fn text(&self, node: Node<'_>) -> String {
        self.text.get(node.byte_range()).unwrap_or_default().to_owned()
    }

    /// Only top-level declarations (and those in `export`); not function bodies.
    fn walk(&mut self, node: Node<'_>, depth: usize) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    let params = node.child_by_field_name("parameters").map(|p| self.text(p));
                    self.push(node, name, SymbolKind::Function, None, params);
                }
            }
            "class_declaration" | "abstract_class_declaration" => self.class(node),
            // TypeScript's own declarations.
            "interface_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.push(node, name, SymbolKind::Interface, None, None);
                }
            }
            "enum_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.push(node, name, SymbolKind::Enum, None, None);
                }
            }
            "type_alias_declaration" => {
                if let Some(name) = node.child_by_field_name("name") {
                    self.push(node, name, SymbolKind::Class, None, None);
                }
            }
            "lexical_declaration" | "variable_declaration" if depth <= 2 => {
                for declarator in children(node).filter(|c| c.kind() == "variable_declarator") {
                    let Some(name) =
                        declarator.child_by_field_name("name").filter(|n| n.kind() == "identifier")
                    else {
                        continue;
                    };
                    let value = declarator.child_by_field_name("value");
                    match value.map(|v| v.kind()) {
                        Some("arrow_function" | "function_expression" | "function") => {
                            let params = value
                                .and_then(|v| {
                                    v.child_by_field_name("parameters")
                                        .or_else(|| v.child_by_field_name("parameter"))
                                })
                                .map(|p| self.text(p));
                            self.push(declarator, name, SymbolKind::Function, None, params);
                        }
                        Some("class") => {
                            self.push(declarator, name, SymbolKind::Class, None, None);
                        }
                        _ => {
                            self.push(declarator, name, SymbolKind::Variable, None, None);
                        }
                    }
                }
            }
            "program" | "export_statement" | "ambient_declaration" => {
                for child in children(node) {
                    self.walk(child, depth + 1);
                }
            }
            _ => {}
        }
    }

    fn class(&mut self, node: Node<'_>) {
        let Some(name) = node.child_by_field_name("name") else { return };
        let class = self.text(name);
        let index = self.push(node, name, SymbolKind::Class, None, None);
        if let Some(heritage) = children(node).find(|c| c.kind() == "class_heritage") {
            // TypeScript wraps it: `extends_clause` (then maybe `implements_clause`).
            let heritage =
                children(heritage).find(|c| c.kind() == "extends_clause").unwrap_or(heritage);
            let parent: String = self
                .text(heritage)
                .trim_start_matches("extends")
                .trim()
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '$' || *c == '.')
                .collect();
            if !parent.is_empty() {
                // A class extends one: its parents, made once (a fixed slice).
                let symbol = &mut self.file.symbols[index];
                let mut parents = std::mem::take(&mut symbol.parents).into_vec();
                parents.push(parent.into());
                symbol.parents = parents.into();
            }
        }
        let Some(body) = node.child_by_field_name("body") else { return };
        for member in children(body) {
            let is_static = children(member).any(|c| c.kind() == "static");
            match member.kind() {
                "method_definition" => {
                    if let Some(name) = member.child_by_field_name("name") {
                        let params = member.child_by_field_name("parameters").map(|p| self.text(p));
                        let i = self.push(member, name, SymbolKind::Method, Some(&class), params);
                        self.file.symbols[i].is_static = is_static;
                    }
                }
                // `public_field_definition`: TypeScript's (its name is `name`).
                "field_definition" | "public_field_definition" => {
                    let name = member
                        .child_by_field_name("property")
                        .or_else(|| member.child_by_field_name("name"));
                    if let Some(name) = name {
                        let i = self.push(member, name, SymbolKind::Property, Some(&class), None);
                        self.file.symbols[i].is_static = is_static;
                    }
                }
                _ => {}
            }
        }
    }

    fn push(
        &mut self,
        node: Node<'_>,
        name: Node<'_>,
        kind: SymbolKind,
        container: Option<&str>,
        detail: Option<String>,
    ) -> usize {
        let text = self.text(name);
        // `#field` / `#method()`: private members.
        let text_is_private = text.starts_with('#');
        self.file.symbols.push(Symbol {
            fqn: text.clone().into_boxed_str(),
            name: text.into_boxed_str(),
            kind,
            container: container.map(std::sync::Arc::from),
            parents: Box::default(),
            range: node.byte_range(),
            offset: name.start_byte(),
            line: name.start_position().row as u32,
            detail: (detail.map(|d| d.split_whitespace().collect::<Vec<_>>().join(" ")))
                .map(String::into_boxed_str),
            is_static: false,
            visibility: if text_is_private { Visibility::Private } else { Visibility::Public },
            note: None,
            types: None,
        });
        self.file.symbols.len() - 1
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Inside a comment or string on the current line (heuristic, as for PHP).
fn in_comment_or_string(before: &str) -> bool {
    if let (Some(open), close) = (before.rfind("/*"), before.rfind("*/"))
        && close.is_none_or(|c| c < open)
    {
        return true;
    }
    let line = before.rsplit('\n').next().unwrap_or(before);
    let (mut quote, mut escaped) = (None, false);
    let chars: Vec<char> = line.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (_, '\\') => escaped = true,
            (None, '\'' | '"' | '`') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '/') if chars.get(i + 1) == Some(&'/') => return true,
            _ => {}
        }
    }
    quote.is_some()
}

fn context(text: &str, offset: usize) -> CompletionContext {
    let offset = text.floor_char_boundary(offset);
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let prefix = text[start..offset].to_owned();
    let none = CompletionContext {
        replace: offset..offset,
        prefix: String::new(),
        kind: ContextKind::None,
    };
    // `@param {Us|}`: a type's name.
    if let Some(at) = crate::comment_places::doc_type_at(text, offset) {
        return CompletionContext {
            replace: at..offset,
            prefix: text[at..offset].to_owned(),
            kind: ContextKind::TypeName,
        };
    }
    // `@ret|` in a doc comment: its tags.
    if let Some(at) = code_intelligence::comments::tag_at(text, offset)
        && crate::comment_places::in_doc_comment(text, offset)
    {
        return CompletionContext {
            replace: at..offset,
            prefix: text[at..offset].to_owned(),
            kind: ContextKind::Vocabulary(DOC_TAG),
        };
    }
    if in_comment_or_string(&text[..offset]) || prefix.starts_with(|c: char| c.is_ascii_digit()) {
        return none;
    }
    let pre = &text[..start];
    let kind = if let Some(rest) = pre.strip_suffix('.').or_else(|| pre.strip_suffix("?.")) {
        let receiver_start = rest
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_ident(*c))
            .last()
            .map_or(rest.len(), |(i, _)| i);
        let receiver = &rest[receiver_start..];
        match receiver {
            "this" => ContextKind::Member(Receiver::This),
            r if crate::data::get().owner(r).is_some() => {
                ContextKind::Vocabulary(crate::data::get().owner(r).unwrap_or_default())
            }
            "" => return none,
            _ => ContextKind::Member(Receiver::Unknown),
        }
    } else if prefix.is_empty() {
        return none;
    } else if pre.trim_end().ends_with("new") || pre.trim_end().ends_with("extends") {
        ContextKind::TypeName
    } else {
        ContextKind::Name
    };
    CompletionContext { replace: start..offset, prefix, kind }
}

/// `name` starts with the first character of `prefix` (case-insensitive; any for an empty
/// prefix): a cheap pre-filter before fuzzy matching.
fn starts_like(name: &str, prefix: &str) -> bool {
    match prefix.chars().next() {
        Some(c) => name.chars().next().is_some_and(|n| n.eq_ignore_ascii_case(&c)),
        None => true,
    }
}

/// The status shown in the list for flagged entries ("global" when none).
fn status(flags: &str) -> &'static str {
    if flags.contains('d') {
        "deprecated"
    } else if flags.contains('x') {
        "experimental"
    } else if flags.contains('n') {
        "non-standard"
    } else {
        "global"
    }
}

fn target(text: &str, offset: usize) -> Option<Target> {
    let offset = text.floor_char_boundary(offset);
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_ident(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|(_, c)| !is_ident(*c))
        .map_or(text.len(), |(i, _)| offset + i);
    let word = text[start..end].to_owned();
    if word.is_empty() || KEYWORDS.contains(&word.as_str()) {
        return None;
    }
    let pre = &text[..start];
    if let Some(rest) = pre.strip_suffix('.') {
        let receiver = if rest.ends_with("this") { Receiver::This } else { Receiver::Unknown };
        return Some(Target::Member { receiver, name: word });
    }
    Some(Target::Named {
        name: word,
        kinds: vec![SymbolKind::Function, SymbolKind::Class, SymbolKind::Variable],
    })
}

fn locals(text: &str, offset: usize) -> Vec<String> {
    static DECL: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\b(?:let|const|var)\s+([A-Za-z_$][\w$]*)").ok());
    let offset = text.floor_char_boundary(offset);
    let scope =
        text[..offset].rfind("function").or_else(|| text[..offset].rfind("=>")).unwrap_or(0);
    let mut names: Vec<String> = Vec::new();
    if let Some(re) = &*DECL {
        for c in re.captures_iter(&text[scope..offset]) {
            if !names.iter().any(|n| *n == c[1]) {
                names.push(c[1].to_owned());
            }
        }
    }
    names
}

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
