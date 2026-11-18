//! CSS and SCSS intelligence: selectors (classes, ids), variables (`--x`, `$x`) and
//! mixins from the project, plus the vocabulary (properties and their values).
//!
//! Everything is scanned directly from text (fast and robust to incomplete code). Also works for
//! CSS in `<style>` (HTML, PHP, Twig): the context starts at the last `<style>`.

use code_intelligence::{
    CompletionContext, CompletionItem, CompletionKind, ContextKind, FileSymbols,
    LanguageIntelligence, Symbol, SymbolKind, Target,
};
use languages::LanguageId;
use stylesheet::is_name;

const GLOBAL_VALUES: &[&str] =
    &["inherit", "initial", "unset", "revert", "auto", "none", "var()", "calc()"];

#[derive(Debug)]
pub struct CssIntelligence {
    language: LanguageId,
    extensions: &'static [&'static str],
    scss: bool,
}

impl CssIntelligence {
    pub fn css() -> Self {
        Self { language: LanguageId::new_static("css"), extensions: &["css"], scss: false }
    }

    pub fn scss() -> Self {
        Self { language: LanguageId::new_static("scss"), extensions: &["scss", "sass"], scss: true }
    }
}

/// What "Move Statement" moves whole: rules, at-rules, declarations.
const MOVED_WHOLE: &[&str] = &[
    "rule_set",
    "at_rule",
    "media_statement",
    "import_statement",
    "keyframes_statement",
    "keyframe_block",
    "supports_statement",
    "charset_statement",
    "namespace_statement",
    "declaration",
];

/// Where a stylesheet's selectors (classes, ids) can be used: markup, templates, scripts.
pub(crate) const SELECTOR_USES: &[&str] = &[
    "html", "htm", "php", "phtml", "twig", "js", "jsx", "ts", "tsx", "vue", "svelte", "css",
    "scss", "sass", "less",
];
/// Where its variables and mixins can be used.
pub(crate) const VALUE_USES: &[&str] =
    &["css", "scss", "sass", "less", "html", "vue", "svelte", "js", "jsx", "tsx"];

impl LanguageIntelligence for CssIntelligence {
    fn completion_triggers(&self) -> &'static str {
        "-:#.@\"' /"
    }

    /// Its selectors across the languages using them; its variables and mixins.
    fn usage_extensions(&self, kind: SymbolKind) -> &'static [&'static str] {
        match kind {
            SymbolKind::CssClass | SymbolKind::CssId => SELECTOR_USES,
            SymbolKind::CssVariable | SymbolKind::Mixin => VALUE_USES,
            _ => &[],
        }
    }

    fn css_references(&self) -> Option<code_intelligence::CssReferences> {
        Some(code_intelligence::CssReferences { stylesheet: true, ..Default::default() })
    }

    fn word_chars(&self) -> &'static str {
        "-"
    }

    fn statement_move(
        &self,
        text: &str,
        lines: std::ops::Range<usize>,
        up: bool,
    ) -> code_intelligence::StatementMove {
        let mut parser = syntax_ts::tree_sitter::Parser::new();
        if parser.set_language(&tree_sitter_css::LANGUAGE.into()).is_err() {
            return code_intelligence::StatementMove::Lines;
        }
        match parser.parse(text, None) {
            Some(tree) => syntax_ts::statement_move(&tree, text, lines, up, MOVED_WHOLE),
            None => code_intelligence::StatementMove::Lines,
        }
    }

    fn language(&self) -> LanguageId {
        self.language.clone()
    }

    fn extensions(&self) -> &[&'static str] {
        self.extensions
    }

    /// A class's or an id's whole rule (what it does), not just its name.
    fn declaration_code(&self, text: &str, offset: usize, kind: SymbolKind) -> Option<String> {
        matches!(kind, SymbolKind::CssClass | SymbolKind::CssId)
            .then(|| stylesheet::rule_at(text, offset))
            .flatten()
    }

    // Code beside a file in its folder; nothing the dependency graph follows.
    fn source_rules(&self) -> Option<code_intelligence::SourceRules> {
        let comments = match self.scss {
            true => crate::comment_places::in_scss,
            false => crate::comment_places::in_css,
        };
        Some(code_intelligence::SourceRules {
            comments: Some(code_intelligence::CommentReader(comments)),
            ..code_intelligence::SourceRules::plain(self.extensions)
        })
    }

    fn extract(&self, text: &str) -> FileSymbols {
        extract(text)
    }

    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        context(text, offset, self.scss)
    }

    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        target(text, offset)
    }

    /// CSS's comments are text; SCSS's SassDoc (`///`) names mixins, variables, parameters.
    fn comment_place(&self, text: &str, offset: usize) -> Option<code_intelligence::CommentPlace> {
        crate::comment_places::place(text, offset, self.scss)
    }

    /// The declaration in this same stylesheet (a variable, mixin, class or id) wins over
    /// the ones in other files.
    fn local_definition(&self, text: &str, offset: usize) -> Option<usize> {
        let Some(Target::Named { name, kinds }) = target(text, offset) else { return None };
        extract(text)
            .symbols
            .into_iter()
            .find(|s| *s.name == *name && kinds.contains(&s.kind))
            .map(|s| s.offset)
    }

    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }

    fn keywords(&self) -> &[&'static str] {
        &[]
    }

    fn deprecated(&self, text: &str) -> Vec<code_intelligence::Deprecated> {
        crate::deprecated::find(text)
    }

    fn hover(&self, text: &str, offset: usize) -> Option<code_intelligence::HoverInfo> {
        crate::hover::hover(text, offset)
    }

    fn vocabulary(&self, context: &CompletionContext, text: &str) -> Vec<CompletionItem> {
        match context.kind {
            ContextKind::Vocabulary(SASSDOC_TAG) => {
                code_intelligence::comments::tag_items(SASSDOC_TAGS, "sassdoc", SASSDOC_SPACED)
            }
            ContextKind::Vocabulary("property") => crate::data::get()
                .properties
                .iter()
                .map(|p| {
                    let mut item =
                        CompletionItem::word(p.name, CompletionKind::Attribute, Some(&detail(p)));
                    item.insert = format!("{}: ", p.name);
                    // Deprecated / non-standard ones after the others.
                    item.score -= if p.flags.is_empty() { 0 } else { 4 };
                    item
                })
                .collect(),
            ContextKind::Vocabulary("value") => {
                let property = property_before(text, context.replace.start);
                let css = crate::data::get();
                let specific = css.values(&property);
                specific
                    .iter()
                    .chain(GLOBAL_VALUES)
                    .map(|v| {
                        let mut item = CompletionItem::word(v, CompletionKind::Value, None);
                        if v.ends_with("()") {
                            item.cursor_back = 1;
                        }
                        // The property's own values before the global ones.
                        item.score += if specific.contains(v) { 6 } else { 0 };
                        item
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

// ─── Text scanning ───

/// Start of the CSS region containing `offset` (after the last `<style…>`, if any).
fn region_start(text: &str, offset: usize) -> usize {
    let before = &text[..offset];
    match (before.rfind("<style"), before.rfind("</style")) {
        (Some(open), close) if close.is_none_or(|c| c < open) => {
            before[open..].find('>').map_or(open, |i| open + i + 1)
        }
        _ => 0,
    }
}

/// State at `offset`: brace depth, whether we are in a comment, and the start
/// of the current statement (after the last `{`, `}` or `;`).
struct State {
    depth: usize,
    in_comment: bool,
    statement: usize,
}

fn scan(text: &str, start: usize, offset: usize) -> State {
    let bytes = text.as_bytes();
    let (mut depth, mut statement, mut i) = (0usize, start, start);
    let mut quote: Option<u8> = None;
    while i < offset {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 1;
            } else if b == q {
                quote = None;
            }
        } else if b == b'/' && bytes.get(i + 1) == Some(&b'*') {
            match text[i + 2..].find("*/") {
                Some(end) if i + 2 + end + 2 <= offset => i += 2 + end + 1,
                _ => return State { depth, in_comment: true, statement },
            }
        } else if b == b'"' || b == b'\'' {
            quote = Some(b);
        } else if b == b'{' {
            depth += 1;
            statement = i + 1;
        } else if b == b'}' {
            depth = depth.saturating_sub(1);
            statement = i + 1;
        } else if b == b';' {
            statement = i + 1;
        }
        i += 1;
    }
    State { depth, in_comment: quote.is_some(), statement }
}

/// Property name of the statement containing `at` (`color: re|d` → `color`).
fn property_before(text: &str, at: usize) -> String {
    let state = scan(text, region_start(text, at), at);
    let statement = &text[state.statement..at];
    statement.split(':').next().unwrap_or("").trim().to_ascii_lowercase()
}

/// The completion of SassDoc's tags (`/// @param`…).
const SASSDOC_TAG: &str = "sassdoc-tag";

/// SassDoc's tags (their descriptions in the locales: `sassdoc.<tag>`).
const SASSDOC_TAGS: &[&str] = &[
    "@param",
    "@return",
    "@type",
    "@prop",
    "@content",
    "@output",
    "@require",
    "@see",
    "@link",
    "@example",
    "@deprecated",
    "@since",
    "@author",
    "@group",
    "@access",
    "@alias",
    "@throw",
    "@todo",
    "@ignore",
    "@name",
];

/// The SassDoc tags followed by something: a space after them.
const SASSDOC_SPACED: &[&str] = &[
    "param",
    "return",
    "type",
    "prop",
    "content",
    "output",
    "require",
    "see",
    "link",
    "since",
    "author",
    "group",
    "access",
    "alias",
    "throw",
    "todo",
    "name",
    "deprecated",
];

fn context(text: &str, offset: usize, scss: bool) -> CompletionContext {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let prefix = text[start..offset].to_owned();
    let none = CompletionContext {
        replace: offset..offset,
        prefix: String::new(),
        kind: ContextKind::None,
    };
    // `/// @ret|` (SassDoc): its tags.
    let line = &text[text[..offset].rfind('\n').map_or(0, |i| i + 1)..offset];
    if scss
        && line.trim_start().starts_with("///")
        && let Some(at) = code_intelligence::comments::tag_at(text, offset)
    {
        return CompletionContext {
            replace: at..offset,
            prefix: text[at..offset].to_owned(),
            kind: ContextKind::Vocabulary(SASSDOC_TAG),
        };
    }
    let state = scan(text, region_start(text, offset), offset);
    if state.in_comment {
        return none;
    }
    let pre = &text[..start];
    let with = |replace: std::ops::Range<usize>, kind| CompletionContext {
        replace,
        prefix: prefix.clone(),
        kind,
    };
    // Variables: `--x` (in `var(`), `$x` (SCSS); mixins after `@include`.
    if prefix.starts_with("--") {
        return with(start..offset, ContextKind::Symbols(vec![SymbolKind::CssVariable]));
    }
    if scss && pre.ends_with('$') {
        return CompletionContext {
            replace: start - 1..offset,
            prefix: format!("${prefix}"),
            kind: ContextKind::Symbols(vec![SymbolKind::CssVariable]),
        };
    }
    if scss && pre.trim_end().ends_with("@include") {
        return with(start..offset, ContextKind::Symbols(vec![SymbolKind::Mixin]));
    }
    let statement = &text[state.statement..start];
    let in_selector =
        state.depth == 0 || (scss && statement.trim_start().starts_with(['.', '#', '&']));
    if in_selector {
        return match pre.chars().last() {
            Some('.') => with(start..offset, ContextKind::Symbols(vec![SymbolKind::CssClass])),
            Some('#') => with(start..offset, ContextKind::Symbols(vec![SymbolKind::CssId])),
            _ => none,
        };
    }
    if statement.contains(':') {
        return with(start..offset, ContextKind::Vocabulary("value"));
    }
    if prefix.is_empty() {
        return none;
    }
    with(start..offset, ContextKind::Vocabulary("property"))
}

fn target(text: &str, offset: usize) -> Option<Target> {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let end = text[offset..]
        .char_indices()
        .find(|(_, c)| !is_name(*c))
        .map_or(text.len(), |(i, _)| offset + i);
    let word = &text[start..end];
    if word.is_empty() {
        return None;
    }
    let pre = &text[..start];
    let named = |name: String, kind| Some(Target::Named { name, kinds: vec![kind] });
    if word.starts_with("--") {
        return named(word.to_owned(), SymbolKind::CssVariable);
    }
    if pre.ends_with('$') {
        return named(format!("${word}"), SymbolKind::CssVariable);
    }
    if pre.trim_end().ends_with("@include") {
        return named(word.to_owned(), SymbolKind::Mixin);
    }
    match pre.chars().last() {
        Some('.') => named(word.to_owned(), SymbolKind::CssClass),
        Some('#') => named(word.to_owned(), SymbolKind::CssId),
        _ => None,
    }
}

/// Declarations: classes and ids from selectors, variables and mixins.
fn extract(text: &str) -> FileSymbols {
    let mut file = FileSymbols::default();
    let line_starts: Vec<usize> =
        std::iter::once(0).chain(text.match_indices('\n').map(|(i, _)| i + 1)).collect();
    let line_of =
        |offset: usize| line_starts.partition_point(|&s| s <= offset).saturating_sub(1) as u32;
    // Each name once (found in a set: a framework's stylesheet names thousands, each many
    // times).
    let mut seen: std::collections::HashSet<(SymbolKind, String)> =
        std::collections::HashSet::new();
    let mut push = |file: &mut FileSymbols, name: String, kind: SymbolKind, offset: usize| {
        if seen.insert((kind, name.clone())) {
            file.symbols.push(Symbol {
                fqn: name.clone().into_boxed_str(),
                name: name.into_boxed_str(),
                kind,
                container: None,
                parents: Box::default(),
                range: offset..offset,
                offset,
                line: line_of(offset),
                detail: None,
                is_static: false,
                visibility: Default::default(),
                note: None,
                types: None,
            });
        }
    };
    let bytes = text.as_bytes();
    let (mut statement, mut i) = (0usize, 0usize);
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = quote {
            if b == b'\\' {
                i += 1;
            } else if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match b {
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 2);
                statement = i;
                continue;
            }
            b'"' | b'\'' => quote = Some(b),
            b'{' => {
                selector(&text[statement..i], statement, &mut file, &mut push);
                statement = i + 1;
            }
            b';' | b'}' => {
                declaration(&text[statement..i], statement, &mut file, &mut push);
                statement = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    file
}

type Push<'a> = dyn FnMut(&mut FileSymbols, String, SymbolKind, usize) + 'a;

/// A selector (or `@mixin name(...)`) before an opening brace.
fn selector(segment: &str, base: usize, file: &mut FileSymbols, push: &mut Push<'_>) {
    let trimmed = segment.trim_start();
    let lead = segment.len() - trimmed.len();
    for at in ["@mixin", "@function"] {
        if let Some(rest) = trimmed.strip_prefix(at) {
            let name: String = rest.trim_start().chars().take_while(|c| is_name(*c)).collect();
            if !name.is_empty() {
                let offset = base + lead + at.len() + (rest.len() - rest.trim_start().len());
                push(file, name, SymbolKind::Mixin, offset);
            }
            return;
        }
    }
    if trimmed.starts_with('@') {
        return;
    }
    for (kind, name, at) in stylesheet::selector_names(segment) {
        push(file, name, kind, base + at);
    }
}

/// A `--x: …` or `$x: …` declaration (variable).
fn declaration(segment: &str, base: usize, file: &mut FileSymbols, push: &mut Push<'_>) {
    let trimmed = segment.trim_start();
    let lead = segment.len() - trimmed.len();
    let Some((name, _)) = trimmed.split_once(':') else { return };
    let name = name.trim_end();
    let is_var = (name.starts_with("--") || name.starts_with('$'))
        && name[1..].chars().all(|c| is_name(c) || c == '-');
    if is_var && name.len() > 2 {
        push(file, name.to_owned(), SymbolKind::CssVariable, base + lead);
    }
}

/// The detail of a property in the list: deprecated / experimental, else its syntax
/// (shortened).
fn detail(p: &crate::data::Property) -> String {
    if p.flags.contains('d') {
        return "deprecated".into();
    }
    if p.flags.contains('x') {
        return "experimental".into();
    }
    let mut syntax: String = p.syntax.chars().take(40).collect();
    if p.syntax.chars().count() > 40 {
        syntax.push('…');
    }
    syntax
}

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
