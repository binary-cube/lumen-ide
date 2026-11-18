//! A `.http` file read into its variables and requests, in the `.http` syntax
//! other HTTP clients write too:
//!
//! - requests separated by `###` lines (the text after it is the request's title, and its
//!   name unless `# @name` gives another);
//! - before a request line: comments (`#`, `//`), what they ask (`@name`, `@no-redirect`,
//!   `@no-cookie-jar`, `@no-log`, `@timeout N`, `@connection-timeout N` — seconds, or with
//!   `ms` / `s` / `m`), file variables (`@host = example.com`, anywhere outside a body), a
//!   script run first (`< {% … %}`, `< script.js`);
//! - the request line: `METHOD URL [HTTP/version]`, or only a URL (`GET`); the URL goes on
//!   over lines starting with blanks then `?` or `&`;
//! - its headers (`Name: value`), up to a blank line;
//! - its body, up to the next separator: text, files (`< ./body.json`), then what's done
//!   with the response: a script (`> {% … %}`, `> handler.js`), written to a file too
//!   (`>> file`, `>>! file`); `<> file` (an earlier response) is skipped.
//!
//! Nothing is resolved here ([`crate::vars`]) and nothing read from disk.

use std::time::Duration;

use crate::model::{BodyPart, FileVariable, Header, HttpFile, Options, Request, SaveTo, Script};

/// The methods a request line may start with (others: the line is a URL).
pub const METHODS: &[&str] = &[
    "GET",
    "POST",
    "PUT",
    "PATCH",
    "DELETE",
    "HEAD",
    "OPTIONS",
    "TRACE",
    "CONNECT",
    "GRAPHQL",
    "WEBSOCKET",
];

/// A line of the text: where it starts (bytes), its text without its end, its number.
#[derive(Debug, Clone, Copy)]
struct Line<'a> {
    start: usize,
    text: &'a str,
    number: u32,
}

fn lines(text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (number, raw) in text.split_inclusive('\n').enumerate() {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line = line.strip_suffix('\r').unwrap_or(line);
        out.push(Line { start, text: line, number: number as u32 });
        start += raw.len();
    }
    out
}

/// A `###` separator line, and the title after it.
fn separator(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix("###").map(str::trim)
}

/// A comment line's text (after `#` or `//`), unless it's a separator.
fn comment(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if separator(trimmed).is_some() {
        return None;
    }
    trimmed.strip_prefix('#').or_else(|| trimmed.strip_prefix("//")).map(str::trim)
}

/// `@name = value` (outside a body).
fn file_variable(line: &str) -> Option<(String, String)> {
    let rest = line.trim_start().strip_prefix('@')?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    let valid = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '$'));
    valid.then(|| (name.to_owned(), value.trim().to_owned()))
}

/// A duration as `@timeout` takes it: seconds, or with a unit (`500ms`, `5s`, `2m`).
pub fn duration(text: &str) -> Option<Duration> {
    let text = text.trim();
    let (number, unit) = text
        .find(|c: char| !c.is_ascii_digit())
        .map_or((text, ""), |at| (&text[..at], text[at..].trim()));
    let n: u64 = number.parse().ok()?;
    match unit {
        "" | "s" => Some(Duration::from_secs(n)),
        "ms" => Some(Duration::from_millis(n)),
        "m" => Some(Duration::from_secs(n.checked_mul(60)?)),
        _ => None,
    }
}

/// What a comment asks, into `options` / `name`.
fn directive(text: &str, options: &mut Options, name: &mut Option<String>) {
    let Some(rest) = text.strip_prefix('@') else { return };
    let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let value = value.trim();
    match key {
        "name" if !value.is_empty() => *name = Some(value.to_owned()),
        "no-redirect" => options.no_redirect = true,
        "no-cookie-jar" => options.no_cookie_jar = true,
        "no-log" => options.no_log = true,
        "no-auto-encoding" => options.no_auto_encoding = true,
        "timeout" => options.timeout = duration(value),
        "connection-timeout" => options.connection_timeout = duration(value),
        _ => {}
    }
}

/// A script starting on line `i` after `prefix` (`<` or `>`): inline (`{% … %}`, maybe over
/// lines) or a file; the script and the line after it.
fn script(lines: &[Line<'_>], i: usize, prefix: char) -> Option<(Script, usize)> {
    let rest = lines[i].text.trim_start().strip_prefix(prefix)?.trim_start();
    let Some(inline) = rest.strip_prefix("{%") else {
        let file = rest.trim();
        return (!file.is_empty()).then(|| (Script::File(file.to_owned()), i + 1));
    };
    if let Some(end) = inline.find("%}") {
        return Some((Script::Inline(inline[..end].trim().to_owned()), i + 1));
    }
    let mut body = vec![inline.to_owned()];
    for (j, line) in lines.iter().enumerate().skip(i + 1) {
        if let Some(end) = line.text.find("%}") {
            body.push(line.text[..end].to_owned());
            let code = body.join("\n");
            return Some((Script::Inline(code.trim().to_owned()), j + 1));
        }
        body.push(line.text.to_owned());
    }
    // Never closed: up to the end.
    Some((Script::Inline(body.join("\n").trim().to_owned()), lines.len()))
}

/// The request line: its method, URL and version.
fn request_line(line: &str) -> (String, String, Option<String>) {
    let trimmed = line.trim();
    let (first, rest) = trimmed.split_once(char::is_whitespace).unwrap_or((trimmed, ""));
    let (method, target) = if METHODS.contains(&first.to_ascii_uppercase().as_str()) {
        (first.to_ascii_uppercase(), rest.trim())
    } else {
        ("GET".to_owned(), trimmed)
    };
    // The version after the URL's last space before `HTTP/` (`HTTP/2 (Prior Knowledge)`
    // has spaces of its own).
    let version_at = target
        .rmatch_indices("HTTP/")
        .map(|(at, _)| at)
        .find(|&at| target[..at].ends_with(char::is_whitespace));
    match version_at {
        Some(at) => (method, target[..at].trim().to_owned(), Some(target[at..].trim().to_owned())),
        None => (method, target.to_owned(), None),
    }
}

/// `text` read.
pub fn parse(text: &str) -> HttpFile {
    let lines = lines(text);
    let mut file = HttpFile::default();
    // The blocks: (first line, title), each up to the next separator.
    let mut blocks: Vec<(usize, Option<String>, usize)> = Vec::new();
    let mut start = 0;
    let mut title = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some(t) = separator(line.text) {
            blocks.push((start, title.take(), i));
            start = i + 1;
            title = (!t.is_empty()).then(|| t.to_owned());
        }
    }
    blocks.push((start, title, lines.len()));
    let text_end = text.len();
    for (first, title, end) in blocks {
        let range_start = lines.get(first).map_or(text_end, |l| l.start);
        let range_end = lines.get(end).map_or(text_end, |l| l.start);
        if let Some(request) =
            block(&lines[first..end], title, range_start..range_end, &mut file.variables)
        {
            file.requests.push(request);
        }
    }
    file
}

/// One block's request (`None`: only comments and variables), its file variables into
/// `variables`.
fn block(
    lines: &[Line<'_>],
    title: Option<String>,
    range: std::ops::Range<usize>,
    variables: &mut Vec<FileVariable>,
) -> Option<Request> {
    let mut options = Options::default();
    let mut name = title.clone();
    let mut pre_script = None;
    let mut i = 0;
    // Before the request line.
    let request_at = loop {
        let line = lines.get(i)?;
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            i += 1;
        } else if let Some(text) = comment(line.text) {
            directive(text, &mut options, &mut name);
            i += 1;
        } else if let Some((name, value)) = file_variable(line.text) {
            variables.push(FileVariable { name, value, line: line.number });
            i += 1;
        } else if trimmed.starts_with('<') {
            match script(lines, i, '<') {
                Some((found, next)) => {
                    pre_script = Some(found);
                    i = next;
                }
                None => i += 1,
            }
        } else {
            break i;
        }
    };
    let (method, mut url, version) = request_line(lines[request_at].text);
    let mut i = request_at + 1;
    // The URL going on (`?a=1`, `&b=2` on lines of their own, indented).
    while let Some(line) = lines.get(i) {
        let starts_blank = line.text.starts_with([' ', '\t']);
        let piece = line.text.trim();
        if starts_blank && (piece.starts_with('?') || piece.starts_with('&')) {
            url.push_str(piece);
            i += 1;
        } else {
            break;
        }
    }
    // The headers, up to a blank line.
    let mut headers = Vec::new();
    while let Some(line) = lines.get(i) {
        let trimmed = line.text.trim();
        if trimmed.is_empty() {
            i += 1;
            break;
        }
        if comment(line.text).is_none()
            && let Some((name, value)) = trimmed.split_once(':')
        {
            headers.push(Header { name: name.trim().to_owned(), value: value.trim().to_owned() });
        }
        i += 1;
    }
    // The body, then what's done with the response.
    let mut body: Vec<BodyPart> = Vec::new();
    let mut text = String::new();
    let mut post_script = None;
    let mut save_to = None;
    let flush = |text: &mut String, body: &mut Vec<BodyPart>| {
        let piece = std::mem::take(text);
        if !piece.trim().is_empty() {
            body.push(BodyPart::Text(piece));
        }
    };
    while let Some(line) = lines.get(i) {
        let trimmed = line.text.trim_start();
        if let Some(rest) = trimmed.strip_prefix(">>") {
            let (replace, path) = rest.strip_prefix('!').map_or((false, rest), |path| (true, path));
            save_to = Some(SaveTo { path: path.trim().to_owned(), replace });
            i += 1;
        } else if trimmed.starts_with("<>") {
            i += 1;
        } else if trimmed.starts_with('>') {
            match script(lines, i, '>') {
                Some((found, next)) => {
                    post_script = Some(found);
                    i = next;
                }
                None => i += 1,
            }
        } else if let Some(path) = trimmed.strip_prefix("< ") {
            flush(&mut text, &mut body);
            body.push(BodyPart::File(path.trim().to_owned()));
            i += 1;
        } else {
            if post_script.is_none() && save_to.is_none() {
                text.push_str(line.text);
                text.push('\n');
            }
            i += 1;
        }
    }
    // The blank lines before the next request aren't the body's.
    let kept = text.trim_end_matches(['\n', '\r', ' ', '\t']).len();
    text.truncate(kept);
    flush(&mut text, &mut body);
    Some(Request {
        name,
        title,
        method,
        url,
        version,
        headers,
        body,
        options,
        pre_script,
        post_script,
        save_to,
        range,
        line: lines[request_at].number,
        line_range: lines[request_at].start..lines[request_at].start + lines[request_at].text.len(),
    })
}

#[cfg(test)]
#[path = "../tests/unit/parse.rs"]
mod tests;
