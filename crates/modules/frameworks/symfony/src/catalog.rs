//! The names Symfony's configuration declares — services, parameters, translation keys — as
//! one shape (a [`Catalog`] of [`Declared`] names) for what completion, definition, hover and
//! the problems do with them: each kind says how it's gathered and how code names it.

use std::ops::Range;
use std::path::PathBuf;

use code_intelligence::Project;

use crate::{parameters, services, translations};

/// A kind of name the configuration declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Named {
    Service = 0,
    Parameter = 1,
    Translation = 2,
}

impl Named {
    pub const ALL: [Self; 3] = [Self::Service, Self::Parameter, Self::Translation];

    /// The names of this kind `text` names: each one's range.
    pub fn uses(self, text: &str) -> Vec<Range<usize>> {
        match self {
            Self::Service => services::uses(text),
            Self::Parameter => parameters::uses(text),
            Self::Translation => translations::uses(text),
        }
    }

    fn named_at(self, text: &str, offset: usize) -> Option<Range<usize>> {
        match self {
            Self::Service => services::named_at(text, offset),
            Self::Parameter => parameters::named_at(text, offset),
            Self::Translation => translations::named_at(text, offset),
        }
    }

    /// The hover card's word for it (`service app.mailer → App\Mailer`).
    pub fn word(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Parameter => "parameter",
            Self::Translation => "translation",
        }
    }

    /// Between its name and its detail on the card.
    pub fn joiner(self) -> &'static str {
        match self {
            Self::Service => "→",
            Self::Parameter | Self::Translation => "=",
        }
    }

    /// The locale key of "no such name".
    pub fn missing_key(self) -> &'static str {
        match self {
            Self::Service => "symfony.no_service",
            Self::Parameter => "symfony.no_parameter",
            Self::Translation => "symfony.no_translation",
        }
    }

    /// Whether a name used that isn't declared is a problem: a translation written as a
    /// sentence is its own text (no key to find).
    pub fn checked(self, name: &str) -> bool {
        self != Self::Translation || !name.contains(char::is_whitespace)
    }
}

/// The name at `offset` and its kind (a string can name only one).
pub fn named_at(text: &str, offset: usize) -> Option<(Named, Range<usize>)> {
    Named::ALL.into_iter().find_map(|kind| Some((kind, kind.named_at(text, offset)?)))
}

/// A name declared: what shows next to it (a service's class, a parameter's value, a
/// translation's text), more on the card (each locale's text), where it's declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub name: String,
    pub detail: Option<String>,
    pub doc: Option<String>,
    pub file: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// The names of a kind, by name; `complete`: every one is known (a name missing doesn't
/// exist — not so for the services and parameters without the compiled container).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub all: Vec<Declared>,
    pub complete: bool,
}

impl Catalog {
    pub fn get(&self, name: &str) -> Option<&Declared> {
        self.all.binary_search_by(|d| d.name.as_str().cmp(name)).ok().map(|at| &self.all[at])
    }
}

/// What a kind's names are made from: its files (each with when it last changed) and the
/// project's file list's generation (the classes `src/` holds: autowired services) — the
/// same print, the same names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Print {
    files: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    structure: u64,
}

impl Print {
    /// `kind`'s print in `project` (`structure`: its files' list's generation). Cheap: a
    /// look at a few files' dates, a listing of `config/packages` / `translations`.
    pub fn of(project: &Project<'_>, kind: Named, structure: u64) -> Self {
        let Some(root) = project.root else { return Self { files: Vec::new(), structure } };
        let mut files: Vec<PathBuf> = Vec::new();
        let container = services::compiled_container(project);
        match kind {
            Named::Service => {
                files.extend(container);
                files.push(root.join("config/services.yaml"));
            }
            Named::Parameter => {
                files.extend(container);
                files.push(root.join("config/services.yaml"));
                let packages = project.files_under("config/packages", &[".yaml", ".yml"], 500);
                files.extend(packages.iter().map(|f| root.join("config/packages").join(f)));
            }
            Named::Translation => {
                let found =
                    project.files_under("translations", &[".yaml", ".yml", ".xlf", ".xliff"], 2000);
                files.extend(found.iter().map(|f| root.join("translations").join(f)));
            }
        }
        let files = files.into_iter().map(|f| {
            let modified = (project.modified)(&f);
            (f, modified)
        });
        Self { files: files.collect(), structure }
    }
}

/// A kind's names as last made: for which generation of Symfony's, from what.
#[derive(Debug)]
pub struct Built {
    pub own: u64,
    pub print: Print,
    pub catalog: std::sync::Arc<Catalog>,
}

/// The project's names of `kind` (`sources`: its `src` files, relative to `src`).
pub fn gather(project: &Project<'_>, kind: Named, sources: &[String]) -> Catalog {
    let mut catalog = match kind {
        Named::Service => {
            let found = services::gather(project, sources);
            let all = found
                .all
                .into_iter()
                .map(|s| Declared {
                    name: s.id,
                    detail: s.class,
                    doc: None,
                    file: s.file,
                    offset: s.offset,
                    line: s.line,
                })
                .collect();
            Catalog { all, complete: found.complete }
        }
        Named::Parameter => {
            let container = services::compiled_container(project);
            let all = parameters::gather(project, container.as_deref())
                .into_iter()
                .map(|p| Declared {
                    name: p.name,
                    detail: p.value,
                    doc: None,
                    file: p.file,
                    offset: p.offset,
                    line: p.line,
                })
                .collect();
            Catalog { all, complete: container.is_some() }
        }
        Named::Translation => translations_catalog(translations::gather(project)),
    };
    catalog.all.sort_by(|a, b| a.name.cmp(&b.name));
    catalog.all.dedup_by(|a, b| a.name == b.name);
    catalog
}

/// The translations by key: its first locale's text beside it, every locale's on the card,
/// its first file as where it's declared. Known whole once there's any.
fn translations_catalog(all: Vec<translations::Translation>) -> Catalog {
    let mut out: Vec<Declared> = Vec::new();
    for t in all {
        let line = format!("{} ({}): {}", t.locale, t.domain, t.text.as_deref().unwrap_or("—"));
        match out.last_mut().filter(|d| d.name == t.key) {
            Some(declared) => {
                let doc = declared.doc.get_or_insert_with(String::new);
                doc.push('\n');
                doc.push_str(&line);
            }
            None => out.push(Declared {
                name: t.key,
                detail: t.text,
                doc: Some(line),
                file: t.file,
                offset: t.offset,
                line: t.line,
            }),
        }
    }
    let complete = !out.is_empty();
    Catalog { all: out, complete }
}

#[cfg(test)]
#[path = "../tests/unit/catalog.rs"]
mod tests;
