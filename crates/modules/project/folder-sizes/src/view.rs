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
use render_api::Quad;
use text_layout::Font;
use ui_components::charts::{Chart, ChartLook, ChartParts, ValueFormat};
use ui_components::control::Control;
use ui_components::ds::{self, ButtonKind, ButtonSpec, Ctx, button_size, radius};
use ui_components::{Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint};
use ui_kit::{draw_icon, draw_text};

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
/// The ring's hole, a share of its radius (percent).
const HOLE: u8 = 58;
/// A row's bar: its height, its track's alpha of the text and the least a share shows
/// (logical pixels).
const BAR_H: f32 = 6.0;
const BAR_TRACK: f32 = 0.07;
const BAR_MIN: f32 = 2.0;

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
    /// The ring: the shared chart (a pie with a hole), its data made again only when the
    /// folder shown changes (`shown`, a generation).
    chart: Chart,
    shown: u64,
    charted: Option<u64>,
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
                tracing::warn!(error = %e, "a folder's sizes weren't kept");
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
        use std::sync::mpsc::TryRecvError;
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok(kept) => {
                    self.loading = None;
                    changed = true;
                    match kept {
                        Some(kept) => self.show(kept),
                        None => self.measure(),
                    }
                }
                // The reader gone without an answer: measured anew (never waited for).
                Err(TryRecvError::Disconnected) => {
                    self.loading = None;
                    changed = true;
                    self.measure();
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        if let Some(m) = &self.measuring {
            match m.rx.try_recv() {
                Ok(measured) => {
                    self.measuring = None;
                    changed = true;
                    match measured {
                        Some(measured) => self.show(measured),
                        None => self.failed = self.report.is_none(),
                    }
                }
                // The measure gone without its result: said so (never waited for).
                Err(TryRecvError::Disconnected) => {
                    self.measuring = None;
                    changed = true;
                    self.failed = self.report.is_none();
                }
                Err(TryRecvError::Empty) => {}
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
        self.shown += 1;
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
        self.shown += 1;
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        if self.ring_at.contains(p)
            && let Some(slice) = self.chart.slice_at(p)
        {
            return Some(Hit::Slice(slice));
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

    /// The ring's slices: the biggest entries by name, then the rest as one (how many,
    /// their sum).
    fn slices(&self) -> Option<charts_model::Prepared> {
        let node = self.node()?;
        let named = node.children.iter().take(SLICES).map(|c| (c.name.clone(), c.size as f64));
        let rest: Vec<u64> = node.children.iter().skip(SLICES).map(|c| c.size).collect();
        let rest = (!rest.is_empty()).then(|| (rest.len(), rest.iter().sum::<u64>() as f64));
        Some(charts_model::Prepared::slices(named.collect(), rest))
    }

    /// The slice lit by the list: the row under the mouse's.
    fn lit_by_row(&self) -> Option<usize> {
        match self.hover {
            Some(Hit::Row(r) | Hit::Locate(r)) => Some(self.slice_of(r)),
            _ => None,
        }
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

/// The ring's look: the shared chart's, bare (no axes, no legend: the list is its legend),
/// its tooltip and its lit slice kept; the slices the chart's series colors, the rest
/// muted.
fn ring_look(ds: &Ctx<'_>, background: Rgba) -> ChartLook {
    let look = ChartLook::new(ds, background);
    let mut colors = [ds.palette.text_muted.with_alpha(0.45); SLICES + 1];
    for (i, c) in colors.iter_mut().take(SLICES).enumerate() {
        *c = look.series(i);
    }
    look.colors(&colors).parts(ChartParts::bare())
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
        self.take() | self.chart.poll()
    }

    fn is_loading(&self) -> bool {
        // The ring's slices being drawn (on a worker) too.
        self.loading.is_some() || self.measuring.is_some() || self.chart.busy()
    }

    /// Its ring moving (a transition, a slice lit). Reading and measuring aren't motion: they
    /// show as loading (`is_loading`: the progress line at a loader's pace).
    fn is_animating(&self) -> bool {
        self.chart.animating()
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
                self.shown += 1;
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
                self.shown += 1;
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
        let on_ring = self.ring_at.contains(p).then_some(p);
        let ring = Control::hover(&mut self.chart, on_ring);
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        // A row under the mouse lights its slice.
        let lit = self.chart.set_lit(self.lit_by_row());
        changed || ring || lit
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        if self.ring_at.contains(p) {
            return Control::tooltip(&self.chart, p);
        }
        None
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
        let measuring = self.measuring.is_some();
        let spec = ButtonSpec::new(ButtonKind::Toolbar)
            .label(&label)
            .icon("refresh")
            .busy(measuring)
            .hovered(self.hover == Some(Hit::Refresh));
        ds::button(layer, ctx.text, ctx.icons, &ds, button, spec);
        if !measuring {
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
                let at = Rect::new(cx, y + px(4.0), px(12.0), px(12.0));
                ui_components::tree::chevron(layer, ctx.icons, at, false, p.text_muted);
                cx += px(18.0);
            }
            let shaped = ctx.text.shape_line(name, &crumb_font);
            let r = Rect::new(cx - px(4.0), y - px(2.0), shaped.width + px(8.0), px(24.0));
            if depth + 1 < names.len() {
                // A folder gone through: a link back to it.
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .label(name)
                    .tone(ds::Tone::Accent)
                    .hovered(self.hover == Some(Hit::Crumb(depth)));
                ds::button(layer, ctx.text, ctx.icons, &ds, r, spec);
                self.hits.push((r, Hit::Crumb(depth)));
            } else {
                draw_text(
                    layer,
                    &shaped,
                    cx,
                    ds::optical_baseline(y - px(2.0), px(24.0), &crumb_font),
                    p.text,
                );
            }
            cx += shaped.width + px(6.0);
        }
        y += px(40.0);

        let Some(node) = self.node().cloned() else { return };
        let look = ring_look(&ds, p.editor_background);

        // The ring (the shared chart), its center saying the folder's size.
        let ring_size = px(RING).min(width * 0.4);
        let ring = Rect::new(x0, y, ring_size, ring_size);
        self.ring_at = ring;
        if self.charted != Some(self.shown) {
            self.charted = Some(self.shown);
            let data = self.slices().filter(|_| !node.children.is_empty());
            self.chart.set_data(data.map(Arc::new), self.shown + 1);
            // Its tooltips say sizes as the list does.
            let bytes = |_: usize, v: f64| i18n::bytes(v.max(0.0) as u64);
            self.chart.set_format(Some(ValueFormat(Arc::new(bytes))));
            self.chart.set_title(&node.name);
            let ring = charts_model::spec::Options { hole: HOLE, ..Default::default() };
            self.chart.set_options(ring);
        }
        self.chart.set_waker(&ctx.waker);
        self.chart.paint(layer, ctx.text, &ds, &look, ring);
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
            let shown = hovered || (lit == Some(self.slice_of(i)) && i < SLICES);
            ds::RowHighlight::new(false, shown)
                .hover(ds::HoverFill::Tree)
                .radius(radius::SM)
                .paint(rows, &ds, row);
            if let Some(shown) = row.intersect(&self.list) {
                self.hits.push((shown, Hit::Row(i)));
            }
            // Its slice's color.
            let swatch = look.series(self.slice_of(i));
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
            let locate = ds::square_button(
                &ds,
                (lx + px(50.0) + name_w + px(6.0), top + self.row_h / 2.0),
                button_size::SMALL,
            );
            if hovered && !child.other {
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon("crosshair")
                    .tone(ds::Tone::Accent)
                    .hovered(self.hover == Some(Hit::Locate(i)));
                ds::button(rows, ctx.text, ctx.icons, &ds, locate, spec);
                self.hits.push((locate, Hit::Locate(i)));
            }
            // Its bar, beside its size and its share.
            let bar_x = lx + px(50.0) + name_w + px(38.0);
            let bar_w = (lw - (bar_x - lx) - px(150.0)).max(0.0);
            let bar_h = px(BAR_H);
            let track = Rect::new(bar_x, top + (self.row_h - bar_h) / 2.0, bar_w, bar_h);
            ds::ProgressBar::of(child.size as f32 / biggest as f32)
                .color(swatch)
                .track(ds::ProgressTrack::Faint(BAR_TRACK))
                .min_fill(BAR_MIN)
                .paint_bar(rows, &ds, track);
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
