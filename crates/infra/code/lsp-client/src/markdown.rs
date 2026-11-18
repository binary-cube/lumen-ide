//! A hover's Markdown as the hover card shows it: the first code block is its signature
//! (the card colors it), the rest its documentation as plain lines (the markup's marks
//! taken off: the card draws plain text). HTML in it (PHP's manual writes tables and
//! paragraphs so) becomes text too, by the shared `markup::text`.

/// The signature (the first fenced code block, or the whole text if it's only code) and
/// the documentation (what's left, `None` if nothing).
pub fn split(markdown: &str) -> (String, Option<String>) {
    let mut code: Option<String> = None;
    let mut doc: Vec<String> = Vec::new();
    let markdown = html_outside_code(markdown);
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(fence) = ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
            let mut block = Vec::new();
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with(fence) {
                    break;
                }
                block.push(inner);
            }
            let block = block.join("\n");
            if code.is_none() {
                code = Some(block);
            } else {
                doc.push(block);
            }
            continue;
        }
        doc.push(plain(line));
    }
    // A horizontal rule between the parts says nothing on the card.
    doc.retain(|l| !matches!(l.trim(), "---" | "***" | "___"));
    while doc.first().is_some_and(|l| l.trim().is_empty()) {
        doc.remove(0);
    }
    while doc.last().is_some_and(|l| l.trim().is_empty()) {
        doc.pop();
    }
    let doc = (!doc.is_empty()).then(|| doc.join("\n"));
    match code {
        Some(code) => (code, doc),
        None => (String::new(), doc),
    }
}

/// `markdown` with the HTML outside its fenced code blocks made text (`markup::text`); the
/// blocks as they are.
fn html_outside_code(markdown: &str) -> String {
    if !markdown.contains('<') && !markdown.contains('&') {
        return markdown.to_owned();
    }
    let mut out = String::with_capacity(markdown.len());
    let mut prose = String::new();
    let mut fence: Option<&str> = None;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        match fence {
            Some(f) => {
                out.push_str(line);
                if trimmed.starts_with(f) {
                    fence = None;
                }
            }
            None => match ["```", "~~~"].into_iter().find(|f| trimmed.starts_with(f)) {
                Some(f) => {
                    out.push_str(&markup::text::to_text(&std::mem::take(&mut prose)));
                    out.push_str(line);
                    fence = Some(f);
                }
                None => prose.push_str(line),
            },
        }
    }
    out.push_str(&markup::text::to_text(&prose));
    out
}

/// A Markdown line's text: emphasis, code spans and link targets taken off, a heading's
/// and a quote's marks too.
pub fn plain(line: &str) -> String {
    let line = line.trim_end();
    let body = line.trim_start_matches('#').trim_start_matches('>');
    let body = if body.len() != line.len() { body.trim_start() } else { line };
    let mut out = String::with_capacity(body.len());
    let chars: Vec<char> = body.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' if i + 1 < chars.len() && chars[i + 1].is_ascii_punctuation() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '*' | '`' => i += 1,
            '_' if i == 0
                || i + 1 == chars.len()
                || !chars[i - 1].is_alphanumeric()
                || !chars[i + 1].is_alphanumeric() =>
            {
                i += 1
            }
            '[' => {
                // `[text](target)`: the text.
                let close = chars[i..].iter().position(|&c| c == ']').map(|p| p + i);
                match close {
                    Some(close) if chars.get(close + 1) == Some(&'(') => {
                        out.extend(&chars[i + 1..close]);
                        let end = chars[close..]
                            .iter()
                            .position(|&c| c == ')')
                            .map_or(chars.len(), |p| p + close + 1);
                        i = end;
                    }
                    _ => {
                        out.push('[');
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/markdown.rs"]
mod tests;
