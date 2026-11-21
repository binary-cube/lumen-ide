//! The consoles' results when nobody shows them (no Services window): a tab showing what the
//! last run wanted seen, its Output or its result. The same services the window would ask.

use std::path::Path;
use std::sync::Arc;

use kernel::Point;
use render_api::Quad;
use ui_components::ds::{self, Ctx};
use ui_components::{
    ProjectServices, ServiceId, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerProvider,
};

use crate::services::ConsoleServices;

/// The extension of its virtual tab.
pub const EXTENSION: &str = "view-console-results";

/// What it's opened with: the project whose consoles it shows.
#[derive(Debug, Clone, Copy)]
pub struct ResultsData {
    pub project: kernel::ProjectId,
}

#[derive(Debug)]
pub(crate) struct ResultsProvider;

impl ViewerProvider for ResultsProvider {
    fn id(&self) -> &'static str {
        "database.console.results"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ResultsTab::default()))
    }
}

/// Shows the tab each run asks to be seen.
#[derive(Default)]
pub struct ResultsTab {
    services: Option<ConsoleServices>,
    /// The tab shown and its view.
    shown: Option<(ServiceId, Box<dyn Viewer>)>,
}

impl std::fmt::Debug for ResultsTab {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultsTab").field("shown", &self.shown.is_some()).finish_non_exhaustive()
    }
}

impl ResultsTab {
    /// What a run asked to be seen: its tab, or its console's first.
    fn follow(&mut self) -> bool {
        let Some(services) = &mut self.services else { return false };
        let mut changed = services.poll();
        if let Some((node, tab)) = services.take_reveal() {
            let tab = tab.or_else(|| services.tabs(node).first().map(|t| t.id));
            if let Some((tab, view)) = tab.and_then(|t| Some((t, services.open_tab(t)?))) {
                self.shown = Some((tab, view));
                changed = true;
            }
        }
        changed
    }

    fn view(&mut self) -> Option<&mut Box<dyn Viewer>> {
        self.shown.as_mut().map(|(_, view)| view)
    }
}

impl Viewer for ResultsTab {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<ResultsData>() else { return };
        let Some(consoles) = crate::consoles::consoles(data.project) else { return };
        self.services =
            Some(ConsoleServices::new(consoles, Arc::new(workbench_model::app_wide::wake)));
        self.follow();
    }

    fn poll(&mut self) -> bool {
        let followed = self.follow();
        let view = self.view().is_some_and(|v| v.poll());
        followed || view
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(view) = self.view() {
            view.paint(ctx);
            return;
        }
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        layer.quad(Quad::filled(ctx.bounds, ctx.palette.editor_background));
        let empty = i18n::t("console.results_none");
        ds::empty_state(layer, ctx.text, &ds, ctx.bounds, &empty, None);
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view().is_some_and(|v| v.scroll(dx, dy))
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.view()?.click(p)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        self.view()?.press(p, clicks)
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.view()?.context_menu(p)
    }

    fn take_menu(&mut self) -> Option<(kernel::Rect, Vec<ui_components::ViewMenuEntry>)> {
        self.view()?.take_menu()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.view().is_some_and(|v| v.drag(p))
    }

    fn release(&mut self) -> bool {
        self.view().is_some_and(|v| v.release())
    }

    fn hover(&mut self, p: Point) -> bool {
        self.view().is_some_and(|v| v.hover(p))
    }

    fn wants_text(&self) -> bool {
        self.shown.as_ref().is_some_and(|(_, v)| v.wants_text())
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.view().is_some_and(|v| v.text_input(text))
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.view().is_some_and(|v| v.edit_key(key))
    }

    fn command(&mut self, id: &str) -> bool {
        self.view().is_some_and(|v| v.command(id))
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.view()?.take_action()
    }

    fn selected_text(&self) -> Option<String> {
        self.shown.as_ref()?.1.selected_text()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.view().is_some_and(|v| v.paste(text))
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.shown.as_ref()?.1.tooltip_at(p)
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.shown.as_ref()?.1.cursor_at(p)
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.shown.as_ref()?.1.frame_deadline()
    }

    fn is_animating(&self) -> bool {
        self.shown.as_ref().is_some_and(|(_, v)| v.is_animating())
    }

    fn is_loading(&self) -> bool {
        self.shown.as_ref().is_some_and(|(_, v)| v.is_loading())
    }
}

/// The run's Output and results brought to the eye: the Services window, or, with nobody to
/// show one, this tab.
pub fn show(host: &mut dyn workbench_model::WorkbenchHost) {
    if workbench_model::presence::command(workbench_model::TOGGLE_SERVICES) {
        host.show_side_panel(workbench_model::SERVICES_TOOL);
    } else if !host.focus_view(EXTENSION)
        && let Some(project) = host.project_id()
    {
        host.open_view(&i18n::t("console.results_tab"), EXTENSION, &ResultsData { project });
    }
}

#[cfg(test)]
#[path = "../tests/unit/results_tab.rs"]
mod tests;
