//! Emmet module: HTML abbreviations expand on Tab.
//!
//! `div.card#main` → `<div class="card" id="main"></div>`, `ul>li.item$*3>a`,
//! `form>input:email[required]+btn:s{Send}`, `!` (HTML5 skeleton), `lorem10`. Missing
//! tags are inferred from the parent (`ul>.x` → `li`), void tags have no closing tag, and
//! the cursor lands on the first empty attribute or the first empty content.

mod parse;

use std::sync::Arc;

use code_intelligence::{Expander, Expansion};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

use crate::parse::Node;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("emmet"),
    name: "Emmet",
    description: "HTML abbreviations expanded on Tab: div.card#main, ul>li*3, a[href], !, lorem",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "zap", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct EmmetModule;

impl Module for EmmetModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn Expander>>(Arc::new(Emmet));
        Ok(())
    }
}

#[derive(Debug)]
pub struct Emmet;

impl Expander for Emmet {
    fn languages(&self) -> &[&'static str] {
        &["html", "twig", "blade", "xml", "svg", "vue"]
    }

    fn expand(&self, line_before: &str, indent: &str, unit: &str) -> Option<Expansion> {
        let abbreviation = extract(line_before)?;
        let text = expand(abbreviation, indent, unit)?;
        // Cursor: the first empty attribute (`=""`), otherwise the first empty content (`><`).
        let cursor = text
            .find("=\"\"")
            .map(|i| i + 2)
            .or_else(|| text.find("></").map(|i| i + 1))
            .unwrap_or(text.len());
        Some(Expansion { replace: abbreviation.len(), text, cursor })
    }
}

/// The abbreviation at the end of the line: allowed characters, with spaces only inside `[ ]`,
/// `{ }` or quotes; before it, line start, a space or `>`.
fn extract(line: &str) -> Option<&str> {
    let bytes: Vec<(usize, char)> = line.char_indices().collect();
    let mut depth_square = 0i32;
    let mut depth_brace = 0i32;
    let mut start = line.len();
    for &(i, c) in bytes.iter().rev() {
        match c {
            ']' => depth_square += 1,
            '[' => depth_square -= 1,
            '}' => depth_brace += 1,
            '{' => depth_brace -= 1,
            _ => {}
        }
        let inside = depth_square > 0 || depth_brace > 0;
        let allowed = c.is_alphanumeric()
            || matches!(
                c,
                '.' | '#'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '>'
                    | '+'
                    | '^'
                    | '*'
                    | '$'
                    | '('
                    | ')'
                    | '!'
                    | ':'
                    | '@'
                    | '-'
                    | '_'
                    | '='
                    | '"'
                    | '\''
            );
        if !(allowed || inside) {
            break;
        }
        if depth_square < 0 || depth_brace < 0 {
            break;
        }
        start = i;
    }
    // Scanning stopped at `<` (e.g. `<p>div.a`): the abbreviation starts after the open tag.
    let mut abbreviation = &line[start..];
    if line[..start].ends_with('<') {
        let gt = abbreviation.find('>')?;
        abbreviation = &abbreviation[gt + 1..];
    }
    // A leading `>` belongs to the preceding tag (`<p>ul>li`): the abbreviation starts after it.
    while let Some(rest) = abbreviation.strip_prefix('>') {
        abbreviation = rest;
    }
    let first = abbreviation.chars().next()?;
    if !(first.is_alphanumeric() || matches!(first, '.' | '#' | '[' | '(' | '!')) {
        return None;
    }
    // A plain word expands only if it is a known tag, alias or snippet
    // (Tab does not mangle ordinary text).
    let plain = abbreviation.chars().all(|c| c.is_alphanumeric() || c == '-');
    if plain && !is_known(abbreviation) {
        return None;
    }
    // Not inside a tag (`<div class="x|`).
    let before = &line[..line.len() - abbreviation.len()];
    if before.rfind('<').is_some_and(|lt| before.rfind('>').is_none_or(|gt| gt < lt)) {
        return None;
    }
    Some(abbreviation)
}

const TAGS: &[&str] = &[
    "a",
    "abbr",
    "address",
    "area",
    "article",
    "aside",
    "audio",
    "b",
    "base",
    "blockquote",
    "body",
    "br",
    "button",
    "canvas",
    "caption",
    "code",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "em",
    "embed",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "i",
    "iframe",
    "img",
    "input",
    "label",
    "legend",
    "li",
    "link",
    "main",
    "mark",
    "meta",
    "nav",
    "noscript",
    "object",
    "ol",
    "optgroup",
    "option",
    "output",
    "p",
    "picture",
    "pre",
    "progress",
    "section",
    "select",
    "small",
    "source",
    "span",
    "strong",
    "style",
    "sub",
    "summary",
    "sup",
    "svg",
    "table",
    "tbody",
    "td",
    "template",
    "textarea",
    "tfoot",
    "th",
    "thead",
    "time",
    "title",
    "tr",
    "u",
    "ul",
    "video",
];

// Tags without a closing tag: HTML's one list.
use markup::VOID;

/// Elements written on a single line, even with children.
const INLINE: &[&str] = &[
    "a", "abbr", "b", "br", "button", "code", "em", "i", "img", "input", "label", "mark", "small",
    "span", "strong", "sub", "sup", "u", "option", "td", "th", "li", "h1", "h2", "h3", "h4", "h5",
    "h6", "p", "title", "dt", "dd",
];

/// Aliases: (abbreviation, what it means).
const ALIASES: &[(&str, &str)] = &[
    ("btn", "button"),
    ("btn:s", "button[type=submit]"),
    ("btn:r", "button[type=reset]"),
    ("btn:b", "button[type=button]"),
    ("inp", "input[type=text name id]"),
    ("a:link", "a[href=http://]"),
    ("a:mail", "a[href=mailto:]"),
    ("a:blank", "a[href target=_blank rel=noopener]"),
    ("link:css", "link[rel=stylesheet href=style.css]"),
    ("link:favicon", "link[rel=icon type=image/x-icon href=favicon.ico]"),
    ("script:src", "script[src]"),
    ("img:srcset", "img[srcset src alt]"),
    ("meta:vp", "meta[name=viewport content=\"width=device-width, initial-scale=1\"]"),
    ("meta:utf", "meta[charset=UTF-8]"),
    ("form:post", "form[action method=post]"),
    ("form:get", "form[action method=get]"),
    ("select", "select[name id]>option"),
    ("ul+", "ul>li"),
    ("ol+", "ol>li"),
    ("dl+", "dl>dt+dd"),
    ("table+", "table>tr>td"),
    ("tr+", "tr>td"),
    ("bq", "blockquote"),
    ("fig", "figure"),
    ("figc", "figcaption"),
    ("pic", "picture"),
    ("art", "article"),
    ("sect", "section"),
    ("hdr", "header"),
    ("ftr", "footer"),
    ("str", "strong"),
    ("leg", "legend"),
    ("opt", "option"),
    ("tarea", "textarea"),
];

/// Default attributes (empty, to fill in) of some tags.
fn default_attrs(tag: &str) -> &'static [(&'static str, Option<&'static str>)] {
    match tag {
        "a" => &[("href", Some(""))],
        "img" => &[("src", Some("")), ("alt", Some(""))],
        "input" => &[("type", Some("text"))],
        "link" => &[("rel", Some("stylesheet")), ("href", Some(""))],
        "form" => &[("action", Some(""))],
        "label" => &[("for", Some(""))],
        "iframe" => &[("src", Some(""))],
        "script" => &[],
        "source" => &[("src", Some("")), ("type", Some(""))],
        "textarea" => &[("name", Some("")), ("id", Some(""))],
        "video" | "audio" => &[("src", Some(""))],
        "abbr" => &[("title", Some(""))],
        "time" => &[("datetime", Some(""))],
        _ => &[],
    }
}

fn is_known(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    TAGS.contains(&lower.as_str())
        || ALIASES.iter().any(|(a, _)| *a == lower)
        || lower.starts_with("lorem")
        || lower == "html"
}

const HTML5: &str = "<!DOCTYPE html>\n<html lang=\"ro\">\n<head>\n\t<meta charset=\"UTF-8\">\n\t<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n\t<title>Document</title>\n</head>\n<body>\n\t\n</body>\n</html>";

const LOREM: &[&str] = &[
    "Lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "Ut",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
];

/// The most words a `loremN` gives.
pub const MOST_WORDS: usize = 10_000;

fn too_many_words(nodes: &[parse::Node]) -> bool {
    nodes.iter().any(|n| {
        n.name
            .strip_prefix("lorem")
            .and_then(|w| w.parse::<usize>().ok())
            .is_some_and(|w| w > MOST_WORDS)
            || too_many_words(&n.children)
    })
}

fn lorem(words: usize) -> String {
    let mut text: String = (0..words).map(|i| LOREM[i % LOREM.len()]).collect::<Vec<_>>().join(" ");
    text.push('.');
    text
}

/// The full, indented expansion (`indent` on every line after the first).
pub fn expand(abbreviation: &str, indent: &str, unit: &str) -> Option<String> {
    if abbreviation == "!" || abbreviation == "html:5" {
        return Some(HTML5.replace('\t', unit).replace('\n', &format!("\n{indent}")));
    }
    let resolved = resolve_aliases(abbreviation);
    let nodes = parse::parse(&resolved)?;
    // `lorem999999999`: a text of gigabytes; past the most words, not expanded.
    if too_many_words(&nodes) {
        return None;
    }
    let mut out = String::new();
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
            out.push_str(indent);
        }
        render(node, None, indent, unit, &mut out);
    }
    Some(out)
}

/// Replaces aliases in the abbreviation (`btn:s` → `button[type=submit]`, `ul+`…), on
/// the segments between operators.
fn resolve_aliases(abbreviation: &str) -> String {
    // The whole thing first (e.g. `ul+` would otherwise be `ul` + an empty sibling).
    if let Some((_, full)) = ALIASES.iter().find(|(a, _)| *a == abbreviation) {
        return (*full).to_owned();
    }
    let mut out = String::new();
    let mut word = String::new();
    let mut depth = 0;
    let flush = |word: &mut String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        // The name (up to `.`, `#`, `[`, `{`, `*`) can be an alias.
        let split = word.find(['.', '#', '[', '{', '*']).unwrap_or(word.len());
        let (name, rest) = word.split_at(split);
        match ALIASES.iter().find(|(a, _)| *a == name) {
            Some((_, full)) if !full.contains('>') || rest.is_empty() => {
                // `btn:s.x` → `button[type=submit].x` (modifiers after the alias attributes).
                out.push_str(full);
                out.push_str(rest);
            }
            _ => match name.split_once(':') {
                // `input:email` → `input[type=email]`, `button:submit` etc.
                Some((tag @ ("input" | "button"), kind)) if !kind.is_empty() => {
                    out.push_str(&format!("{tag}[type={kind}]"));
                    out.push_str(rest);
                }
                _ => out.push_str(word),
            },
        }
        word.clear();
    };
    for c in abbreviation.chars() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && matches!(c, '>' | '+' | '^' | '(' | ')') {
            flush(&mut word, &mut out);
            out.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

/// Default tag based on the parent (`ul>.x` → `li`).
fn implicit(parent: Option<&str>) -> &'static str {
    match parent {
        Some("ul" | "ol") => "li",
        Some("table" | "tbody" | "thead" | "tfoot") => "tr",
        Some("tr") => "td",
        Some("select" | "optgroup") => "option",
        Some("dl") => "dt",
        Some(p)
            if INLINE.contains(&p)
                && !matches!(
                    p,
                    "li" | "td"
                        | "th"
                        | "p"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "dt"
                        | "dd"
                ) =>
        {
            "span"
        }
        _ => "div",
    }
}

fn render(node: &Node, parent: Option<&str>, indent: &str, unit: &str, out: &mut String) {
    // `lorem` / `loremN` as an element: text only.
    if let Some(n) = node.name.strip_prefix("lorem") {
        let words = n.parse().unwrap_or(30);
        out.push_str(&lorem(words));
        return;
    }
    let tag = if node.name.is_empty() {
        implicit(parent).to_owned()
    } else {
        node.name.to_ascii_lowercase()
    };
    out.push('<');
    out.push_str(&tag);
    // Attributes: id, classes, defaults (if not written), written ones.
    let mut attrs: Vec<(String, Option<String>)> = Vec::new();
    if !node.classes.is_empty() {
        attrs.push(("class".into(), Some(node.classes.join(" "))));
    }
    if let Some(id) = &node.id {
        attrs.push(("id".into(), Some(id.clone())));
    }
    for (name, value) in default_attrs(&tag) {
        if !node.attrs.iter().any(|(n, _)| n == name) {
            attrs.push(((*name).to_owned(), value.map(str::to_owned)));
        }
    }
    for (name, value) in &node.attrs {
        // `[href]` without a value = attribute to fill in (`href=""`); boolean exceptions
        // (`required`, `disabled`…) stay bare.
        let boolean = matches!(
            name.as_str(),
            "required"
                | "disabled"
                | "checked"
                | "selected"
                | "readonly"
                | "multiple"
                | "autofocus"
                | "hidden"
                | "defer"
                | "async"
                | "novalidate"
        );
        let value = match value {
            None if !boolean => Some(String::new()),
            v => v.clone(),
        };
        match attrs.iter_mut().find(|(n, _)| n == name) {
            Some(existing) => existing.1 = value,
            None => attrs.push((name.clone(), value)),
        }
    }
    for (name, value) in &attrs {
        out.push(' ');
        out.push_str(name);
        if let Some(value) = value {
            out.push_str("=\"");
            out.push_str(value);
            out.push('"');
        }
    }
    out.push('>');
    if VOID.contains(&tag.as_str()) {
        return;
    }
    let text = node.text.as_deref().unwrap_or("");
    let inline_children = node.children.iter().all(|c| {
        c.children.is_empty()
            && INLINE.contains(&c.name.as_str())
            && !matches!(
                tag.as_str(),
                "ul" | "ol" | "table" | "tr" | "select" | "dl" | "tbody" | "thead"
            )
    });
    if node.children.is_empty() || (inline_children && INLINE.contains(&tag.as_str())) {
        out.push_str(text);
        for child in &node.children {
            render(child, Some(&tag), indent, unit, out);
        }
    } else {
        let inner = format!("{indent}{unit}");
        if !text.is_empty() {
            out.push('\n');
            out.push_str(&inner);
            out.push_str(text);
        }
        for child in &node.children {
            out.push('\n');
            out.push_str(&inner);
            render(child, Some(&tag), &inner, unit, out);
        }
        out.push('\n');
        out.push_str(indent);
    }
    out.push_str("</");
    out.push_str(&tag);
    out.push('>');
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
