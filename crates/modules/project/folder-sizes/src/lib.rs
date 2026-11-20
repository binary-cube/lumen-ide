//! Folder sizes, as a module: a folder's right-click menu in the project's tree offers
//! "Analyze Folder Sizes", which opens an editor tab ([`view`]) with how much it holds,
//! folder by folder — a ring of the biggest, the list of them all to go down into — so
//! what takes room is found at a glance. Measured on a worker ([`scan`]: no links followed,
//! with limits), kept in the project's data folder with when it was ([`report`]), shown
//! from there the next time; "Refresh" measures again.
//!
//! Nothing runs before it's asked: the tab reads what's kept when it opens, measures only
//! then or on Refresh, and lets everything go when it closes (a measure going on stops).

pub mod donut;
pub mod report;
pub mod scan;
mod view;

use std::path::Path;
use std::sync::Arc;

use command_model::{CommandId, CommandSpec};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, ContextMenuItem, MenuPlace, MenuSection, WorkbenchHost,
};

pub use view::{EXTENSION, SizesData, SizesView};

const MODULE_ID: &str = "folder-sizes";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "Folder Sizes",
    description: "How much a folder holds, folder by folder: a ring of the biggest, a list to go down into, kept with when it was measured",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "hard-drive", ..extensions::ModuleInfo::BUILT_IN },
};

/// A folder of the tree measured, in a tab.
pub const ANALYZE: CommandId = CommandId("folder_sizes.analyze");

/// Its icon (of our own drawing): a ring with a slice out.
const ICON: &str = "folder-sizes";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3a9 9 0 1 0 9 9h-9z"/><path d="M15 3.5a9 9 0 0 1 5.5 5.5H15z"/></svg>"##;

#[derive(Debug, Default)]
pub struct FolderSizesModule;

impl Module for FolderSizesModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        // Its tab and breadcrumb show it.
        ctx.contribute(languages::FileIconRule::new(
            languages::FileMatcher::Extension(EXTENSION),
            ICON,
            "config",
        ));
        ctx.contribute(CommandContribution {
            spec: CommandSpec {
                id: ANALYZE,
                label: "Analyze Folder Sizes",
                menu: None,
                shortcuts: &[],
            },
            icon: Some(ICON),
            handler: Arc::new(Analyze),
        });
        ctx.contribute(
            ContextMenuItem::new(ANALYZE, &[MenuPlace::TreeFolder], MenuSection::Path, 90)
                .label("sizes.menu"),
        );
        // Its threads: their CPU counts on its row in the Task Manager.
        ctx.contribute(workbench_model::ModuleThreads(&["folder-sizes"]));
        Ok(())
    }
}

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(SizesView::default()))
    }
}

/// The folder the tree's menu was opened on (else the project's), measured in a tab.
struct Analyze;

impl CommandHandler for Analyze {
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.target_path().is_some() || host.project_root().is_some()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(folder) = host.target_path().or_else(|| host.project_root()) else {
            return false;
        };
        let name = kernel::paths::file_name(&folder);
        let data = SizesData { folder, storage: host.project_storage() };
        host.open_view(&i18n::tr!("sizes.tab", name = name.as_str()), EXTENSION, &data)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
