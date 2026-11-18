//! What's completed at a place of a `.http` file, read from the text around the cursor: a
//! method at a request line's start, a header's name in a request's headers, its value
//! after `Name:`, a comment's directive after `# @`, a variable inside `{{`; and the items
//! each place offers (the variables' names are given: the file's, the environments', the
//! requests' — whoever completes knows them).

use std::ops::Range;

use code_intelligence::{CompletionItem, CompletionKind};

/// Where the cursor is, for completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// A request line's first word.
    Method,
    /// A header's name.
    HeaderName,
    /// The value of header `name`.
    HeaderValue(String),
    /// A comment's directive (`# @…`).
    Directive,
    /// Inside `{{` (`{{tok`).
    Variable,
}

/// The place at `offset`, and the text it replaces (the word typed so far).
pub fn place(text: &str, offset: usize) -> Option<(Place, Range<usize>)> {
    let offset = offset.min(text.len());
    let line_start = text[..offset].rfind('\n').map_or(0, |n| n + 1);
    let before = &text[line_start..offset];
    // Inside `{{`, not closed yet on this line.
    if let Some(open) = before.rfind("{{")
        && !before[open..].contains("}}")
    {
        let start = line_start + open + 2;
        let typed = &text[start..offset];
        let trimmed = typed.len() - typed.trim_start().len();
        return Some((Place::Variable, start + trimmed..offset));
    }
    let trimmed = before.trim_start();
    for prefix in ["# @", "// @"] {
        if let Some(rest) = trimmed.strip_prefix(prefix)
            && !rest.contains(char::is_whitespace)
        {
            return Some((Place::Directive, offset - rest.len()..offset));
        }
    }
    if trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    match region(text, line_start) {
        Region::RequestLine if !trimmed.contains(char::is_whitespace) => {
            Some((Place::Method, offset - trimmed.len()..offset))
        }
        Region::Headers => match trimmed.split_once(':') {
            None => Some((Place::HeaderName, offset - trimmed.len()..offset)),
            Some((name, value)) => {
                // After its last `,` or `;` (a list of values, a type's parameters).
                let piece = value.rsplit([',', ';']).next().unwrap_or(value);
                let typed = piece.trim_start();
                Some((Place::HeaderValue(name.trim().to_owned()), offset - typed.len()..offset))
            }
        },
        _ => None,
    }
}

/// What part of its request a line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Region {
    /// Before the request line, or the first line that could be one.
    RequestLine,
    Headers,
    Body,
}

/// The region of the line starting at `line_start`: from its block's start (the last `###`),
/// the first line that isn't blank, a comment or a variable is the request line, the lines
/// up to a blank one its headers, the rest its body.
fn region(text: &str, line_start: usize) -> Region {
    let block_start = text[..line_start]
        .rmatch_indices('\n')
        .map(|(at, _)| at + 1)
        .chain(std::iter::once(0))
        .find(|&at| text[at..].trim_start_matches([' ', '\t']).starts_with("###"))
        .map_or(0, |at| text[at..].find('\n').map_or(text.len(), |n| at + n + 1));
    let mut seen_request = false;
    for line in text[block_start.min(line_start)..line_start].lines() {
        let trimmed = line.trim();
        if !seen_request {
            let skipped = trimmed.is_empty()
                || trimmed.starts_with('#')
                || trimmed.starts_with("//")
                || trimmed.starts_with('@')
                || trimmed.starts_with('<');
            seen_request = !skipped;
        } else if trimmed.is_empty() {
            return Region::Body;
        }
    }
    if seen_request { Region::Headers } else { Region::RequestLine }
}

/// The headers offered (the common ones).
pub const HEADERS: &[&str] = &[
    "Accept",
    "Accept-Encoding",
    "Accept-Language",
    "Authorization",
    "Cache-Control",
    "Connection",
    "Content-Disposition",
    "Content-Encoding",
    "Content-Length",
    "Content-Type",
    "Cookie",
    "Host",
    "If-Match",
    "If-Modified-Since",
    "If-None-Match",
    "Origin",
    "Pragma",
    "Range",
    "Referer",
    "User-Agent",
    "X-Api-Key",
    "X-Correlation-Id",
    "X-Forwarded-For",
    "X-Request-Id",
    "X-Requested-With",
];

/// The media types offered for `Content-Type` and `Accept`.
const MEDIA_TYPES: &[&str] = &[
    "application/json",
    "application/x-www-form-urlencoded",
    "multipart/form-data",
    "application/xml",
    "text/xml",
    "text/plain",
    "text/html",
    "application/graphql",
    "application/octet-stream",
    "application/pdf",
    "image/png",
    "image/jpeg",
    "*/*",
];

/// The directives a comment may give (`# @name`).
pub const DIRECTIVES: &[(&str, &str)] = &[
    ("name", "the request's name (other requests use its response by it)"),
    ("no-redirect", "redirects aren't followed"),
    ("no-cookie-jar", "the project's cookies aren't sent nor kept"),
    ("no-log", "not kept in the history"),
    ("timeout", "seconds (or 500ms, 2m) for the whole response"),
    ("connection-timeout", "seconds for the connection"),
];

/// The dynamic variables (`{{$uuid}}`).
pub const DYNAMIC: &[(&str, &str)] = &[
    ("$uuid", "a random UUID"),
    ("$random.uuid", "a random UUID"),
    ("$timestamp", "seconds since 1970"),
    ("$isoTimestamp", "the time now (ISO 8601, UTC)"),
    ("$randomInt", "a number from 0 to 999"),
    ("$random.integer(0, 100)", "a number in a range"),
    ("$random.alphanumeric(8)", "letters, digits, _"),
    ("$random.alphabetic(8)", "letters"),
    ("$random.hexadecimal(8)", "hexadecimal digits"),
    ("$random.float(0, 1)", "a decimal number in a range"),
    ("$random.email", "an e-mail address"),
    ("$env.", "the system's environment variable (`$env.USER`)"),
];

/// The values offered for header `name`.
fn values_of(name: &str) -> &'static [&'static str] {
    match name.to_ascii_lowercase().as_str() {
        "content-type" | "accept" => MEDIA_TYPES,
        "authorization" => &["Bearer ", "Basic "],
        "cache-control" => &["no-cache", "no-store", "max-age=0"],
        "accept-encoding" => &["gzip, deflate, br", "identity"],
        "connection" => &["keep-alive", "close"],
        _ => &[],
    }
}

fn item(label: &str, kind: CompletionKind, detail: Option<&str>, insert: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind,
        detail: detail.map(str::to_owned),
        insert: insert.to_owned(),
        ..CompletionItem::default()
    }
}

/// The items `place` offers; `variables`: the names a `{{` may take (with a detail: where
/// it comes from), beside the dynamic ones.
pub fn items(place: &Place, variables: &[(String, String)]) -> Vec<CompletionItem> {
    match place {
        Place::Method => http_requests::parse::METHODS
            .iter()
            .map(|m| item(m, CompletionKind::Keyword, None, &format!("{m} ")))
            .collect(),
        Place::HeaderName => HEADERS
            .iter()
            .map(|h| item(h, CompletionKind::Property, None, &format!("{h}: ")))
            .collect(),
        Place::HeaderValue(name) => {
            values_of(name).iter().map(|v| item(v.trim(), CompletionKind::Value, None, v)).collect()
        }
        Place::Directive => DIRECTIVES
            .iter()
            .map(|(name, what)| item(name, CompletionKind::Keyword, Some(what), name))
            .collect(),
        Place::Variable => variables
            .iter()
            .map(|(name, from)| item(name, CompletionKind::Variable, Some(from), name))
            .chain(
                DYNAMIC
                    .iter()
                    .map(|(name, what)| item(name, CompletionKind::Function, Some(what), name)),
            )
            .collect(),
    }
}

#[cfg(test)]
#[path = "../tests/unit/complete.rs"]
mod tests;
