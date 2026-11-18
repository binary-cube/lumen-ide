//! From a key — in a `.env` file or where code reads it — to its definitions: Go to
//! Definition (one file: there; several: a list in the order they're loaded) and the hover
//! card (its value in each file, a secret's hidden). A [`DocumentAssist`] for the `.env`
//! language and every language whose code reads variables (as the modules contributed), so
//! it's asked before the frameworks and the language; it answers from the project's catalog
//! of `.env` files ([`crate::catalog`]), never waiting for the disk.

use std::collections::BTreeSet;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use code_intelligence::{
    AssistContext, AssistPlace, AssistTarget, CompletionItem, CompletionKind, DocumentAssist,
    FrameworkCompletion, HoverInfo, HoverParam, Location,
};
use i18n::{t, tr};
use kernel::ProjectId;

use crate::catalog::{self, Catalog, EnvDoc};
use crate::{keys, values};

#[derive(Debug, Default)]
pub struct Navigation;

impl DocumentAssist for Navigation {
    fn id(&self) -> &'static str {
        crate::ID
    }

    /// The `.env` language and the languages reading variables (read once the modules
    /// are in).
    fn languages(&self) -> &'static [&'static str] {
        static LANGUAGES: OnceLock<Vec<&'static str>> = OnceLock::new();
        LANGUAGES.get_or_init(|| {
            let mut all = vec![crate::ID];
            for language in code_intelligence::env::reads().iter().flat_map(|r| r.languages) {
                if !all.contains(language) {
                    all.push(language);
                }
            }
            all
        })
    }

    fn definition(&self, at: &AssistContext<'_>) -> Option<AssistTarget> {
        let (key, docs) = lookup(at)?;
        let here = (at.language == crate::ID).then_some((at.path, key.start));
        let mut places = definitions(&docs, &at.text[key.clone()], here, at.root);
        match places.len() {
            0 => None,
            1 => places.pop().map(|p| AssistTarget::Location(p.at)),
            _ => Some(AssistTarget::Choose {
                title: tr!("dotenv.choose_definition", key = &at.text[key]),
                places,
            }),
        }
    }

    /// The `.env` files' keys, where code reads one.
    fn complete(&self, at: &AssistContext<'_>) -> Option<FrameworkCompletion> {
        if at.language == crate::ID {
            return None;
        }
        let (key, docs) = lookup(at)?;
        let replace = key.start..at.offset.min(key.end).max(key.start);
        let items = suggestions(&docs, &at.text[replace.clone()]);
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn hover(&self, at: &AssistContext<'_>) -> Option<HoverInfo> {
        let (key, docs) = lookup(at)?;
        Some(hover(&docs, &at.text[key.clone()], key, at.root))
    }

    /// Changes as the project's `.env` files are read.
    fn generation(&self, project: Option<ProjectId>) -> u64 {
        project.and_then(catalog::of).map_or(0, |c| c.generation())
    }
}

/// The key at the cursor and the `.env` files serving the document — an open `.env` file
/// as it is in the editor.
fn lookup(at: &AssistContext<'_>) -> Option<(Range<usize>, Arc<[EnvDoc]>)> {
    let name = at.path.file_name()?.to_string_lossy();
    let registered = code_intelligence::env::reads();
    let key = keys::key_at(&registered, at.language, &name, at.text, at.offset)?;
    let catalog: Arc<Catalog> = at.project.and_then(catalog::of)?;
    let docs = catalog.files_for(at.path)?;
    if at.language != crate::ID {
        return Some((key, docs));
    }
    let mut docs: Vec<EnvDoc> = docs.iter().filter(|d| d.path != at.path).cloned().collect();
    docs.push(EnvDoc::new(at.path.to_path_buf(), at.text.into()));
    crate::order::sort(&mut docs, catalog.order(), |d| &d.name);
    Some((key, docs.into()))
}

/// The keys `docs` define that `query` matches, each with its value (a secret's hidden).
pub fn suggestions(docs: &[EnvDoc], query: &str) -> Vec<CompletionItem> {
    let keys: BTreeSet<&str> =
        docs.iter().flat_map(|d| d.file.names().map(|(r, _)| &d.text[r])).collect();
    let mut items =
        code_intelligence::suggest(query, keys.iter().copied(), CompletionKind::Constant, None);
    for item in &mut items {
        let value = docs.iter().find_map(|d| d.value(&item.label));
        item.detail = value.map(|value| shown(&item.label, value));
    }
    items
}

/// Where a file is shown: relative to the project's root, else by its name.
fn shown_path(path: &Path, root: Option<&Path>) -> String {
    root.and_then(|r| path.strip_prefix(r).ok())
        .or_else(|| path.file_name().map(Path::new))
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// `key`'s `value` as quoted: `(empty)`, or as written with its secret part hidden.
fn shown(key: &str, value: &str) -> String {
    match value.is_empty() {
        true => t("dotenv.empty"),
        false => values::shown(key, value),
    }
}

/// The places defining `key` among `docs`, in their order — but the one at `here` (the
/// definition the cursor is on).
pub fn definitions(
    docs: &[EnvDoc],
    key: &str,
    here: Option<(&Path, usize)>,
    root: Option<&Path>,
) -> Vec<AssistPlace> {
    docs.iter()
        .filter_map(|doc| {
            let (line, entry) = doc.defined(key)?;
            if here == Some((doc.path.as_path(), entry.key.start)) {
                return None;
            }
            let value = shown(key, &doc.text[entry.value.content.clone()]);
            let detail = match doc.documents {
                true => format!("= {value} · {}", t("dotenv.documentation")),
                false => format!("= {value}"),
            };
            let at = Location { path: doc.path.clone(), offset: entry.key.start, line: line.first };
            Some(AssistPlace { label: shown_path(&doc.path, root), detail, at })
        })
        .collect()
}

/// The hover card of `key` (written at `range`): its value in each of `docs`, in their
/// order (a secret's hidden), the documentation marked.
pub fn hover(docs: &[EnvDoc], key: &str, range: Range<usize>, root: Option<&Path>) -> HoverInfo {
    let params: Vec<HoverParam> = docs
        .iter()
        .filter_map(|doc| {
            let value = doc.value(key)?;
            Some(HoverParam {
                ty: Some(shown_path(&doc.path, root)),
                name: shown(key, value),
                doc: doc.documents.then(|| t("dotenv.documentation")),
            })
        })
        .collect();
    HoverInfo {
        range,
        code: key.to_owned(),
        title: Some((t("dotenv.hover_title"), Some(CompletionKind::Constant))),
        doc: params.is_empty().then(|| t("dotenv.undefined")),
        params_label: Some(t("dotenv.hover_values")),
        params,
        ..HoverInfo::default()
    }
}

#[cfg(test)]
#[path = "../tests/unit/navigation.rs"]
mod tests;
