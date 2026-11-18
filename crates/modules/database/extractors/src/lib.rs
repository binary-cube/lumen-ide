//! Data extractors, as a module (plan 39o): what every grid copies and exports with is the
//! catalog of [`extractors`] (the built-in ones, those the modules contribute, a project's
//! templates); this module keeps what the user chooses of it — the extractor a project's
//! grids use (chosen in their "CSV ▾" popover, kept in the project's own settings), "Skip
//! Computed / Skip Generated Columns" and the separated formats (kept in the settings,
//! edited in "Configure CSV Formats…": [`csv_formats`]) — and hands it to the catalog
//! ([`settings::apply`], [`ExtractorsService`]). Nothing runs until a grid asks: the catalog
//! is plain data.
//!
//! The user's own extractors (39o2) are templates ([`templates`]: `*.extractor` files in the
//! project's `<project dir>/extractors/` and in the config folder's `extractors/`, read in the
//! background and again when they change; a few [`examples`] the first time "Go to Extractors
//! Directory" makes the project's folder), their files a language of their own — colored and
//! completed ([`language`]).

pub mod csv_formats;
pub mod examples;
pub mod language;
pub mod settings;
pub mod templates;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::Subscription;
use workbench_model::{
    CommandContribution, CommandHandler, ProjectContext, ProjectService, SettingsContribution,
    WorkbenchHost,
};

/// The module's id.
pub const MODULE_ID: &str = "database.extractors";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Data extractors",
    description: "What a grid's rows become when copied or exported (SQL INSERTs and UPDATEs, a WHERE clause, CSV and the user's separated formats, JSON, Markdown, HTML, XML, an aligned table, Excel), the one chosen kept per project",
    depends_on: &[],
};

/// The project's choice and the skips saved (a grid's popover runs it).
pub const SAVE: CommandId = CommandId(extractors::SAVE_COMMAND);
/// "Configure CSV Formats…".
pub const CONFIGURE_CSV: CommandId = CommandId(extractors::CONFIGURE_CSV_COMMAND);
/// "Go to Extractors Directory".
pub const OPEN_DIRECTORY: CommandId = CommandId(extractors::OPEN_DIRECTORY_COMMAND);

#[derive(Debug, Default)]
pub struct DataExtractorsModule;

/// A command: its id, its name, its icon, what runs it.
type Spec = (CommandId, &'static str, Option<&'static str>, Arc<dyn CommandHandler>);

/// [`SAVE`]: what the catalog holds now saved — the skips in the settings, the project's
/// choice in its own.
#[derive(Debug, Default)]
pub struct SaveCommand;

impl CommandHandler for SaveCommand {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        save(host)
    }
}

/// See [`SaveCommand`].
pub fn save(host: &mut dyn WorkbenchHost) -> bool {
    let catalog = extractors::catalog();
    if let Some(user) = host.user_settings() {
        let options = catalog.options();
        settings::save(user, &settings::SKIP_COMPUTED, options.skip_computed);
        settings::save(user, &settings::SKIP_GENERATED, options.skip_generated);
    }
    if let (Some(project), Some(own)) = (host.project_id(), host.project_settings())
        && let Some(chosen) = catalog.chosen_id(Some(project))
    {
        settings::save(own, &settings::CHOSEN, chosen);
    }
    true
}

/// [`CONFIGURE_CSV`]: the formats' modal opened.
#[derive(Debug, Default)]
pub struct ConfigureCommand;

impl CommandHandler for ConfigureCommand {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        csv_formats::ask(host)
    }
}

/// [`OPEN_DIRECTORY`]: the project's extractors' folder opened (made the first time, with
/// the examples), its first template shown in the editor.
#[derive(Debug, Default)]
pub struct OpenDirectoryCommand;

impl CommandHandler for OpenDirectoryCommand {
    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        open_directory(host)
    }
}

/// See [`OpenDirectoryCommand`]: the folder made (with examples) if missing and its first
/// template opened — what's on disk looked at and written in the background (a click never
/// reads the templates), what the user is told and shown once it's done.
pub fn open_directory(host: &mut dyn WorkbenchHost) -> bool {
    use workbench_model::Notification;
    let Some(root) = host.project_root() else {
        host.notify(Notification::info(i18n::t("extract.open_directory_no_project")));
        return false;
    };
    let dir = templates::own_dir(&root);
    let project = host.project_id();
    host.spawn(Box::new(move || {
        let made = (!dir.exists()).then(|| examples::write(&dir).map(|w| w.len()));
        let first = templates::read_dir(&dir, templates::Origin::Project)
            .into_iter()
            .find_map(|t| extractors::Extractor::origin(&t).map(std::path::Path::to_path_buf));
        Box::new(move |host: &mut dyn WorkbenchHost| {
            opened_directory(host, &dir, made, first, project)
        })
    }));
    true
}

/// What [`open_directory`] found: the folder made (how many examples written, or why not),
/// its first template.
fn opened_directory(
    host: &mut dyn WorkbenchHost,
    dir: &std::path::Path,
    made: Option<std::io::Result<usize>>,
    first: Option<std::path::PathBuf>,
    project: Option<kernel::ProjectId>,
) {
    use workbench_model::Notification;
    match made {
        Some(Ok(count)) => host.notify(Notification::success(i18n::tr!(
            "extract.open_directory_made",
            count = count,
            path = dir.display().to_string()
        ))),
        Some(Err(e)) => {
            host.notify(Notification::error(i18n::tr!(
                "extract.open_directory_failed",
                why = e.to_string()
            )));
            return;
        }
        None => {}
    }
    if let Some(parent) = dir.parent() {
        host.relist(parent);
    }
    host.relist(dir);
    if let Some(templates) = project.and_then(templates::of) {
        templates.reload();
    }
    match first {
        Some(path) => {
            host.open_file(&path);
        }
        None => host.notify(Notification::info(i18n::tr!(
            "extract.open_directory_empty",
            path = dir.display().to_string()
        ))),
    }
}

/// Where the templates every project shares are looked for.
#[derive(Debug, Clone, Default)]
pub enum UserDir {
    /// The config folder's `extractors/`.
    #[default]
    Config,
    /// This folder (the tests').
    At(std::path::PathBuf),
    None,
}

/// Each project's choice handed to the catalog when it opens (from its own settings), and
/// its templates read and followed; let go with it.
#[derive(Debug, Default)]
pub struct ExtractorsService {
    pub user: UserDir,
}

impl ExtractorsService {
    fn user_dir(&self) -> Option<std::path::PathBuf> {
        match &self.user {
            UserDir::Config => templates::user_dir(),
            UserDir::At(dir) => Some(dir.clone()),
            UserDir::None => None,
        }
    }
}

impl ProjectService for ExtractorsService {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        let id = project.id;
        if let Some(own) = &project.settings
            && own.is_set(&settings::CHOSEN)
        {
            let chosen = own.get(&settings::CHOSEN);
            if !chosen.is_empty() {
                extractors::catalog().choose(Some(id), &chosen);
            }
        }
        let user = self.user_dir();
        let templates =
            templates::Templates::new(id, project.root, user.clone(), project.events.clone());
        templates.register();
        templates.reload();
        let mut subscriptions = templates.follow();
        if let Some(dir) = &user
            && let Some(watch) = templates::watch_user_dir(dir)
        {
            subscriptions.push(watch);
        }
        subscriptions.push(Subscription::on_end(move || {
            drop(templates);
            extractors::catalog().forget(id);
        }));
        subscriptions
    }

    fn busy(&self, id: kernel::ProjectId) -> bool {
        templates::of(id).is_some_and(|t| t.busy())
    }

    /// The templates in the project's own folder are the user's: their changes are heard.
    fn own_dirs(&self) -> &'static [&'static str] {
        &[templates::DIR]
    }
}

impl Module for DataExtractorsModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleThreads(&["extractors"]));
        ctx.contribute::<Arc<dyn SettingsContribution>>(Arc::new(settings::ExtractorSettings));
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(ExtractorsService::default()));
        // `*.extractor` files: a language of their own, colored and completed.
        ctx.contribute(language::descriptor());
        ctx.contribute(language::grammar());
        ctx.contribute::<Arc<dyn code_intelligence::LanguageIntelligence>>(Arc::new(
            language::TemplateIntelligence,
        ));
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            language::TemplateAssist,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            csv_formats::FormatsProvider,
        ));
        let commands: [Spec; 4] = [
            (SAVE, "Save Data Extractor", None, Arc::new(SaveCommand)),
            (CONFIGURE_CSV, "Configure CSV Formats…", Some("table"), Arc::new(ConfigureCommand)),
            (csv_formats::GIVEN, "CSV Formats Given", None, Arc::new(csv_formats::GivenCommand)),
            (
                OPEN_DIRECTORY,
                "Go to Extractors Directory",
                Some("folder-open"),
                Arc::new(OpenDirectoryCommand),
            ),
        ];
        for (id, label, icon, handler) in commands {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts: &[] },
                icon,
                handler,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/memory.rs"]
mod memory;
