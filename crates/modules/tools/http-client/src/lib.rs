//! The HTTP client as a module (plan 77d): `.http` files run — each
//! request's ▶ in the gutter, ⌘↵ the one at the cursor, the bar's Run All / Stop / "Run
//! with:" an environment ([`bar`]) —, their responses in the Services window ([`services`]:
//! a node per run, its Response tab — the status, the time, the size, the body readable and
//! colored, Open in Editor, Copy Body, Copy as cURL: [`response_view`]). A run ([`job`]) is
//! sent on the pool `http-run`: the variables resolved (the file's, the environment's), the
//! project's cookies, the response saved for the editor. What a request is, its transport,
//! its environments: the domain's `http-requests`, infra's `http-transport` and
//! `http-environments`; the
//! language (colors, completion) is another module's, never named here.

pub mod assist;
mod auth;
pub mod bar;
pub mod chain_view;
pub mod collection_view;
mod collections;
pub mod editor;
pub mod env_view;
pub mod format;
pub mod gitignore;
mod importing;
pub mod job;
mod page;
mod pending;
pub mod preview_view;
pub mod request_tree;
pub mod response_view;
pub mod runs;
pub mod services;
pub mod tasks;
pub mod window;

use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::editor_toolbar::{EditorToolbar, EditorToolbarProvider, ToolbarContext};
use workbench_model::{CommandContribution, CommandHandler, WorkbenchHost};

/// Runs the request at the cursor (⌘↵ in a `.http` file).
pub const RUN: CommandId = CommandId("http.run");
/// Runs every request of the file.
pub const RUN_ALL: CommandId = CommandId("http.run_all");

/// Shows the file's requests as a graph (their chain).
pub const GRAPH: CommandId = CommandId("http.graph");
/// The request at the cursor in the visual editor.
pub const EDIT: CommandId = CommandId("http.edit");

/// The files whose requests it runs.
const EXTENSIONS: &[&str] = &["http", "rest"];

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tools.http_client"),
    name: "HTTP client",
    description: "Runs the requests of .http files: environments, cookies, the responses in Services",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct HttpClientModule;

impl Module for HttpClientModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn EditorToolbarProvider>>(Arc::new(BarProvider));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            chain_view::ChainProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            preview_view::PreviewProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(editor::EditorProvider));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(env_view::EnvProvider));
        ctx.contribute::<Arc<dyn ui_components::ViewerProvider>>(Arc::new(
            collection_view::CollectionProvider,
        ));
        ctx.contribute::<Arc<dyn ui_components::ServiceProvider>>(Arc::new(services::HttpServices));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RUN,
                label: "Run HTTP Request",
                menu: None,
                shortcuts: &["CmdOrCtrl+Enter"],
            },
            icon: Some("play"),
            handler: Arc::new(OnRequestFile(false)),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: RUN_ALL,
                label: "Run All HTTP Requests",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("play"),
            handler: Arc::new(OnRequestFile(true)),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: GRAPH,
                label: "Show HTTP Requests Chain",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("graph"),
            handler: Arc::new(ShowGraph),
        });
        ctx.contribute(CommandContribution {
            spec: CommandSpec { id: EDIT, label: "Edit HTTP Request", menu: None, shortcuts: &[] },
            icon: Some("file-pen"),
            handler: Arc::new(EditAtCursor),
        });
        ctx.contribute::<Arc<dyn ui_components::ToolWindowProvider>>(Arc::new(window::HttpTool));
        // Its tab and breadcrumb show the button's globe.
        ctx.contribute(languages::FileIconRule::new(
            languages::FileMatcher::Extension(collection_view::EXTENSION),
            "globe",
            "config",
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: window::TOGGLE,
                label: "HTTP Client",
                // In View › Tool Windows: its tool window's row.
                menu: None,
                shortcuts: &[],
            },
            icon: Some("globe"),
            handler: Arc::new(window::Open),
        });
        ctx.contribute(workbench_model::ModuleThreads(&[
            job::THREAD,
            chain_view::THREAD,
            response_view::THREAD,
            window::THREAD,
        ]));
        ctx.contribute::<Arc<dyn code_intelligence::DocumentAssist>>(Arc::new(
            assist::ResponseCompletion,
        ));
        ctx.contribute::<Arc<dyn workbench_model::ResourceReporter>>(Arc::new(tasks::Reporter));
        Ok(())
    }
}

fn is_request_file(path: &std::path::Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e))
}

/// The bar above a request file.
#[derive(Debug)]
struct BarProvider;

impl EditorToolbarProvider for BarProvider {
    fn id(&self) -> &'static str {
        "http-requests"
    }

    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>> {
        if !is_request_file(context.path) {
            return None;
        }
        let runs = runs::Runs::of(context.project, context.root);
        Some(Box::new(bar::HttpBar::new(
            runs,
            context.path,
            context.root,
            Arc::clone(&context.wake),
        )))
    }
}

/// ⌘↵ (the request at the cursor) or Run All, on a request file only.
#[derive(Debug)]
struct OnRequestFile(bool);

impl CommandHandler for OnRequestFile {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some_and(|f| is_request_file(&f.path))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file().filter(|f| is_request_file(&f.path)) else {
            return false;
        };
        let runs = runs::Runs::of(host.project_id(), host.project_root().as_deref());
        let ran = if self.0 {
            job::run_all(&runs, &file.path, &file.text)
        } else {
            job::run_at(&runs, &file.path, &file.text, file.cursor)
        };
        if ran && workbench_model::presence::command(workbench_model::TOGGLE_SERVICES) {
            host.show_side_panel(workbench_model::SERVICES_TOOL);
        }
        ran
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage(lang_id())
    }
}

/// [`EDIT`]: the request at the cursor in the visual editor (the file written first, if
/// changed, so the editor reads what's shown).
#[derive(Debug)]
struct EditAtCursor;

impl CommandHandler for EditAtCursor {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some_and(|f| is_request_file(&f.path))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file().filter(|f| is_request_file(&f.path)) else {
            return false;
        };
        let parsed = http_requests::parse(&file.text);
        // On its `###` title too (before its block): the request it heads.
        let Some(index) = parsed
            .index_at(file.cursor)
            .or_else(|| parsed.requests.iter().position(|r| r.range.end >= file.cursor))
        else {
            return false;
        };
        host.save_active();
        let data = editor::EditorData {
            runs: runs::Runs::of(host.project_id(), host.project_root().as_deref()),
            file: file.path,
            index: Some(index),
        };
        host.open_view(&parsed.requests[index].label(), editor::EXTENSION, &data)
    }

    fn when(&self) -> command_model::When {
        command_model::When::EditorLanguage(lang_id())
    }
}

/// [`GRAPH`]: the active request file's chain, in a tab.
#[derive(Debug)]
struct ShowGraph;

impl CommandHandler for ShowGraph {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.active_file().is_some_and(|f| is_request_file(&f.path))
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(file) = host.active_file().filter(|f| is_request_file(&f.path)) else {
            return false;
        };
        let runs = runs::Runs::of(host.project_id(), host.project_root().as_deref());
        let name =
            file.path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let title = i18n::tr!("http.chain_title", file = name.as_str());
        let data = chain_view::ChainData { runs, file: file.path.clone(), text: file.text };
        host.open_view(&title, chain_view::EXTENSION, &data)
    }
}

/// The language id of request files (as their language module names it).
const fn lang_id() -> &'static str {
    "http"
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
