//! PHP intelligence: a file's declarations (from the tree-sitter tree), the context at
//! the cursor for completion, the target for "go to definition" and variable types
//! (simple inference: `new`, parameter type, `@var`).

use std::sync::{LazyLock, Mutex};

use code_intelligence::{
    CompletionContext, CompletionItem, CompletionKind, ContextKind, FileSymbols,
    LanguageIntelligence, Naming, Project, Receiver, Symbol, SymbolKind, Target, UseImport,
    Visibility,
};
use languages::LanguageId;
use regex::Regex;
use syntax_ts::tree_sitter::{Node, Parser};

use crate::{LANGUAGE, docblock, version};

mod context;
mod extract;
pub(crate) use context::in_attribute_name;
use context::*;

/// What code shown without a parser colors as keywords (built-in types too).
const CODE_KEYWORDS: &[&str] = &[
    "abstract",
    "array",
    "as",
    "bool",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "fn",
    "for",
    "foreach",
    "function",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "int",
    "interface",
    "match",
    "mixed",
    "namespace",
    "new",
    "null",
    "parent",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "self",
    "static",
    "string",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "use",
    "var",
    "void",
    "while",
    "yield",
];

const KEYWORDS: &[&str] = &[
    "abstract",
    "array",
    "as",
    "break",
    "case",
    "catch",
    "class",
    "clone",
    "const",
    "continue",
    "declare",
    "default",
    "do",
    "echo",
    "else",
    "elseif",
    "empty",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "fn",
    "for",
    "foreach",
    "function",
    "global",
    "if",
    "implements",
    "include",
    "include_once",
    "instanceof",
    "insteadof",
    "interface",
    "isset",
    "list",
    "match",
    "namespace",
    "new",
    "null",
    "private",
    "protected",
    "public",
    "readonly",
    "require",
    "require_once",
    "return",
    "self",
    "static",
    "switch",
    "throw",
    "trait",
    "true",
    "try",
    "unset",
    "use",
    "var",
    "while",
    "yield",
    "parent",
    "string",
    "int",
    "float",
    "bool",
    "void",
    "mixed",
    "never",
    "iterable",
    "callable",
    "object",
];

/// Magic constants and common predefined ones.
const CONSTANTS: &[&str] = &[
    "__DIR__",
    "__FILE__",
    "__LINE__",
    "__CLASS__",
    "__FUNCTION__",
    "__METHOD__",
    "__NAMESPACE__",
    "__TRAIT__",
    "PHP_EOL",
    "PHP_VERSION",
    "PHP_OS",
    "PHP_OS_FAMILY",
    "PHP_INT_MAX",
    "PHP_INT_MIN",
    "PHP_INT_SIZE",
    "PHP_FLOAT_EPSILON",
    "DIRECTORY_SEPARATOR",
    "PATH_SEPARATOR",
    "E_ALL",
    "E_ERROR",
    "E_WARNING",
    "E_NOTICE",
    "E_DEPRECATED",
    "E_STRICT",
    "JSON_THROW_ON_ERROR",
    "JSON_PRETTY_PRINT",
    "JSON_UNESCAPED_UNICODE",
    "JSON_UNESCAPED_SLASHES",
    "JSON_ERROR_NONE",
    "SORT_REGULAR",
    "SORT_NUMERIC",
    "SORT_STRING",
    "COUNT_RECURSIVE",
    "ARRAY_FILTER_USE_KEY",
    "ARRAY_FILTER_USE_BOTH",
    "ENT_QUOTES",
    "ENT_HTML5",
    "FILTER_VALIDATE_EMAIL",
    "FILTER_VALIDATE_INT",
    "FILTER_VALIDATE_URL",
    "FILTER_VALIDATE_BOOLEAN",
    "FILE_APPEND",
    "LOCK_EX",
    "PREG_SPLIT_NO_EMPTY",
    "M_PI",
];

/// Superglobal variables.
const SUPERGLOBALS: &[&str] = &[
    "$_GET",
    "$_POST",
    "$_REQUEST",
    "$_SERVER",
    "$_SESSION",
    "$_COOKIE",
    "$_FILES",
    "$_ENV",
    "$GLOBALS",
];

/// The methods PHP calls by itself (offered after `function` in a class), with their
/// parameters as the detail.
const MAGIC_METHODS: &[(&str, &str)] = &[
    ("__construct", "()"),
    ("__destruct", "()"),
    ("__get", "(string $name)"),
    ("__set", "(string $name, mixed $value)"),
    ("__isset", "(string $name)"),
    ("__unset", "(string $name)"),
    ("__call", "(string $name, array $arguments)"),
    ("__callStatic", "(string $name, array $arguments)"),
    ("__toString", "(): string"),
    ("__invoke", "()"),
    ("__clone", "()"),
    ("__debugInfo", "(): array"),
    ("__serialize", "(): array"),
    ("__unserialize", "(array $data)"),
    ("__sleep", "(): array"),
    ("__wakeup", "()"),
    ("__set_state", "(array $properties)"),
];

/// The position of a method's name being declared in a class (`function __con|`).
const METHOD_NAME: &str = "method_name";

/// What "Move Statement" moves whole in PHP: statements, declarations, class members,
/// `switch` cases.
const MOVED_WHOLE: &[&str] = &[
    "expression_statement",
    "echo_statement",
    "return_statement",
    "if_statement",
    "for_statement",
    "foreach_statement",
    "while_statement",
    "do_statement",
    "switch_statement",
    "try_statement",
    "break_statement",
    "continue_statement",
    "unset_statement",
    "global_declaration",
    "function_static_declaration",
    "function_definition",
    "class_declaration",
    "interface_declaration",
    "trait_declaration",
    "enum_declaration",
    "method_declaration",
    "property_declaration",
    "const_declaration",
    "use_declaration",
    "namespace_use_declaration",
    "enum_case",
    "case_statement",
    "default_statement",
    "declare_statement",
];

#[derive(Default)]
pub struct PhpIntelligence {
    /// `composer.json`'s PSR-4 mapping, for the project and the file's modification time
    /// it was read at (not parsed again on every "go to definition").
    psr4: Mutex<kernel::Memo<(std::path::PathBuf, std::time::SystemTime), Psr4>>,
}

thread_local! {
    /// Each thread's parser (creating one is costly): the indexers, the symbols' worker and
    /// the UI parse at the same time, none waiting for another. It goes with its thread (a
    /// pool's threads end when idle).
    static PARSER: std::cell::RefCell<Option<Parser>> = const { std::cell::RefCell::new(None) };
}

/// The declarations of `text`, from its `tree`.
fn extract_from(tree: &syntax_ts::tree_sitter::Tree, text: &str) -> FileSymbols {
    let mut out = Extractor {
        text,
        file: FileSymbols::default(),
        parents: Vec::new(),
        docs: std::collections::HashMap::new(),
    };
    out.walk(tree.root_node(), None);
    let mut file = out.finish();
    // The CSS of its HTML parts (`<style>` blocks).
    stylesheet::add_style_blocks(text, &mut file);
    file
}

/// `text` parsed with this thread's PHP parser.
fn parse(text: &str) -> Option<syntax_ts::tree_sitter::Tree> {
    parse_from(text, None)
}

/// `text` parsed with this thread's PHP parser, reusing `old` (a tree of an earlier text,
/// already edited to match this one) where nothing changed.
pub(crate) fn parse_from(
    text: &str,
    old: Option<&syntax_ts::tree_sitter::Tree>,
) -> Option<syntax_ts::tree_sitter::Tree> {
    PARSER.with_borrow_mut(|parser| {
        let parser = parser.get_or_insert_with(|| {
            let mut parser = Parser::new();
            let _ = parser.set_language(&tree_sitter_php::LANGUAGE_PHP.into());
            parser
        });
        parser.parse(text, old)
    })
}

/// The declarations of `text` from its tree (the inference's view of the document).
pub(crate) fn symbols_of(tree: &syntax_ts::tree_sitter::Tree, text: &str) -> FileSymbols {
    let mut out = Extractor {
        text,
        file: FileSymbols::default(),
        parents: Vec::new(),
        docs: std::collections::HashMap::new(),
    };
    out.walk(tree.root_node(), None);
    out.finish()
}

/// A parameter's type and name (`?Device $device = null` → `?Device`, `$device`).
pub(crate) fn split_param_pub(param: &str) -> (Option<String>, String) {
    split_param(param)
}

/// How PHP writes names.
/// Whether the end of `before` may be in a string or a comment (by its line: cheap).
pub(crate) fn maybe_in_string(before: &str) -> bool {
    context::in_comment_or_string(before)
}

pub(crate) fn naming() -> Naming {
    NAMING
}

/// A parameter list on one line (`(int $a,\n string $b)` → `(int $a, string $b)`).
pub(crate) fn compact_params(text: &str) -> String {
    extract::compact(text)
}

/// (namespace prefix, directory), longest prefixes first.
pub(crate) type Psr4 = std::sync::Arc<Vec<(String, String)>>;

/// The PSR-4 mapping of a `composer.json` (`autoload` and `autoload-dev`).
pub(crate) fn psr4(json: &str) -> Psr4 {
    let Ok(composer) = serde_json::from_str::<serde_json::Value>(json) else {
        return Psr4::default();
    };
    let mut mappings: Vec<(String, String)> = Vec::new();
    for section in ["autoload", "autoload-dev"] {
        let Some(psr4) =
            composer.get(section).and_then(|a| a.get("psr-4")).and_then(|p| p.as_object())
        else {
            continue;
        };
        for (prefix, dirs) in psr4 {
            let dirs: Vec<&str> = match dirs {
                serde_json::Value::String(d) => vec![d.as_str()],
                serde_json::Value::Array(list) => list.iter().filter_map(|d| d.as_str()).collect(),
                _ => Vec::new(),
            };
            mappings.extend(dirs.into_iter().map(|d| (prefix.clone(), d.to_owned())));
        }
    }
    mappings.sort_by_key(|(prefix, _)| std::cmp::Reverse(prefix.len()));
    std::sync::Arc::new(mappings)
}

impl std::fmt::Debug for PhpIntelligence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PhpIntelligence")
    }
}

impl LanguageIntelligence for PhpIntelligence {
    /// `<?` outside the code: `<?php` and a new line (see [`crate::typing`]).
    fn on_type(
        &self,
        text: &str,
        offset: usize,
        typed: char,
    ) -> Option<code_intelligence::TypedInsert> {
        crate::typing::open_tag(text, offset, typed)
    }

    fn on_type_triggers(&self) -> &'static [char] {
        &['?']
    }

    fn completion_triggers(&self) -> &'static str {
        "$>:\\@\"' /<#-."
    }

    /// The HTML around its code.
    fn css_references(&self) -> Option<code_intelligence::CssReferences> {
        Some(code_intelligence::CssReferences {
            class_attributes: &["class=\"", "class='"],
            id_attributes: &["id=\"", "id='"],
            ..Default::default()
        })
    }

    fn language(&self) -> LanguageId {
        LANGUAGE
    }

    fn extensions(&self) -> &[&'static str] {
        &["php", "phtml", "inc"]
    }

    fn features(&self) -> &'static [code_intelligence::LanguageFeature] {
        code_intelligence::LanguageFeature::ALL
    }

    /// PHP code, and the templates that call it.
    fn usage_extensions(&self, _kind: SymbolKind) -> &'static [&'static str] {
        &["php", "phtml", "inc", "module", "twig"]
    }

    /// `$variables` (declared by assigning them, scoped to their function), `$this`,
    /// `__construct` (method names ignore case), a class per `.php` file (PSR-4).
    fn word_chars(&self) -> &'static str {
        "$\\"
    }

    fn naming(&self) -> Naming {
        NAMING
    }

    fn named_argument(&self) -> Option<&'static str> {
        // PHP 8: `f(name: 1)`, `#[AsCommand(name: 'app:x')]`.
        Some(":")
    }

    fn constructs_at(&self, text: &str, word_start: usize) -> bool {
        let Some(head) = text.get(..word_start) else { return false };
        // The whole name (`ORM\Column`, `\App\User`), then what's before it.
        let start = word_start - class_token_before(text, word_start).len();
        // `new X(`, and an attribute: `#[X(…)]` makes an X too.
        head[..start].trim_end().ends_with("new") || in_attribute_name(text, start)
    }

    fn keyword_insert(&self, keyword: &str) -> code_intelligence::KeywordInsert {
        use code_intelligence::KeywordInsert;
        if CALL_LIKE.contains(&keyword) {
            KeywordInsert::Call
        } else if EXACT.contains(&keyword) {
            KeywordInsert::Exact
        } else {
            KeywordInsert::Spaced
        }
    }

    fn parameter_types(&self, detail: &str) -> Vec<String> {
        crate::parameters::parameter_types(detail)
    }

    fn source_rules(&self) -> Option<code_intelligence::SourceRules> {
        Some(code_intelligence::SourceRules {
            extensions: &["php", "phtml", "inc"],
            includes: &["require", "require_once", "include", "include_once"],
            include_suffixes: &[],
            before_type: &["new", "extends", "implements", "instanceof", "#["],
            static_access: Some("::"),
            member_access: &["->", "::"],
            declares_function: &["function"],
            declares_type: &["class", "interface", "trait", "enum"],
            modifiers: &[
                "public",
                "private",
                "protected",
                "static",
                "function",
                "abstract",
                "final",
                "readonly",
            ],
            naming: NAMING,
            comments: Some(code_intelligence::CommentReader(crate::comment_places::place)),
        })
    }

    fn fits_kind(&self, text: &str, range: &std::ops::Range<usize>, kind: SymbolKind) -> bool {
        fits_kind(text, range, kind)
    }

    /// Methods and properties with their modifiers: `public static function name(…)`,
    /// `private $name`.
    fn signature(&self, symbol: &Symbol, detail: Option<&str>) -> String {
        let visibility = match symbol.visibility {
            Visibility::Public => "public",
            Visibility::Protected => "protected",
            Visibility::Private => "private",
        };
        let modifiers =
            if symbol.is_static { format!("{visibility} static") } else { visibility.to_owned() };
        let detail = detail.unwrap_or("");
        match symbol.kind {
            SymbolKind::Method => format!("{modifiers} function {}{detail}", symbol.name),
            SymbolKind::Property => format!("{modifiers} ${}{detail}", symbol.name),
            _ => code_intelligence::signature_plainly(symbol, Some(detail)),
        }
    }

    /// `?Device $device = null` → `?Device`, `$device` (`...$rest` keeps its dots).
    fn split_param(&self, param: &str) -> (Option<String>, String) {
        split_param(param)
    }

    fn extract(&self, text: &str) -> FileSymbols {
        let Some(tree) = parse(text) else { return FileSymbols::default() };
        extract_from(&tree, text)
    }

    /// One parse for all of it (the declarations, then each analysis on the same tree).
    fn analyse(&self, text: &str, level: Option<&str>) -> code_intelligence::DocumentAnalysis {
        // The tree kept for the document's texts (parsed again only where it changed: the
        // same one its checks and completions use), not one from scratch per version.
        let Some(parsed) = crate::doctree::parsed(text) else { return Default::default() };
        let tree = &parsed.tree;
        let root = tree.root_node();
        code_intelligence::DocumentAnalysis {
            symbols: extract_from(tree, text),
            issues: level.map(|l| crate::level_check::check(root, text, l)).unwrap_or_default(),
            deprecated: self.deprecated(text),
            unused: crate::unused::find(root, text),
            requirements: crate::requirements::find(root, text),
        }
    }

    fn statement_move(
        &self,
        text: &str,
        lines: std::ops::Range<usize>,
        up: bool,
    ) -> code_intelligence::StatementMove {
        match parse(text) {
            Some(tree) => syntax_ts::statement_move(&tree, text, lines, up, MOVED_WHOLE),
            None => code_intelligence::StatementMove::Lines,
        }
    }

    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        context(text, offset)
    }

    fn local_definition(&self, text: &str, offset: usize) -> Option<usize> {
        crate::locals::definition(text, offset)
    }

    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        target(text, offset)
    }

    /// In a doc comment, the tags' types and names; every other comment's text.
    fn comment_place(&self, text: &str, offset: usize) -> Option<code_intelligence::CommentPlace> {
        crate::comment_places::place(text, offset)
    }

    fn type_at(&self, text: &str, offset: usize) -> Option<String> {
        context::type_at(text, offset)
    }

    /// The inference's: the expression before `->` typed knowing the project.
    fn receiver_type(
        &self,
        text: &str,
        offset: usize,
        types: &code_intelligence::Types<'_>,
    ) -> Option<String> {
        crate::infer::api::receiver_type(text, offset, types)
    }

    fn receiver_types(
        &self,
        text: &str,
        offset: usize,
        types: &code_intelligence::Types<'_>,
    ) -> Vec<String> {
        crate::infer::api::receiver_types(text, offset, types)
    }

    /// Where the member at `offset` is declared: an ancestor's, a `@mixin`'s, the builder
    /// a model's `__callStatic` forwards to.
    fn member_owner(
        &self,
        text: &str,
        offset: usize,
        types: &code_intelligence::Types<'_>,
    ) -> Option<String> {
        crate::infer::api::member_owner(text, offset, types)
    }

    /// PHP's own members: a built-in ancestor's (the stubs), an enum's.
    fn builtin_members(&self, class: &str, types: &code_intelligence::Types<'_>) -> Vec<Symbol> {
        crate::infer::api::builtin_members(class, types.index)
    }

    fn typed_hover(
        &self,
        text: &str,
        offset: usize,
        types: &code_intelligence::Types<'_>,
    ) -> Option<code_intelligence::HoverInfo> {
        crate::infer::api::hover(text, offset, types)
    }

    /// What's certainly wrong knowing the project: unknown classes, argument counts,
    /// methods a closed class doesn't have, deprecated calls.
    fn check(
        &self,
        text: &str,
        types: &code_intelligence::Types<'_>,
    ) -> code_intelligence::Checked {
        crate::checks::check(text, types)
    }

    fn checks_project(&self) -> bool {
        true
    }

    /// `@deprecated` in its doc comment or `#[\Deprecated]` on it.
    fn is_deprecated(&self, symbol: &Symbol) -> bool {
        crate::phpdoc::Facts::says_deprecated(symbol.types.as_deref())
    }

    /// 2: PHPDoc types, magic members; 3: constants' values (their cards), `define()`.
    fn version(&self) -> u32 {
        3
    }

    fn locals(&self, text: &str, offset: usize) -> Vec<String> {
        locals(text, offset)
    }

    fn levels(&self) -> &[&'static str] {
        version::levels()
    }

    fn detect_level(&self, project: &Project<'_>) -> Option<code_intelligence::DetectedLevel> {
        version::detect(project)
    }

    /// Finds the installed PHP (running it for its version): in the background, once, on
    /// the module's own thread (its CPU on PHP's row in the Task Manager).
    #[allow(clippy::disallowed_methods)]
    fn warm(&self) {
        let found = std::thread::Builder::new()
            .name(crate::DETECT_THREAD.into())
            .spawn(|| crate::interpreter::installed().is_some());
        if let Ok(found) = found {
            let _ = found.join();
        }
    }

    fn since(&self, word: &str) -> Option<&'static str> {
        version::since(word)
    }

    fn hover(&self, text: &str, offset: usize) -> Option<code_intelligence::HoverInfo> {
        crate::hover::hover(text, offset)
    }

    fn level_issues(&self, text: &str, level: &str) -> Vec<code_intelligence::LevelIssue> {
        let Some(tree) = parse(text) else { return Vec::new() };
        crate::level_check::check(tree.root_node(), text, level)
    }

    fn requirements(&self, text: &str) -> Vec<code_intelligence::Requirement> {
        let Some(tree) = parse(text) else { return Vec::new() };
        crate::requirements::find(tree.root_node(), text)
    }

    fn manifest(&self) -> Option<&'static str> {
        Some("composer.json")
    }

    fn unused(&self, text: &str) -> Vec<code_intelligence::Unused> {
        let Some(tree) = parse(text) else { return Vec::new() };
        crate::unused::find(tree.root_node(), text)
    }

    fn code_style(&self) -> Option<code_intelligence::code_style::CodeStyle> {
        Some(code_intelligence::code_style::CodeStyle {
            extensions: &["php", "phtml", "inc"],
            keywords: CODE_KEYWORDS,
            sigil: Some('$'),
            comments: &["//", "#", "/*", "*"],
        })
    }

    fn doc_rules(&self) -> Option<code_intelligence::DocRules> {
        Some(code_intelligence::DocRules {
            opener: "/**",
            closer: "*/",
            attribute: Some("#["),
            decorator: None,
            sigil: Some('$'),
        })
    }

    fn doc_comment(&self, text: &str, offset: usize) -> Vec<String> {
        docblock::stub(text, offset)
    }

    fn keywords(&self) -> &[&'static str] {
        KEYWORDS
    }

    /// `use Fqn;` after the last top-level `use`, otherwise after `namespace …;`, otherwise
    /// after `<?php`.
    fn import_edit(&self, text: &str, fqn: &str) -> Option<(usize, String)> {
        static USE: LazyLock<Option<Regex>> =
            LazyLock::new(|| Regex::new(r"(?m)^use\s+[^;(]+;[^\n]*").ok());
        static NAMESPACE: LazyLock<Option<Regex>> =
            LazyLock::new(|| Regex::new(r"(?m)^namespace\s+[^;{]+;[^\n]*").ok());
        // The first type declaration (made once: asked at each suggestion taken).
        static TYPE: LazyLock<Option<Regex>> = LazyLock::new(|| {
            Regex::new(
                r"(?m)^\s*(?:abstract\s+|final\s+|readonly\s+)*(?:class|interface|trait|enum)\s",
            )
            .ok()
        });
        let statement = format!("use {};", fqn.trim_start_matches('\\'));
        // Only before the first type declaration (imports inside class bodies are traits).
        let limit = TYPE.as_ref().and_then(|re| re.find(text)).map_or(text.len(), |m| m.start());
        let head = &text[..limit];
        if let Some(last) = USE.as_ref().and_then(|re| re.find_iter(head).last()) {
            return Some((last.end(), format!("\n{statement}")));
        }
        if let Some(ns) = NAMESPACE.as_ref().and_then(|re| re.find(head)) {
            return Some((ns.end(), format!("\n\n{statement}")));
        }
        let open = text.find("<?php")?;
        let line_end = text[open..].find('\n').map_or(text.len(), |i| open + i);
        Some((line_end, format!("\n\n{statement}")))
    }

    fn includes(&self, text: &str) -> Vec<String> {
        static INCLUDE: LazyLock<Option<Regex>> = LazyLock::new(|| {
            Regex::new(
                r#"\b(?:include|require)(?:_once)?\s*\(?\s*(?:__DIR__\s*\.\s*)?['"]([^'"]+)['"]"#,
            )
            .ok()
        });
        INCLUDE
            .as_ref()
            .map_or_else(Vec::new, |re| re.captures_iter(text).map(|c| c[1].to_owned()).collect())
    }

    /// PSR-4 from `composer.json` (`autoload` and `autoload-dev`): `App\Models\User` with
    /// `"App\\": "app/"` → `app/Models/User.php`. The longest prefix wins.
    fn locate(&self, project: &Project<'_>, fqn: &str) -> Vec<std::path::PathBuf> {
        let Some(root) = project.root else { return Vec::new() };
        let file = root.join("composer.json");
        let read = || (project.read)(&file).map(|json| psr4(&json)).unwrap_or_default();
        let mappings = match ((project.modified)(&file), self.psr4.lock()) {
            (Some(modified), Ok(mut memo)) => {
                std::sync::Arc::clone(memo.get_or_update((root.to_path_buf(), modified), read))
            }
            _ => read(),
        };
        let fqn = fqn.trim_start_matches('\\');
        mappings
            .iter()
            .filter_map(|(prefix, dir)| {
                let rest = fqn.strip_prefix(prefix.as_str())?;
                Some(root.join(dir).join(format!("{}.php", rest.replace('\\', "/"))))
            })
            .collect()
    }

    fn vocabulary(&self, context: &CompletionContext, text: &str) -> Vec<CompletionItem> {
        if docblock::open_at(text, context.replace.start).is_some() {
            return docblock::vocabulary(context, text);
        }
        match context.kind {
            // After a namespace's `\` with nothing typed: only its namespaces (the engine's).
            ContextKind::Name | ContextKind::TypeName if context.prefix.is_empty() => Vec::new(),
            ContextKind::Variable => SUPERGLOBALS
                .iter()
                .map(|g| CompletionItem::word(g, CompletionKind::Variable, Some("superglobal")))
                .collect(),
            ContextKind::Name => CONSTANTS
                .iter()
                .map(|c| CompletionItem::word(c, CompletionKind::Constant, Some("PHP")))
                // PHP's own functions (from the stubs), with their signature; only those
                // starting like the typed text (there are thousands).
                .chain(crate::stubs::get().functions_with_prefix(first_char(&context.prefix)).map(
                    |f| {
                        let signature = f.signature();
                        let mut item = CompletionItem::word(
                            f.name,
                            CompletionKind::Function,
                            Some(&signature),
                        );
                        item.insert = format!("{}()", f.name);
                        item.cursor_back = 1;
                        item
                    },
                ))
                .chain(stub_classes(&context.prefix))
                .collect(),
            ContextKind::TypeName => stub_classes(&context.prefix).collect(),
            // A method being named: those PHP calls by itself, `()` added.
            ContextKind::Vocabulary(METHOD_NAME) => MAGIC_METHODS
                .iter()
                .map(|(name, params)| {
                    let mut item = CompletionItem::word(name, CompletionKind::Method, Some(params));
                    item.insert = format!("{name}()");
                    item.cursor_back = 1;
                    item
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// PHP's own classes (and those of its extensions) starting like `prefix`: the namespace
/// or the extension as the detail, imported on accept when needed.
fn stub_classes(prefix: &str) -> impl Iterator<Item = CompletionItem> + '_ {
    let prefix = prefix.trim_start_matches('\\');
    let stubs = crate::stubs::get();
    let items: Vec<CompletionItem> = stubs
        .classes_with_prefix(first_char(prefix))
        .map(|c| {
            let detail = match c.name.rsplit_once('\\') {
                Some((namespace, _)) => namespace.to_owned(),
                None if !matches!(c.ext, "core" | "standard") => format!("ext-{}", c.ext),
                None => "PHP".to_owned(),
            };
            let kind = if c.kind == "interface" {
                CompletionKind::Interface
            } else {
                CompletionKind::Class
            };
            let mut item = CompletionItem::word(c.short(), kind, Some(&detail));
            item.import = Some(c.name.to_owned());
            item
        })
        .collect();
    items.into_iter()
}

/// The first character of `text` (as a string slice; empty for empty text).
fn first_char(text: &str) -> &str {
    text.chars().next().map_or("", |c| &text[..c.len_utf8()])
}

/// How PHP writes names (rename, resolution, the separator).
const NAMING: Naming = Naming {
    sigil: Some('$'),
    receiver: Some("$this"),
    implicit_locals: true,
    constructor: Some("__construct"),
    methods_ignore_case: true,
    names_ignore_case: true,
    namespace_separator: Some('\\'),
    type_per_file: Some("php"),
    own_type: &["self", "static"],
    parent_type: Some("parent"),
    type_name_member: Some("class"),
    construct: Some("new"),
    parameter_modifiers: &["public", "protected", "private", "readonly"],
};

/// Keywords followed by something other than a space.
const EXACT: &[&str] = &[
    "true", "false", "null", "self", "parent", "break", "continue", "default", "array", "void",
    "mixed", "never",
];
/// Keywords used as functions: `isset()`, with the cursor between the parentheses.
const CALL_LIKE: &[&str] = &["isset", "empty", "unset", "list", "die", "exit", "eval"];

// ─── Declaration extraction ───

struct Extractor<'a> {
    text: &'a str,
    file: FileSymbols,
    /// (index of the type in `file.symbols`, parent names as in the code); resolved at the end,
    /// once all `use` statements are known.
    parents: Vec<(usize, Vec<String>)>,
    /// The doc comment right before each declaration the walk passed (by node), noted as
    /// it goes: `prev_named_sibling` counts from the first sibling — at a file's top level,
    /// thousands of nodes for each declaration.
    docs: std::collections::HashMap<usize, Option<DocComment>>,
}

/// A doc comment (`/** … */`): where it is (bytes) and its first line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DocComment {
    pub start: usize,
    pub end: usize,
    pub row: usize,
}

// ─── Context at the cursor (from text: while typing, the tree is often incomplete) ───

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
