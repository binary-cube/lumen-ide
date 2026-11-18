//! Symfony module: active only in projects that use Symfony (from `composer.json` or
//! `bin/console`). Template names — in `render('…')` and in Twig's `{% extends '…' %}`,
//! `include('…')` — are suggested from `templates/` and lead to their file; route names —
//! in `path('…')`, `url('…')`, `redirectToRoute('…')` — come from the `#[Route]`
//! attributes in `src/` and from `config/routes*.yaml`, with their path on hover; console
//! command names — in `->find('…')`, `'command' => '…'` — come from the `#[AsCommand]`
//! attributes in `src/`, with their description. Routes are found and renamed wherever
//! they're used or declared, and one used but not declared is pointed out. Services
//! (`'@id'`, `service('id')`, `$container->get('id')`), parameters (`%name%`,
//! `getParameter('name')`) and translation keys (`trans('key')`, `'key'|trans`) are
//! completed, lead to where they're declared and show their class, value or text
//! ([`catalog`]); `$container->get(X::class)` is an `X` to the PHP inference. Its model
//! follows only its own files ([`WATCHES`]), made in the background. Its `.env` files'
//! sections are Flex's `###> vendor/package ###` blocks.

mod catalog;
mod commands;
mod config_reference;
mod config_yaml;
mod parameters;
mod routes;
mod services;
mod translations;
mod twig;
mod yaml;

use std::sync::{Arc, Mutex};

use code_intelligence::env::{EnvAccess, EnvFileOrder, EnvReads, EnvSections};
use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkName,
    FrameworkProblem, FrameworkSupport, HoverInfo, Location, ProblemLevel, Project, before_string,
    has_dependency, quoted_at, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::SharedMemo;

use crate::commands::Command;
use crate::routes::Route;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.symfony"),
    name: "Symfony",
    description: "The project's Twig templates, routes, console commands, services, parameters and translations: completion, go to definition, hover, Find Usages and Rename of routes, the missing ones pointed out",
    depends_on: &[],
};

/// `%env(X)%`, `%env(int:X)%` (YAML and PHP configuration), `#[Autowire(env: 'X')]` and
/// `env('X')` (PHP).
const ENV_READS: [EnvReads; 2] = [
    EnvReads::new(&["yaml", "php"], &[EnvAccess::placeholder("%env(", &[")%"])]),
    EnvReads::new(&["php"], &[EnvAccess::named("env"), EnvAccess::call("env")]),
];

#[derive(Debug, Default)]
pub struct SymfonyModule;

impl Module for SymfonyModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Its cache.
        ctx.contribute(kernel::ProjectDirs { generated: &["var/cache"], ..Default::default() });
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(Symfony));
        // Runs the project's code: only when asked.
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: READ_REFERENCE,
                label: "Read Symfony Configuration Options",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("download"),
            handler: Arc::new(config_reference::ReadReference),
        });
        // `$container->get(Mailer::class)` (PSR's and Symfony's containers) gives a `Mailer`;
        // `$container->get('mailer')`, the class its project's services say ([`SERVICES`]).
        for class in [
            "Psr\\Container\\ContainerInterface",
            "Symfony\\Component\\DependencyInjection\\ContainerInterface",
        ] {
            let rule = code_intelligence::calls::ClassArgumentCall::new(class, "get", 0);
            ctx.contribute(rule.names(SERVICES));
        }
        // Flex's recipes write their `.env` keys in blocks: `###> symfony/mailer ###` …
        // `###< symfony/mailer ###`.
        ctx.contribute(EnvSections::Fenced { open: "###> ", close: "###< ", trail: " ###" });
        // Its configuration reads variables as `%env(int:X)%` (YAML, PHP), its services as
        // `#[Autowire(env: 'X')]` and `env('X')`; it loads `.env`, `.env.local`,
        // `.env.{APP_ENV}`, `.env.{APP_ENV}.local`.
        for reads in ENV_READS {
            ctx.contribute(reads);
        }
        ctx.contribute(EnvFileOrder {
            name: "Symfony",
            markers: &["symfony.lock", "config/bundles.php"],
            files: &[".env", ".env.local", ".env.{env}", ".env.{env}.local"],
        });
        Ok(())
    }
}

#[derive(Debug)]
pub struct Symfony;

impl Framework for Symfony {
    fn id(&self) -> &'static str {
        "symfony"
    }

    fn name(&self) -> &'static str {
        "Symfony"
    }

    fn family(&self) -> Option<&'static str> {
        Some("php")
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let composer = project.read_relative("composer.json").unwrap_or_default();
        if ["symfony/framework-bundle", "symfony/symfony"]
            .iter()
            .any(|d| has_dependency(&composer, d))
        {
            return Some("composer.json".into());
        }
        (project.read_relative("bin/console").is_some_and(|c| c.contains("Symfony")))
            .then(|| "bin/console".into())
    }

    fn activate(&self, _project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Support::default())
    }
}

/// Where a string names a template or a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Template,
    Route,
    Command,
}

/// Which kind of name the string at `content` holds, from the code before it.
fn slot(text: &str, content: &std::ops::Range<usize>) -> Option<Slot> {
    let before = before_string(text, content);
    const TEMPLATES: &[&str] = &[
        "render(",
        "renderView(",
        "renderBlock(",
        "renderBlockView(",
        "stream(",
        "Template(",
        "include(",
        "source(",
        "{% extends",
        "{% include",
        "{% embed",
        "{% import",
        "{% from",
        "{% use",
        "{%- extends",
        "{%- include",
        "{%- embed",
    ];
    // `$application->find('app:x')`, `$tester->execute(['command' => 'app:x'])`.
    const COMMANDS: &[&str] = &["->find(", "'command' =>", "\"command\" =>"];
    if TEMPLATES.iter().any(|t| before.ends_with(t)) {
        return Some(Slot::Template);
    }
    if COMMANDS.iter().any(|c| before.ends_with(c)) {
        return Some(Slot::Command);
    }
    routes::USES.iter().any(|r| before.ends_with(r)).then_some(Slot::Route)
}

/// The kind of name a route's is ([`FrameworkName::kind`]).
const ROUTE: &str = "route";

/// The document's path relative to the project's root, with `/`.
fn relative(at: &FrameworkContext<'_>) -> Option<String> {
    let relative = at.path.strip_prefix(at.project.root?).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// The route declared at `offset`: `name: '…'` in a `#[Route(…)]`, a route file's key.
fn declaration_at(relative: &str, text: &str, offset: usize) -> Option<std::ops::Range<usize>> {
    let offset = text.floor_char_boundary(offset.min(text.len()));
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[offset..].find('\n').map_or(text.len(), |i| offset + i);
    let line = &text[line_start..line_end];
    if routes::is_route_file(relative) {
        // A key at the line's start: `blog_show:`.
        let key =
            line.strip_suffix(':').filter(|k| !k.is_empty() && !k.starts_with([' ', '\t', '#']))?;
        return (offset <= line_start + key.len()).then_some(line_start..line_start + key.len());
    }
    let content = quoted_at(text, offset)?;
    let before = before_string(text, &content);
    (before.ends_with("name:") && line.contains("Route(")).then_some(content)
}

/// The command name at `offset` after `…/console ` on its line (`bin/console app:x --y`):
/// its range (what's typed of it so far counts too); options (`-x`) aren't one.
fn console_command_at(text: &str, offset: usize) -> Option<std::ops::Range<usize>> {
    let offset = text.floor_char_boundary(offset);
    let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let head = &text[line_start..offset];
    let after = head.rfind("console ")? + "console ".len();
    let before = &head[..after - "console ".len()];
    // The console program run by its path (`bin/console`, `./bin/console`,
    // `php bin/console`), never the word in prose (`open the console and…`).
    if !before.ends_with(['/', '\\']) {
        return None;
    }
    let start = line_start + after + (head[after..].len() - head[after..].trim_start().len());
    let token_end = |c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '`' | ';' | '&' | '|');
    if text[start..offset].contains(token_end) || text[start..].starts_with('-') {
        return None;
    }
    let end = text[start..].find(token_end).map_or(text.len(), |e| start + e);
    Some(start..end)
}

/// The project's index table of its services' classes, by id (`$container->get('id')`).
const SERVICES: &str = "symfony.services";

/// The files Symfony's model is made from: its routes and commands (`src/`, `config/`), its
/// templates; nothing else changes it.
const WATCHES: &[&str] = &[
    "src/**/*.php",
    "config/**",
    "templates/**",
    "translations/**",
    "var/cache/dev/*Container.xml",
];

/// What Symfony gathers from one project, recomputed when its files change (its own
/// generation: [`WATCHES`]).
/// Reads the options of the project's bundles from its console.
pub const READ_REFERENCE: command_model::CommandId =
    command_model::CommandId("symfony.read_config_reference");

/// The options read from the project's console, as of the file's date.
type KeptReference = (Option<std::time::SystemTime>, Arc<config_reference::Reference>);

#[derive(Debug, Default)]
struct Support {
    templates: SharedMemo<u64, Arc<Vec<String>>>,
    /// The PHP files under `src` (listed again only when files are made or removed).
    sources: SharedMemo<u64, Arc<Vec<String>>>,
    routes: SharedMemo<u64, Arc<Vec<Route>>>,
    /// Each controller's routes (re-read only when its file changes).
    files: Mutex<Option<routes::FileRoutes>>,
    commands: SharedMemo<u64, Arc<Vec<Command>>>,
    /// Its services, parameters and translations (by [`catalog::Named`]).
    catalogs: [Mutex<Option<catalog::Built>>; 3],
    /// Each class's commands (re-read only when its file changes).
    command_files: Mutex<Option<commands::FileCommands>>,
    /// What Twig gets from Symfony.
    twig: twig::Twig,
    reference: Mutex<Option<KeptReference>>,
}

impl Support {
    /// The suggestions of a YAML file of the configuration, if `at` is in one.
    fn complete_config(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        let relative = relative(at).filter(|r| config_yaml::is_config(r))?;
        let (place, replace) = config_yaml::place_at(at.text, at.offset)?;
        let query = &at.text[replace.clone()];
        let named = |kind: catalog::Named| {
            let known = self.catalog(at, kind);
            let mut items = suggest(
                query,
                known.all.iter().map(|d| d.name.as_str()),
                CompletionKind::Value,
                None,
            );
            for item in &mut items {
                item.detail = known.get(&item.label).and_then(|d| d.detail.clone());
            }
            items
        };
        match place {
            config_yaml::Place::Service => {
                config_yaml::answer(replace, named(catalog::Named::Service), false)
            }
            config_yaml::Place::Parameter => {
                config_yaml::answer(replace, named(catalog::Named::Parameter), false)
            }
            config_yaml::Place::Key { nested } => {
                let file = config_yaml::File::of(&relative, routes::is_route_file(&relative));
                let parent = config_yaml::parent_at(at.text, replace.start);
                let mut items = config_yaml::keys(file, nested, parent, query);
                if file == config_yaml::File::Package
                    && let Some(path) = config_reference::path_at(at.text, replace.start)
                {
                    let read = self.reference(at).suggestions(&path, query);
                    items = code_intelligence::servers::merge_completions(read, items);
                }
                config_yaml::answer(replace, items, true)
            }
        }
    }

    /// The options read from the console, read again only when its file changed.
    fn reference(&self, at: &FrameworkContext<'_>) -> Arc<config_reference::Reference> {
        let Some(root) = at.project.root else { return Arc::default() };
        let file = config_reference::file_of(root);
        let modified = (at.project.modified)(&file);
        let mut kept = self.reference.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, reference)) = kept.as_ref().filter(|(m, _)| *m == modified) {
            return Arc::clone(reference);
        }
        let text = (at.project.read)(&file).unwrap_or_default();
        let reference = Arc::new(config_reference::Reference::parse(&text));
        *kept = Some((modified, Arc::clone(&reference)));
        reference
    }

    fn templates(&self, at: &FrameworkContext<'_>) -> Arc<Vec<String>> {
        self.templates
            .get_or_make(at.own, || Arc::new(at.project.files_under("templates", &[".twig"], 5000)))
    }

    fn sources(&self, at: &FrameworkContext<'_>) -> Arc<Vec<String>> {
        self.sources
            .get_or_make(at.own, || Arc::new(at.project.files_under("src", &[".php"], 5000)))
    }

    fn routes(&self, at: &FrameworkContext<'_>) -> Arc<Vec<Route>> {
        self.routes.get_or_make(at.own, || {
            let sources = self.sources(at);
            let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
            let files = files.get_or_insert_with(|| routes::FileRoutes::new(10_000));
            Arc::new(routes::gather(at.project, &sources, files))
        })
    }

    fn commands(&self, at: &FrameworkContext<'_>) -> Arc<Vec<Command>> {
        self.commands.get_or_make(at.own, || {
            let mut files = self.command_files.lock().unwrap_or_else(|e| e.into_inner());
            let files = files.get_or_insert_with(|| commands::FileCommands::new(10_000));
            Arc::new(commands::gather(at.project, &self.sources(at), files))
        })
    }

    /// The names of `kind` the project declares: made again only when the files they come
    /// from changed (their [`catalog::Print`]) — not at every change Symfony hears (a
    /// controller saved doesn't read the compiled container again).
    fn catalog(&self, at: &FrameworkContext<'_>, kind: catalog::Named) -> Arc<catalog::Catalog> {
        let mut slot = self.catalogs[kind as usize].lock().unwrap_or_else(|e| e.into_inner());
        if let Some(built) = slot.as_ref().filter(|b| b.own == at.own) {
            return Arc::clone(&built.catalog);
        }
        let print = catalog::Print::of(at.project, kind, at.structure);
        if let Some(built) = slot.as_mut().filter(|b| b.print == print) {
            built.own = at.own;
            return Arc::clone(&built.catalog);
        }
        let catalog = Arc::new(catalog::gather(at.project, kind, &self.sources(at)));
        *slot = Some(catalog::Built { own: at.own, print, catalog: Arc::clone(&catalog) });
        catalog
    }

    /// The route named at the cursor: in a use (`path('x')`) or where it's declared
    /// (`#[Route(name: 'x')]`, a route file's key). One named through its class's prefix
    /// isn't (its whole name is written nowhere: it can't be renamed as one).
    fn route_at(&self, at: &FrameworkContext<'_>) -> Option<FrameworkName> {
        let relative = relative(at)?;
        let range = match Self::slot_at(at) {
            Some((content, Slot::Route)) => content,
            _ => declaration_at(&relative, at.text, at.offset)?,
        };
        let name = at.text[range.clone()].to_owned();
        let routes = self.routes(at);
        let route = routes.iter().find(|r| r.name == name)?;
        // Its declaration writes it whole (no class prefix before it).
        let declared = (at.project.read)(&route.file).unwrap_or_default();
        declared[route.offset.min(declared.len())..].starts_with(&name).then_some(())?;
        Some(FrameworkName { kind: ROUTE, name, range })
    }

    /// The string at the cursor and what it names; else a command run by the console
    /// (`bin/console app:x` in `composer.json`'s scripts, a shell script, a Makefile).
    fn slot_at(at: &FrameworkContext<'_>) -> Option<(std::ops::Range<usize>, Slot)> {
        let named = quoted_at(at.text, at.offset)
            .and_then(|content| slot(at.text, &content).map(|slot| (content, slot)));
        named.or_else(|| console_command_at(at.text, at.offset).map(|r| (r, Slot::Command)))
    }
}

impl FrameworkSupport for Support {
    fn watches(&self) -> &'static [&'static str] {
        WATCHES
    }

    fn name_at(&self, at: &FrameworkContext<'_>) -> Option<FrameworkName> {
        self.route_at(at)
    }

    /// Routes are used in PHP and Twig, declared in `src/` attributes and route files.
    fn names_in(&self, kind: &str, relative: &str) -> bool {
        kind == ROUTE
            && (relative.ends_with(".php")
                || relative.ends_with(".twig")
                || routes::is_route_file(relative))
    }

    fn name_places(
        &self,
        kind: &str,
        name: &str,
        relative: &str,
        text: &str,
    ) -> Vec<std::ops::Range<usize>> {
        if kind != ROUTE {
            return Vec::new();
        }
        let mut places: Vec<_> =
            routes::uses(text).into_iter().filter(|r| &text[r.clone()] == name).collect();
        places.extend(routes::declarations(relative, text, name));
        places.sort_by_key(|r| r.start);
        places.dedup();
        places
    }

    fn name_taken(&self, kind: &str, name: &str) -> Vec<Location> {
        if kind != ROUTE {
            return Vec::new();
        }
        let known = self.routes.value().unwrap_or_default();
        known
            .iter()
            .filter(|r| r.name == name)
            .map(|r| Location { path: r.file.clone(), offset: r.offset, line: r.line })
            .collect()
    }

    /// A route used that no attribute nor route file declares (only when the project's
    /// routes are known: some are); a service or a parameter the compiled container doesn't
    /// have (only from it: `services.yaml` doesn't list the bundles'); a translation key no
    /// translation file has (one written as a key: no spaces).
    fn diagnostics(&self, at: &FrameworkContext<'_>) -> Vec<FrameworkProblem> {
        let warn = |range: std::ops::Range<usize>, key: &str| FrameworkProblem {
            message: i18n::tr!(key, name = &at.text[range.clone()]),
            range,
            level: ProblemLevel::Warning,
        };
        let mut problems = Vec::new();
        let used = routes::uses(at.text);
        if !used.is_empty() {
            let routes = self.routes(at);
            if !routes.is_empty() {
                problems.extend(
                    used.into_iter()
                        .filter(|r| !routes.iter().any(|route| route.name == at.text[r.clone()]))
                        .map(|r| warn(r, "symfony.no_route")),
                );
            }
        }
        for kind in catalog::Named::ALL {
            let named = kind.uses(at.text);
            if named.is_empty() {
                continue;
            }
            let known = self.catalog(at, kind);
            if !known.complete {
                continue;
            }
            problems.extend(
                named
                    .into_iter()
                    .filter(|r| kind.checked(&at.text[r.clone()]))
                    .filter(|r| known.get(&at.text[r.clone()]).is_none())
                    .map(|r| warn(r, kind.missing_key())),
            );
        }
        problems.sort_by_key(|p| p.range.start);
        problems
    }

    /// Its services' classes, by id (those whose class is known).
    fn named_types(
        &self,
        at: &FrameworkContext<'_>,
    ) -> Vec<(&'static str, std::collections::HashMap<String, String>)> {
        let services = self.catalog(at, catalog::Named::Service);
        let classes =
            services.all.iter().filter_map(|s| Some((s.name.clone(), s.detail.clone()?))).collect();
        vec![(SERVICES, classes)]
    }

    /// Its templates, routes and commands, gathered ahead (the UI thread finds them made).
    fn prepare(&self, at: &FrameworkContext<'_>) {
        self.templates(at);
        self.routes(at);
        self.commands(at);
        self.twig.prepare(at, &self.sources(at));
        for kind in catalog::Named::ALL {
            self.catalog(at, kind);
        }
    }

    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if let Some((kind, name)) = catalog::named_at(at.text, at.offset) {
            let replace = name.start..at.offset;
            let known = self.catalog(at, kind);
            let mut items = suggest(
                &at.text[replace.clone()],
                known.all.iter().map(|d| d.name.as_str()),
                CompletionKind::Value,
                None,
            );
            for item in &mut items {
                item.detail = known.get(&item.label).and_then(|d| d.detail.clone());
            }
            return (!items.is_empty()).then_some(FrameworkCompletion {
                replace,
                items,
                merge: false,
            });
        }
        if let Some(answer) = self.twig.complete(at).or_else(|| self.complete_config(at)) {
            return Some(answer);
        }
        let (content, slot) = Self::slot_at(at)?;
        let replace = content.start..at.offset;
        let query = &at.text[replace.clone()];
        let items = match slot {
            Slot::Template => {
                let templates = self.templates(at);
                suggest(
                    query,
                    templates.iter().map(String::as_str),
                    CompletionKind::Value,
                    Some("template"),
                )
            }
            Slot::Route => {
                let routes = self.routes(at);
                let mut items = suggest(
                    query,
                    routes.iter().map(|r| r.name.as_str()),
                    CompletionKind::Value,
                    None,
                );
                for item in &mut items {
                    item.detail =
                        routes.iter().find(|r| r.name == item.label).and_then(|r| r.path.clone());
                }
                items
            }
            Slot::Command => {
                let commands = self.commands(at);
                let mut items = suggest(
                    query,
                    commands.iter().map(|c| c.name.as_str()),
                    CompletionKind::Value,
                    None,
                );
                for item in &mut items {
                    item.detail = commands
                        .iter()
                        .find(|c| c.name == item.label)
                        .and_then(|c| c.description.clone());
                }
                items
            }
        };
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn definition(&self, at: &FrameworkContext<'_>) -> Option<Location> {
        if let Some((kind, name)) = catalog::named_at(at.text, at.offset) {
            let known = self.catalog(at, kind);
            let declared = known.get(&at.text[name])?;
            let (path, offset, line) = (declared.file.clone(), declared.offset, declared.line);
            return Some(Location { path, offset, line });
        }
        let (content, slot) = Self::slot_at(at)?;
        let name = &at.text[content];
        match slot {
            Slot::Template => {
                // Known from the list (no file read to see that it's there).
                self.templates(at).iter().any(|t| t == name).then(|| {
                    let path = at.project.root.map(|r| r.join("templates").join(name));
                    path.map(|path| Location { path, offset: 0, line: 0 })
                })?
            }
            Slot::Route => {
                let routes = self.routes(at);
                let route = routes.iter().find(|r| r.name == name)?;
                Some(Location { path: route.file.clone(), offset: route.offset, line: route.line })
            }
            Slot::Command => {
                let commands = self.commands(at);
                let command = commands.iter().find(|c| c.name == name)?;
                let (path, offset, line) = (command.file.clone(), command.offset, command.line);
                Some(Location { path, offset, line })
            }
        }
    }

    fn hover(&self, at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        if let Some((kind, name)) = catalog::named_at(at.text, at.offset) {
            let known = self.catalog(at, kind);
            let declared = known.get(&at.text[name.clone()])?;
            let code = match &declared.detail {
                Some(detail) => {
                    format!("{} {} {} {detail}", kind.word(), declared.name, kind.joiner())
                }
                None => format!("{} {}", kind.word(), declared.name),
            };
            let doc = declared.doc.clone();
            return Some(HoverInfo { range: name, code, doc, ..Default::default() });
        }
        let (content, slot) = Self::slot_at(at)?;
        if slot == Slot::Command {
            let commands = self.commands(at);
            let command = commands.iter().find(|c| c.name == at.text[content.clone()])?;
            let code = format!("command {}", command.name);
            let doc = command.description.clone();
            return Some(HoverInfo { range: content, code, doc, ..Default::default() });
        }
        if slot != Slot::Route {
            return None;
        }
        let routes = self.routes(at);
        let route = routes.iter().find(|r| r.name == at.text[content.clone()])?;
        let code = match &route.path {
            Some(path) => format!("route {} → {path}", route.name),
            None => format!("route {}", route.name),
        };
        Some(HoverInfo { range: content, code, notes: Vec::new(), doc: None, ..Default::default() })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
