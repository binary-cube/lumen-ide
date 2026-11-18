//! Language servers, as a module: the servers the languages' modules describe as data
//! (`[[language-servers]]` in their `language-servers.toml`, see `lsp_client::spec`) or
//! the user adds (`<config folder>/language-servers.toml`, or "Add a server" in the
//! settings), bundled with a full build of the app (`lsp/` beside it: Node and the servers)
//! or downloaded once when first needed (`downloads`: the bar over a file, `bar`; the
//! settings' list of what's downloaded, `parts`).
//!
//! - Per project (`projects`): a server starts at the first document of its language whose
//!   engine is a server's, one per server and root; it stops when unused, when its
//!   language's engine stops being its, and when the project closes. Nothing starts for a
//!   project without documents of a served language.
//! - The app asks through the Intelligence port (`port`): completion, hover, signature help,
//!   definition, references, rename's places, highlights, symbols; the built-in engine
//!   answers while no server is ready.
//! - Its diagnostics are announced on the project's bus; the commands (`commands`) run its
//!   code actions and its formatting; the Task Manager lists each server (`report`) with
//!   Restart, Stop, Show Log (`log_view`) and Settings; the settings (`page`, `settings`)
//!   choose each language's engine.

mod bar;
mod catalog;
mod commands;
pub mod downloads;
mod edits;
mod engines;
pub mod install;
pub mod licence;
mod log_view;
mod page;
mod parts;
mod port;
pub mod projects;
mod report;
pub mod settings;

use std::sync::Arc;

use command_model::{CommandSpec, MenuAt, MenuSectionSpec, SubmenuSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::Subscription;
use workbench_model::menus::sections;
use workbench_model::{CommandContribution, ProjectContext, ProjectService, WorkbenchHost};

pub use bar::{DownloadBar, dismiss as dismiss_offer, offer};
pub use catalog::{Catalog, USER_FILE, use_bundle, use_user_file};
pub use downloads::{use_downloads, use_manifest};
pub use install::{use_fetch, use_install_dir, wait_known};
pub use licence::{use_licence_files, use_secrets};
pub use log_view::{EXTENSION as LOG_EXTENSION, LogData};
pub use projects::{idle_after, launch_with, request_timeout};

/// The app's own Node (bundled beside it, else downloaded for the servers): what a
/// project's Node tools (its ESLint, its Prettier) run on when the system has no Node.
/// Looked up on the caller's thread (it reads the disk): never the UI's.
pub fn node_program() -> Option<std::path::PathBuf> {
    catalog::bundle().and_then(|b| b.node()).or_else(downloads::node)
}

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lsp"),
    name: "Language Servers",
    description: "Language servers (LSP) for the languages that describe one, downloaded once when first needed (or bundled with a full build): started per project when a file of their language opens, stopped when unused, shown in the Task Manager",
    depends_on: &[],
};

/// A command: its id, label, shortcuts, icon and handler.
type Command = (
    command_model::CommandId,
    &'static str,
    &'static [&'static str],
    &'static str,
    Arc<dyn workbench_model::CommandHandler>,
    Option<MenuAt>,
);

/// Tools › Language Servers: the module's submenu, and its sections.
const SERVERS_MENU: &str = "lsp.servers";
const ENGINE: &str = "lsp.engine";
const DOWNLOADS: &str = "lsp.downloads";
const SERVERS: &str = "lsp.servers.running";

#[derive(Debug, Default)]
pub struct LanguageServersModule;

impl Module for LanguageServersModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // The servers' threads (their drivers, readers, writers): their CPU counts here.
        ctx.contribute(workbench_model::ModuleThreads(&["lsp-"]));
        // The servers an earlier run left running (it ended without stopping them: a crash,
        // killed): stopped, off the UI thread (a look at every process).
        if let Some(root) = downloads::root() {
            kernel::work::run("lsp-left-behind", move || {
                lsp_client::orphans::stop_left_behind(&root);
            });
        }
        ctx.contribute::<Arc<dyn ProjectService>>(Arc::new(Service));
        ctx.contribute::<Arc<dyn code_intelligence::servers::LanguageServers>>(Arc::new(
            port::Servers,
        ));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(report::Reporter));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            page::ServerSettings::default(),
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            parts::DownloadedParts::default(),
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            page::AddServer::default(),
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            page::ServerStatus,
        ));
        ctx.contribute::<Arc<dyn workbench_model::SettingsContribution>>(Arc::new(
            page::ServerLimits,
        ));
        // The offer to download a file's server, over its editor.
        ctx.contribute::<Arc<dyn ui_components::editor_toolbar::EditorToolbarProvider>>(Arc::new(
            bar::OfferProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(log_view::LogProvider));
        let commands: [Command; 6] = [
            (
                workbench_model::CHOOSE_ENGINE,
                "Choose the File's Language Engine…",
                &[],
                "server",
                Arc::new(engines::ChooseEngine),
                Some(MenuAt::new(ENGINE, 10)),
            ),
            (
                commands::FORMAT,
                "Format with Language Server",
                &["Shift+Alt+F"],
                "braces",
                Arc::new(commands::Format),
                Some(MenuAt::new(sections::CODE_FORMAT, 30)),
            ),
            (
                commands::RESTART,
                "Restart Language Servers",
                &[],
                "rotate-ccw",
                Arc::new(commands::Restart),
                Some(MenuAt::new(SERVERS, 10)),
            ),
            (
                commands::SHOW_LOG,
                "Show Language Server Log",
                &[],
                "file-text",
                Arc::new(commands::ShowLog),
                Some(MenuAt::new(SERVERS, 20)),
            ),
            (
                commands::DOWNLOAD,
                "Download the File's Language Server",
                &[],
                "download",
                Arc::new(commands::Download),
                Some(MenuAt::new(DOWNLOADS, 10)),
            ),
            (
                commands::USE_BUILT_IN,
                "Use the Built-in Engine for the File's Language",
                &[],
                "cpu",
                Arc::new(commands::UseBuiltIn),
                Some(MenuAt::new(ENGINE, 20)),
            ),
        ];
        // The bar's pick isn't a command of its own to run from a menu.
        for (id, label, shortcuts, icon, handler, menu) in commands {
            // The file's commands need a file open; restarting and the log, a project.
            let file = ![commands::RESTART, commands::SHOW_LOG].contains(&id);
            let requires: &'static [command_model::Requires] = if file {
                &[command_model::Requires::TextFile]
            } else {
                &[command_model::Requires::Project]
            };
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu, shortcuts },
                icon: Some(icon),
                handler: workbench_model::Requiring::new(requires, handler),
            });
        }
        ctx.contribute(SubmenuSpec {
            id: SERVERS_MENU,
            at: MenuAt::new(sections::TOOLS_LANGUAGES, 10),
            label_key: "menu.language_servers",
            label: "Language Servers",
            icon: Some("server"),
        });
        for (id, order) in [(ENGINE, 10), (DOWNLOADS, 20), (SERVERS, 30)] {
            ctx.contribute(MenuSectionSpec::new(id, SERVERS_MENU, order));
        }
        Ok(())
    }
}

/// Follows each project's documents; on the UI thread, makes the edits its servers asked
/// for.
#[derive(Debug)]
struct Service;

impl ProjectService for Service {
    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        // The shared folder looked after once, in the background (before any server starts
        // from it: they start at the documents, later).
        downloads::maintain(Some(Arc::clone(&project.wake)));
        projects::ProjectServers::attach(project).1
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(project) = host.project_id().and_then(projects::project) else { return false };
        let edits = project.take_edits();
        let any = !edits.is_empty();
        for (_, change) in edits {
            if !change.is_empty()
                && let Err(why) = host.apply_refactoring(&edits::plan(&change))
            {
                host.notify(workbench_model::Notification::error(i18n::tr!(
                    "lsp.not_applied",
                    reason = why
                )));
            }
        }
        any
    }
}

#[cfg(test)]
#[path = "../tests/unit/harness.rs"]
pub(crate) mod harness;
