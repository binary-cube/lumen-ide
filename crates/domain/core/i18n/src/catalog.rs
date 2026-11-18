//! The catalog reader: the TOML subset used (`#` comments, `[a.b]` sections,
//! `key = "text"` with the escapes `\n`, `\t`, `\"`, `\\`, `\u{…}`), with no dependencies.

/// The (full key, message) pairs in the catalog, in file order.
pub fn parse_catalog(source: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut section = String::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().trim_matches('"');
        let Some(value) = unquote(value.trim()) else { continue };
        let full = if section.is_empty() { key.to_owned() } else { format!("{section}.{key}") };
        out.push((full, value));
    }
    out
}

/// [`parse_catalog`] of a catalog built into the binary: each text borrowed from it unless
/// it has escapes (most don't).
pub(crate) fn parse_catalog_static(
    source: &'static str,
) -> Vec<(String, std::borrow::Cow<'static, str>)> {
    use std::borrow::Cow;
    let mut out = Vec::new();
    let mut section = String::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_owned();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim().trim_matches('"');
        let value = value.trim();
        // Up to its closing quote: borrowed when nothing in it is escaped.
        let text = match value.strip_prefix('"').map(|inner| (inner, inner.find(['"', '\\']))) {
            Some((inner, Some(end))) if inner.as_bytes()[end] == b'"' => {
                Cow::Borrowed(&inner[..end])
            }
            _ => match unquote(value) {
                Some(text) => Cow::Owned(text),
                None => continue,
            },
        };
        let full = if section.is_empty() { key.to_owned() } else { format!("{section}.{key}") };
        out.push((full, text));
    }
    out
}

fn unquote(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
