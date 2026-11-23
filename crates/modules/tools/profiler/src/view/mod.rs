//! A recording shown (a `profile-….json` file of the app's recordings folder, opened in its
//! tab). The file is read, summed up and made into its page ([`prepared`]: texts, rows,
//! charts' values, flame nodes) on a worker (`profiler-read`), never on the interface's
//! thread; the recording itself is let go then. The page ([`page`]) says, top to bottom:
//!
//! - what it is and where it ran, with "Copy Summary" (Markdown, for a bug report) and
//!   "Show in Folder";
//! - **What stands out** ([`profiling::Finding`]): each with where to look and what to do;
//! - the figures as metric cards with their charts; the timeline, the values under the
//!   mouse;
//! - tables: the modules by CPU, who asks for frames and why, what takes the interface's
//!   time, the flame graph, the network and the disk, the threads, the parts' own probes.
//!
//! A frame only draws: the plots are made again only when the width changes. The wheel, the
//! scrollbar's thumb and the keys (↑ ↓ Page Up / Down Home End; the flame graph's when the
//! mouse is over it) scroll it.

mod flame;
pub(crate) mod page;
mod prepared;
pub mod words;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use i18n::t;
use kernel::{Point, Rect};
use profiling::Recording;
use render_api::Quad;
use ui_components::charts::spark::Plot;
use ui_components::charts::{ChartLook, ChartParts};
use ui_components::control::{Control, Controls, Shown, Thumb};
use ui_components::ds::{self, Ctx};
use ui_components::{
    AppRequest, EdgeScrollbar, KeyReach, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};

use page::{Col, Hit, Page};
use prepared::Prepared;

/// The thread reading a recording.
pub const THREAD: &str = profiling_collect::READ_THREAD;

/// A line's height for the keys' scroll (logical pixels).
const LINE: f32 = 40.0;

/// What reading the file gave.
enum Read {
    Done(Box<Prepared>),
    Failed(String),
}

/// The cards' plots for a width (by series), made again only when the columns change.
#[derive(Default)]
struct Plots {
    columns: usize,
    cards: Vec<Plot>,
}

/// A recording's view.
pub struct ProfileView {
    path: PathBuf,
    asked: Option<Receiver<Read>>,
    read: Option<Result<Box<Prepared>, String>>,
    plots: Plots,
    scroll: f32,
    content_h: f32,
    view_h: f32,
    bar: EdgeScrollbar,
    hits: Vec<(Rect, Hit)>,
    hover: Option<Hit>,
    mouse: Option<Point>,
    timeline: Option<Rect>,
    /// The timeline's chart (the app's one: lanes along time, its tooltip), its data made
    /// once a recording is read.
    chart: ui_components::charts::Chart,
    /// The flame graph (its zoom and hover kept), and where it was drawn.
    flame: ui_components::Icicle,
    flame_area: Option<Rect>,
    /// Its parts' state of use (see [`ProfileView::with_controls`]).
    controls: Controls,
}

impl std::fmt::Debug for ProfileView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProfileView").field("path", &self.path).finish_non_exhaustive()
    }
}

/// Whether `name` is a recording's (`profile-<date>-<time>.json`, or `-<n>` after it when
/// two were made in the same second).
pub fn is_recording(name: &str) -> bool {
    let Some(stamp) = name.strip_prefix("profile-").and_then(|r| r.strip_suffix(".json")) else {
        return false;
    };
    let (stamp, n) = match stamp.get(15..) {
        Some(rest) if !rest.is_empty() => (&stamp[..15], rest.strip_prefix('-')),
        _ => (stamp, Some("1")),
    };
    stamp.len() == 15
        && stamp
            .bytes()
            .enumerate()
            .all(|(i, b)| (i == 8 && b == b'-') || (i != 8 && b.is_ascii_digit()))
        && n.is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl ProfileView {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            asked: None,
            read: None,
            plots: Plots::default(),
            scroll: 0.0,
            content_h: 0.0,
            view_h: 0.0,
            bar: EdgeScrollbar::default(),
            hits: Vec::new(),
            hover: None,
            mouse: None,
            timeline: None,
            chart: ui_components::charts::Chart::new(),
            flame: flame::icicle(),
            flame_area: None,
            controls: Controls::default(),
        }
    }

    /// Its parts (the page's scrollbar with its offset; the flame graph while it's drawn),
    /// the topmost first, with their state of use.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut page = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        // Tab with the flame graph alone to go to leaves its keys there (nowhere else).
        let mut flame = Shown { part: &mut self.flame, shown: self.flame_area.is_some() };
        let mut parts: [&mut dyn Control; 2] = [&mut page, &mut flame];
        f(&mut self.controls, &mut parts)
    }

    /// The file asked to be read and made into its page, on a worker (once), waking the
    /// view when done.
    fn ask(&mut self, wake: Option<Arc<dyn Fn() + Send + Sync>>) {
        if self.asked.is_some() || self.read.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let path = self.path.clone();
        kernel::work::run(THREAD, move || {
            let read = match std::fs::read_to_string(&path).map_err(|e| e.to_string()) {
                Ok(text) => match Recording::from_json(&text) {
                    Ok(recording) => {
                        let summary = profiling::summary(&recording);
                        Read::Done(Box::new(prepared::prepare(&recording, &summary)))
                    }
                    Err(why) => Read::Failed(why),
                },
                Err(why) => Read::Failed(why),
            };
            let _ = tx.send(read);
            if let Some(wake) = wake {
                wake();
            }
        });
        self.asked = Some(rx);
    }

    fn take_read(&mut self) -> bool {
        let Some(read) = self.asked.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        self.asked = None;
        self.read = Some(match read {
            Read::Done(mut prepared) => {
                self.flame.set_nodes(std::mem::take(&mut prepared.flame_nodes));
                timeline_data(&mut self.chart, &prepared);
                Ok(prepared)
            }
            Read::Failed(why) => Err(why),
        });
        self.plots = Plots::default();
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, h)| *h)
    }

    /// The scroll kept within the page (a smaller window, a page grown shorter).
    fn clamp_scroll(&mut self) {
        self.scroll = self.scroll.clamp(0.0, (self.content_h - self.view_h).max(0.0)).round();
    }
}

impl Plots {
    /// The plots for `cards` columns (each card's chart), made again only when they change.
    fn for_width(&mut self, prepared: &Prepared, cards: usize) -> &Self {
        if self.columns != cards || self.cards.is_empty() {
            self.columns = cards;
            self.cards = prepared.series.iter().map(|s| page::plot(&s.values, cards)).collect();
        }
        self
    }
}

/// The timeline's series, in their lanes' order.
const TIMELINE: [usize; 4] = [prepared::CPU, prepared::FPS, prepared::NETWORK, prepared::DISK];

/// The timeline's data, made once from a recording: each series' values over its length,
/// their moments as durations (`1:20`), each value in its unit.
fn timeline_data(chart: &mut ui_components::charts::Chart, prepared: &Prepared) {
    use ui_components::charts::ValueFormat;
    let series: Vec<&prepared::Series> =
        TIMELINE.iter().filter_map(|&i| prepared.series.get(i)).collect();
    let n = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
    let step = if n > 1 { prepared.seconds / (n - 1) as f64 } else { 0.0 };
    let times: Vec<f64> = (0..n).map(|i| i as f64 * step).collect();
    let labels = times.iter().map(|&t| words::length(t)).collect();
    let units: Vec<&'static str> = series.iter().map(|s| s.unit).collect();
    let data = charts_model::Prepared::over_time(
        charts_model::ChartKind::Area,
        labels,
        times,
        series
            .iter()
            .map(|s| (s.label.clone(), s.values.iter().map(|&v| f64::from(v)).collect()))
            .collect(),
    );
    chart.set_lanes(true);
    chart.set_options(charts_model::Options { stepped: true, ..charts_model::Options::default() });
    chart.set_format(Some(ValueFormat(std::sync::Arc::new(move |s, v| {
        format!("{v:.1}{}", units.get(s).copied().unwrap_or(""))
    }))));
    chart.set_x_format(Some(ValueFormat(std::sync::Arc::new(|_, t| words::length(t)))));
    chart.set_title(&t("profiler.over_time"));
    // A recording read again (its file changed) is new data.
    chart.set_data(Some(std::sync::Arc::new(data)), kernel::id::new_uuid() as u64);
}

/// A table drawn under its title.
fn paint_table(page: &mut Page<'_, '_>, table: &prepared::Table) {
    page.title(&table.title, &table.hint);
    let cols: Vec<Col<'_>> = table
        .cols
        .iter()
        .map(|(name, sub, share, right)| Col { name, sub, share: *share, right: *right })
        .collect();
    page.table(&cols, &table.rows);
}

/// The page of a recording, from what was made of it.
fn paint_page(
    page: &mut Page<'_, '_>,
    prepared: &Prepared,
    plots: &mut Plots,
    (icicle, chart): (&mut ui_components::Icicle, &mut ui_components::charts::Chart),
) {
    let p = page.ds.palette;
    page.header(&prepared.title, &prepared.meta);
    page.title(&t("profiler.findings"), "");
    page.findings(&prepared.findings);
    let cards_w = (page.width / 4.0 / page.ds.px(3.0)).max(1.0) as usize;
    let plots = plots.for_width(prepared, cards_w);
    let cards: Vec<page::Card<'_>> = prepared
        .cards
        .iter()
        .map(|c| {
            let s = &prepared.series[c.series];
            (
                c.label.as_str(),
                c.value.clone(),
                c.sub.clone(),
                plots.cards.get(c.series),
                c.tone.color(p),
                s.top,
            )
        })
        .collect();
    page.cards(&cards);
    page.title(&t("profiler.over_time"), &t("profiler.over_time_hint"));
    let colors = TIMELINE.map(|i| prepared.series.get(i).map_or(p.accent, |s| s.tone.color(p)));
    let lanes = TIMELINE.iter().filter(|&&i| prepared.series.get(i).is_some()).count();
    let look = ChartLook::new(page.ds, p.editor_background).colors(&colors).parts(ChartParts {
        x_axis: true,
        y_axis: false,
        grid: false,
        legend: false,
        hover: true,
        // The wheel scrolls the page, as everywhere on it.
        zoom: false,
    });
    page.timeline(chart, &look, lanes);
    for table in &prepared.before_flame {
        paint_table(page, table);
    }
    page.title(&t("profiler.flame"), &t("profiler.flame_hint"));
    page.flame(icicle, prepared.flame_depth);
    for table in &prepared.after_flame {
        paint_table(page, table);
    }
}

impl Viewer for ProfileView {
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.ask(ctx.waker.clone());
        self.chart.set_waker(&ctx.waker);
        self.take_read();
        let ViewerPaint { bounds: b, scale, text, scene, palette, icons, .. } = ctx;
        let ds = Ctx::new(palette, scale);
        scene.push_layer(Some(b)).quad(Quad::filled(b, palette.editor_background));
        self.view_h = b.size.height;
        self.clamp_scroll();
        let (x, width) = page::column(&ds, b);
        let top = b.min_y() + ds.px(page::PAD) - self.scroll;
        let mut page = Page::new(scene, text, icons, &ds, (x, width, top), b);
        page.mouse = self.mouse;
        page.hover = self.hover;
        match &self.read {
            None => page.loading(),
            Some(Err(why)) => {
                let layer = page.scene.push_layer(Some(b));
                ds::empty_state(layer, page.text, &ds, b, &t("profiler.unreadable"), Some(why));
            }
            Some(Ok(prepared)) => {
                paint_page(
                    &mut page,
                    prepared,
                    &mut self.plots,
                    (&mut self.flame, &mut self.chart),
                );
            }
        }
        self.content_h = page.y - top + ds.px(page::PAD);
        self.hits = std::mem::take(&mut page.hits);
        self.timeline = page.timeline;
        self.flame_area = page.flame;
        self.bar.paint(scene, palette, scale, b, self.content_h, self.scroll);
    }

    fn poll(&mut self) -> bool {
        // The timeline's lines rasterized meanwhile.
        self.chart.poll() | self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.asked.is_some()
    }

    /// The timeline appearing or lit under the mouse.
    fn is_animating(&self) -> bool {
        self.chart.animating() || self.controls.is_animating()
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        // Over a part that scrolls (a zoomed flame graph), it scrolls itself first.
        if let Some(m) = self.mouse
            && self.with_controls(|c, parts| c.scroll(parts, m, dx, dy)).is_some()
        {
            return true;
        }
        let before = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        self.scroll != before
    }

    /// It says when a move changes what's under the mouse (the timeline's reading, a row,
    /// a button): nothing is drawn again for a move that changes nothing.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let on_timeline = self.timeline.is_some_and(|r| r.contains(p));
        let was_on = self.mouse.is_some_and(|m| self.timeline.is_some_and(|r| r.contains(m)));
        let lit = self.chart.hover(on_timeline.then_some(p));
        let changed = hover != self.hover || lit || (was_on && !on_timeline);
        self.hover = hover;
        self.mouse = Some(p);
        // The keys go to the part under the mouse that takes them (the flame graph's walk).
        self.with_controls(|c, parts| {
            let changed = c.hover(parts, Some(p));
            c.focus(parts, c.hovered());
            changed
        }) | changed
    }

    /// A hand on what's clicked (a button, a part's own: a bar of the flame graph).
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.hit(p).is_some() {
            return Some(ViewerCursor::Pointer);
        }
        Some(self.controls.cursor_at(p).unwrap_or(ViewerCursor::Arrow))
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        // The timeline's reading: every series at the moment under the mouse.
        if self.timeline.is_some_and(|r| r.contains(p)) {
            return self.chart.tooltip(p);
        }
        self.controls.tooltip_at(p)
    }

    /// A click (as a tab or a modal window sends it: both come here).
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // On its parts: the scrollbar's thumb taken (or the page jumped there, a drag
        // follows), a bar of the flame graph chosen.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        match self.hit(p)? {
            Hit::Copy => {
                let Some(Ok(prepared)) = &self.read else { return None };
                Some(ViewerAction::Request(AppRequest::CopyText(prepared.text.clone())))
            }
            Hit::Reveal => Some(ViewerAction::Request(AppRequest::Reveal(self.path.clone()))),
        }
    }

    /// Its scrollbar's thumb dragged: the page follows.
    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, changed)| changed)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    /// The keys: the part's under the mouse that takes them (the flame graph's), else the
    /// page's scroll.
    fn edit_key(&mut self, key: ViewerKey) -> bool {
        // Only a part that has them: with none, Tab isn't one to give them to one.
        if self.controls.focused().is_some()
            && self.with_controls(|c, parts| c.key(parts, key, KeyReach::default())).is_some()
        {
            return true;
        }
        let page = (self.view_h - LINE).max(LINE);
        let to = match key {
            ViewerKey::Up => self.scroll - LINE,
            ViewerKey::Down => self.scroll + LINE,
            ViewerKey::PageUp => self.scroll - page,
            ViewerKey::PageDown => self.scroll + page,
            ViewerKey::Home => 0.0,
            ViewerKey::End => self.content_h,
            _ => return false,
        };
        self.scroll = to;
        self.clamp_scroll();
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
