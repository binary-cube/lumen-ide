//! Document links: the places in a text that lead somewhere ⌘-click opens — a URL (in the
//! browser), a path in quotes that names a file of the project (`require 'lib/db.php'`,
//! `import x from './x'`, `href="/css/app.css"`, `src`, CSS's `url(…)`), a YAML schema named
//! by its path (`# yaml-language-server: $schema=../schema.json`). Found on a line's text
//! alone; a path counts only if the file is there (`exists`).

use std::ops::Range;
use std::path::{Path, PathBuf};

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A file (at a line, 0-based, when the link says one).
    File { path: PathBuf, line: Option<u32> },
    /// A web address, opened in the browser.
    Url(String),
}

/// A link: the bytes it covers and where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentLink {
    pub range: Range<usize>,
    pub target: LinkTarget,
}

/// What a link's path is looked for from: the file's folder, the project's root (paths
/// starting with `/` too: a web root, or its `public/`), and whether a file is there.
#[derive(Clone, Copy)]
pub struct LinkBase<'a> {
    pub file: &'a Path,
    pub root: Option<&'a Path>,
    pub exists: &'a dyn Fn(&Path) -> bool,
}

impl std::fmt::Debug for LinkBase<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkBase").field("file", &self.file).field("root", &self.root).finish()
    }
}

/// The extensions tried for an import written without one (`import x from './x'`).
const IMPLIED: &[&str] =
    &["ts", "tsx", "js", "jsx", "mjs", "cjs", "vue", "svelte", "scss", "css", "php", "json"];

/// The links in `line` (a line of the text, starting at byte `start` of it).
pub fn line_links(line: &str, start: usize, base: &LinkBase<'_>) -> Vec<DocumentLink> {
    let mut out = Vec::new();
    for (range, url) in urls(line) {
        out.push(DocumentLink {
            range: start + range.start..start + range.end,
            target: LinkTarget::Url(url),
        });
    }
    // `$schema=…` written without quotes (the YAML language server's line): its path.
    if let Some(range) = schema(line)
        && !out
            .iter()
            .any(|l| l.range.start <= start + range.start && start + range.end <= l.range.end)
        && let Some(path) = resolve(&line[range.clone()], false, base)
    {
        let target = LinkTarget::File { path, line: None };
        out.push(DocumentLink { range: start + range.start..start + range.end, target });
    }
    for range in quoted(line) {
        if out
            .iter()
            .any(|l| l.range.start <= start + range.start && start + range.end <= l.range.end)
        {
            continue;
        }
        let written = &line[range.clone()];
        // `__DIR__ . '/x.php'`: from the file's folder.
        let from_dir = line[..range.start.saturating_sub(1)].trim_end().ends_with('.')
            && line[..range.start].contains("__DIR__");
        if let Some(path) = resolve(written, from_dir, base) {
            let target = LinkTarget::File { path, line: None };
            out.push(DocumentLink { range: start + range.start..start + range.end, target });
        }
    }
    out.sort_by_key(|l| l.range.start);
    out
}

/// The link at byte `offset` of `text`, looked for on its line.
pub fn link_at(text: &str, offset: usize, base: &LinkBase<'_>) -> Option<DocumentLink> {
    let offset = offset.min(text.len());
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    // A line too long to be code (minified) isn't looked through.
    if end - start > 4096 {
        return None;
    }
    line_links(&text[start..end], start, base)
        .into_iter()
        .find(|l| l.range.start <= offset && offset < l.range.end.max(l.range.start + 1))
}

/// Where the path after `$schema=` is in `line` (up to the next space).
fn schema(line: &str) -> Option<Range<usize>> {
    const KEY: &str = "$schema=";
    let at = line.find(KEY)? + KEY.len();
    let len = line[at..].find(char::is_whitespace).unwrap_or(line.len() - at);
    (len > 0).then_some(at..at + len)
}

/// The web addresses in `line` (`http://`, `https://`), their trailing punctuation left
/// out.
fn urls(line: &str) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = line[from..].find("http") {
        let at = from + i;
        let rest = &line[at..];
        let scheme = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            from = at + 4;
            continue;
        };
        let before = line[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric()) {
            from = at + scheme;
            continue;
        }
        let len = rest
            .find(|c: char| {
                c.is_whitespace()
                    || matches!(
                        c,
                        '"' | '\'' | '`' | '<' | '>' | ')' | '(' | '{' | '}' | '|' | '\\'
                    )
            })
            .unwrap_or(rest.len());
        let url = rest[..len].trim_end_matches(['.', ',', ';', ':', '!', '?', ']']);
        if url.len() > scheme {
            out.push((at..at + url.len(), url.to_owned()));
        }
        from = at + len.max(1);
    }
    out
}

/// What's inside the quotes of `line` (`'…'`, `"…"`, `` `…` ``), and the unquoted
/// argument of CSS's `url(…)`.
fn quoted(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            q @ (b'\'' | b'"' | b'`') => {
                let mut j = i + 1;
                while j < bytes.len() && bytes[j] != q {
                    j += if bytes[j] == b'\\' { 2 } else { 1 };
                }
                if j < bytes.len() && j > i + 1 {
                    out.push(i + 1..j);
                }
                i = j + 1;
            }
            b'u' if line[i..].starts_with("url(") => {
                let open = i + 4;
                let inner = &line[open..];
                if !inner.starts_with(['\'', '"']) {
                    let len = inner.find(')').unwrap_or(0);
                    if len > 0 {
                        out.push(open..open + len);
                    }
                }
                i = open;
            }
            _ => i += 1,
        }
    }
    out
}

/// The project file `written` names, if it's there: from the file's folder (`./`, `../`,
/// a plain name), from the root (`/…`, also its `public/`), with an implied extension for
/// imports.
fn resolve(written: &str, from_dir: bool, base: &LinkBase<'_>) -> Option<PathBuf> {
    let written = written.trim();
    // A path, not a sentence or a pattern: no spaces, no wildcards or placeholders, and a
    // `/` or an extension.
    let pathlike = !written.is_empty()
        && written.len() <= 300
        && !written.contains(|c: char| {
            c.is_whitespace()
                || matches!(c, '*' | '$' | '{' | '}' | '<' | '>' | '?' | '#' | '%' | ':')
        })
        && (written.contains('/') || has_extension(written));
    if !pathlike {
        return None;
    }
    let dir = base.file.parent()?;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(absolute) = written.strip_prefix('/') {
        if from_dir {
            candidates.push(dir.join(absolute));
        }
        if let Some(root) = base.root {
            candidates.push(root.join(absolute));
            candidates.push(root.join("public").join(absolute));
        }
    } else {
        candidates.push(dir.join(written));
        if !written.starts_with('.')
            && let Some(root) = base.root
        {
            candidates.push(root.join(written));
        }
    }
    let inside = |p: &Path| base.root.is_none_or(|root| normalize(p).starts_with(root));
    for candidate in candidates {
        let candidate = normalize(&candidate);
        if !inside(&candidate) {
            continue;
        }
        if (base.exists)(&candidate) {
            return Some(candidate);
        }
        if !has_extension(written) {
            for ext in IMPLIED {
                let with = candidate.with_extension(ext);
                if (base.exists)(&with) {
                    return Some(with);
                }
            }
            for ext in IMPLIED {
                let index = candidate.join(format!("index.{ext}"));
                if (base.exists)(&index) {
                    return Some(index);
                }
            }
        }
    }
    None
}

/// The last part has an extension (`app.css`, not `.env` alone).
fn has_extension(written: &str) -> bool {
    let name = written.rsplit('/').next().unwrap_or(written);
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty()
            && !ext.is_empty()
            && ext.len() <= 6
            && ext.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

/// `a/./b/../c` → `a/c` (no disk access).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/links.rs"]
mod tests;
