//! The project's React components: functions and classes exported with a capitalized
//! name (`export function UserCard`, `export default function Page`,
//! `export const Button = …`, `export default class App`). Each file is read again only
//! when it changes.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use code_intelligence::Project;
use kernel::GenerationalCache;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: String,
    pub file: PathBuf,
    pub offset: usize,
}

/// Each file's components, with the time it was read.
pub type FileComponents = GenerationalCache<PathBuf, (Option<SystemTime>, Arc<Vec<Component>>)>;

const SUFFIXES: &[&str] = &[".jsx", ".tsx", ".js", ".ts"];

pub fn gather(project: &Project<'_>, files: &mut FileComponents) -> Vec<Component> {
    let Some(root) = project.root else { return Vec::new() };
    let mut out = Vec::new();
    for relative in project.files_under("", SUFFIXES, 4000) {
        let file = root.join(&relative);
        let modified = (project.modified)(&file);
        let cached = files.get(&file).filter(|(m, _)| modified.is_some() && *m == modified);
        let found = match cached {
            Some((_, found)) => Arc::clone(found),
            None => {
                let text = (project.read)(&file).unwrap_or_default();
                let found = Arc::new(exported(&text, &file));
                files.insert(file, (modified, Arc::clone(&found)));
                found
            }
        };
        out.extend(found.iter().cloned());
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name == b.name);
    out
}

/// The capitalized names a file exports.
fn exported(text: &str, file: &std::path::Path) -> Vec<Component> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices("export ") {
        if at > 0 && !text[..at].ends_with(['\n', ';', ' ', '\t']) {
            continue;
        }
        let mut rest = &text[at + "export ".len()..];
        let skip = |word: &str, rest: &mut &str| {
            if let Some(r) = rest.trim_start().strip_prefix(word) {
                *rest = r;
                true
            } else {
                false
            }
        };
        skip("default ", &mut rest);
        skip("async ", &mut rest);
        if !(skip("function ", &mut rest) || skip("class ", &mut rest) || skip("const ", &mut rest))
        {
            continue;
        }
        let trimmed = rest.trim_start();
        let name: String =
            trimmed.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if name.starts_with(|c: char| c.is_uppercase()) {
            let offset = text.len() - trimmed.len();
            out.push(Component { name, file: file.to_path_buf(), offset });
        }
    }
    out
}

#[cfg(test)]
#[path = "../tests/unit/components.rs"]
mod tests;
