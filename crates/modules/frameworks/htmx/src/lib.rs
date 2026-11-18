//! htmx module: active only in projects that use htmx (`htmx.org` in `package.json`, or
//! htmx loaded by a usual layout file), for the project's major version (2 or 4: their
//! attributes differ). In a tag, the `hx-*` attributes join HTML's suggestions; in their
//! values, the keywords (`hx-swap="outerMorph"`, `hx-trigger="click once"`); on hover, an
//! attribute's syntax and what it does. In any template, the `hx-on:*` handlers are
//! JavaScript.

mod attributes;
mod data;
mod handlers;

use std::ops::Range;
use std::sync::Arc;

use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkSupport, HoverInfo,
    Project, before_string, has_dependency, quoted_at, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

use crate::data::Entry;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.htmx"),
    name: "htmx",
    description: "The hx-* attributes in tags, their values and their syntax on hover",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct HtmxModule;

impl Module for HtmxModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(Htmx));
        ctx.contribute::<Arc<dyn syntax_ts::InjectionProvider>>(Arc::new(
            handlers::EventHandlers::default(),
        ));
        Ok(())
    }
}

/// Layout files that usually load htmx from a CDN (checked when `package.json` doesn't
/// list it).
const LAYOUTS: &[&str] = &[
    "index.html",
    "public/index.html",
    "src/index.html",
    "templates/base.html.twig",
    "resources/views/layouts/app.blade.php",
    "views/layouts/main.php",
];

#[derive(Debug)]
pub struct Htmx;

impl Framework for Htmx {
    fn id(&self) -> &'static str {
        "htmx"
    }

    fn name(&self) -> &'static str {
        "htmx"
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let (source, major) = version(project)?;
        Some(format!("{source} · htmx {major}"))
    }

    fn activate(&self, project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        let major = version(project).map_or_else(data::newest, |(_, major)| major);
        Arc::new(Support { major, entries: data::for_version(major) })
    }
}

/// Where htmx comes from and its major version: `package.json`'s `"htmx.org": "^4.0.0"`,
/// or a layout loading `htmx.org@4` (without a version, a CDN serves htmx 2).
fn version(project: &Project<'_>) -> Option<(String, u32)> {
    let major = |after: &str| -> Option<u32> {
        let digits: String = after
            .trim_start_matches(|c: char| !c.is_ascii_digit() && c != '"' && c != '\'')
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().ok()
    };
    let package = project.read_relative("package.json").unwrap_or_default();
    if has_dependency(&package, "htmx.org") {
        let at = package.find("\"htmx.org\"").map_or(0, |i| i + "\"htmx.org\"".len());
        let value = package[at..].split_once('"').map_or("", |(_, v)| v);
        return Some(("package.json".into(), major(value).unwrap_or(2)));
    }
    LAYOUTS.iter().find_map(|layout| {
        let text = project.read_relative(layout)?;
        let at = text.find("htmx")?;
        let pinned = text[at..].find("htmx.org@").map(|i| at + i + "htmx.org@".len());
        Some(((*layout).to_owned(), pinned.and_then(|i| major(&text[i..])).unwrap_or(2)))
    })
}

/// The project's htmx: its version's attributes (parsed when the project uses htmx).
#[derive(Debug)]
struct Support {
    major: u32,
    entries: Vec<Entry>,
}

impl Support {
    #[cfg(test)]
    fn attribute(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| !e.element && e.name == name)
    }
}

/// The markup languages where tags are written.
fn markup(language: &str) -> bool {
    matches!(language, "html" | "twig" | "blade" | "php" | "vue" | "xml")
}

/// The name of the attribute whose value holds the string at `content` (without a
/// `:inherited` modifier).
fn attribute_of<'a>(text: &'a str, content: &Range<usize>) -> Option<&'a str> {
    let before = before_string(text, content).strip_suffix('=')?.trim_end();
    let name_start = before.rfind(|c: char| c.is_whitespace()).map_or(0, |i| i + 1);
    let name = &before[name_start..];
    Some(name.strip_suffix(":inherited").unwrap_or(name))
}

impl FrameworkSupport for Support {
    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if !markup(at.language) {
            return None;
        }
        // In a value: the keyword being typed (the last space-separated word).
        if let Some(content) = quoted_at(at.text, at.offset) {
            let name = attribute_of(at.text, &content)?;
            let values = attributes::values(name, self.major);
            let typed = &at.text[content.start..at.offset];
            let word = typed.rfind(' ').map_or(content.start, |i| content.start + i + 1);
            let replace = word..at.offset;
            let query = &at.text[replace.clone()];
            let items = suggest(query, values, CompletionKind::Value, Some("htmx"));
            return (!items.is_empty()).then_some(FrameworkCompletion {
                replace,
                items,
                merge: false,
            });
        }
        let replace = code_intelligence::attribute_name_at(at.text, at.offset, &['-', ':'])?;
        let names = self.entries.iter().filter(|e| !e.element).map(|e| e.name);
        let mut items =
            suggest(&at.text[replace.clone()], names, CompletionKind::Attribute, Some("htmx"));
        for item in &mut items {
            // `hx-get=""` with the cursor in the quotes; `hx-on:` for the event; `hx-ignore`.
            if attributes::takes_suffix(&item.label) {
                item.insert = format!("{}:", item.label.trim_end_matches(":*"));
            } else if !attributes::bare(&item.label) {
                item.insert = format!("{}=\"\"", item.label);
                item.cursor_back = 1;
            }
        }
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: true })
    }

    fn hover(&self, at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        if !markup(at.language) {
            return None;
        }
        let range = code_intelligence::word_at(at.text, at.offset, "-")?;
        let word = &at.text[range.clone()];
        let entry = self.entries.iter().find(|e| e.name == word)?;
        let syntax = attributes::syntax(entry.name, self.major);
        let code = match (entry.element, syntax) {
            (true, _) => format!("<{}>", entry.name),
            (false, "") => entry.name.to_owned(),
            (false, syntax) => format!("{}=\"{syntax}\"", entry.name),
        };
        let doc = (!entry.doc.is_empty()).then(|| entry.doc.to_owned());
        Some(HoverInfo { range, code, notes: Vec::new(), doc, ..Default::default() })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
