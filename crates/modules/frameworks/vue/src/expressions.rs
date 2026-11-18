//! Vue's template expressions, as code: the directives' values (`v-if="…"`, `:class="…"`,
//! `@click="…"`, `#item="…"`) wherever they're written, and the `{{ }}` interpolations in a
//! component file (one with a top-level `<template>`: elsewhere `{{ }}` may be another
//! template language's). JavaScript, or TypeScript when the component's script is
//! `lang="ts"`.

use std::sync::OnceLock;

use languages::LanguageId;
use syntax_ts::tree_sitter::{Node, Query};
use syntax_ts::{Injection, InjectionContext, InjectionProvider, InjectionSource};

const HTML: LanguageId = LanguageId::new_static("html");

/// A directive's value, and the text between tags.
const PATTERNS: &str = r#"
(attribute (attribute_name) @name (quoted_attribute_value (attribute_value) @value))
(text) @text
"#;

/// Directives without a value that is code.
const NOT_CODE: &[&str] = &["v-else", "v-cloak", "v-once", "v-pre", "v-text"];

#[derive(Debug, Default)]
pub struct TemplateExpressions {
    query: OnceLock<Option<Query>>,
}

/// A directive: `v-…`, `:…` (bind), `@…` (on), `#…` (slot).
fn is_directive(name: &str) -> bool {
    (name.starts_with("v-") || name.starts_with([':', '@', '#'])) && !NOT_CODE.contains(&name)
}

/// The component file's facts: whether it has a top-level `<template>`, and its script's
/// language.
fn component(ctx: &InjectionContext<'_>) -> (bool, LanguageId) {
    let mut template = false;
    let mut language = LanguageId::new_static("javascript");
    let mut cursor = ctx.root.walk();
    for child in ctx.root.children(&mut cursor) {
        let Some(tag) = child.child(0).filter(|t| t.kind() == "start_tag") else { continue };
        let name = tag.child(1).map(|n| ctx.node_text(n)).unwrap_or_default();
        match (child.kind(), name.as_str()) {
            ("element", "template") => template = true,
            ("script_element", _) => {
                if let Some(lang) = attribute(ctx, tag, "lang") {
                    language = match lang.as_str() {
                        "ts" | "typescript" => LanguageId::new_static("typescript"),
                        "tsx" => LanguageId::new_static("tsx"),
                        _ => language,
                    };
                }
            }
            _ => {}
        }
    }
    (template, language)
}

/// The value of a start tag's attribute `name`.
fn attribute(ctx: &InjectionContext<'_>, tag: Node<'_>, name: &str) -> Option<String> {
    let mut cursor = tag.walk();
    let found = tag.children(&mut cursor).find(|a| {
        a.kind() == "attribute" && a.child(0).is_some_and(|n| ctx.node_text(n) == name)
    })?;
    let value = found.child(2)?;
    let value = value.child(1).filter(|v| v.kind() == "attribute_value").unwrap_or(value);
    Some(ctx.node_text(value))
}

/// The `{{ … }}` inside `text` (starting at `start`): the ranges between the braces.
pub(crate) fn interpolations(text: &str, start: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find("{{") {
        let inner = from + open + 2;
        let Some(close) = text[inner..].find("}}") else { break };
        if close > 0 {
            out.push(start + inner..start + inner + close);
        }
        from = inner + close + 2;
    }
    out
}

impl InjectionProvider for TemplateExpressions {
    fn id(&self) -> &str {
        "vue.template_expressions"
    }

    fn hosts(&self) -> &[LanguageId] {
        static HOSTS: [LanguageId; 1] = [HTML];
        &HOSTS
    }

    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>) {
        let Some(query) = self.query.get_or_init(|| Query::new(ctx.language, PATTERNS).ok()) else {
            return;
        };
        let (Some(name), Some(value), Some(text)) = (
            query.capture_index_for_name("name"),
            query.capture_index_for_name("value"),
            query.capture_index_for_name("text"),
        ) else {
            return;
        };
        let (component, language) = component(ctx);
        let mut push = |range: std::ops::Range<usize>| {
            out.push(Injection {
                ranges: vec![range],
                language: language.clone(),
                source: InjectionSource::Rule,
            });
        };
        ctx.for_each_match(query, |m| {
            let node = |index| m.captures().iter().find(|c| c.index == index).map(|c| c.node);
            if let (Some(n), Some(v)) = (node(name), node(value)) {
                if is_directive(&ctx.node_text(n)) && !v.byte_range().is_empty() {
                    push(v.byte_range());
                }
            } else if let Some(t) = node(text).filter(|_| component) {
                for range in interpolations(&ctx.node_text(t), t.start_byte()) {
                    push(range);
                }
            }
        });
    }
}

#[cfg(test)]
#[path = "../tests/unit/expressions.rs"]
mod tests;
