//! What a signature's parameters name: the types a class diagram links a constructor to.

/// The class types in a signature's parameters (`(private readonly Cache $cache, ?Clock
/// $clock = null, A|B $x)`), as written; scalars and pseudo-types are left out.
pub(crate) fn parameter_types(detail: &str) -> Vec<String> {
    // The parameter list: inside the first `(` and the `)` closing it.
    let Some(open) = detail.find('(') else { return Vec::new() };
    let mut depth = 0i32;
    let mut end = detail.len();
    for (i, c) in detail[open..].char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    end = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    let params = &detail[open + 1..end];
    let mut out: Vec<String> = Vec::new();
    for param in split_top_level(params) {
        // Drop attributes (`#[…]`), then take what's before the variable.
        let mut param = param.trim();
        while let Some(rest) = param.strip_prefix("#[") {
            param = skip_bracketed(rest).trim_start();
        }
        let before = param.split('$').next().unwrap_or("");
        for word in before.split_whitespace() {
            let word = word.trim_end_matches('&').trim_end_matches("...");
            for ty in word.split(['|', '&', '(', ')']) {
                let ty = ty.trim_start_matches('?');
                if ty.is_empty() || is_pseudo_type(ty) {
                    continue;
                }
                if !out.iter().any(|o| o.eq_ignore_ascii_case(ty)) {
                    out.push(ty.to_owned());
                }
            }
        }
    }
    out
}

/// Splits on commas outside brackets.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// After `#[`: the text past its closing `]`.
fn skip_bracketed(s: &str) -> &str {
    let mut depth = 1i32;
    for (i, c) in s.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return &s[i + 1..];
                }
            }
            _ => {}
        }
    }
    ""
}

/// Modifiers, scalars and pseudo-types: not classes.
fn is_pseudo_type(word: &str) -> bool {
    const WORDS: &[&str] = &[
        "public",
        "protected",
        "private",
        "readonly",
        "int",
        "float",
        "string",
        "bool",
        "array",
        "iterable",
        "callable",
        "object",
        "mixed",
        "null",
        "false",
        "true",
        "void",
        "never",
        "self",
        "static",
        "parent",
        "integer",
        "boolean",
        "double",
    ];
    WORDS.iter().any(|w| w.eq_ignore_ascii_case(word))
}

#[cfg(test)]
#[path = "../tests/unit/parameters.rs"]
mod tests;
