//! What a document is: by its name's extension, else by how its content begins (a `{` or
//! `[`, an XML declaration or tag, an HTML doctype or `<html>` / `<table>`, YAML's `---` or
//! `key:` lines, lines split by one delimiter the same number of times).

/// A document's format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Json,
    Yaml,
    Xml,
    Html,
    /// Delimited lines, split by this.
    Csv(u8),
    Unknown,
}

/// The delimiters a delimited text may use, most likely first.
const DELIMITERS: [u8; 4] = *b",\t;|";
/// The lines looked at to tell a delimited text.
const SAMPLE_LINES: usize = 20;

/// `text`'s format, `name` (a file's) first.
pub fn detect(name: &str, text: &str) -> Format {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "json" | "geojson" | "jsonl" => return Format::Json,
        "yaml" | "yml" => return Format::Yaml,
        "xml" | "svg" | "rss" | "atom" | "xlf" => return Format::Xml,
        "html" | "htm" | "xhtml" => return Format::Html,
        "csv" => return Format::Csv(delimiter(text).unwrap_or(b',')),
        "tsv" | "tab" => return Format::Csv(b'\t'),
        _ => {}
    }
    by_content(text)
}

/// The format told by the content alone.
pub fn by_content(text: &str) -> Format {
    let t = text.trim_start_matches('\u{feff}').trim_start();
    let head: String = t.chars().take(256).collect::<String>().to_ascii_lowercase();
    if t.starts_with('{') || t.starts_with('[') {
        return Format::Json;
    }
    if head.starts_with("<!doctype html") || head.starts_with("<html") || head.contains("<table") {
        return Format::Html;
    }
    if t.starts_with('<') {
        return Format::Xml;
    }
    if t.starts_with("---") || looks_like_yaml(t) {
        return Format::Yaml;
    }
    delimiter(t).map_or(Format::Unknown, Format::Csv)
}

/// Its first lines are `key: value` or `- item`.
fn looks_like_yaml(t: &str) -> bool {
    let lines: Vec<&str> = t
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .take(5)
        .collect();
    !lines.is_empty()
        && lines.iter().all(|l| {
            let s = l.trim_start();
            s.starts_with("- ")
                || s.split_once(": ").is_some_and(|(k, _)| !k.contains(',') && !k.contains('\t'))
                || s.ends_with(':')
        })
}

/// The delimiter splitting its first lines the same number of times (at least once),
/// outside quotes.
fn delimiter(text: &str) -> Option<u8> {
    let lines: Vec<&str> =
        text.lines().filter(|l| !l.trim().is_empty()).take(SAMPLE_LINES).collect();
    if lines.is_empty() {
        return None;
    }
    DELIMITERS.into_iter().find(|&d| {
        let counts: Vec<usize> = lines.iter().map(|l| outside_quotes(l, d)).collect();
        counts[0] > 0 && counts.iter().all(|&c| c == counts[0])
    })
}

fn outside_quotes(line: &str, d: u8) -> usize {
    let mut quoted = false;
    line.bytes()
        .filter(|&b| {
            if b == b'"' {
                quoted = !quoted;
            }
            b == d && !quoted
        })
        .count()
}
