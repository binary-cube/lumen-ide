//! HTML intelligence: tags, attributes (global and tag-specific) and, inside
//! `class="…"` / `id="…"`, the CSS selectors from the whole project. Also works for HTML in
//! PHP or Twig (the context is read from the text around the cursor).

use code_intelligence::{
    CompletionContext, CompletionItem, CompletionKind, ContextKind, FileSymbols,
    LanguageIntelligence, SymbolKind, Target,
};
use languages::LanguageId;

use crate::LANGUAGE;

/// Attributes without a value (inserted without `=""`).
const BOOLEAN: &[&str] = &[
    "hidden",
    "required",
    "disabled",
    "checked",
    "readonly",
    "autofocus",
    "multiple",
    "selected",
    "defer",
    "async",
    "controls",
    "autoplay",
    "muted",
    "loop",
    "playsinline",
    "novalidate",
    "download",
];

const ATTRIBUTE_VALUES: &[(&str, &[&str])] = &[
    (
        "type",
        &[
            "text",
            "email",
            "password",
            "number",
            "tel",
            "url",
            "search",
            "date",
            "datetime-local",
            "time",
            "checkbox",
            "radio",
            "file",
            "hidden",
            "submit",
            "button",
            "reset",
            "color",
            "range",
        ],
    ),
    ("target", &["_blank", "_self", "_parent", "_top"]),
    ("method", &["get", "post"]),
    (
        "rel",
        &[
            "stylesheet",
            "icon",
            "preload",
            "preconnect",
            "noopener",
            "noreferrer",
            "nofollow",
            "canonical",
        ],
    ),
    ("loading", &["lazy", "eager"]),
    (
        "autocomplete",
        &["on", "off", "email", "username", "current-password", "new-password", "name"],
    ),
];

#[derive(Debug, Default)]
pub struct HtmlIntelligence;

/// What "Move Statement" moves whole: elements (with their content).
const MOVED_WHOLE: &[&str] = &["element", "script_element", "style_element"];

impl LanguageIntelligence for HtmlIntelligence {
    fn completion_triggers(&self) -> &'static str {
        "<\"'- /#@:."
    }

    /// The selectors of its `<style>` blocks, where a stylesheet's are used.
    fn usage_extensions(&self, kind: SymbolKind) -> &'static [&'static str] {
        match kind {
            SymbolKind::CssClass | SymbolKind::CssId => &[
                "html", "htm", "php", "phtml", "twig", "js", "jsx", "ts", "tsx", "vue", "svelte",
                "css", "scss", "sass", "less",
            ],
            _ => &[],
        }
    }

    fn css_references(&self) -> Option<code_intelligence::CssReferences> {
        Some(code_intelligence::CssReferences {
            class_attributes: &["class=\"", "class='"],
            id_attributes: &["id=\"", "id='"],
            ..Default::default()
        })
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
        if parser.set_language(&tree_sitter_html::LANGUAGE.into()).is_err() {
            return code_intelligence::StatementMove::Lines;
        }
        match parser.parse(text, None) {
            Some(tree) => syntax_ts::statement_move(&tree, text, lines, up, MOVED_WHOLE),
            None => code_intelligence::StatementMove::Lines,
        }
    }

    fn language(&self) -> LanguageId {
        LANGUAGE
    }

    // HTML declares no symbols used in other files (classes come from CSS).
    fn extensions(&self) -> &[&'static str] {
        &["html", "htm", "vue"]
    }

    // Pages and templates beside a file in its folder; nothing the graph follows (a Vue
    // component's script is JavaScript's).
    fn source_rules(&self) -> Option<code_intelligence::SourceRules> {
        Some(code_intelligence::SourceRules::plain(&["html", "htm", "twig"]))
    }

    /// The classes and ids its `<style>` blocks declare (so `class="…"` leads to them).
    fn extract(&self, text: &str) -> FileSymbols {
        let mut file = FileSymbols::default();
        stylesheet::add_style_blocks(text, &mut file);
        file
    }

    fn context(&self, text: &str, offset: usize) -> CompletionContext {
        context(text, offset)
    }

    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        target(text, offset)
    }

    /// A comment (`<!-- … -->`) is text: what's commented out leads nowhere (plan 75).
    fn comment_place(&self, text: &str, offset: usize) -> Option<code_intelligence::CommentPlace> {
        in_comment(text, offset).then_some(code_intelligence::CommentPlace::Text)
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

    fn on_type(
        &self,
        text: &str,
        offset: usize,
        typed: char,
    ) -> Option<code_intelligence::TypedInsert> {
        if typed != '>' {
            return None;
        }
        closing_tag(text, offset).map(code_intelligence::TypedInsert::after_cursor)
    }

    fn on_type_triggers(&self) -> &'static [char] {
        &['>']
    }

    fn vocabulary(&self, context: &CompletionContext, text: &str) -> Vec<CompletionItem> {
        let tag = || current_tag(text, context.replace.start).unwrap_or_default();
        match context.kind {
            ContextKind::Vocabulary("tag") => crate::data::get()
                .elements
                .iter()
                .map(|(t, flags)| {
                    let mut item = CompletionItem::word(t, CompletionKind::Tag, status(flags));
                    item.score -= if flags.is_empty() { 0 } else { 4 };
                    item
                })
                .collect(),
            ContextKind::Vocabulary("attribute") => {
                let tag = tag();
                let data = crate::data::get();
                let specific = data.attributes(&tag);
                specific
                    .iter()
                    .map(|a| (a, 6))
                    .chain(data.global_attributes().iter().map(|a| (a, 0)))
                    .map(|((a, flags), own)| {
                        let mut item =
                            CompletionItem::word(a, CompletionKind::Attribute, status(flags));
                        if !BOOLEAN.contains(a) && !a.ends_with('-') {
                            item.insert = format!("{a}=\"\"");
                            item.cursor_back = 1;
                        }
                        // The tag's own attributes before the global ones; deprecated last.
                        item.score += own - if flags.is_empty() { 0 } else { 4 };
                        item
                    })
                    .collect()
            }
            ContextKind::Vocabulary("value") => {
                let attribute = attribute_name(text, context.replace.start).unwrap_or_default();
                let data = crate::data::get();
                let known = data.values(&tag(), &attribute);
                let listed = ATTRIBUTE_VALUES
                    .iter()
                    .find(|(a, _)| *a == attribute)
                    .map_or(&[][..], |(_, v)| v);
                let mut values: Vec<&str> = known.iter().chain(listed).copied().collect();
                values.sort_unstable();
                values.dedup();
                values
                    .into_iter()
                    .map(|v| CompletionItem::word(v, CompletionKind::Value, None))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
}

/// "deprecated" / "experimental" / "non-standard", for the list.
fn status(flags: &str) -> Option<&'static str> {
    if flags.contains('d') {
        Some("deprecated")
    } else if flags.contains('x') {
        Some("experimental")
    } else if flags.contains('n') {
        Some("non-standard")
    } else {
        None
    }
}

// Tags without a closing tag: HTML's one list.
use markup::VOID;

/// `</tag>` for a tag just opened (`>` is at `offset - 1`), if it needs closing.
fn closing_tag(text: &str, offset: usize) -> Option<String> {
    let gt = offset.checked_sub(1).filter(|&i| text.as_bytes().get(i) == Some(&b'>'))?;
    let (start, inside) = open_tag(text, gt)?;
    if inside.starts_with(['/', '!', '?']) || inside.trim_end().ends_with('/') {
        return None;
    }
    let tag: String = text[start..gt].chars().take_while(|c| is_name(*c)).collect();
    if tag.is_empty() || VOID.contains(&tag.to_ascii_lowercase().as_str()) {
        return None;
    }
    // Already closed right after (e.g. the completion inserted it).
    let closing = format!("</{tag}>");
    (!text[offset..].starts_with(&closing)).then_some(closing)
}

fn is_name(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '-' | '_' | ':' | '.' | '@')
}

/// Text of the open tag that contains `offset` (after `<`), if any.
fn open_tag(text: &str, offset: usize) -> Option<(usize, &str)> {
    let before = &text[..text.floor_char_boundary(offset)];
    let lt = before.rfind('<')?;
    if before[lt..].contains('>') {
        return None;
    }
    Some((lt + 1, &before[lt + 1..]))
}

fn current_tag(text: &str, offset: usize) -> Option<String> {
    let (_, inside) = open_tag(text, offset)?;
    Some(inside.chars().take_while(|c| is_name(*c)).collect::<String>().to_ascii_lowercase())
}

/// Attribute whose (quoted) value contains `offset`: (name, value start).
fn attribute_value(text: &str, offset: usize) -> Option<(String, usize)> {
    let (start, inside) = open_tag(text, offset)?;
    let bytes = inside.as_bytes();
    let mut quote: Option<(u8, usize)> = None;
    for (i, &b) in bytes.iter().enumerate() {
        match quote {
            Some((q, _)) if b == q => quote = None,
            None if b == b'"' || b == b'\'' => quote = Some((b, i)),
            _ => {}
        }
    }
    let (_, open) = quote?;
    let before = inside[..open].trim_end().strip_suffix('=')?.trim_end();
    let name: String = before
        .chars()
        .rev()
        .take_while(|c| is_name(*c))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    Some((name.to_ascii_lowercase(), start + open + 1))
}

fn attribute_name(text: &str, offset: usize) -> Option<String> {
    attribute_value(text, offset).map(|(name, _)| name)
}

fn context(text: &str, offset: usize) -> CompletionContext {
    let offset = text.floor_char_boundary(offset);
    let none = CompletionContext {
        replace: offset..offset,
        prefix: String::new(),
        kind: ContextKind::None,
    };
    let Some((tag_start, inside)) = open_tag(text, offset) else { return none };
    // An attribute value: one word (classes are separated by spaces).
    if let Some((attribute, value_start)) = attribute_value(text, offset) {
        let start = text[value_start..offset]
            .rfind(char::is_whitespace)
            .map_or(value_start, |i| value_start + i + 1);
        let prefix = text[start..offset].to_owned();
        let kind = match attribute.as_str() {
            "class" | "classname" | ":class" => ContextKind::Symbols(vec![SymbolKind::CssClass]),
            "id" => ContextKind::Symbols(vec![SymbolKind::CssId]),
            _ => ContextKind::Vocabulary("value"),
        };
        return CompletionContext { replace: start..offset, prefix, kind };
    }
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_name(*c))
        .last()
        .map_or(offset, |(i, _)| i);
    let prefix = text[start..offset].to_owned();
    // Tag name (right after `<` or `</`).
    let name_start = tag_start + usize::from(inside.starts_with('/'));
    if start == name_start {
        return CompletionContext {
            replace: start..offset,
            prefix,
            kind: ContextKind::Vocabulary("tag"),
        };
    }
    if prefix.is_empty() && !text[..offset].ends_with(char::is_whitespace) {
        return none;
    }
    CompletionContext { replace: start..offset, prefix, kind: ContextKind::Vocabulary("attribute") }
}

/// Whether `offset` is inside a `<!-- … -->` comment.
fn in_comment(text: &str, offset: usize) -> bool {
    let before = &text[..text.floor_char_boundary(offset)];
    before.rfind("<!--").is_some_and(|open| !before[open + 4..].contains("-->"))
}

fn target(text: &str, offset: usize) -> Option<Target> {
    let offset = text.floor_char_boundary(offset);
    let (attribute, value_start) = attribute_value(text, offset)?;
    let kind = match attribute.as_str() {
        "class" | "classname" | ":class" => SymbolKind::CssClass,
        "id" => SymbolKind::CssId,
        _ => return None,
    };
    let word = |c: char| !c.is_whitespace() && c != '"' && c != '\'';
    let start = text[value_start..offset]
        .rfind(|c: char| !word(c))
        .map_or(value_start, |i| value_start + i + 1);
    let end = text[offset..].find(|c: char| !word(c)).map_or(text.len(), |i| offset + i);
    let name = text.get(start..end)?.to_owned();
    (!name.is_empty()).then_some(Target::Named { name, kinds: vec![kind] })
}

#[cfg(test)]
#[path = "../tests/unit/intelligence.rs"]
mod tests;
