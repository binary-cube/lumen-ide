//! JavaScript's parameters for "Change Signature": read and written back as written (rest,
//! type annotations, defaults, destructuring).

use refactor::signature::{CallRules, SignatureSyntax};

/// JS / TS parameters: `...name: Type = default` (no named arguments).
#[derive(Debug, Default)]
pub struct JsParams;

impl SignatureSyntax for JsParams {
    fn calls(&self) -> CallRules {
        CallRules {
            member_access: &["."],
            declares: &["function"],
            constructs: Some("new"),
            sigil: None,
        }
    }

    /// Scripts, and the pages / components with them.
    fn extensions(&self) -> &'static [&'static str] {
        &["js", "mjs", "cjs", "jsx", "ts", "tsx", "vue", "svelte", "html", "htm"]
    }

    fn parse(&self, text: &str) -> refactor::signature::Param {
        let (head, default) = refactor::signature::split_default(text);
        let variadic = head.starts_with("...");
        let head = head.trim_start_matches("...");
        // `name: Type` (TypeScript); a destructured `{ a, b }` stays whole.
        let (name, type_hint) = match head.split_once(':') {
            Some((n, t)) if !n.contains(['{', '[']) => (n.trim(), t.trim()),
            _ => (head.trim(), ""),
        };
        refactor::signature::Param {
            prefix: String::new(),
            type_hint: type_hint.to_owned(),
            name: name.to_owned(),
            default: default.to_owned(),
            variadic,
            by_ref: false,
        }
    }

    fn render(&self, p: &refactor::signature::Param) -> String {
        let mut out = String::new();
        if p.variadic {
            out.push_str("...");
        }
        out.push_str(&p.name);
        if !p.type_hint.is_empty() {
            out.push_str(": ");
            out.push_str(&p.type_hint);
        }
        if !p.default.is_empty() {
            out.push_str(" = ");
            out.push_str(&p.default);
        }
        out
    }
}

#[cfg(test)]
#[path = "../tests/unit/signature.rs"]
mod tests;
