//! A diagram opened larger: a modal of the module's own (`host.open_modal`: closed only
//! by ×, Esc or its buttons) showing the preview's diagram to pan and zoom (drag, scroll,
//! pinch or ⌘-scroll, `0` fits it, `1` is 100%), its shapes drawn again for the zoom on
//! the render pool, its texts by the app (crisp at any zoom). Copied as PNG (to the
//! clipboard) or SVG (as text), or its source; saved as either.
//!
//! The preview opens it through its action (`ViewerAction::OpenView`): asks
//! the app to run [`OPEN`], whose handler opens it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::diagram::{Release, Viewport};
use ui_components::{
    AppRequest, ViewMenuEntry, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;

use crate::diagrams::text_items;
use crate::jobs::{self, Jobs};
use crate::layout::Item;
use crate::mermaid::scene::{self, Scene};
use crate::mermaid::{self as mm, Look};

/// The modal's virtual extension (its view's kind).
pub const EXTENSION: &str = "view-markdown-diagram";

/// What the modal shows.
#[derive(Clone)]
pub struct DiagramRequest {
    pub scene: Arc<Scene>,
    pub source: String,
    pub title: Option<String>,
    /// Where it's saved first (the document's folder).
    pub dir: PathBuf,
}

/// The modal's provider (its virtual extension).
#[derive(Debug, Default)]
pub struct DiagramProvider;

impl ViewerProvider for DiagramProvider {
    fn id(&self) -> &'static str {
        EXTENSION
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        (path.extension().and_then(|e| e.to_str()) == Some(EXTENSION))
            .then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(DiagramView::default()))
    }
}

/// What a copy or a save waits for: the pixels (made on the pool).
enum Export {
    CopyPng,
    SavePng(PathBuf),
}

/// The picture of a diagram for the clipboard or a file: its size, its pixels, its PNG.
type Picture = Option<(u32, u32, Vec<u8>, Vec<u8>)>;

#[derive(Default)]
pub struct DiagramView {
    request: Option<DiagramRequest>,
    viewport: Viewport,
    look: Option<Look>,
    tiles: Jobs<Arc<Vec<crate::tiles::Mask>>>,
    /// The last scale whose tiles in view were all there (shown, stretched, while the
    /// zoom's are made).
    shown_k: Option<f32>,
    export: Option<(Export, Receiver<Picture>)>,
    /// A save asked for: the format the chosen path gets.
    saving: Option<bool>,
    action: Option<ViewerAction>,
    notifications: Vec<workbench_model::Notification>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
}

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 6.0;

impl DiagramView {
    fn scene(&self) -> Option<&Arc<Scene>> {
        self.request.as_ref().map(|r| &r.scene)
    }

    fn bounds(&self) -> Rect {
        self.scene().map_or(Rect::default(), |s| Rect::new(0.0, 0.0, s.width, s.height))
    }

    /// The picture made on the pool (2× the diagram), for the clipboard or a file.
    fn export(&mut self, export: Export) {
        let (Some(scene), Some(look)) = (self.scene().cloned(), self.look.clone()) else { return };
        let (tx, rx) = std::sync::mpsc::channel();
        let wake = self.wake.clone();
        kernel::work::run(jobs::RENDER_THREAD, move || {
            let _ = tx.send(mm::picture(&scene, &look, 2.0));
            if let Some(wake) = wake {
                wake();
            }
        });
        self.export = Some((export, rx));
    }

    fn take_export(&mut self) -> bool {
        let Some((_, rx)) = &self.export else { return false };
        let Ok(picture) = rx.try_recv() else { return false };
        let Some((export, _)) = self.export.take() else { return false };
        let Some((w, h, rgba, png)) = picture else { return true };
        match export {
            Export::CopyPng => {
                self.action = Some(ViewerAction::Request(AppRequest::CopyImage {
                    width: w as usize,
                    height: h as usize,
                    rgba: Arc::new(rgba),
                }));
                self.notifications.push(workbench_model::Notification::success(i18n::t(
                    "markdown.diagram_copied",
                )));
            }
            Export::SavePng(path) => self.write(&path, &png),
        }
        true
    }

    fn write(&mut self, path: &Path, bytes: &[u8]) {
        let note = match std::fs::write(path, bytes) {
            Ok(()) => workbench_model::Notification::success(i18n::tr!(
                "markdown.diagram_saved",
                path = path.display().to_string()
            )),
            Err(e) => workbench_model::Notification::error(i18n::t("markdown.diagram_not_saved"))
                .with_detail(e.to_string()),
        };
        self.notifications.push(note);
    }

    fn svg(&self) -> Option<String> {
        let (scene, look) = (self.scene()?, self.look.as_ref()?);
        let font = match &look.font {
            text_layout::FontFamily::Named(name) => name.to_string(),
            _ => "sans-serif".to_owned(),
        };
        Some(scene::svg(scene, Some(look.background), &font))
    }

    fn save_as(&mut self, png: bool) {
        let Some(r) = &self.request else { return };
        let name = format!("{}.{}", file_stem(r.title.as_deref()), if png { "png" } else { "svg" });
        self.saving = Some(png);
        self.action = Some(ViewerAction::Request(AppRequest::PickSavePath {
            start: Some(r.dir.clone()),
            name,
        }));
    }
}

/// A file name from the diagram's title.
fn file_stem(title: Option<&str>) -> String {
    let stem: String = title
        .unwrap_or("diagram")
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let stem = stem.trim_matches('-');
    if stem.is_empty() { "diagram".to_owned() } else { stem.to_owned() }
}

impl Viewer for DiagramView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(request) = data.downcast_ref::<DiagramRequest>() {
            self.request = Some(request.clone());
            self.viewport = Viewport::new(0.0);
            self.viewport.fit_pending = true;
        }
    }

    fn memory(&self) -> ui_components::ViewMemory {
        let gpu = self.tiles.values().flat_map(|t| t.iter()).map(|(i, _)| i.gpu_memory()).sum();
        let ram = self.scene().map_or(0, |s| s.memory() as u64);
        ui_components::ViewMemory { ram, gpu }
    }

    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        vec![
            (vec!["0"], i18n::t("markdown.diagram_fit")),
            (vec!["1"], "100%".to_owned()),
            (vec!["⌘C"], i18n::t("markdown.diagram_copy_png")),
        ]
    }

    fn modal_icon(&self) -> Option<&'static str> {
        Some("graph")
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.wake.clone_from(&ctx.waker);
        self.tiles.set_waker(ctx.waker.clone());
        self.tiles.poll();
        self.take_export();
        let palette = ctx.palette;
        if self.look.as_ref().is_none_or(|l| *l != Look::of(palette)) {
            self.look = Some(Look::of(palette));
        }
        let b = ctx.bounds;
        self.viewport.bounds = b;
        self.viewport.scale = ctx.scale;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, palette.editor_background));
        let Some(scene) = self.scene().cloned() else { return };
        if self.viewport.fit_pending {
            let bounds = self.bounds();
            self.viewport.fit(bounds, None, MIN_ZOOM, 1.5);
        }
        ui_components::diagram::dot_grid(
            layer,
            palette,
            b,
            self.viewport.to_screen(Point::new(0.0, 0.0)),
            self.viewport.k(),
        );
        let k = self.viewport.k();
        // The shapes in tiles made for this zoom (rounded to an eighth), only those in view;
        // the last complete ones stretched under them meanwhile.
        let kb = ((k * 8.0).round() / 8.0).max(0.125);
        let origin = self.viewport.to_screen(Point::new(0.0, 0.0));
        let base = jobs::key(&(Arc::as_ptr(&scene) as usize,));
        let view = self.viewport.area();
        if let Some(prev) = self.shown_k.filter(|p| (*p - kb).abs() > f32::EPSILON) {
            let at = crate::tiles::Placement {
                origin,
                stretch: k / prev,
                view,
                margin: 0.0,
                fetch: false,
            };
            crate::tiles::draw(
                layer,
                &mut self.tiles,
                &scene,
                prev,
                jobs::key(&(base, prev.to_bits())),
                at,
            );
        }
        let at =
            crate::tiles::Placement { origin, stretch: k / kb, view, margin: 256.0, fetch: true };
        if crate::tiles::draw(
            layer,
            &mut self.tiles,
            &scene,
            kb,
            jobs::key(&(base, kb.to_bits())),
            at,
        ) {
            self.shown_k = Some(kb);
        }
        crate::budget::frame_done(crate::budget::GPU_BUDGET);
        let look = self.look.clone().unwrap_or_else(|| Look::of(palette));
        for item in text_items(&scene, &look, origin, k, ctx.text) {
            if let Item::Text { x, baseline, text, font, color } = item {
                if baseline + font.size < b.min_y() || baseline - font.size > b.max_y() {
                    continue;
                }
                let shaped = ctx.text.shape_line(&text, &font);
                draw_text(layer, &shaped, x, baseline, color);
            }
        }
    }

    fn is_loading(&self) -> bool {
        self.tiles.loading() || self.export.is_some()
    }

    fn poll(&mut self) -> bool {
        self.tiles.poll() | self.take_export()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.viewport.scroll(dx, dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.viewport.zoom_at(factor, at, MIN_ZOOM, MAX_ZOOM)
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.viewport.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.viewport.reset_zoom(MIN_ZOOM, MAX_ZOOM)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        if clicks >= 2 {
            self.viewport.zoom_at(1.6, p, MIN_ZOOM, MAX_ZOOM);
            return None;
        }
        self.viewport.press(p);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        self.viewport.drag(p)
    }

    fn release(&mut self) -> bool {
        !matches!(self.viewport.release(), Release::None)
    }

    fn key(&mut self, key: char) -> bool {
        match key {
            '0' => {
                let bounds = self.bounds();
                self.viewport.fit(bounds, None, MIN_ZOOM, MAX_ZOOM);
                true
            }
            '1' => self.viewport.reset_zoom(MIN_ZOOM, MAX_ZOOM),
            '+' | '=' => {
                let c = self.viewport.visible().center();
                self.viewport.zoom_at(1.25, c, MIN_ZOOM, MAX_ZOOM)
            }
            '-' => {
                let c = self.viewport.visible().center();
                self.viewport.zoom_at(0.8, c, MIN_ZOOM, MAX_ZOOM)
            }
            _ => false,
        }
    }

    fn context_menu(&mut self, _p: Point) -> Option<Vec<ViewMenuEntry>> {
        Some(menu())
    }

    fn command(&mut self, id: &str) -> bool {
        if let Some(path) = id.strip_prefix("picked-save:") {
            let path = PathBuf::from(path);
            match self.saving.take() {
                Some(true) => self.export(Export::SavePng(path)),
                Some(false) => {
                    if let Some(svg) = self.svg() {
                        self.write(&path, svg.as_bytes());
                    }
                }
                None => {}
            }
            return true;
        }
        run_menu(self, id)
    }

    fn selected_text(&self) -> Option<String> {
        None
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        std::mem::take(&mut self.notifications)
    }
}

/// The diagram's menu (the preview's and the modal's).
pub fn menu() -> Vec<ViewMenuEntry> {
    vec![
        ViewMenuEntry::item("diagram.copy-png", i18n::t("markdown.diagram_copy_png"))
            .with_icon("copy"),
        ViewMenuEntry::item("diagram.copy-svg", i18n::t("markdown.diagram_copy_svg")),
        ViewMenuEntry::item("diagram.copy-source", i18n::t("markdown.diagram_copy_source")),
        ViewMenuEntry::Separator,
        ViewMenuEntry::item("diagram.save-png", i18n::t("markdown.diagram_save_png"))
            .with_icon("download"),
        ViewMenuEntry::item("diagram.save-svg", i18n::t("markdown.diagram_save_svg")),
    ]
}

fn run_menu(view: &mut DiagramView, id: &str) -> bool {
    match id {
        "diagram.copy-png" | "copy" => view.export(Export::CopyPng),
        "diagram.copy-svg" => {
            if let Some(svg) = view.svg() {
                view.action = Some(ViewerAction::Request(AppRequest::CopyText(svg)));
                view.notifications.push(workbench_model::Notification::success(i18n::t(
                    "markdown.diagram_copied",
                )));
            }
        }
        "diagram.copy-source" => {
            if let Some(r) = &view.request {
                view.action = Some(ViewerAction::Request(AppRequest::CopyText(r.source.clone())));
            }
        }
        "diagram.save-png" => view.save_as(true),
        "diagram.save-svg" => view.save_as(false),
        _ => return false,
    }
    true
}

/// A view of the preview's diagram (its menu's rows run there as here).
pub fn for_preview(request: DiagramRequest, look: Look) -> DiagramView {
    DiagramView { request: Some(request), look: Some(look), ..DiagramView::default() }
}

impl DiagramView {
    /// A row of the diagram's menu chosen (from the preview).
    pub fn run(&mut self, id: &str) -> bool {
        run_menu(self, id)
    }

    /// What the view asked for meanwhile (a copy).
    pub fn waiting(&self) -> bool {
        self.export.is_some()
    }

    pub fn set_waker(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        self.wake = wake;
    }

    /// The export arrived: what it asks for.
    pub fn settle(&mut self) -> (Option<ViewerAction>, Vec<workbench_model::Notification>) {
        self.take_export();
        (self.action.take(), std::mem::take(&mut self.notifications))
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
