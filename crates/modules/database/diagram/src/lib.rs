//! The relations diagram, as a module: a data source's tables as cards (their columns,
//! the key's marked, the types beside them) and the lines between them — from their foreign
//! keys, labelled with the columns joining them (`orders.user_id → users.id`) and how many
//! rows meet on each side (1–n, 1–1).
//!
//! The explorer, owning the sessions, opens it as a view with a
//! [`database::diagram::DiagramRequest`] (the domain's port to read the source's structure
//! and open a table's data); the two modules never depend on each other. The structure is
//! read in the background, the layout computed there too (once for each set of tables
//! shown: [`layout`]), followed as commits change it (the project's bus). Large schemas
//! stay usable: a table and its neighbors one to three links away, a filter by name, pan,
//! zoom, fit; only the cards in view are drawn. Click a card: it's centered (← goes
//! back); double click: its data opens; its menu shows its neighbors.

pub mod layout;
pub mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{CommandContribution, ViewAction};

pub use view::RelationsView;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("database.diagram"),
    name: "Relations Diagram",
    description: "A data source's tables and the relations between them, as a diagram",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "schema", ..extensions::ModuleInfo::BUILT_IN },
};

/// The view's kind (its viewer's id: where its keys work).
pub const VIEW: &str = "relations";

/// The pool its reads and layouts run on.
pub const THREAD: &str = "db-diagram";

#[derive(Debug, Default)]
pub struct DatabaseDiagramModule;

impl Module for DatabaseDiagramModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(workbench_model::ModuleThreads(&[THREAD]));
        ctx.contribute(languages::IconSvg {
            name: "relations",
            svg: include_str!("../svg/relations.svg"),
        });
        // Its tab and breadcrumb show it.
        ctx.contribute(languages::FileIconRule::new(
            languages::FileMatcher::Extension(database::diagram::VIEW_EXTENSION),
            "relations",
            "database",
        ));
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        for (id, label, shortcuts, action) in ACTIONS {
            ctx.contribute(CommandContribution {
                spec: CommandSpec { id, label, menu: None, shortcuts },
                icon: None,
                handler: Arc::new(ViewAction { view: VIEW, action }),
            });
        }
        Ok(())
    }
}

/// The view's actions (remappable): the command, its label, its keys, what the view is
/// sent.
pub const ACTIONS: [(CommandId, &str, &[&str], &str); 4] = [
    (CommandId("database.diagram_fit"), "Fit (Relations)", &["0"], "fit"),
    (CommandId("database.diagram_back"), "Back (Relations)", &["Backspace"], "back"),
    (CommandId("database.diagram_find"), "Find Table (Relations)", &["CmdOrCtrl+F"], "find"),
    (CommandId("database.diagram_refresh"), "Refresh (Relations)", &["F5"], "refresh"),
];

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension()
            .is_some_and(|e| e == database::diagram::VIEW_EXTENSION)
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RelationsView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
