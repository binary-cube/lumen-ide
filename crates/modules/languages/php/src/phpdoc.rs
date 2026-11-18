//! What a PHPDoc comment says of types: `@var`, `@param`, `@return`, `@template`,
//! `@extends` / `@implements` / `@use`, `@mixin`, `@property*`, `@method`, `@deprecated` —
//! with the static analysers' variants (`@psalm-…`, `@phpstan-…`, which win over the plain
//! tag) — and the compact form the index keeps of it ([`Facts`], a symbol's `types`).

use crate::types::split_doc_type;

/// A `@property` / `@method` tag: a member the class has by magic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Magic {
    /// Where its name starts in the comment's text (a property's `$`, as in code).
    pub offset: usize,
    pub name: String,
    /// The property's type, or the method's `(params): type`.
    pub detail: String,
    pub is_method: bool,
    pub is_static: bool,
    /// `@property`, `@property-read`, `@method`…
    pub tag: &'static str,
}

/// The tags of one comment.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags {
    pub var: Option<(Option<String>, String)>,
    pub returns: Option<String>,
    /// `$name` (without `$`) → its type.
    pub params: Vec<(String, String)>,
    /// A template's name and its bound (`of Model`).
    pub templates: Vec<(String, Option<String>)>,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub uses: Vec<String>,
    pub mixins: Vec<String>,
    pub magic: Vec<Magic>,
    pub deprecated: bool,
}

/// How sure a tag is of its type: a static analyser's own variant wins.
fn rank(prefix: &str) -> u8 {
    match prefix {
        "phpstan-" => 2,
        "psalm-" => 1,
        _ => 0,
    }
}

/// The tags of a doc comment (`/** … */`, the whole text).
pub fn tags(comment: &str) -> Tags {
    let mut tags = Tags::default();
    let mut ranks: [u8; 3] = [0; 3];
    let mut have = [false; 3];
    let mut param_ranks: Vec<u8> = Vec::new();
    let base = comment.as_ptr() as usize;
    for line in comment.lines() {
        let trimmed = line.trim_start().trim_start_matches("/**").trim_start_matches('*');
        let trimmed = trimmed.trim_start();
        let Some(tag) = trimmed.strip_prefix('@') else { continue };
        let end = tag.find(|c: char| c.is_whitespace()).unwrap_or(tag.len());
        let (name, rest) = (&tag[..end], tag[end..].trim_end_matches("*/"));
        let (prefix, bare) = ["phpstan-", "psalm-"]
            .iter()
            .find_map(|p| name.strip_prefix(p).map(|b| (*p, b)))
            .unwrap_or(("", name));
        let r = rank(prefix);
        let (ty, after) = split_doc_type(rest);
        let mut better = |slot: usize| {
            let wins = !have[slot] || r >= ranks[slot];
            if wins {
                have[slot] = true;
                ranks[slot] = r;
            }
            wins
        };
        match bare {
            "var" if !ty.is_empty() => {
                if better(0) {
                    let name = variable(after);
                    tags.var = Some((name, ty.to_owned()));
                }
            }
            "return" if !ty.is_empty() => {
                if better(1) {
                    tags.returns = Some(ty.to_owned());
                }
            }
            "param" if !ty.is_empty() && !ty.starts_with('$') => {
                if let Some(name) = variable(after) {
                    match tags.params.iter().position(|(n, _)| *n == name) {
                        Some(i) if r > param_ranks[i] => {
                            tags.params[i].1 = ty.to_owned();
                            param_ranks[i] = r;
                        }
                        Some(_) => {}
                        None => {
                            tags.params.push((name, ty.to_owned()));
                            param_ranks.push(r);
                        }
                    }
                }
            }
            "template" | "template-covariant" | "template-contravariant" => {
                let mut words = rest.split_whitespace();
                if let Some(name) = words.next() {
                    let bound = match words.next() {
                        Some("of" | "as") => {
                            let (bound, _) = split_doc_type(
                                rest.trim_start()[name.len()..].trim_start()[2..].trim_start(),
                            );
                            Some(bound.to_owned())
                        }
                        _ => None,
                    };
                    if !tags.templates.iter().any(|(n, _)| n == name) {
                        tags.templates.push((name.to_owned(), bound));
                    }
                }
            }
            "extends" | "template-extends" if !ty.is_empty() => push_once(&mut tags.extends, ty),
            "implements" | "template-implements" if !ty.is_empty() => {
                push_once(&mut tags.implements, ty);
            }
            "use" | "template-use" if !ty.is_empty() => push_once(&mut tags.uses, ty),
            "mixin" if !ty.is_empty() => push_once(&mut tags.mixins, ty),
            "property" | "property-read" | "property-write" if prefix.is_empty() => {
                if let Some(at) = after.find('$') {
                    let name: String = after[at + 1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        let offset = after[at..].as_ptr() as usize - base;
                        let tag = match bare {
                            "property-read" => "@property-read",
                            "property-write" => "@property-write",
                            _ => "@property",
                        };
                        tags.magic.push(Magic {
                            offset,
                            name,
                            detail: ty.to_owned(),
                            is_method: false,
                            is_static: false,
                            tag,
                        });
                    }
                } else if let Some(bare) = ty.strip_prefix('$') {
                    // `@property $name` (no type).
                    let name: String =
                        bare.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                    if !name.is_empty() {
                        let offset = ty.as_ptr() as usize - base;
                        tags.magic.push(Magic {
                            offset,
                            name,
                            detail: String::new(),
                            is_method: false,
                            is_static: false,
                            tag: "@property",
                        });
                    }
                }
            }
            "method" if prefix.is_empty() => {
                if let Some(magic) = method(rest, base) {
                    tags.magic.push(magic);
                }
            }
            "deprecated" if prefix.is_empty() => tags.deprecated = true,
            _ => {}
        }
    }
    tags
}

fn push_once(list: &mut Vec<String>, ty: &str) {
    if !list.iter().any(|t| t == ty) {
        list.push(ty.to_owned());
    }
}

/// The `$name` a tag's text starts with (after its type), without `$` / `...` / `&`.
fn variable(after: &str) -> Option<String> {
    let word = after.split_whitespace().next()?;
    let word = word.trim_start_matches('&').trim_start_matches("...");
    let name = word.strip_prefix('$')?;
    let name: String = name.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    (!name.is_empty()).then_some(name)
}

/// `@method [static] [Type] name(params) [description]`.
fn method(rest: &str, base: usize) -> Option<Magic> {
    let open = rest.find('(')?;
    let head = rest[..open].trim_end();
    let name_start = head.rfind(|c: char| !(c.is_alphanumeric() || c == '_')).map_or(0, |i| i + 1);
    let name = &head[name_start..];
    if name.is_empty() {
        return None;
    }
    let before = head[..name_start].trim();
    let (is_static, ret) = match before.strip_prefix("static") {
        // `static Type name()`; `static name()` alone is a static method (phpDocumentor).
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
            (true, rest.trim())
        }
        _ => (false, before),
    };
    // The parameters: up to the matching `)`.
    let mut depth = 0usize;
    let mut close = None;
    for (i, c) in rest[open..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let params = crate::intelligence::compact_params(&rest[open..=close]);
    let detail = if ret.is_empty() { params } else { format!("{params}: {ret}") };
    let offset = head[name_start..].as_ptr() as usize - base;
    Some(Magic {
        offset,
        name: name.to_owned(),
        detail,
        is_method: true,
        is_static,
        tag: "@method",
    })
}

/// The compact form a symbol's `types` holds: lines `<key> <text>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    pub returns: Option<String>,
    pub var: Option<String>,
    pub params: Vec<(String, String)>,
    pub templates: Vec<(String, Option<String>)>,
    pub extends: Vec<String>,
    pub mixins: Vec<String>,
    pub deprecated: bool,
    /// An enum's backing type (`string`, `int`).
    pub backed: Option<String>,
}

impl Facts {
    pub fn is_empty(&self) -> bool {
        *self == Facts::default()
    }

    /// As stored in the index (`None`: nothing to say).
    pub fn encode(&self) -> Option<Box<str>> {
        if self.is_empty() {
            return None;
        }
        let mut out = String::new();
        let mut line = |key: &str, text: &str| {
            out.push_str(key);
            out.push(' ');
            out.push_str(&text.replace('\n', " "));
            out.push('\n');
        };
        if let Some(r) = &self.returns {
            line("r", r);
        }
        if let Some(v) = &self.var {
            line("v", v);
        }
        for (name, ty) in &self.params {
            line("p", &format!("{name} {ty}"));
        }
        for (name, bound) in &self.templates {
            match bound {
                Some(b) => line("t", &format!("{name} {b}")),
                None => line("t", name),
            }
        }
        for e in &self.extends {
            line("x", e);
        }
        for m in &self.mixins {
            line("m", m);
        }
        if let Some(b) = &self.backed {
            line("b", b);
        }
        if self.deprecated {
            out.push_str("d\n");
        }
        Some(out.into_boxed_str())
    }

    /// From a symbol's `types`.
    pub fn decode(text: Option<&str>) -> Facts {
        let mut facts = Facts::default();
        for line in text.unwrap_or("").lines() {
            let (key, rest) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "r" => facts.returns = Some(rest.to_owned()),
                "v" => facts.var = Some(rest.to_owned()),
                "p" => {
                    if let Some((name, ty)) = rest.split_once(' ') {
                        facts.params.push((name.to_owned(), ty.to_owned()));
                    }
                }
                "t" => {
                    let (name, bound) = match rest.split_once(' ') {
                        Some((n, b)) => (n, Some(b.to_owned())),
                        None => (rest, None),
                    };
                    facts.templates.push((name.to_owned(), bound));
                }
                "x" => facts.extends.push(rest.to_owned()),
                "m" => facts.mixins.push(rest.to_owned()),
                "b" => facts.backed = Some(rest.to_owned()),
                "d" => facts.deprecated = true,
                _ => {}
            }
        }
        facts
    }

    /// Whether `types` says the declaration is deprecated (without decoding the rest).
    pub fn says_deprecated(types: Option<&str>) -> bool {
        types.is_some_and(|t| t.lines().any(|l| l == "d"))
    }
}

#[cfg(test)]
#[path = "../tests/unit/phpdoc.rs"]
mod tests;
