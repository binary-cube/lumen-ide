//! A release's notes (Markdown, as written on its page) as the lines a window shows: titles,
//! points, paragraphs and the space between them. Only what notes use is read — `#` titles,
//! `-` / `*` / `1.` points, `**bold**`, `` `code` ``, `[links](…)` — the marks taken off
//! the text; anything else is shown as it's written.

/// Where a release's notes for the app begin and end (HTML comments: the release's page
/// shows nothing of them).
pub const APP_START: &str = "<!-- app -->";
pub const APP_END: &str = "<!-- /app -->";

/// The part of a release's notes the app shows: what's between [`APP_START`] and
/// [`APP_END`] when the notes have it (written for the app); else all of them but their
/// download section (the files to pick: the app picks its own), which runs from a
/// `Download` title to the next title of the same level or higher.
pub fn for_app(notes: &str) -> String {
    if let Some(start) = notes.find(APP_START) {
        let after = &notes[start + APP_START.len()..];
        let end = after.find(APP_END).unwrap_or(after.len());
        return after[..end].trim().to_owned();
    }
    let mut out = Vec::new();
    let mut skipping: Option<usize> = None;
    for line in notes.lines() {
        let trimmed = line.trim();
        let level = trimmed.len() - trimmed.trim_start_matches('#').len();
        let is_title = (1..=6).contains(&level) && trimmed[level..].starts_with(' ');
        if is_title {
            if skipping.is_some_and(|at| level <= at) {
                skipping = None;
            }
            let name = trimmed[level..].trim().to_ascii_lowercase();
            if name.starts_with("download") {
                skipping = Some(level);
                continue;
            }
        }
        if skipping.is_none() {
            out.push(line);
        }
    }
    out.join("\n").trim().to_owned()
}

/// What a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A title (`#`, `##`…).
    Title,
    /// A point of a list, `depth` deep (0: the first level).
    Point { depth: u8 },
    /// A paragraph's text.
    Text,
    /// The space between two blocks.
    Gap,
}

/// A line of the notes, its marks taken off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

/// `notes` (Markdown) as lines: consecutive text lines joined into one paragraph, one gap
/// between blocks (never two, none first or last).
pub fn lines(notes: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    let mut gap = false;
    let push = |out: &mut Vec<Line>, gap: &mut bool, line: Line| {
        if *gap && !out.is_empty() {
            out.push(Line { kind: Kind::Gap, text: String::new() });
        }
        *gap = false;
        out.push(line);
    };
    for raw in notes.lines() {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with("```") || is_rule(trimmed) {
            gap = true;
            continue;
        }
        if let Some(title) = title(trimmed) {
            gap = true;
            push(&mut out, &mut gap, Line { kind: Kind::Title, text: plain(title) });
            gap = true;
            continue;
        }
        if let Some((depth, point)) = point(raw) {
            push(&mut out, &mut gap, Line { kind: Kind::Point { depth }, text: plain(point) });
            continue;
        }
        // A paragraph goes on: joined to the line before (a point's too, when indented).
        match out.last_mut() {
            Some(last) if !gap && matches!(last.kind, Kind::Text | Kind::Point { .. }) => {
                last.text.push(' ');
                last.text.push_str(&plain(trimmed));
            }
            _ => push(&mut out, &mut gap, Line { kind: Kind::Text, text: plain(trimmed) }),
        }
    }
    out
}

/// A thematic break (`---`, `***`).
fn is_rule(line: &str) -> bool {
    line.len() >= 3 && (line.bytes().all(|b| b == b'-') || line.bytes().all(|b| b == b'*'))
}

/// A title's text (`## What's new` → `What's new`).
fn title(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches('#');
    let level = line.len() - rest.len();
    ((1..=6).contains(&level) && rest.starts_with(' ')).then(|| rest.trim())
}

/// A point's depth (two spaces or a tab a level) and its text.
fn point(raw: &str) -> Option<(u8, &str)> {
    let indent: usize =
        raw.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 2 } else { 1 }).sum();
    let line = raw.trim_start();
    let text = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("+ "))
        .or_else(|| {
            let digits = line.bytes().take_while(u8::is_ascii_digit).count();
            (digits > 0).then(|| line[digits..].strip_prefix(". ")).flatten()
        })?;
    Some((u8::try_from(indent / 2).unwrap_or(u8::MAX).min(3), text.trim()))
}

/// The text without its marks: `**`, `__`, backticks gone, a link's text kept (its address
/// left out), an image's left out.
pub fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with("**") || rest.starts_with("__") {
            rest = &rest[2..];
            continue;
        }
        if c == '`' {
            rest = &rest[1..];
            continue;
        }
        let image = rest.starts_with("![");
        if (c == '[' || image)
            && let Some((label, after)) = link(&rest[if image { 2 } else { 1 }..])
        {
            if !image {
                out.push_str(label);
            }
            rest = after;
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// After a link's `[`: its text and what follows `(…)`.
fn link(rest: &str) -> Option<(&str, &str)> {
    let close = rest.find("](")?;
    let after = &rest[close + 2..];
    let end = after.find(')')?;
    Some((&rest[..close], &after[end + 1..]))
}

#[cfg(test)]
#[path = "../tests/unit/notes.rs"]
mod tests;
