//! A small reader of TypeScript's `lib.*.d.ts` declarations (Apache-2.0), enough to take
//! the signatures of the built-in objects and the DOM: top-level `interface`s (merged
//! across files, with their `extends`), `declare var X: {…}` / `declare var X: XType` and
//! `declare function`. Namespaces (`Intl`) and anything fancier are skipped.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Default)]
struct Interface {
    extends: Vec<String>,
    /// Member name → its overloads (`(params): type` or `: type`).
    members: HashMap<String, Vec<String>>,
}

#[derive(Debug, Default)]
pub struct Decls {
    interfaces: HashMap<String, Interface>,
    /// `declare var X`: the interface describing `X` itself (its static side).
    vars: HashMap<String, String>,
    functions: HashMap<String, Vec<String>>,
}

impl Decls {
    pub fn add(&mut self, source: &str) {
        let text = strip_comments(source);
        let mut at = 0;
        while at < text.len() {
            let line_end = text[at..].find('\n').map_or(text.len(), |n| at + n);
            let line = &text[at..line_end];
            let next = if let Some(rest) = line.strip_prefix("interface ") {
                self.interface(&text, at + line.len() - rest.len(), None)
            } else if let Some(rest) = line.strip_prefix("declare var ") {
                self.var(&text, at + line.len() - rest.len())
            } else if let Some(rest) = line.strip_prefix("declare function ") {
                let start = at + line.len() - rest.len();
                let end = statement_end(&text, start);
                if let Some((name, sig)) = member(&text[start..end]) {
                    self.functions.entry(name).or_default().push(sig);
                }
                end
            } else {
                line_end
            };
            at = next.max(line_end.min(text.len())) + 1;
        }
    }

    /// `Name<…> extends A, B<…> { … }` at `start`; `named` overrides the name (inline
    /// types of `declare var`). Returns where it ends.
    fn interface(&mut self, text: &str, start: usize, named: Option<String>) -> usize {
        let Some(open) = text[start..].find('{').map(|n| start + n) else { return text.len() };
        let close = matching(text, open);
        let head = &text[start..open];
        let name = named.unwrap_or_else(|| ident(head).to_owned());
        let mut extends = Vec::new();
        if let Some(list) = head.split_once(" extends ").map(|(_, l)| l) {
            for part in split_top(list, b',') {
                let base = ident(part.trim());
                if !base.is_empty() {
                    extends.push(base.to_owned());
                }
            }
        }
        let entry = self.interfaces.entry(name).or_default();
        entry.extends.extend(extends);
        for item in split_top(&text[open + 1..close], b';') {
            if let Some((name, sig)) = member(item) {
                entry.members.entry(name).or_default().push(sig);
            }
        }
        close
    }

    /// `X: {…};` (an inline interface) or `X: XType;` at `start`.
    fn var(&mut self, text: &str, start: usize) -> usize {
        let end = statement_end(text, start);
        let Some((name, ty)) = text[start..end].split_once(':') else { return end };
        let name = name.trim().to_owned();
        let ty = ty.trim();
        if ty.starts_with('{') {
            let key = format!("{name}#static");
            self.vars.insert(name, key.clone());
            let open = start + text[start..].find('{').unwrap_or(0);
            return self.interface(text, open, Some(key));
        }
        self.vars.insert(name, ident(ty).to_owned());
        end
    }

    /// The signature of `owner.member`: on instances of interface `owner` (with what it
    /// extends), else on the object `owner` itself (static members).
    pub fn member(&self, owner: &str, member: &str) -> Option<String> {
        let mut seen = HashSet::new();
        self.find(owner, member, &mut seen)
            .or_else(|| self.find(self.vars.get(owner)?, member, &mut seen))
            .map(best)
    }

    /// The signature of a global function, or of an object's constructor (`new X(…)`).
    pub fn global(&self, name: &str) -> Option<String> {
        if let Some(overloads) = self.functions.get(name) {
            return Some(best(overloads));
        }
        let mut seen = HashSet::new();
        self.find(self.vars.get(name)?, "new", &mut seen).map(best)
    }

    fn find<'a>(
        &'a self,
        interface: &str,
        member: &str,
        seen: &mut HashSet<String>,
    ) -> Option<&'a [String]> {
        if !seen.insert(interface.to_owned()) {
            return None;
        }
        let found = self.interfaces.get(interface)?;
        if let Some(overloads) = found.members.get(member) {
            return Some(overloads);
        }
        found.extends.iter().find_map(|base| self.find(base, member, seen))
    }
}

/// The clearest of the overloads: one whose type parameters can all be replaced by their
/// default / constraint (`<E extends Element = Element>` → `Element`), then the one with
/// the most parameters (the general form), the first on a tie.
fn best(overloads: &[String]) -> String {
    let resolved: Vec<(bool, String)> = overloads.iter().map(|o| resolve_generics(o)).collect();
    let params = |s: &str| match s.starts_with('(') {
        true => split_top(&s[1..matching(s, 0).min(s.len())], b',').count(),
        false => 0,
    };
    resolved
        .iter()
        .enumerate()
        .max_by_key(|(i, (clear, sig))| (*clear, params(sig), usize::MAX - i))
        .map(|(_, (_, sig))| sig.clone())
        .unwrap_or_default()
}

/// `<A extends X = Y>(…)…` → `(…)…` with `A` replaced by `Y` (or `X`). `true` when every
/// type parameter could be replaced.
fn resolve_generics(sig: &str) -> (bool, String) {
    let Some(rest) = sig.strip_prefix('<') else { return (true, sig.to_owned()) };
    let close = matching(sig, 0);
    let mut out = sig[close + 1..].to_owned();
    let mut clear = true;
    for param in split_top(&rest[..close - 1], b',') {
        let param = param.trim();
        let name = ident(param);
        let replacement = param
            .split_once(" = ")
            .map(|(_, d)| d.trim())
            .or_else(|| param.split_once(" extends ").map(|(_, c)| c.trim()))
            .filter(|r| !r.starts_with("keyof"));
        match replacement {
            Some(r) => out = replace_word(&out, name, r),
            None => clear = false,
        }
    }
    (clear, out)
}

fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::new();
    let mut token = String::new();
    let flush = |token: &mut String, out: &mut String| {
        out.push_str(if token == word { with } else { token });
        token.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' || c == '$' {
            token.push(c);
        } else {
            flush(&mut token, &mut out);
            out.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

/// A member declaration: `name(…): T`, `name<…>(…): T`, `name?: T`, `readonly name: T`,
/// `get name(): T`, `new(…): T` (as `new`). Index / call signatures and quoted names: `None`.
fn member(item: &str) -> Option<(String, String)> {
    let item = collapse(item);
    let mut item = item.trim();
    for prefix in ["readonly ", "static "] {
        item = item.strip_prefix(prefix).unwrap_or(item);
    }
    let getter = item.starts_with("get ") && !item[4..].starts_with(['(', ':', '<']);
    if item.starts_with("set ") && !item[4..].starts_with(['(', ':', '<']) {
        return None;
    }
    if getter {
        item = &item[4..];
    }
    let name = ident(item);
    if name.is_empty() {
        return None;
    }
    let rest = item[name.len()..].trim_start();
    let rest = rest.strip_prefix('?').unwrap_or(rest);
    if getter {
        // `get x(): T` reads like a property.
        let ty = rest.split_once("):").map(|(_, t)| t.trim())?;
        return Some((name.to_owned(), format!(": {ty}")));
    }
    if rest.starts_with('(') || rest.starts_with('<') || rest.starts_with(':') {
        return Some((name.to_owned(), rest.to_owned()));
    }
    None
}

fn ident(text: &str) -> &str {
    let end =
        text.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$')).unwrap_or(text.len());
    &text[..end]
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").replace("( ", "(").replace(" )", ")")
}

/// Where the statement at `start` ends (its `;` outside brackets).
fn statement_end(text: &str, start: usize) -> usize {
    let mut depth = 0i32;
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        match b {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth -= 1,
            b';' if depth <= 0 => return i,
            _ => {}
        }
    }
    text.len()
}

/// The bracket closing the one at `open` (`{`, `(`, `[` or `<`; `=>` isn't a bracket).
fn matching(text: &str, open: usize) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0i32;
    for i in open..bytes.len() {
        match bytes[i] {
            b'{' | b'(' | b'[' | b'<' => depth += 1,
            b'>' if i > 0 && bytes[i - 1] == b'=' => {}
            b'}' | b')' | b']' | b'>' => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// `text` split at `sep` outside brackets (`=>` isn't a bracket), empty parts dropped.
fn split_top(text: &str, sep: u8) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for i in 0..bytes.len() {
        match bytes[i] {
            b'{' | b'(' | b'[' | b'<' => depth += 1,
            b'>' if i > 0 && bytes[i - 1] == b'=' => {}
            b'}' | b')' | b']' | b'>' => depth -= 1,
            b if b == sep && depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts.into_iter().filter(|p| !p.trim().is_empty())
}

/// The text without `/* … */` and `// …` comments (strings, e.g. `"http://…"` literal
/// types, are kept whole).
fn strip_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'"' | b'\'' | b'`') => {
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                out.push_str(&source[copied..i]);
                i = source[i + 2..].find("*/").map_or(bytes.len(), |e| i + 2 + e + 2);
                copied = i;
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                out.push_str(&source[copied..i]);
                i = source[i..].find('\n').map_or(bytes.len(), |e| i + e);
                copied = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&source[copied.min(bytes.len())..]);
    out
}

#[cfg(test)]
#[path = "../tests/unit/dts.rs"]
mod tests;
