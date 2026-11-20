//! draw.io itself, on demand (plan 82f, 82g): "Edit in draw.io" opens a `.drawio` file in a
//! tab where the system's web engine runs draw.io's own editor, offline, from a copy
//! downloaded once (a pinned, checked release: [`package`], [`install`]) — nothing of it in
//! the app's binary. It also opens a `.drawio` file itself: asked after our own diagram
//! editor (registered before it), it's the file's default while that one is hidden or off —
//! and then "Edit in draw.io" isn't offered (it would open the same).
//!
//! The tab's view ([`view`]) offers the download while it isn't there, then shows our page
//! around draw.io ([`page`]) and talks to it ([`protocol`]): the file read and written by
//! us on workers (autosave, through a temporary file), a change on disk merged in when
//! nothing's unsaved, the theme's colors over draw.io's ([`theme`]). The page exists only
//! while its tab is open.
//!
//! draw.io is © JGraph Ltd and draw.io AG, Apache 2.0; the name says what's run, never an
//! affiliation (no logo, an attribution in the offer and the notice).

pub mod file;
pub mod install;
pub mod package;
pub mod page;
pub mod protocol;
pub mod theme;
pub mod view;

use std::path::Path;
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::{
    CommandContribution, CommandHandler, ContextMenuItem, MenuPlace, MenuSection, WorkbenchHost,
};

pub use view::{DiagramWebView, EXTENSION, OpenData};

/// The files it edits.
pub const FILE_EXTENSION: &str = "drawio";

/// Edit in draw.io: the target `.drawio` file opened in a draw.io tab.
pub const EDIT: command_model::CommandId = command_model::CommandId("diagram_web.edit");

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tool.diagram-web"),
    name: "Edit in draw.io",
    description: "A .drawio file edited in draw.io itself, downloaded once on demand and run \
                  offline by the system's web engine",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "globe", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct DiagramWebModule;

impl Module for DiagramWebModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(workbench_model::ModuleThreads(&[install::THREAD, view::FILE_THREAD]));
        ctx.contribute(CommandContribution {
            spec: command_model::CommandSpec {
                id: EDIT,
                label: "Edit in draw.io",
                menu: None,
                shortcuts: &[],
            },
            icon: Some("file-pen"),
            handler: Arc::new(Edit),
        });
        // A `.drawio` file's menu in the project tree, and its tab's.
        ctx.contribute(ContextMenuItem::new(
            EDIT,
            &[MenuPlace::TreeFile, MenuPlace::Tab],
            MenuSection::Run,
            50,
        ));
        Ok(())
    }
}

/// The name its tab is opened by: the file's path in the project (or the whole path, outside
/// it) — two files of the same name in different folders are two tabs, and the tab made
/// again from a session knows its file ([`file_of_view`]).
pub fn view_name(root: Option<&Path>, path: &Path) -> String {
    let shown = root.and_then(|r| path.strip_prefix(r).ok()).unwrap_or(path);
    shown.to_string_lossy().into_owned()
}

/// The diagram a tab's path (`<root>/<view_name>.view-diagram-web`) is for.
pub fn file_of_view(view: &Path) -> Option<std::path::PathBuf> {
    let text = view.to_str()?;
    let file = std::path::PathBuf::from(text.strip_suffix(&format!(".{EXTENSION}"))?);
    is_diagram(&file).then_some(file)
}

/// Whether `path` is a diagram this edits.
pub fn is_diagram(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case(FILE_EXTENSION))
}

#[derive(Debug)]
struct Edit;

impl CommandHandler for Edit {
    /// Only for a `.drawio` file (what's targeted, from memory), while draw.io isn't what
    /// opens it anyway.
    fn enabled(&self, host: &dyn WorkbenchHost) -> bool {
        host.target_path().is_some_and(|p| is_diagram(&p)) && !opens_diagrams()
    }

    fn run(&self, host: &mut dyn WorkbenchHost) -> bool {
        let Some(path) = host.target_path().filter(|p| is_diagram(p)) else { return false };
        let title = view_name(host.project_root().as_deref(), &path);
        let data = OpenData { path, events: Some(host.events().clone()) };
        host.open_view(&title, EXTENSION, &data)
    }
}

/// draw.io is what a `.drawio` file opens in (no other viewer before it).
fn opens_diagrams() -> bool {
    workbench_model::presence::opener(FILE_EXTENSION) == Some(PROVIDER_ID)
}

/// Its viewer's id.
const PROVIDER_ID: &str = "diagram-web";

/// The tabs it opens: its own (by their virtual extension) and a `.drawio` file's, when no
/// viewer asked before it opens the file.
#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        PROVIDER_ID
    }

    /// By the name only (nothing read to decide).
    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        let own = path.extension().is_some_and(|e| e == EXTENSION);
        (own || is_diagram(path)).then_some(ViewerKind::Replace)
    }

    /// Its file from the tab's path: the file itself, or its own tab's (a tab made again
    /// from a session; the command's open gives it too, with the project's events).
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        let file = if is_diagram(path) { Some(path.to_path_buf()) } else { file_of_view(path) };
        Ok(Box::new(DiagramWebView::for_file(file)))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
