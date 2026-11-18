//! The project's parameters: the compiled container's (`<parameter key="…">`, every one, the
//! bundles' and the kernel's too) when there is one, else those under `parameters:` in
//! `config/services.yaml` and `config/packages/*.yaml`. Named as `%name%` in the
//! configuration (`'%kernel.project_dir%/var'`, `#[Autowire('%app.email%')]`), as
//! `#[Autowire(param: 'name')]` and `$this->getParameter('name')`.

use std::ops::Range;
use std::path::{Path, PathBuf};

use code_intelligence::{Project, before_string, line_of, quoted_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parameter {
    pub name: String,
    pub value: Option<String>,
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// The project's parameters (`container`: the compiled container, if there is one).
pub fn gather(project: &Project<'_>, container: Option<&Path>) -> Vec<Parameter> {
    let Some(root) = project.root else { return Vec::new() };
    if let Some(file) = container
        && let Some(text) = (project.read)(file)
    {
        return from_container(&text, file);
    }
    let mut files = vec!["config/services.yaml".to_owned()];
    files.extend(
        project
            .files_under("config/packages", &[".yaml", ".yml"], 500)
            .into_iter()
            .map(|f| format!("config/packages/{f}")),
    );
    let mut all = Vec::new();
    for relative in files {
        let file = root.join(&relative);
        let Some(text) = (project.read)(&file) else { continue };
        for key in crate::yaml::keys(&text, Some("parameters")) {
            let line = line_of(&text, key.offset);
            all.push(Parameter {
                name: key.name,
                value: key.value,
                file: file.clone(),
                offset: key.offset,
                line,
            });
        }
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    all
}

/// `<parameter key="name">value</parameter>` (a collection's: no value).
fn from_container(text: &str, file: &Path) -> Vec<Parameter> {
    let mut all = Vec::new();
    for (at, _) in text.match_indices("<parameter key=\"") {
        let start = at + "<parameter key=\"".len();
        let Some(end) = text[start..].find('"').map(|e| start + e) else { continue };
        let tag_end = text[end..].find('>').map_or(text.len(), |e| end + e);
        let value = (!text[..tag_end].ends_with('/') && !text[tag_end..].starts_with(">\n"))
            .then(|| {
                let from = tag_end + 1;
                let to = text[from..].find("</parameter>").map_or(from, |e| from + e);
                text[from..to].to_owned()
            })
            .filter(|v| !v.contains('<'));
        let name = text[start..end].to_owned();
        all.push(Parameter {
            name,
            value,
            file: file.to_path_buf(),
            offset: start,
            line: line_of(text, start),
        });
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all.dedup_by(|a, b| a.name == b.name);
    all
}

/// The parameter named at `offset` (see the module's doc): its name's range — an empty
/// `getParameter('')` too (being typed).
pub fn named_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let named = uses(text).into_iter().find(|r| r.start <= offset && offset <= r.end);
    named.or_else(|| {
        let content = quoted_at(text, offset)?;
        let before = before_string(text, &content);
        (content.is_empty() && CALLS.iter().any(|c| before.ends_with(c))).then_some(content)
    })
}

/// The calls naming a parameter bare.
const CALLS: [&str; 2] = ["getParameter(", "param:"];

/// The parameters `text` names: each name's range.
pub fn uses(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    // `%name%`: a name between two `%` (`%%` is an escaped `%`), not `%env(…)%`.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            i += 1;
            continue;
        }
        if bytes.get(i + 1) == Some(&b'%') {
            i += 2;
            continue;
        }
        let start = i + 1;
        let end = text[start..]
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .map_or(text.len(), |e| start + e);
        if end > start && bytes.get(end) == Some(&b'%') && !text[start..end].starts_with("env") {
            found.push(start..end);
            i = end + 1;
        } else {
            i += 1;
        }
    }
    // A bare name: `param: 'name'`, `getParameter('name')`.
    for call in CALLS {
        for (at, _) in text.match_indices(call) {
            let after = at + call.len();
            let skipped = text[after..].len() - text[after..].trim_start().len();
            let open = after + skipped;
            if !text[open..].starts_with(['\'', '"']) {
                continue;
            }
            if let Some(content) = quoted_at(text, open + 1)
                && !text[content.clone()].is_empty()
                && before_string(text, &content).ends_with(call)
            {
                found.push(content);
            }
        }
    }
    found.sort_by_key(|r| r.start);
    found.dedup();
    found
}

#[cfg(test)]
#[path = "../tests/unit/parameters.rs"]
mod tests;
