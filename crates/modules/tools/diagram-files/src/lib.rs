//! The project's diagrams: the Diagrams window lists the `.drawio` files kept in
//! `<project dir>/diagrams/` (folders, renames, moves, deletions), New Diagram makes an empty
//! one there. A diagram is opened as any file is: by whatever opens `.drawio` (draw.io, or
//! the diagram editor while it's shown), never named here.

mod create;
mod window;
mod window_actions;

use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Dock, ToolContent, ToolPanel, ToolWindowProvider};

/// The diagrams' files.
pub const EXTENSION: &str = "drawio";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tool.diagram-files"),
    name: "Diagrams",
    description: "The project's diagrams (.drawio): listed, made, renamed and moved",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "graph", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct DiagramFilesModule;

impl Module for DiagramFilesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleThreads(&["diagram-list"]));
        ctx.contribute(languages::IconSvg {
            name: "diagram",
            svg: include_str!("../svg/diagram.svg"),
        });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(Window));
        ctx.contribute::<Arc<dyn workbench_model::ProjectService>>(Arc::new(OwnFolder));
        // New Diagram (in the command palette and the Diagrams window's header).
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: NEW_DIAGRAM,
                label: "New Diagram",
                // The File menu is full: the palette and the Diagrams window offer it.
                menu: None,
                shortcuts: &[],
            },
            icon: Some("diagram"),
            handler: Arc::new(NewDiagram),
        });
        Ok(())
    }
}

/// New Diagram: an empty diagram in the project's diagrams folder, opened.
pub const NEW_DIAGRAM: command_model::CommandId = command_model::CommandId("diagram.new");

#[derive(Debug)]
struct NewDiagram;

impl workbench_model::CommandHandler for NewDiagram {
    fn requires(&self) -> &'static [command_model::Requires] {
        &[command_model::Requires::Project]
    }

    /// Made on a worker, opened when it's there.
    fn run(&self, host: &mut dyn workbench_model::WorkbenchHost) -> bool {
        let Some(root) = host.project_root() else { return false };
        host.spawn(Box::new(move || {
            let made = create::new_diagram(&root, &create::folder(&root));
            Box::new(move |host: &mut dyn workbench_model::WorkbenchHost| match made {
                Ok(path) => {
                    host.open_file(&path);
                }
                Err(e) => host.notify(workbench_model::Notification::error(i18n::tr!(
                    "diagram_files.cant_create",
                    reason = e.to_string()
                ))),
            })
        }));
        true
    }
}

/// The Diagrams window: a panel on the right, its button the last of the strip's top ones
/// (after the database's).
#[derive(Debug)]
struct Window;

impl ToolWindowProvider for Window {
    fn id(&self) -> &'static str {
        "diagram-files"
    }

    fn icon(&self) -> &'static str {
        "diagram"
    }

    fn title(&self) -> String {
        i18n::t("diagram_files.window")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        45
    }

    /// Its group on the strip: the project's documents.
    fn group(&self) -> &'static str {
        "documents"
    }

    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(window::DiagramsPanel::default()))
    }
}

/// The diagrams' folder in the project's own folder: the user's files (the watcher hears
/// them as the project's).
#[derive(Debug)]
struct OwnFolder;

impl workbench_model::ProjectService for OwnFolder {
    fn attach(&self, _project: &workbench_model::ProjectContext<'_>) -> Vec<kernel::Subscription> {
        Vec::new()
    }

    fn own_dirs(&self) -> &'static [&'static str] {
        &[window::FOLDER]
    }
}
