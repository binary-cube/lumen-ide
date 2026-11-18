//! What PHP adds as you type: `<?` written outside the code (in the HTML around it) becomes
//! `<?php` and a new line, the cursor on it, indented as the tag's line. Not
//! inside the code (a `<?` in a string or a comment stays), not where `php` / `=` / `xml`
//! already follow; `=` typed right after takes it back (`<?=`, the short `echo`), and one
//! Undo leaves `<?` as typed.

use code_intelligence::TypedInsert;

/// The opening tag's completion for `typed` at `offset` (the `?` already in `text`).
pub(crate) fn open_tag(text: &str, offset: usize, typed: char) -> Option<TypedInsert> {
    if typed != '?' || offset < 2 || !text.is_char_boundary(offset) {
        return None;
    }
    let tag = offset - 2;
    if !text[..offset].ends_with("<?") || in_code(&text[..tag]) {
        return None;
    }
    let rest = &text[offset..];
    let line_rest = rest.split('\n').next().unwrap_or_default();
    if ["php", "=", "xml"].iter().any(|after| line_rest.to_ascii_lowercase().starts_with(after)) {
        return None;
    }
    let line_start = text[..tag].rfind('\n').map_or(0, |at| at + 1);
    let indent: String =
        text[line_start..tag].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    // Something after it on the line (`<??>`): a space, the code goes there.
    let insert = if line_rest.trim().is_empty() { format!("php\n{indent}") } else { "php ".into() };
    Some(TypedInsert::before_cursor(insert, &['=']))
}

/// Whether the end of `before` is inside PHP code: a tag opened (`<?php`, `<?=`, `<?`) and
/// not closed by `?>` since — a `?>` inside a string doesn't close it.
fn in_code(before: &str) -> bool {
    let bytes = before.as_bytes();
    let (mut code, mut quote, mut i) = (false, None::<u8>, 0);
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        match (code, quote) {
            (false, _) if b == b'<' && next == Some(b'?') => {
                code = true;
                i += 1;
            }
            (true, Some(q)) => {
                if b == b'\\' {
                    i += 1;
                } else if b == q {
                    quote = None;
                }
            }
            (true, None) if b == b'\'' || b == b'"' => quote = Some(b),
            (true, None) if b == b'?' && next == Some(b'>') => {
                code = false;
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    code
}

#[cfg(test)]
#[path = "../tests/unit/typing.rs"]
mod tests;
