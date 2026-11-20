//! The Remote tool window: the connection, and every transfer (newest first) — when, which
//! way, the file, its size, and why it failed if it did; a loader while transfers run.

use std::path::Path;

use i18n::{t, tr};
use kernel::{Point, Rect};
use remote::application::Action;
use render_api::Quad;
use text_layout::Font;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::{EdgeScrollbar, Viewer, ViewerAction, ViewerKind, ViewerPaint, ViewerProvider};
use ui_kit::draw_text;

/// The extension of the view (in the Remote tool window).
pub const EXTENSION: &str = "view-remote-log";

/// What the view is opened with: the remote project's root, and its events (the view
/// follows its transfers through them).
#[derive(Debug, Clone)]
pub struct LogData {
    pub project: kernel::ProjectId,
    pub events: kernel::EventBus,
}

#[derive(Debug)]
pub(crate) struct LogProvider;

impl ViewerProvider for LogProvider {
    fn id(&self) -> &'static str {
        "remote.log"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(TransferLog::default()))
    }
}

#[derive(Debug, Default)]
pub struct TransferLog {
    project: Option<kernel::ProjectId>,
    /// A transfer ended or the work changed since the last look (heard on the project's
    /// events): a redraw.
    heard: std::sync::Arc<std::sync::atomic::AtomicBool>,
    subscriptions: Vec<kernel::Subscription>,
    scroll: f32,
    /// How far the list scrolls (measured when painted).
    max_scroll: f32,
    bar: EdgeScrollbar,
    /// Its controls' state of use (see [`TransferLog::with_controls`]).
    controls: Controls,
    list: Rect,
    row_h: f32,
    /// The file of the right-clicked row (its menu acts on it), and what's asked of the app.
    menu: Option<String>,
    action: Option<ViewerAction>,
}

/// The rows' menu: the file's properties on the server.
const PROPERTIES: &str = "remote.log.properties";

impl TransferLog {
    /// Its controls (the list's scrollbar), with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut list = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 1] = [&mut list];
        f(&mut self.controls, &mut parts)
    }

    /// The file of the row at `p` (the newest first, as shown).
    fn path_at(&self, p: Point) -> Option<String> {
        if !self.list.contains(p) || self.row_h <= 0.0 {
            return None;
        }
        let i = ((p.y - self.list.min_y() + self.scroll.round()) / self.row_h) as usize;
        let state = self.project.and_then(crate::session::session)?;
        let log = state.log.lock().ok()?;
        log.iter().rev().nth(i).map(|t| t.path.clone())
    }
}

/// `14:32:05` (UTC).
fn clock(at: i64) -> String {
    let s = at.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

impl Viewer for TransferLog {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(data) = data.downcast_ref::<LogData>() {
            self.project = Some(data.project);
            let (finished, changed) = (self.heard.clone(), self.heard.clone());
            let heard = |flag: &std::sync::atomic::AtomicBool| {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            };
            self.subscriptions = vec![
                data.events.subscribe(move |_: &remote::events::TransferFinished| heard(&finished)),
                data.events.subscribe(move |_: &remote::events::TransfersChanged| heard(&changed)),
            ];
        }
    }

    fn poll(&mut self) -> bool {
        self.heard.swap(false, std::sync::atomic::Ordering::Relaxed)
    }

    fn is_loading(&self) -> bool {
        self.project
            .and_then(crate::session::session)
            .is_some_and(|s| s.busy.load(std::sync::atomic::Ordering::Relaxed) > 0)
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press(parts, p, 1));
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        self.menu = Some(self.path_at(p)?);
        let entry = ui_components::ViewMenuEntry::item(PROPERTIES, t("remote.menu_properties"));
        Some(vec![entry.with_icon("info")])
    }

    fn command(&mut self, id: &str) -> bool {
        let (PROPERTIES, Some(project)) = (id, self.project) else { return false };
        let Some(path) = self.menu.take() else { return false };
        self.action = Some(crate::properties_view::action(project, vec![path]));
        true
    }

    /// A fixed tab of the Remote window (as Git's Log and Changes): never closed.
    fn closable(&self) -> bool {
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let Some(state) = self.project.and_then(crate::session::session) else {
            ds::empty_state(layer, ctx.text, &ds, b, &t("remote.not_remote"), None);
            return;
        };
        // The header: the connection, where, and whether it's busy.
        let header_h = px(44.0);
        let mid = b.min_y() + header_h / 2.0;
        let strong = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.0);
        let mut x = b.min_x() + px(16.0);
        let name = ctx.text.shape_line(&state.saved.connection.name, &strong);
        draw_text(layer, &name, x, ds::optical_baseline(b.min_y(), header_h, &strong), p.text);
        x += name.width + px(10.0);
        let address = state.saved.connection.address();
        let chip = ds::badge(
            layer,
            ctx.text,
            &ds,
            ds::BadgeSpec { label: &address, x, mid, align_right: false, tone: Tone::Neutral },
        );
        x = chip.max_x() + px(12.0);
        let busy = state.busy.load(std::sync::atomic::Ordering::Relaxed);
        if busy > 0 {
            let label = tr!("remote.transferring", count = busy);
            ds::loader(
                layer,
                ctx.text,
                &ds,
                x,
                mid,
                &ds::Loader { size: 14.0, ..ds::Loader::with_text(&label) },
            );
        }
        // Only counted here; the rows drawn are copied below (not the whole log every frame).
        let len = state.log.lock().map(|l| l.len()).unwrap_or_default();
        let count = tr!("remote.transfers", count = len);
        let shaped = ctx.text.shape_line(&count, &small);
        draw_text(
            layer,
            &shaped,
            b.max_x() - px(16.0) - shaped.width,
            ds::optical_baseline(b.min_y(), header_h, &small),
            p.text_muted,
        );
        layer.quad(Quad::filled(
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, ds.line()),
            p.card_border,
        ));
        self.list =
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, b.size.height - header_h);
        if len == 0 {
            ds::empty_state(layer, ctx.text, &ds, self.list, &t("remote.no_transfers"), None);
            return;
        }
        // The transfers, newest first.
        let font = ds.ui_font(-0.5);
        let mono = ds.code_font(-1.0);
        self.row_h = px(28.0);
        self.max_scroll = (len as f32 * self.row_h - self.list.size.height).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        // Whole pixels: no jitter with trackpad momentum.
        let scroll = self.scroll.round();
        let rows = ctx.scene.push_layer(Some(self.list));
        let first = (scroll / self.row_h) as usize;
        let visible = (self.list.size.height / self.row_h) as usize + 2;
        let shown: Vec<_> = state
            .log
            .lock()
            .map(|l| l.iter().rev().skip(first).take(visible).cloned().collect())
            .unwrap_or_default();
        for (i, transfer) in shown.iter().enumerate().map(|(i, t)| (first + i, t)) {
            let top = self.list.min_y() + i as f32 * self.row_h - scroll;
            let base = ds::optical_baseline(top, self.row_h, &font);
            let mut x = self.list.min_x() + px(16.0);
            let time = ctx.text.shape_line(&clock(transfer.at), &mono);
            draw_text(rows, &time, x, ds::optical_baseline(top, self.row_h, &mono), p.text_muted);
            x += time.width + px(14.0);
            let (icon, color) = match transfer.action {
                Action::Upload => ("upload", p.accent),
                Action::Download => ("download", p.vcs_added),
                Action::DeleteLocal | Action::DeleteRemote => ("trash", p.vcs_deleted),
            };
            if let Some(id) = ctx.icons.id(icon) {
                let s = px(14.0);
                ui_kit::draw_icon(
                    rows,
                    ctx.icons,
                    id,
                    Rect::new(x, top + (self.row_h - s) / 2.0, s, s),
                    color,
                );
            }
            x += px(24.0);
            // A long path: its file's name kept, its folders cut first (its size or why it
            // failed keep some room after it).
            let room = (b.max_x() - x - px(120.0)).max(px(80.0));
            let shown = ds::ellipsize_path(ctx.text, &transfer.path, &font, room);
            let path = ctx.text.shape_line(&shown, &font);
            draw_text(rows, &path, x, base, p.text);
            x += path.width + px(12.0);
            let row_mid = top + self.row_h / 2.0;
            match &transfer.error {
                Some(why) => {
                    let why =
                        ds::ellipsize(ctx.text, why, &small, (b.max_x() - x - px(24.0)).max(0.0));
                    ds::badge(
                        rows,
                        ctx.text,
                        &ds,
                        ds::BadgeSpec {
                            label: &why,
                            x,
                            mid: row_mid,
                            align_right: false,
                            tone: Tone::Danger,
                        },
                    );
                }
                None => {
                    let shaped = ctx.text.shape_line(&i18n::bytes(transfer.bytes), &small);
                    draw_text(
                        rows,
                        &shaped,
                        x,
                        ds::optical_baseline(top, self.row_h, &small),
                        p.text_muted,
                    );
                }
            }
        }
        // The app's scrollbar, along the list's right edge.
        let content = self.max_scroll + self.list.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, self.list, content, scroll);
    }
}

#[cfg(test)]
#[path = "../tests/unit/log_view.rs"]
mod tests;
