//! A response's page in a tab (plan 77h): shown by the system's web engine over the tab's
//! area ([`ui_components::WebPage`]: the app places it, hides it while something is open
//! over it, lets it go when the tab closes), with its address and Open in Browser above it.
//! Under the page (while it's hidden, or where the system has no engine) the tab says what
//! it holds. The page is made once, when the tab opens (on a worker).

use std::path::PathBuf;
use std::sync::Arc;

use i18n::t;
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::diagram;
use ui_components::ds::{self, Ctx, ToolbarButton};
use ui_components::{
    Viewer, ViewerAction, ViewerCursor, ViewerKind, ViewerPaint, ViewerProvider, WebPage, WebSource,
};

use crate::pending::Pending;

/// The extension its tab is opened by.
pub const EXTENSION: &str = "view-http-preview";
/// The bar's height above the page (logical pixels).
const BAR: f32 = 40.0;

/// What the tab is opened with: the page (its `<base>` in it), where it came from, the
/// folder a copy for the browser is written to.
#[derive(Debug, Clone)]
pub struct PreviewData {
    pub html: Arc<str>,
    pub url: String,
    pub folder: PathBuf,
}

#[derive(Default)]
pub struct PreviewView {
    data: Option<PreviewData>,
    /// Its page's identity (another for each page given).
    id: u128,
    /// Where the page goes, as last painted (physical pixels).
    area: Rect,
    browser: Rect,
    hover: bool,
    action: Option<ViewerAction>,
    /// The page written for the browser.
    writing: Pending<Option<ViewerAction>>,
}

impl std::fmt::Debug for PreviewView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewView").field("url", &self.data.as_ref().map(|d| &d.url)).finish()
    }
}

impl Viewer for PreviewView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<PreviewData>() else { return };
        self.data = Some(data.clone());
        self.id = kernel::id::new_uuid();
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, waker, .. } = ctx;
        self.writing.set_waker(&waker);
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));
        let bar = diagram::toolbar(layer, &ds, b, BAR);
        let label = t("http.open_in_browser");
        let w = ds::toolbar_button_width(text, &ds, &label);
        let h = px(ds::TOOLBAR_BUTTON_H);
        self.browser =
            Rect::new(bar.max_x() - w - px(10.0), bar.min_y() + (bar.size.height - h) / 2.0, w, h);
        let url = self.data.as_ref().map_or("", |d| d.url.as_str());
        let room = (self.browser.min_x() - bar.min_x() - px(28.0)).max(0.0);
        let font = ds.ui_font(0.0);
        let shown = ds::ellipsize(text, url, &font, room);
        let shaped = text.shape_line(&shown, &font);
        ui_kit::draw_text(
            layer,
            &shaped,
            bar.min_x() + px(14.0),
            ds::optical_baseline(bar.min_y(), bar.size.height, &font),
            p.text_muted,
        );
        let button =
            ToolbarButton { label: &label, icon: "globe", accent: false, hovered: self.hover };
        ds::toolbar_button(layer, text, icons, &ds, self.browser, button);
        self.area =
            Rect::new(b.min_x(), bar.max_y(), b.size.width, (b.max_y() - bar.max_y()).max(0.0));
        // Seen only while the page isn't over it.
        ds::empty_state(
            layer,
            text,
            &ds,
            self.area,
            &t("http.preview_title"),
            Some(&t("http.preview_detail")),
        );
    }

    fn web_page(&self) -> Option<WebPage> {
        let data = self.data.as_ref()?;
        (self.area.size.height > 0.0).then(|| WebPage {
            id: self.id,
            source: WebSource::Html(Arc::clone(&data.html)),
            area: self.area,
        })
    }

    fn poll(&mut self) -> bool {
        match self.writing.take() {
            Some(action) => {
                self.action = action;
                true
            }
            None => false,
        }
    }

    fn pointer_down(
        &mut self,
        p: Point,
        _clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        if !self.browser.contains(p) {
            return None;
        }
        let data = self.data.clone()?;
        // Written beside the project's responses (on a worker), then opened.
        self.writing.start(crate::response_view::THREAD, move || {
            crate::page::open_in_browser(&data.folder, &data.html)
        });
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.browser.contains(p);
        std::mem::replace(&mut self.hover, hover) != hover
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.browser.contains(p).then_some(ViewerCursor::Pointer)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

/// The preview's tab.
#[derive(Debug, Default)]
pub struct PreviewProvider;

impl ViewerProvider for PreviewProvider {
    fn id(&self) -> &'static str {
        "http-preview"
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PreviewView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/preview_view.rs"]
mod tests;
