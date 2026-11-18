//! The project's routes: `#[Route]` attributes in `src/` (with the class's name prefix) and
//! the top-level keys of `config/routes.yaml` / `config/routes/*.yaml`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use code_intelligence::{Project, line_of};
use kernel::GenerationalCache;

/// Each source file's routes, with the time the file was read: only files changed since
/// are read again.
pub type FileRoutes = GenerationalCache<PathBuf, (Option<SystemTime>, Arc<Vec<Route>>)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub name: String,
    /// The URL pattern (`/blog/{slug}`), when declared next to the name.
    pub path: Option<String>,
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// The routes of the project's `src` files (`sources`, relative to it: listed once until
/// files are made or removed) and its route files.
pub fn gather(project: &Project<'_>, sources: &[String], files: &mut FileRoutes) -> Vec<Route> {
    let Some(root) = project.root else { return Vec::new() };
    let mut routes = Vec::new();
    for relative in sources {
        let file = root.join("src").join(relative);
        let modified = (project.modified)(&file);
        let cached = files.get(&file).filter(|(m, _)| modified.is_some() && *m == modified);
        let found = match cached {
            Some((_, found)) => Arc::clone(found),
            None => {
                let text = (project.read)(&file).unwrap_or_default();
                let found = if text.contains("Route(") {
                    Arc::new(from_attributes(&text, &file))
                } else {
                    Arc::default()
                };
                files.insert(file, (modified, Arc::clone(&found)));
                found
            }
        };
        routes.extend(found.iter().cloned());
    }
    let mut yaml = vec!["config/routes.yaml".to_owned()];
    yaml.extend(
        project
            .files_under("config/routes", &[".yaml", ".yml"], 200)
            .into_iter()
            .map(|f| format!("config/routes/{f}")),
    );
    for relative in yaml {
        let file = root.join(&relative);
        if let Some(text) = (project.read)(&file) {
            routes.extend(from_yaml(&text, &file));
        }
    }
    routes.sort_by(|a, b| a.name.cmp(&b.name));
    routes.dedup_by(|a, b| a.name == b.name);
    routes
}

/// The code that names a route by a string: `path('x')`, `redirectToRoute('x')`…
pub(crate) const USES: &[&str] =
    &["path(", "url(", "redirectToRoute(", "generateUrl(", "->generate(", "logout_path("];

/// The route names `text` uses (`path('x')` and the rest of [`USES`]): each one's range.
pub(crate) fn uses(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut found: Vec<std::ops::Range<usize>> = USES
        .iter()
        .flat_map(|call| text.match_indices(call).map(move |(at, _)| at + call.len()))
        .filter_map(|after| {
            let rest = &text[after..];
            let skipped = rest.len() - rest.trim_start().len();
            let quote = rest[skipped..].chars().next().filter(|c| *c == '\'' || *c == '"')?;
            let start = after + skipped + 1;
            let end = start + text[start..].find([quote, '\n'])?;
            (text[end..].starts_with(quote) && end > start).then_some(start..end)
        })
        .collect();
    found.sort_by_key(|r| r.start);
    found.dedup();
    found
}

/// Whether `relative` (to the project) is a route file: `config/routes.yaml`, or one under
/// `config/routes/`.
pub(crate) fn is_route_file(relative: &str) -> bool {
    let yaml = relative.ends_with(".yaml") || relative.ends_with(".yml");
    yaml && (relative == "config/routes.yaml" || relative.starts_with("config/routes/"))
}

/// Where `text` (file `relative`) declares route `name` itself — not through its class's
/// prefix (that one isn't written whole: renamed only through its uses' file).
pub(crate) fn declarations(relative: &str, text: &str, name: &str) -> Vec<std::ops::Range<usize>> {
    let path = std::path::Path::new(relative);
    let declared: Vec<Route> = if is_route_file(relative) {
        from_yaml(text, path)
    } else if text.contains("Route(") {
        from_attributes(text, path)
    } else {
        Vec::new()
    };
    declared
        .iter()
        .filter(|r| r.name == name && text[r.offset..].starts_with(name))
        .map(|r| r.offset..r.offset + name.len())
        .collect()
}

/// `#[Route('/path', name: 'x')]`: before `class`, a name prefix for the class's routes.
fn from_attributes(text: &str, file: &std::path::Path) -> Vec<Route> {
    let class_at = text.find("\nclass ").or_else(|| text.find("\nfinal class ")).unwrap_or(0);
    let mut prefix = String::new();
    let mut prefix_path = String::new();
    let mut routes = Vec::new();
    for (at, _) in text.match_indices("Route(") {
        let end = text[at..].find(")]").map_or(text.len(), |e| at + e);
        let args = &text[at + 6..end];
        let name = named(args, "name");
        let path = named(args, "path").or_else(|| first_string(args));
        if at < class_at {
            prefix = name.map(|(n, _)| n.to_owned()).unwrap_or_default();
            prefix_path = path.map(|(p, _)| p.to_owned()).unwrap_or_default();
            continue;
        }
        let Some((name, name_at)) = name else { continue };
        let offset = at + 6 + name_at;
        routes.push(Route {
            name: format!("{prefix}{name}"),
            path: path.map(|(p, _)| format!("{prefix_path}{p}")),
            file: file.to_path_buf(),
            offset,
            line: line_of(text, offset),
        });
    }
    routes
}

/// The string value of named argument `key` (`name: 'x'`) and where it starts in `args`.
pub(crate) fn named<'a>(args: &'a str, key: &str) -> Option<(&'a str, usize)> {
    let pattern = format!("{key}:");
    let at = args
        .match_indices(&pattern)
        .map(|(i, _)| i)
        .find(|&i| i == 0 || !args.as_bytes()[i - 1].is_ascii_alphanumeric())?;
    let rest = &args[at + pattern.len()..];
    let quote_at = rest.find(['\'', '"'])?;
    if !rest[..quote_at].trim().is_empty() {
        return None;
    }
    let start = at + pattern.len() + quote_at + 1;
    let quote = args.as_bytes()[start - 1] as char;
    let end = args[start..].find(quote)? + start;
    Some((&args[start..end], start))
}

/// The first argument, if it's a string (`Route('/path', …)`).
pub(crate) fn first_string(args: &str) -> Option<(&str, usize)> {
    let trimmed = args.trim_start();
    let quote = trimmed.chars().next().filter(|c| *c == '\'' || *c == '"')?;
    let start = args.len() - trimmed.len() + 1;
    let end = args[start..].find(quote)? + start;
    Some((&args[start..end], start))
}

/// `name:` at the start of a line, then `path: /x` below it.
fn from_yaml(text: &str, file: &std::path::Path) -> Vec<Route> {
    let mut routes: Vec<Route> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        if !trimmed.is_empty() && !line.starts_with([' ', '\t', '#']) && trimmed.ends_with(':') {
            let name = trimmed.trim_end_matches(':').trim_matches(['\'', '"']).to_owned();
            routes.push(Route {
                name,
                path: None,
                file: file.to_path_buf(),
                offset,
                line: line_of(text, offset),
            });
        } else if let Some(path) = trimmed.trim_start().strip_prefix("path:")
            && let Some(last) = routes.last_mut().filter(|r| r.path.is_none())
        {
            last.path = Some(path.trim().trim_matches(['\'', '"']).to_owned());
        }
        offset += line.len();
    }
    routes.retain(|r| r.path.is_some());
    routes
}

#[cfg(test)]
#[path = "../tests/unit/routes.rs"]
mod tests;
