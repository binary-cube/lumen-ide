//! A server's log in the group below the editor: its stderr, its messages, the protocol's
//! traffic when it's followed (Settings), the newest at the bottom (followed while the view
//! is scrolled to its end).

use std::path::Path;

use i18n::t;
use kernel::{Point, ProjectId, Rect};
use render_api::Quad;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{EdgeScrollbar, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerProvider};
use ui_kit::draw_text;

use crate::projects;

/// The view's extension.
pub const EXTENSION: &str = "view-lsp-log";

/// What the view shows: a project's server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogData {
    pub project: ProjectId,
    pub server: u128,
}

/// The log chosen for `project`, else its first server's.
pub fn chosen(project: ProjectId) -> Option<LogData> {
    let servers = projects::project(project)?;
    let chosen = servers.take_log_choice().map(|server| LogData { project, server });
    chosen.or_else(|| servers.servers(|s| s.first().map(|r| LogData { project, server: r.id })))
}

#[derive(Debug)]
pub(crate) struct LogProvider;

impl ViewerProvider for LogProvider {
    fn id(&self) -> &'static str {
        "lsp.log"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(LogView::default()))
    }
}

#[derive(Debug, Default)]
pub struct LogView {
    data: Option<LogData>,
    /// The log's generation last drawn.
    seen: u64,
    scroll: f32,
    max_scroll: f32,
    /// Scrolled to the end: new lines keep it there.
    follow: bool,
    bar: EdgeScrollbar,
    list: Rect,
}

impl LogView {
    fn status(&self) -> Option<std::sync::Arc<lsp_client::status::Status>> {
        let data = self.data?;
        let project = projects::project(data.project)?;
        project.servers(|s| {
            s.iter().find(|r| r.id == data.server).map(|r| std::sync::Arc::clone(r.client.status()))
        })
    }
}

impl Viewer for LogView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(data) = data.downcast_ref::<LogData>() {
            self.data = Some(*data);
            self.follow = true;
            self.seen = 0;
        }
    }

    fn poll(&mut self) -> bool {
        let generation = self.status().map_or(0, |s| s.log_generation());
        generation != self.seen
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.follow = self.scroll >= self.max_scroll;
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
        }
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        self.follow = self.scroll >= self.max_scroll;
        changed
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.bar.hover(p)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let Some(status) = self.status() else {
            ds::empty_state(layer, ctx.text, &ds, b, &t("lsp.log_gone"), None);
            return;
        };
        let row_h = px(20.0);
        let pad = px(10.0);
        // Only the rows in view are copied out of the log (up to 2000 lines, each frame).
        let (lines, len, generation) = status.log_window(|len| {
            self.max_scroll = (len as f32 * row_h + 2.0 * pad - b.size.height).max(0.0);
            if self.follow {
                self.scroll = self.max_scroll;
            }
            self.scroll = self.scroll.min(self.max_scroll);
            let first = ((self.scroll.round() - pad).max(0.0) / row_h) as usize;
            (first, (b.size.height / row_h) as usize + 2)
        });
        self.seen = generation;
        self.list = b;
        if len == 0 {
            ds::empty_state(layer, ctx.text, &ds, b, &t("lsp.log_empty"), None);
            return;
        }
        let mono = ds.code_font(-1.0);
        let scroll = self.scroll.round();
        let rows = ctx.scene.push_layer(Some(b));
        let first = ((scroll - pad).max(0.0) / row_h) as usize;
        for (i, line) in (first..).zip(lines.iter()) {
            let top = b.min_y() + pad + i as f32 * row_h - scroll;
            let color = if line.starts_with("error:") {
                p.danger
            } else if line.starts_with('→') || line.starts_with('←') {
                p.text_muted
            } else {
                p.text
            };
            let shown = ds::ellipsize(
                ctx.text,
                line,
                &mono,
                (b.size.width - 2.0 * pad - px(12.0)).max(0.0),
            );
            let shaped = ctx.text.shape_line(&shown, &mono);
            draw_text(
                rows,
                &shaped,
                b.min_x() + pad,
                ds::optical_baseline(top, row_h, &mono),
                color,
            );
        }
        let content = self.max_scroll + b.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, b, content, scroll);
    }
}
