//! PHP's parameters for "Change Signature": read and written back as written (attributes,
//! modifiers, type, by-reference, variadic, default), named arguments (PHP 8).

use refactor::signature::{CallRules, SignatureSyntax};

/// PHP parameters: `#[Attr] private readonly ?Type &...$name = default`.
#[derive(Debug, Default)]
pub struct PhpParams;

impl SignatureSyntax for PhpParams {
    fn calls(&self) -> CallRules {
        CallRules {
            member_access: &["->", "::"],
            declares: &["function"],
            constructs: Some("new"),
            sigil: Some('$'),
        }
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["php", "phtml", "inc"]
    }

    fn parse(&self, text: &str) -> refactor::signature::Param {
        let (head, default) = refactor::signature::split_default(text);
        // The name is the last `$word`; what's before it: attributes, modifiers, the type.
        let dollar = head.rfind('$').unwrap_or(head.len());
        let name = head[dollar..].trim_start_matches('$').to_owned();
        let mut before = head[..dollar].trim_end();
        let variadic = before.ends_with("...");
        before = before.trim_end_matches("...").trim_end();
        let by_ref = before.ends_with('&');
        before = before.trim_end_matches('&').trim_end();
        // Modifiers and attributes go in the prefix; the rest is the type.
        let mut prefix = Vec::new();
        let mut rest = before;
        loop {
            rest = rest.trim_start();
            if rest.starts_with("#[") {
                // An attribute: up to its closing bracket.
                let mut depth = 0;
                let end = rest
                    .char_indices()
                    .skip(1)
                    .find(|&(_, c)| {
                        match c {
                            '[' => depth += 1,
                            ']' => depth -= 1,
                            _ => {}
                        }
                        depth == 0
                    })
                    .map_or(rest.len(), |(i, _)| i + 1);
                prefix.push(rest[..end].to_owned());
                rest = &rest[end..];
                continue;
            }
            let word = rest.split_whitespace().next().unwrap_or("");
            if ["public", "private", "protected", "readonly"].contains(&word) {
                prefix.push(word.to_owned());
                rest = &rest[word.len()..];
                continue;
            }
            break;
        }
        refactor::signature::Param {
            prefix: prefix.join(" "),
            type_hint: rest.trim().to_owned(),
            name,
            default: default.to_owned(),
            variadic,
            by_ref,
        }
    }

    fn render(&self, p: &refactor::signature::Param) -> String {
        let mut out = String::new();
        for part in [&p.prefix, &p.type_hint] {
            if !part.is_empty() {
                out.push_str(part);
                out.push(' ');
            }
        }
        if p.by_ref {
            out.push('&');
        }
        if p.variadic {
            out.push_str("...");
        }
        out.push('$');
        out.push_str(&p.name);
        if !p.default.is_empty() {
            out.push_str(" = ");
            out.push_str(&p.default);
        }
        out
    }

    fn named_argument<'a>(&self, arg: &'a str) -> Option<(&'a str, &'a str)> {
        let colon = arg.find(':')?;
        let name = arg[..colon].trim();
        let is_name = !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit());
        (is_name && !arg[colon + 1..].starts_with(':')).then(|| (name, arg[colon + 1..].trim()))
    }

    fn render_named(&self, name: &str, value: &str) -> Option<String> {
        Some(format!("{name}: {value}"))
    }
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
