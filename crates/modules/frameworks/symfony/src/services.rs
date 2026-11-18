//! The project's services: from the compiled container (`var/cache/dev/*Container.xml`,
//! written by Symfony as the app runs in `dev`: every service, the bundles' too) when there
//! is one, else from `config/services.yaml` — its ids, and the classes it loads from a
//! folder (`App\: resource: '../src/'`, autowired: their id is their class).
//!
//! Named in `'@id'` (YAML), `service('id')` (PHP configuration), `#[Autowire(service:
//! 'id')]` and `$container->get('id')`.

use std::ops::Range;
use std::path::{Path, PathBuf};

use code_intelligence::{Project, before_string, line_of, quoted_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub id: String,
    pub class: Option<String>,
    /// Where it's declared (the container's file, the YAML file, its class's file).
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// The project's services, and whether they're every one (from the compiled container): only
/// then is an unknown id one that doesn't exist (the bundles' aren't in `services.yaml`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Services {
    pub all: Vec<Service>,
    pub complete: bool,
}

/// The services of the project (`sources`: its `src` files, relative to `src`).
pub fn gather(project: &Project<'_>, sources: &[String]) -> Services {
    let Some(root) = project.root else { return Services::default() };
    if let Some(file) = compiled_container(project)
        && let Some(text) = (project.read)(&file)
    {
        let mut all = from_container(&text, &file);
        // A class of the project's leads to its file, not to the container's line.
        for service in &mut all {
            if let Some(source) = service.class.as_deref().and_then(|c| class_file(c, sources)) {
                (service.file, service.offset, service.line) =
                    (root.join("src").join(source), 0, 0);
            }
        }
        return Services { all, complete: true };
    }
    let file = root.join("config/services.yaml");
    let Some(text) = (project.read)(&file) else { return Services::default() };
    Services { all: from_yaml(&text, &file, &root.join("src"), sources), complete: false }
}

/// The compiled container Symfony wrote as the app ran in `dev`
/// (`var/cache/dev/App_KernelDevDebugContainer.xml`), if there is one.
pub fn compiled_container(project: &Project<'_>) -> Option<PathBuf> {
    let cache = project.root?.join("var/cache/dev");
    (project.list)(&cache)
        .into_iter()
        .find(|(name, is_dir)| !is_dir && name.ends_with("Container.xml"))
        .map(|(name, _)| cache.join(name))
}

/// The source file (relative to `src`) of class `class`, when it's `App\…` (PSR-4: `src/`).
fn class_file<'a>(class: &str, sources: &'a [String]) -> Option<&'a String> {
    let path = format!("{}.php", class.strip_prefix("App\\")?.replace('\\', "/"));
    sources.iter().find(|s| **s == path)
}

/// `<service id="…" class="…"/>` (an alias: `alias="…"`, its class the target's).
fn from_container(text: &str, file: &Path) -> Vec<Service> {
    let attribute = |tag: &str, name: &str| -> Option<String> {
        let key = format!(" {name}=\"");
        let start = tag.find(&key)? + key.len();
        let end = start + tag[start..].find('"')?;
        Some(tag[start..end].replace("&amp;", "&"))
    };
    let mut all: Vec<Service> = Vec::new();
    for (at, _) in text.match_indices("<service ") {
        let end = text[at..].find('>').map_or(text.len(), |e| at + e);
        let tag = &text[at..end];
        let Some(id) = attribute(tag, "id") else { continue };
        if id.starts_with('.') {
            // Symfony's own inner ones (`.service_locator.x`): not named by code.
            continue;
        }
        let class = attribute(tag, "class").or_else(|| {
            let target = attribute(tag, "alias")?;
            all.iter().find(|s| s.id == target).and_then(|s| s.class.clone()).or(Some(target))
        });
        let offset = at + tag.find(" id=\"").unwrap_or(0) + 5;
        all.push(Service {
            id,
            class,
            file: file.to_path_buf(),
            offset,
            line: line_of(text, offset),
        });
    }
    all.sort_by(|a, b| a.id.cmp(&b.id));
    all.dedup_by(|a, b| a.id == b.id);
    all
}

/// The ids under `services:` (one level in), their `class:`; a namespace loaded from a
/// folder (`App\: resource: '../src/'`): its classes, autowired.
fn from_yaml(text: &str, file: &Path, src: &Path, sources: &[String]) -> Vec<Service> {
    let mut all: Vec<Service> = Vec::new();
    let mut in_services = false;
    let mut namespaces: Vec<String> = Vec::new();
    let mut offset = 0;
    let mut indent = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end();
        let depth = line.len() - line.trim_start().len();
        let start = offset;
        offset += line.len();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        if depth == 0 {
            in_services = trimmed == "services:";
            indent = None;
            continue;
        }
        if !in_services {
            continue;
        }
        let level = *indent.get_or_insert(depth);
        let entry = trimmed.trim_start();
        if depth == level {
            let Some(key) =
                entry.strip_suffix(':').or_else(|| entry.split_once(": ").map(|(k, _)| k))
            else {
                continue;
            };
            let id = key.trim_matches(['\'', '"']).to_owned();
            if id.starts_with('_') {
                continue;
            }
            if id.ends_with('\\') {
                namespaces.push(id);
                continue;
            }
            let at = start + depth + usize::from(key.starts_with(['\'', '"']));
            all.push(Service {
                id,
                class: None,
                file: file.to_path_buf(),
                offset: at,
                line: line_of(text, at),
            });
        } else if let Some(class) = entry.strip_prefix("class:")
            && let Some(last) = all.last_mut().filter(|s| s.file == file)
        {
            last.class = Some(class.trim().trim_matches(['\'', '"']).to_owned());
        }
    }
    for service in &mut all {
        if service.class.is_none() && service.id.contains('\\') {
            service.class = Some(service.id.clone());
        }
    }
    // The classes loaded from `src/` (the usual `App\:`), each its own service.
    if namespaces.iter().any(|n| n == "App\\") {
        for source in sources {
            let class = format!("App\\{}", source.trim_end_matches(".php").replace('/', "\\"));
            if all.iter().any(|s| s.id == class) {
                continue;
            }
            let file = src.join(source);
            all.push(Service { id: class.clone(), class: Some(class), file, offset: 0, line: 0 });
        }
    }
    all.sort_by(|a, b| a.id.cmp(&b.id));
    all.dedup_by(|a, b| a.id == b.id);
    all
}

/// The service id named at `offset`: `'@id'`, `service('id')`, `#[Autowire(service: 'id')]`,
/// `$container->get('id')` — its range (without the `@`).
pub fn named_at(text: &str, offset: usize) -> Option<Range<usize>> {
    let content = quoted_at(text, offset)?;
    named(text, content)
}

/// The service ids `text` names (see [`named_at`]).
pub fn uses(text: &str) -> Vec<Range<usize>> {
    let mut found = Vec::new();
    for quote in ['\'', '"'] {
        let mut open: Option<usize> = None;
        for (at, c) in text.char_indices() {
            if c == '\n' {
                open = None;
            } else if c == quote && !text[..at].ends_with('\\') {
                match open.take() {
                    Some(start) => {
                        found.extend(named(text, start + 1..at).filter(|r| !r.is_empty()));
                    }
                    None => open = Some(at),
                }
            }
        }
    }
    found.sort_by_key(|r: &Range<usize>| r.start);
    found
}

/// A string's content `content` as a service id, if the code around it names one (empty:
/// still being typed — completed, never a use).
fn named(text: &str, content: Range<usize>) -> Option<Range<usize>> {
    let value = &text[content.clone()];
    if let Some(id) = value.strip_prefix('@') {
        // `'@@x'` is the escaped text `@x`, not a service.
        return (!id.starts_with('@') && !id.contains(char::is_whitespace))
            .then_some(content.start + 1..content.end);
    }
    let before = before_string(text, &content);
    const CALLS: &[&str] = &["service(", "service:", "container->get("];
    CALLS.iter().any(|c| before.ends_with(c)).then_some(content)
}

#[cfg(test)]
#[path = "../tests/unit/services.rs"]
mod tests;
