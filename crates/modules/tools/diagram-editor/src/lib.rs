//! The diagram editor (plan 80): a `.drawio` file opens as a tab drawing it — its sheet and
//! grid, its shapes, edges and texts as draw.io draws them, its pages as tabs — panned and
//! zoomed. The file is draw.io's own format (`diagram-model`), so what's drawn here opens
//! there and back; the shapes are rasterized in tiles off the UI thread (`vector-tiles`).
//! The project's diagrams are listed by their own module (`diagram-files`).

mod camera;
mod drawing;
mod edit;
mod export;
mod label;
mod look;
mod palette;
mod texts;
mod tiles;
mod view;

use std::path::Path;
use std::sync::Arc;

use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};

pub use view::DiagramView;

/// The diagrams' files.
pub const EXTENSION: &str = "drawio";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("tool.diagram-editor"),
    name: "Diagram Editor",
    description: "draw.io diagrams (.drawio) opened as tabs: their pages, shapes, edges and texts",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct DiagramEditorModule;

impl Module for DiagramEditorModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Provider));
        ctx.contribute(workbench_threads());
        // The drawing tools' (the bar's).
        for (name, svg) in [
            ("tool-select", include_str!("../svg/tool-select.svg")),
            ("tool-ellipse", include_str!("../svg/tool-ellipse.svg")),
            ("tool-rhombus", include_str!("../svg/tool-rhombus.svg")),
            ("tool-text", include_str!("../svg/tool-text.svg")),
            ("tool-line", include_str!("../svg/tool-line.svg")),
            ("line-dashed", include_str!("../svg/line-dashed.svg")),
            ("line-arrow", include_str!("../svg/line-arrow.svg")),
            ("route-straight", include_str!("../svg/route-straight.svg")),
            ("route-elbow", include_str!("../svg/route-elbow.svg")),
            ("route-curved", include_str!("../svg/route-curved.svg")),
        ] {
            ctx.contribute(languages::IconSvg { name, svg });
        }
        Ok(())
    }
}

/// Its threads, named for the Task Manager.
fn workbench_threads() -> workbench_model::ModuleThreads {
    workbench_model::ModuleThreads(&[
        "diagram-read",
        "diagram-tiles",
        "diagram-palette",
        "diagram-export",
    ])
}

#[derive(Debug)]
struct Provider;

impl ViewerProvider for Provider {
    fn id(&self) -> &'static str {
        "drawio"
    }

    /// A `.drawio` file: by its name only (nothing read to decide).
    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension()
            .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    /// Opened at once; the file read on a worker.
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(DiagramView::open(path.to_path_buf())))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
