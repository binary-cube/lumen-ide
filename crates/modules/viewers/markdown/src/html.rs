//! The little HTML a Markdown document carries (`<details>`, `<kbd>`, `<img width>`,
//! `<p align="center">`…), read as tokens: tags with their attributes, text with its
//! entities decoded, comments dropped. Nothing is executed or fetched; what isn't
//! understood is shown as its text, or left out when it's only markup.

/// A piece of HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Text(String),
    /// An opening tag (its name lowercase) and its attributes (names lowercase).
    Open {
        name: String,
        attrs: Vec<(String, String)>,
        closed: bool,
    },
    Close(String),
}

impl Token {
    /// The value of attribute `name` of an opening tag.
    pub fn attr(&self, name: &str) -> Option<&str> {
        match self {
            Self::Open { attrs, .. } => {
                attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
            }
            _ => None,
        }
    }
}

/// `html`'s tokens, in order.
pub fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut rest = html;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            continue;
        }
        if rest.starts_with('<')
            && let Some((token, len)) = tag(rest)
        {
            out.push(token);
            rest = &rest[len..];
            continue;
        }
        // Text up to the next tag (a lone `<` is text).
        let next =
            rest.char_indices().skip(1).find(|&(_, c)| c == '<').map_or(rest.len(), |(i, _)| i);
        out.push(Token::Text(decode(&rest[..next])));
        rest = &rest[next..];
    }
    out
}

/// The tag at the start of `s` and its length in bytes.
fn tag(s: &str) -> Option<(Token, usize)> {
    let end = s.find('>')?;
    let inner = &s[1..end];
    if let Some(name) = inner.strip_prefix('/') {
        let name = name.trim().to_ascii_lowercase();
        return valid(&name).then(|| (Token::Close(name), end + 1));
    }
    let (inner, closed) = match inner.strip_suffix('/') {
        Some(inner) => (inner, true),
        None => (inner, false),
    };
    let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
    let name = inner[..name_end].to_ascii_lowercase();
    if !valid(&name) {
        return None;
    }
    let attrs = attributes(&inner[name_end..]);
    Some((Token::Open { name, attrs, closed }, end + 1))
}

fn valid(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// `a="1" b='2' c=3 d`, names lowercase, values decoded.
fn attributes(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        let name_end = rest.find(|c: char| c == '=' || c.is_whitespace()).unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = String::new();
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (v, len) = match after.chars().next() {
                Some(q @ ('"' | '\'')) => {
                    let close = after[1..].find(q).map_or(after.len(), |i| i + 1);
                    (&after[1..close], (close + 1).min(after.len()))
                }
                _ => {
                    let end = after.find(char::is_whitespace).unwrap_or(after.len());
                    (&after[..end], end)
                }
            };
            value = decode(v);
            rest = after[len..].trim_start();
        }
        let stuck = name.is_empty() && value.is_empty();
        if !name.is_empty() {
            out.push((name, value));
        }
        if stuck {
            // Nothing understood here: skip a character.
            rest = rest.get(1..).unwrap_or("");
        }
    }
    out
}

/// `&amp;`, `&lt;`, `&#123;`, `&#x1F600;`… decoded (unknown ones kept as written).
pub fn decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| entity(&rest[1..e]));
        match (end, decoded) {
            (Some(e), Some(c)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let code = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(code);
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "reg" => '®',
        "trade" => '™',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "laquo" => '«',
        "raquo" => '»',
        "larr" => '←',
        "rarr" => '→',
        "uarr" => '↑',
        "darr" => '↓',
        "times" => '×',
        "check" => '✓',
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../tests/unit/html.rs"]
mod tests;
