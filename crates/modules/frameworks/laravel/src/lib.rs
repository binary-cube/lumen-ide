//! Laravel module: active only in projects that use Laravel (from `composer.json` or
//! `artisan`). View names (`view('users.index')`, Blade's `@extends('…')`,
//! `@include('…')`) are suggested from `resources/views/` and lead to their file; route
//! names (`route('…')`) come from `->name('…')` in `routes/*.php`, with their URI on hover;
//! config keys (`config('app.name')`) from the top-level keys of `config/*.php`. Its `.env`
//! files' sections are their keys' prefixes (`APP_`, `DB_`).

mod blade;
mod scan;

use std::ops::Range;
use std::sync::Arc;

use code_intelligence::env::{EnvAccess, EnvFileOrder, EnvReads, EnvSections};
use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkSupport, HoverInfo,
    Location, Project, before_string, has_dependency, quoted_at, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::SharedMemo;

use crate::scan::Named;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.laravel"),
    name: "Laravel",
    description: "Views, routes and config keys: completion, go to definition, hover",
    depends_on: &[],
};

/// `env('X')`, `env('X', default)`, `Env::get('X')`.
const ENV_READS: EnvReads =
    EnvReads::new(&["php", "blade"], &[EnvAccess::call("env"), EnvAccess::call("Env::get")]);

#[derive(Debug, Default)]
pub struct LaravelModule;

impl Module for LaravelModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its runtime data and Vite's output.
        ctx.contribute(kernel::ProjectDirs {
            generated: &["storage", "public/build"],
            ..Default::default()
        });
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(Laravel));
        // Its `.env` groups its keys by prefix (`APP_`, `DB_`, `MAIL_`), without comments;
        // its code reads them with `env('X', default)`; it loads `.env`, then
        // `.env.{APP_ENV}` when there is one.
        ctx.contribute(EnvSections::KeyPrefix);
        ctx.contribute(ENV_READS);
        ctx.contribute(EnvFileOrder {
            name: "Laravel",
            markers: &["artisan"],
            files: &[".env", ".env.{env}"],
        });
        Ok(())
    }
}

#[derive(Debug)]
pub struct Laravel;

impl Framework for Laravel {
    fn id(&self) -> &'static str {
        "laravel"
    }

    fn name(&self) -> &'static str {
        "Laravel"
    }

    fn family(&self) -> Option<&'static str> {
        Some("php")
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let composer = project.read_relative("composer.json").unwrap_or_default();
        if has_dependency(&composer, "laravel/framework") {
            return Some("composer.json".into());
        }
        project
            .read_relative("artisan")
            .is_some_and(|a| a.contains("Laravel") || a.contains("Illuminate"))
            .then(|| "artisan".into())
    }

    fn activate(&self, _project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Support::default())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    View,
    Route,
    Config,
}

fn slot(text: &str, content: &Range<usize>) -> Option<Slot> {
    let before = before_string(text, content);
    const VIEWS: &[&str] = &[
        "view(",
        "View::make(",
        "->view(",
        "View::exists(",
        "@extends(",
        "@include(",
        "@includeIf(",
        "@includeWhen(",
        "@includeFirst([",
        "@component(",
        "@each(",
        "markdown(",
        "Route::view(",
    ];
    const ROUTES: &[&str] = &["route(", "to_route(", "->route(", "routeIs(", "Route::has("];
    if VIEWS.iter().any(|v| before.ends_with(v)) {
        // `Route::view('/uri', 'name')`: the first string is the URI.
        return (!before.ends_with("Route::view(")).then_some(Slot::View);
    }
    if ROUTES.iter().any(|r| before.ends_with(r)) {
        return Some(Slot::Route);
    }
    ["config(", "Config::get(", "Config::set(", "Config::has("]
        .iter()
        .any(|c| before.ends_with(c))
        .then_some(Slot::Config)
}

/// A view's file name → its name: `users/index.blade.php` → `users.index`.
fn view_name(file: &str) -> String {
    let stem =
        file.strip_suffix(".blade.php").or_else(|| file.strip_suffix(".php")).unwrap_or(file);
    stem.replace('/', ".")
}

/// (view name, file relative to `resources/views`).
type Views = Arc<Vec<(String, String)>>;

#[derive(Debug, Default)]
struct Support {
    views: SharedMemo<u64, Views>,
    routes: SharedMemo<u64, Arc<Vec<Named>>>,
    config: SharedMemo<u64, Arc<Vec<Named>>>,
    /// The PHP files that may name a view (listed again only when files are made or removed).
    sources: SharedMemo<u64, Arc<Vec<String>>>,
    /// What Blade gets from Laravel.
    blade: blade::Blade,
}

impl Support {
    fn views(&self, at: &FrameworkContext<'_>) -> Views {
        self.views.get_or_make(at.own, || {
            let files = at.project.files_under("resources/views", &[".php"], 5000);
            Arc::new(files.into_iter().map(|f| (view_name(&f), f)).collect())
        })
    }

    fn routes(&self, at: &FrameworkContext<'_>) -> Arc<Vec<Named>> {
        self.routes.get_or_make(at.own, || Arc::new(scan::routes(at.project)))
    }

    fn config(&self, at: &FrameworkContext<'_>) -> Arc<Vec<Named>> {
        self.config.get_or_make(at.own, || Arc::new(scan::config(at.project)))
    }

    fn sources(&self, at: &FrameworkContext<'_>) -> Arc<Vec<String>> {
        self.sources.get_or_make(at.own, || {
            let mut all = Vec::new();
            for dir in ["app", "routes"] {
                let files = at.project.files_under(dir, &[".php"], 5000);
                all.extend(files.into_iter().map(|f| format!("{dir}/{f}")));
            }
            Arc::new(all)
        })
    }

    fn slot_at(at: &FrameworkContext<'_>) -> Option<(Range<usize>, Slot)> {
        let content = quoted_at(at.text, at.offset)?;
        let slot = slot(at.text, &content)?;
        Some((content, slot))
    }

    fn named(&self, at: &FrameworkContext<'_>, slot: Slot) -> Arc<Vec<Named>> {
        if slot == Slot::Route { self.routes(at) } else { self.config(at) }
    }
}

/// The files Laravel's model is made from: nothing else changes it.
const WATCHES: &[&str] = &["app/**/*.php", "routes/**/*.php", "resources/views/**", "config/**"];

impl FrameworkSupport for Support {
    fn watches(&self) -> &'static [&'static str] {
        WATCHES
    }

    /// Its views, routes, config keys and what Blade asks for, gathered ahead (the UI
    /// thread finds them made).
    fn prepare(&self, at: &FrameworkContext<'_>) {
        self.blade.prepare(at, &self.sources(at), &self.views(at));
        self.routes(at);
        self.config(at);
    }

    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if let Some(answer) = self.blade.complete(at) {
            return Some(answer);
        }
        let (content, slot) = Self::slot_at(at)?;
        let replace = content.start..at.offset;
        let query = &at.text[replace.clone()];
        let items = match slot {
            Slot::View => {
                let views = self.views(at);
                suggest(
                    query,
                    views.iter().map(|(n, _)| n.as_str()),
                    CompletionKind::Value,
                    Some("view"),
                )
            }
            Slot::Route | Slot::Config => {
                let named = self.named(at, slot);
                let mut items = suggest(
                    query,
                    named.iter().map(|n| n.name.as_str()),
                    CompletionKind::Value,
                    None,
                );
                for item in &mut items {
                    item.detail =
                        named.iter().find(|n| n.name == item.label).and_then(|n| n.detail.clone());
                }
                items
            }
        };
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn definition(&self, at: &FrameworkContext<'_>) -> Option<Location> {
        let (content, slot) = Self::slot_at(at)?;
        let name = &at.text[content];
        match slot {
            Slot::View => {
                let views = self.views(at);
                let (_, file) = views.iter().find(|(n, _)| n == name)?;
                let path = at.project.root?.join("resources/views").join(file);
                Some(Location { path, offset: 0, line: 0 })
            }
            Slot::Route | Slot::Config => {
                let named = self.named(at, slot);
                let found = named.iter().find(|n| n.name == name)?;
                Some(Location { path: found.file.clone(), offset: found.offset, line: found.line })
            }
        }
    }

    fn hover(&self, at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        let (content, Slot::Route) = Self::slot_at(at)? else { return None };
        let routes = self.routes(at);
        let route = routes.iter().find(|r| r.name == at.text[content.clone()])?;
        let code = match &route.detail {
            Some(uri) => format!("route {} → {uri}", route.name),
            None => format!("route {}", route.name),
        };
        Some(HoverInfo { range: content, code, notes: Vec::new(), doc: None, ..Default::default() })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
