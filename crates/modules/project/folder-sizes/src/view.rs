//! A folder's sizes, in an editor tab: how much it holds and when it was measured, a ring of
//! its biggest entries and the list of them all (biggest first: a bar, the size, the share);
//! a folder clicked (in the list or the ring) is gone into, the path above goes back up.
//! Each row goes to its file or folder: selected in the project's tree (its button, on
//! hover), opened (a file), shown in the system's file manager (its right-click menu).
//! The measure kept is shown at once (read on a worker); "Refresh" measures again (on a
//! worker, its progress shown as it goes), and what's measured is kept. Its tab closed, a
//! measure going on stops.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, Quad};
use text_layout::Font;
use ui_components::ds::{self, Ctx, radius};
use ui_components::{Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};
use ui_kit::{draw_icon, draw_text};

use crate::donut::{self, Ring};
use crate::report::{self, Report};
use crate::scan::{self, Node, Progress};

/// The extension of the view's virtual tab.
pub const EXTENSION: &str = "view-folder-sizes";

/// What the view is opened with: the folder, and the project's data folder (where measures
/// are kept; `None`: not kept).
#[derive(Debug, Clone)]
pub struct SizesData {
    pub folder: PathBuf,
    pub storage: Option<PathBuf>,
}

/// The slices the ring shows (the biggest entries; the rest one more).
const SLICES: usize = 8;
const ROW_H: f32 = 34.0;
const RING: f32 = 220.0;

/// A measure going on.
struct Measuring {
    progress: Arc<Progress>,
    rx: Receiver<Option<Report>>,
}

/// What wakes the app (a worker done).
#[derive(Clone)]
struct Wake(Arc<dyn Fn() + Send + Sync>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Refresh,
    Crumb(usize),
    Row(usize),
    Slice(usize),
    /// A row's "select in the project's tree".
    Locate(usize),
}

/// Its rows' menu: what each does with the entry.
const LOCATE: &str = "sizes-locate";
const OPEN: &str = "sizes-open";
const REVEAL: &str = "sizes-reveal";

#[derive(Default)]
pub struct SizesView {
    data: Option<SizesData>,
    report: Option<Report>,
    /// The kept measure being read.
    loading: Option<Receiver<Option<Report>>>,
    measuring: Option<Measuring>,
    /// The folder couldn't be read.
    failed: bool,
    /// The folders gone into, from the measured one (each its index among its parent's).
    trail: Vec<usize>,
    waker: Option<Wake>,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    scroll: f32,
    list: Rect,
    row_h: f32,
    /// The ring as last drawn (made again only when what it shows changes).
    ring: Option<(Ring, ImageData)>,
    ring_at: Rect,
    /// The entry its menu was opened on, and what it asks of the app.
    menu_on: Option<(PathBuf, bool)>,
    pending: Option<ViewerAction>,
}

impl std::fmt::Debug for SizesView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizesView").field("data", &self.data).finish_non_exhaustive()
    }
}

impl Drop for SizesView {
    /// Its tab closed: a measure going on stops.
    fn drop(&mut self) {
        if let Some(m) = &self.measuring {
            m.progress.stop.store(true, Ordering::Relaxed);
        }
    }
}

impl SizesView {
    /// The measure shown (tests).
    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// The folder shown now: the measured one, or the one gone into.
    fn node(&self) -> Option<&Node> {
        let mut node = &self.report.as_ref()?.root;
        for &i in &self.trail {
            node = node.children.get(i)?;
        }
        Some(node)
    }

    /// The kept measure read (on a worker); none kept: measured.
    fn load(&mut self) {
        let Some(data) = self.data.clone() else { return };
        let Some(storage) = data.storage.clone() else {
            self.measure();
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let waker = self.waker.clone();
        kernel::work::run("folder-sizes", move || {
            let _ = tx.send(report::load(&storage, &data.folder));
            if let Some(Wake(wake)) = waker {
                wake();
            }
        });
        self.loading = Some(rx);
    }

    /// Measured again (on a worker), then kept.
    fn measure(&mut self) {
        if self.measuring.is_some() {
            return;
        }
        let Some(data) = self.data.clone() else { return };
        let progress = Arc::new(Progress::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let (working, waker) = (Arc::clone(&progress), self.waker.clone());
        kernel::work::run("folder-sizes", move || {
            let started = Instant::now();
            let measured = scan::measure(&data.folder, &working).map(|root| Report {
                path: data.folder.clone(),
                at: report::now(),
                took_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
                partial: working.entries.load(Ordering::Relaxed) >= scan::ENTRIES_MOST,
                unreadable: working.unreadable.load(Ordering::Relaxed),
                root,
            });
            let stopped = working.stop.load(Ordering::Relaxed);
            if let (Some(measured), Some(storage), false) = (&measured, &data.storage, stopped)
                && let Err(e) = report::save(storage, measured)
            {
                tracing::warn!("a folder's sizes weren't kept: {e}");
            }
            let _ = tx.send(measured);
            if let Some(Wake(wake)) = waker {
                wake();
            }
        });
        self.failed = false;
        self.measuring = Some(Measuring { progress, rx });
    }

    /// What the workers brought, taken in: `true` if anything came.
    fn take(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = &self.loading
            && let Ok(kept) = rx.try_recv()
        {
            self.loading = None;
            changed = true;
            match kept {
                Some(kept) => self.show(kept),
                None => self.measure(),
            }
        }
        if let Some(m) = &self.measuring
            && let Ok(measured) = m.rx.try_recv()
        {
            self.measuring = None;
            changed = true;
            match measured {
                Some(measured) => self.show(measured),
                None => self.failed = self.report.is_none(),
            }
        }
        changed
    }

    /// `report` shown, the folder gone into kept while it's still there.
    fn show(&mut self, report: Report) {
        let names: Vec<String> = self.crumb_names();
        self.report = Some(report);
        self.trail.clear();
        // The same folders gone into again (by name: the indices may have moved).
        for name in names.iter().skip(1) {
            let Some(node) = self.node() else { break };
            match node.children.iter().position(|c| c.dir && !c.other && &c.name == name) {
                Some(i) => self.trail.push(i),
                None => break,
            }
        }
        self.ring = None;
    }

    /// The names of the folders on the path (the measured one first).
    fn crumb_names(&self) -> Vec<String> {
        let Some(report) = &self.report else { return Vec::new() };
        let mut node = &report.root;
        let mut names = vec![node.name.clone()];
        for &i in &self.trail {
            let Some(next) = node.children.get(i) else { break };
            names.push(next.name.clone());
            node = next;
        }
        names
    }

    /// Entry `i` of the folder shown gone into, if it's a folder listed apart.
    fn enter(&mut self, i: usize) -> bool {
        let Some(child) = self.node().and_then(|n| n.children.get(i)) else { return false };
        if !child.dir || child.other || child.children.is_empty() {
            return false;
        }
        self.trail.push(i);
        self.scroll = 0.0;
        self.ring = None;
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        if self.ring_at.contains(p) {
            let shares = self.shares();
            let (cx, cy) = (
                self.ring_at.min_x() + self.ring_at.size.width / 2.0,
                self.ring_at.min_y() + self.ring_at.size.height / 2.0,
            );
            if let Some(slice) =
                donut::hit(&shares, p.x - cx, p.y - cy, self.ring_at.size.width / 2.0)
            {
                return Some(Hit::Slice(slice));
            }
        }
        self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// Where entry `i` of the folder shown is on disk, and whether it's a folder (`None`:
    /// several summed as one).
    fn path_of(&self, i: usize) -> Option<(PathBuf, bool)> {
        let child = self.node()?.children.get(i)?;
        if child.other {
            return None;
        }
        let mut path = self.report.as_ref()?.path.clone();
        for name in self.crumb_names().iter().skip(1) {
            path.push(name);
        }
        path.push(&child.name);
        Some((path, child.dir))
    }

    /// `path` selected in the project's tree.
    fn locate(&mut self, path: PathBuf) {
        let job = ui_components::WithHost::once(move |host| {
            host.reveal_in_project(&path);
        });
        self.pending = Some(ViewerAction::WithHost(job));
    }

    /// The ring's shares: the biggest entries, then the rest as one.
    fn shares(&self) -> Vec<f32> {
        let Some(node) = self.node() else { return Vec::new() };
        let mut shares: Vec<f32> =
            node.children.iter().take(SLICES).map(|c| c.size as f32).collect();
        let rest: u64 = node.children.iter().skip(SLICES).map(|c| c.size).sum();
        if rest > 0 {
            shares.push(rest as f32);
        }
        shares
    }

    /// The slice an entry of the list is in (its own, or the rest's).
    fn slice_of(&self, row: usize) -> usize {
        row.min(SLICES)
    }

    fn lit(&self) -> Option<usize> {
        match self.hover {
            Some(Hit::Slice(s)) => Some(s),
            Some(Hit::Row(r) | Hit::Locate(r)) => Some(self.slice_of(r)),
            _ => None,
        }
    }
}

/// The slices' colors, from the theme (its accent, then its terminal's colors, the accent's
/// own hue — blue in most themes — last), the rest muted.
fn colors(p: &ui_components::Palette, count: usize) -> Vec<Rgba> {
    let ansi = &p.terminal_ansi;
    let mut out = vec![p.accent, ansi[2], ansi[3], ansi[5], ansi[6], ansi[1], ansi[4], ansi[11]];
    out.truncate(SLICES.min(count));
    if count > SLICES {
        out.push(p.text_muted.with_alpha(0.45));
    }
    out
}

/// `2026-11-15 20:10`, in the user's own time zone.
fn when(at: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(at, 0)
        .single()
        .map_or_else(String::new, |d| d.format("%Y-%m-%d %H:%M").to_string())
}

impl Viewer for SizesView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<SizesData>() else { return };
        self.data = Some(data.clone());
        self.report = None;
        self.trail.clear();
        self.load();
    }

    fn poll(&mut self) -> bool {
        self.take() || self.measuring.is_some()
    }

    fn is_loading(&self) -> bool {
        self.loading.is_some() || self.measuring.is_some()
    }

    fn is_animating(&self) -> bool {
        self.measuring.is_some() || (self.loading.is_some() && self.report.is_none())
    }

    fn command(&mut self, id: &str) -> bool {
        if id == "refresh" {
            self.measure();
            return true;
        }
        let Some((path, dir)) = self.menu_on.take() else { return false };
        match id {
            LOCATE => self.locate(path),
            OPEN if !dir => {
                self.pending = Some(ViewerAction::OpenFile { path, line: None, column: None });
            }
            REVEAL => {
                let job = ui_components::WithHost::once(move |host| {
                    host.reveal_in_file_manager(&path);
                });
                self.pending = Some(ViewerAction::WithHost(job));
            }
            _ => return false,
        }
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    /// A row's menu: to its place in the project's tree, opened (a file), in the system's
    /// file manager.
    fn context_menu(&mut self, p: Point) -> Option<Vec<ui_components::ViewMenuEntry>> {
        use ui_components::ViewMenuEntry;
        let (Hit::Row(i) | Hit::Locate(i) | Hit::Slice(i)) = self.hit(p)? else { return None };
        let (path, dir) = self.path_of(i)?;
        self.menu_on = Some((path, dir));
        let mut rows = vec![ViewMenuEntry::item(LOCATE, t("sizes.locate")).with_icon("crosshair")];
        if !dir {
            rows.push(ViewMenuEntry::item(OPEN, t("sizes.open")).with_icon("file"));
        }
        rows.push(ViewMenuEntry::item(REVEAL, t("sizes.reveal")).with_icon("folder"));
        Some(rows)
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        match key {
            ViewerKey::Backspace | ViewerKey::Left if !self.trail.is_empty() => {
                self.trail.pop();
                self.scroll = 0.0;
                self.ring = None;
                true
            }
            _ => false,
        }
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        match self.hit(p)? {
            Hit::Refresh => self.measure(),
            Hit::Crumb(depth) => {
                self.trail.truncate(depth);
                self.scroll = 0.0;
                self.ring = None;
            }
            Hit::Locate(i) => {
                if let Some((path, _)) = self.path_of(i) {
                    self.locate(path);
                }
            }
            // A folder gone into; a file opened.
            Hit::Row(i) | Hit::Slice(i) => {
                if !self.enter(i)
                    && let Some((path, false)) = self.path_of(i)
                {
                    return Some(ViewerAction::OpenFile { path, line: None, column: None });
                }
            }
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p)? {
            Hit::Row(i) | Hit::Slice(i) => {
                let child = self.node()?.children.get(i)?;
                (!child.other && (!child.dir || !child.children.is_empty()))
                    .then_some(ViewerCursor::Pointer)
            }
            _ => Some(ViewerCursor::Pointer),
        }
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let count = self.node().map_or(0, |n| n.children.len());
        let content = count as f32 * self.row_h;
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, (content - self.list.size.height).max(0.0));
        self.scroll != before
    }

    fn status(&self) -> Option<String> {
        let node = self.node()?;
        Some(format!("{} · {}", i18n::bytes(node.size), tr!("sizes.files", count = node.files)))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if let Some(wake) = &ctx.waker {
            self.waker = Some(Wake(Arc::clone(wake)));
        }
        self.take();
        self.hits.clear();
        self.ring_at = Rect::default();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        let width = px(980.0).min(b.size.width - px(48.0));
        let x0 = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = b.min_y() + px(28.0);

        // The title, and Refresh on its right.
        let title_font = ds.ui_font(4.0).with_weight(Font::SEMIBOLD);
        let title = ctx.text.shape_line(&t("sizes.title"), &title_font);
        draw_text(layer, &title, x0, y + title_font.size, p.text);
        let label = t("sizes.refresh");
        let bw = ds::toolbar_button_width(ctx.text, &ds, &label);
        let button = Rect::new(x0 + width - bw, y, bw, px(ds::TOOLBAR_BUTTON_H));
        let spec = ds::ToolbarButton {
            label: &label,
            icon: "refresh",
            accent: false,
            hovered: self.hover == Some(Hit::Refresh),
        };
        if self.measuring.is_some() {
            ds::busy_toolbar_button(layer, ctx.text, ctx.icons, &ds, button, spec);
        } else {
            ds::toolbar_button(layer, ctx.text, ctx.icons, &ds, button, spec);
            self.hits.push((button, Hit::Refresh));
        }
        y += px(46.0);

        // What it holds, when it was measured; or how far the measure has come.
        let small = ds.ui_font(-1.0);
        let line = if let Some(m) = &self.measuring {
            tr!(
                "sizes.measuring",
                files = m.progress.files.load(Ordering::Relaxed),
                size = i18n::bytes(m.progress.bytes.load(Ordering::Relaxed)).as_str()
            )
        } else if let Some(r) = &self.report {
            let mut line = tr!(
                "sizes.measured",
                size = i18n::bytes(r.root.size).as_str(),
                files = r.root.files,
                when = when(r.at).as_str(),
                ago = i18n::ago(report::now() - r.at).as_str()
            );
            if r.partial {
                line = format!("{line} · {}", t("sizes.partial"));
            }
            if r.unreadable > 0 {
                line = format!("{line} · {}", tr!("sizes.unreadable", count = r.unreadable));
            }
            line
        } else {
            String::new()
        };
        if self.measuring.is_some() {
            ds::loader(layer, ctx.text, &ds, x0, y + px(10.0), &ds::Loader::with_text(&line));
        } else {
            let shaped = ctx.text.shape_line(&line, &small);
            draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(20.0), &small), p.text_muted);
        }
        y += px(34.0);

        if self.report.is_none() {
            let area = Rect::new(x0, y, width, (b.max_y() - y - px(16.0)).max(0.0));
            if self.failed {
                ds::empty_state(layer, ctx.text, &ds, area, &t("sizes.failed"), None);
            }
            return;
        }

        // The path gone down: each folder on it goes back there.
        let names = self.crumb_names();
        let crumb_font = ds.ui_font(-0.5);
        let mut cx = x0;
        for (depth, name) in names.iter().enumerate() {
            if depth > 0 {
                if let Some(id) = ctx.icons.id("chevron-right") {
                    draw_icon(
                        layer,
                        ctx.icons,
                        id,
                        Rect::new(cx, y + px(4.0), px(12.0), px(12.0)),
                        p.text_muted,
                    );
                }
                cx += px(18.0);
            }
            let shaped = ctx.text.shape_line(name, &crumb_font);
            let r = Rect::new(cx - px(4.0), y - px(2.0), shaped.width + px(8.0), px(24.0));
            let last = depth + 1 == names.len();
            if !last {
                if self.hover == Some(Hit::Crumb(depth)) {
                    layer.quad(Quad::filled(r, p.tree_hover).rounded(px(6.0)));
                }
                self.hits.push((r, Hit::Crumb(depth)));
            }
            let color = if last { p.text } else { p.accent };
            draw_text(
                layer,
                &shaped,
                cx,
                ds::optical_baseline(y - px(2.0), px(24.0), &crumb_font),
                color,
            );
            cx += shaped.width + px(6.0);
        }
        y += px(40.0);

        let Some(node) = self.node().cloned() else { return };
        let shares = self.shares();
        let palette_colors = colors(p, node.children.len());

        // The ring, its center saying the folder's size.
        let ring_size = px(RING).min(width * 0.4);
        let ring = Rect::new(x0, y, ring_size, ring_size);
        self.ring_at = ring;
        if !shares.is_empty() {
            let wanted = Ring {
                shares,
                colors: palette_colors.clone(),
                lit: self.lit(),
                size: ring_size.round().max(1.0) as u32,
            };
            if self.ring.as_ref().is_none_or(|(had, _)| *had != wanted) {
                let image = wanted.image();
                self.ring = Some((wanted, image));
            }
            if let Some((_, image)) = &self.ring {
                layer.image(image, ring);
            }
        }
        let big = ds.ui_font(3.0).with_weight(Font::SEMIBOLD);
        let size_line = ctx.text.shape_line(&i18n::bytes(node.size), &big);
        let mid = ring.min_y() + ring.size.height / 2.0;
        draw_text(
            layer,
            &size_line,
            ring.min_x() + (ring.size.width - size_line.width) / 2.0,
            mid + px(2.0),
            p.text,
        );
        let files = ctx.text.shape_line(&tr!("sizes.files", count = node.files), &small);
        draw_text(
            layer,
            &files,
            ring.min_x() + (ring.size.width - files.width) / 2.0,
            mid + px(22.0),
            p.text_muted,
        );

        // The list: every entry, biggest first.
        let lx = ring.max_x() + px(32.0);
        let lw = (x0 + width - lx).max(0.0);
        self.row_h = px(ROW_H);
        self.list = Rect::new(lx, y, lw, (b.max_y() - y - px(16.0)).max(0.0));
        if node.children.is_empty() {
            ds::empty_state(layer, ctx.text, &ds, self.list, &t("sizes.empty"), None);
            return;
        }
        let rows = ctx.scene.push_layer(Some(self.list));
        let font = ds.ui_font(-0.5);
        let biggest = node.children.first().map_or(1, |c| c.size).max(1);
        let first = (self.scroll / self.row_h).floor() as usize;
        let visible = (self.list.size.height / self.row_h).ceil() as usize + 2;
        let lit = self.lit();
        for (i, child) in node.children.iter().enumerate().skip(first).take(visible) {
            let top = self.list.min_y() + i as f32 * self.row_h - self.scroll;
            let row = Rect::new(lx, top, lw, self.row_h);
            let enterable = child.dir && !child.other && !child.children.is_empty();
            let hovered = matches!(self.hover, Some(Hit::Row(h) | Hit::Locate(h)) if h == i);
            if hovered || (lit == Some(self.slice_of(i)) && i < SLICES) {
                rows.quad(Quad::filled(row, p.tree_hover).rounded(px(6.0)));
            }
            if let Some(shown) = row.intersect(&self.list) {
                self.hits.push((shown, Hit::Row(i)));
            }
            // Its slice's color.
            let swatch = palette_colors.get(self.slice_of(i)).copied().unwrap_or(p.text_muted);
            rows.quad(
                Quad::filled(
                    Rect::new(
                        lx + px(8.0),
                        top + (self.row_h - px(10.0)) / 2.0,
                        px(10.0),
                        px(10.0),
                    ),
                    swatch,
                )
                .rounded(px(3.0)),
            );
            let icon = if child.dir { "folder" } else { "file" };
            if let Some(id) = ctx.icons.id(icon) {
                draw_icon(
                    rows,
                    ctx.icons,
                    id,
                    Rect::new(
                        lx + px(26.0),
                        top + (self.row_h - px(16.0)) / 2.0,
                        px(16.0),
                        px(16.0),
                    ),
                    p.text_muted,
                );
            }
            let name = if child.other {
                if child.dir {
                    tr!("sizes.other_folders", count = child.name.parse::<u64>().unwrap_or(0))
                } else {
                    tr!("sizes.other_files", count = child.name.parse::<u64>().unwrap_or(0))
                }
            } else {
                child.name.clone()
            };
            let name_w = lw * 0.42;
            let shown = ds::ellipsize(ctx.text, &name, &font, name_w);
            let shaped = ctx.text.shape_line(&shown, &font);
            let baseline = ds::optical_baseline(top, self.row_h, &font);
            draw_text(
                rows,
                &shaped,
                lx + px(50.0),
                baseline,
                if enterable { p.text } else { p.text_muted },
            );
            // On hover: its button to its place in the project's tree.
            let locate = Rect::new(
                lx + px(50.0) + name_w + px(6.0),
                top + (self.row_h - px(24.0)) / 2.0,
                px(24.0),
                px(24.0),
            );
            if hovered
                && !child.other
                && let Some(id) = ctx.icons.id("crosshair")
            {
                if self.hover == Some(Hit::Locate(i)) {
                    rows.quad(Quad::filled(locate, p.text.with_alpha(0.08)).rounded(px(6.0)));
                }
                let icon = Rect::new(
                    locate.min_x() + px(4.0),
                    locate.min_y() + px(4.0),
                    px(16.0),
                    px(16.0),
                );
                draw_icon(rows, ctx.icons, id, icon, p.accent);
                self.hits.push((locate, Hit::Locate(i)));
            }
            // Its bar, beside its size and its share.
            let bar_x = lx + px(50.0) + name_w + px(38.0);
            let bar_w = (lw - (bar_x - lx) - px(150.0)).max(0.0);
            let track = Rect::new(bar_x, top + (self.row_h - px(6.0)) / 2.0, bar_w, px(6.0));
            rows.quad(Quad::filled(track, p.text.with_alpha(0.07)).rounded(px(3.0)));
            let fill = (child.size as f32 / biggest as f32).clamp(0.0, 1.0) * bar_w;
            rows.quad(
                Quad::filled(Rect::new(bar_x, track.min_y(), fill.max(px(2.0)), px(6.0)), swatch)
                    .rounded(px(3.0)),
            );
            let share =
                if node.size > 0 { child.size as f64 * 100.0 / node.size as f64 } else { 0.0 };
            let size_text = ctx.text.shape_line(&i18n::bytes(child.size), &font);
            draw_text(rows, &size_text, lx + lw - px(64.0) - size_text.width, baseline, p.text);
            let pct = ctx.text.shape_line(&i18n::decimal(format!("{share:.1}%")), &small);
            draw_text(rows, &pct, lx + lw - pct.width - px(6.0), baseline, p.text_muted);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
