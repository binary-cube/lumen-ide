//! LSP snippets (`foo(${1:a}, ${2:b})$0`) as the editor inserts them: the text with each
//! placeholder's own text (choices: the first; variables: their default), and where the
//! cursor goes — the first tab stop (`$1`), else the final one (`$0`), else the end.

/// The text `snippet` inserts, and the cursor's place in it (bytes).
pub fn expand(snippet: &str) -> (String, usize) {
    let mut out = String::with_capacity(snippet.len());
    let mut stops: Vec<(u32, usize)> = Vec::new();
    let chars: Vec<char> = snippet.chars().collect();
    let mut i = 0;
    parse(&chars, &mut i, &mut out, &mut stops, false);
    let first = stops.iter().filter(|(n, _)| *n > 0).min_by_key(|(n, _)| *n);
    let cursor =
        first.or_else(|| stops.iter().find(|(n, _)| *n == 0)).map_or(out.len(), |(_, at)| *at);
    (out, cursor)
}

/// Reads until the end (or, `nested`, a `}` closing the placeholder being read).
fn parse(
    chars: &[char],
    i: &mut usize,
    out: &mut String,
    stops: &mut Vec<(u32, usize)>,
    nested: bool,
) {
    while *i < chars.len() {
        let c = chars[*i];
        match c {
            '\\' if *i + 1 < chars.len()
                && matches!(chars[*i + 1], '$' | '}' | '\\' | ',' | '|') =>
            {
                out.push(chars[*i + 1]);
                *i += 2;
            }
            '}' if nested => {
                *i += 1;
                return;
            }
            '$' => {
                *i += 1;
                dollar(chars, i, out, stops);
            }
            _ => {
                out.push(c);
                *i += 1;
            }
        }
    }
}

/// What follows a `$`: a tab stop, a placeholder, a choice or a variable.
fn dollar(chars: &[char], i: &mut usize, out: &mut String, stops: &mut Vec<(u32, usize)>) {
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < chars.len() && chars[*i].is_ascii_digit() {
            *i += 1;
        }
        (start < *i)
            .then(|| chars[start..*i].iter().collect::<String>().parse::<u32>().unwrap_or(0))
    };
    if let Some(n) = digits(i) {
        stops.push((n, out.len()));
        return;
    }
    if chars.get(*i) != Some(&'{') {
        // A variable (`$TM_FILENAME`): nothing known of it, nothing inserted.
        let start = *i;
        while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
            *i += 1;
        }
        if start == *i {
            out.push('$');
        }
        return;
    }
    *i += 1;
    if let Some(n) = digits(i) {
        stops.push((n, out.len()));
        match chars.get(*i) {
            Some(':') => {
                *i += 1;
                parse(chars, i, out, stops, true);
            }
            Some('|') => {
                *i += 1;
                choice(chars, i, out);
            }
            Some('}') => *i += 1,
            _ => parse(chars, i, out, stops, true),
        }
        return;
    }
    // `${VARIABLE}` or `${VARIABLE:default}`.
    while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
        *i += 1;
    }
    match chars.get(*i) {
        Some(':') => {
            *i += 1;
            parse(chars, i, out, stops, true);
        }
        _ => {
            // Skipped to its end (a transform `/…/…/` included).
            while *i < chars.len() && chars[*i] != '}' {
                *i += 1;
            }
            *i += 1;
        }
    }
}

/// `${1|one,two|}`: the first choice inserted.
fn choice(chars: &[char], i: &mut usize, out: &mut String) {
    let mut first = true;
    while *i < chars.len() {
        match chars[*i] {
            '\\' if *i + 1 < chars.len() => {
                if first {
                    out.push(chars[*i + 1]);
                }
                *i += 2;
            }
            ',' => {
                first = false;
                *i += 1;
            }
            '|' => {
                *i += 1;
                if chars.get(*i) == Some(&'}') {
                    *i += 1;
                }
                return;
            }
            c => {
                if first {
                    out.push(c);
                }
                *i += 1;
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/snippet.rs"]
mod tests;
