//! What Laravel projects declare in code: route names in `routes/*.php` and config keys
//! in `config/*.php`.

use std::path::{Path, PathBuf};

use code_intelligence::{Project, line_of};

/// A name declared in a file (a route, a config key), with what to show next to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub name: String,
    /// A route's URI, a config key's file.
    pub detail: Option<String>,
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

pub fn routes(project: &Project<'_>) -> Vec<Named> {
    let Some(root) = project.root else { return Vec::new() };
    let mut out = Vec::new();
    for relative in project.files_under("routes", &[".php"], 200) {
        let file = root.join("routes").join(&relative);
        if let Some(text) = (project.read)(&file) {
            out.extend(route_names(&text, &file));
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// `Route::get('/users/{id}', …)->name('users.show')`: the name, with the URI of its
/// statement.
fn route_names(text: &str, file: &Path) -> Vec<Named> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("->name(") {
        let Some((name, start)) = string_arg(text, at + "->name(".len()) else { continue };
        let statement = text[..at].rfind(';').map_or(0, |s| s + 1);
        let uri =
            ["get(", "post(", "put(", "patch(", "delete(", "any(", "match(", "view(", "resource("]
                .iter()
                .filter_map(|m| {
                    text[statement..at]
                        .rfind(&format!("Route::{m}"))
                        .map(|i| statement + i + "Route::".len() + m.len())
                })
                .max()
                .and_then(|i| string_arg(text, i))
                .map(|(uri, _)| uri.to_owned());
        out.push(Named {
            name: name.to_owned(),
            detail: uri,
            file: file.to_path_buf(),
            offset: start,
            line: line_of(text, start),
        });
    }
    out
}

/// `config/app.php`'s `'name' => …` (first level) → `app.name`; each file → its name too.
pub fn config(project: &Project<'_>) -> Vec<Named> {
    let Some(root) = project.root else { return Vec::new() };
    let mut out = Vec::new();
    for relative in project.files_under("config", &[".php"], 300) {
        let file = root.join("config").join(&relative);
        let prefix = relative.trim_end_matches(".php").replace('/', ".");
        out.push(Named {
            name: prefix.clone(),
            detail: Some(format!("config/{relative}")),
            file: file.clone(),
            offset: 0,
            line: 0,
        });
        if let Some(text) = (project.read)(&file) {
            out.extend(top_keys(&text).into_iter().map(|(key, offset)| Named {
                name: format!("{prefix}.{key}"),
                detail: Some(format!("config/{relative}")),
                file: file.clone(),
                offset,
                line: line_of(&text, offset),
            }));
        }
    }
    out
}

/// The keys of the returned array's first level: `'key' =>` at bracket depth 1.
fn top_keys(text: &str) -> Vec<(String, usize)> {
    let Some(start) = text.find("return [").or_else(|| text.find("return array(")) else {
        return Vec::new();
    };
    let bytes = text.as_bytes();
    let mut keys = Vec::new();
    let mut depth = 0;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                i = text[i..].find('\n').map_or(bytes.len(), |n| i + n);
                continue;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = text[i..].find("*/").map_or(bytes.len(), |n| i + n + 2);
                continue;
            }
            q @ (b'\'' | b'"') => {
                let end = text[i + 1..].find(q as char).map_or(bytes.len(), |n| i + 1 + n);
                if depth == 1 && text[end + 1..].trim_start().starts_with("=>") {
                    keys.push((text[i + 1..end].to_owned(), i + 1));
                }
                i = end + 1;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    keys
}

/// The string literal starting at `at` (spaces allowed before it) and where its content starts.
fn string_arg(text: &str, at: usize) -> Option<(&str, usize)> {
    let rest = &text[at..];
    let trimmed = rest.trim_start();
    let quote = trimmed.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let start = at + (rest.len() - trimmed.len()) + 1;
    let end = text[start..].find(quote)? + start;
    Some((&text[start..end], start))
}

#[cfg(test)]
#[path = "../tests/unit/scan.rs"]
mod tests;
