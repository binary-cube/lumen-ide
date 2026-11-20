//! The question asked before closing a window, a project or the app while work still runs
//! (a program in a terminal, an upload, a query, a push): a modal of the module's own
//! ([`workbench_model::running::PROMPT_VIEW`], opened by the app's closing with
//! `open_modal`) listing what runs, by project and module, with how far each got. Its
//! buttons: "Stop and close", "Wait" (the list follows the work, the closing goes on by
//! itself once it ends) and "Cancel"; closed only by them, × or Esc (= Cancel) — never a
//! click outside. The answer goes back to the app as
//! [`ui_components::AppRequest::RunningAnswer`]; what runs is the modules' to say
//! ([`workbench_model::ResourceReporter::running`]), not this one's. As the app ends, the same
//! window shows the programs it started being stopped (`stopping`): a progress bar and
//! the list, nothing to answer, nothing to close.

mod stopping;
mod view;

use std::path::Path;
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};
use workbench_model::running::PROMPT_VIEW;

pub use view::RunningView;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("running-work"),
    name: "Running work",
    description: "Asks before closing while work still runs: what runs, Stop and close, Wait, Cancel",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "play", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct RunningWorkModule;

impl Module for RunningWorkModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Prompt));
        Ok(())
    }
}

/// The question's view (its virtual extension).
#[derive(Debug)]
struct Prompt;

impl ViewerProvider for Prompt {
    fn id(&self) -> &'static str {
        PROMPT_VIEW
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(PROMPT_VIEW))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(RunningView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
