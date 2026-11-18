//! Markup (HTML and what embeds it): the elements still open, found in the text — as a
//! bracket is matched — where a syntax tree, broken mid-tag while typing, can't tell; and
//! HTML as plain text ([`text`]) for what draws text only.

pub mod text;

/// HTML's elements without a closing tag.
pub const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// An element written without a closing tag: a void one, or the doctype.
fn is_void(name: &str) -> bool {
    name == "!doctype" || VOID.contains(&name)
}

/// Where the last element still open in `html` starts (its `<`): tags matched with a stack,
/// void and self-closing ones (`<br>`, `<x />`), comments and `<?…?>` skipped.
pub fn unclosed_tag(html: &str) -> Option<usize> {
    let mut stack: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    while let Some(at) = html[i..].find('<').map(|a| a + i) {
        let rest = &html[at..];
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[1..end];
        i = at + end + 1;
        if tag.starts_with("!--") || tag.starts_with('?') {
            continue;
        }
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '-' | ':' | '!'))
            .collect::<String>()
            .to_ascii_lowercase();
        if name.is_empty() || is_void(&name) || tag.ends_with('/') {
            continue;
        }
        if closing {
            if let Some(open) = stack.iter().rposition(|(n, _)| *n == name) {
                stack.truncate(open);
            }
        } else {
            stack.push((name, at));
        }
    }
    stack.last().map(|(_, at)| *at)
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
