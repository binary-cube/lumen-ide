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
use ui_components::chart::Plot;
use ui_components::ds::{self, Ctx};
use ui_components::{
    AppRequest, EdgeScrollbar, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
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

/// The plots for a width: the cards' (by series) and the timeline's (by series), made again
/// only when the columns change.
#[derive(Default)]
struct Plots {
    columns: (usize, usize),
    cards: Vec<Plot>,
    lines: Vec<Plot>,
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
    /// The flame graph (its zoom and hover kept), and where it was drawn.
    flame: ui_components::Icicle,
    flame_area: Option<Rect>,
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
            flame: flame::icicle(),
            flame_area: None,
        }
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
    /// The plots for `cards` columns (each card's chart) and `lines` columns (the
    /// timeline's), made again only when they change.
    fn for_width(&mut self, prepared: &Prepared, cards: usize, lines: usize) -> &Self {
        if self.columns != (cards, lines) || self.cards.is_empty() {
            self.columns = (cards, lines);
            self.cards = prepared.series.iter().map(|s| page::plot(&s.values, cards)).collect();
            self.lines = prepared.series.iter().map(|s| page::plot(&s.values, lines)).collect();
        }
        self
    }
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
    icicle: &mut ui_components::Icicle,
) {
    let p = page.ds.palette;
    page.header(&prepared.title, &prepared.meta);
    page.title(&t("profiler.findings"), "");
    page.findings(&prepared.findings);
    let cards_w = (page.width / 4.0 / page.ds.px(3.0)).max(1.0) as usize;
    let line_w = ((page.width - page.ds.px(160.0)) / page.ds.px(2.0)).max(1.0) as usize;
    let plots = plots.for_width(prepared, cards_w, line_w);
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
    let timeline: Vec<page::Series<'_>> =
        [prepared::CPU, prepared::FPS, prepared::NETWORK, prepared::DISK]
            .iter()
            .filter_map(|&i| {
                let s = prepared.series.get(i)?;
                Some((
                    s.label.as_str(),
                    plots.lines.get(i)?,
                    s.tone.color(p),
                    s.top,
                    s.values.as_slice(),
                    s.unit,
                ))
            })
            .collect();
    page.timeline(&timeline, prepared.seconds);
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
            Some(Ok(prepared)) => paint_page(&mut page, prepared, &mut self.plots, &mut self.flame),
        }
        self.content_h = page.y - top + ds.px(page::PAD);
        self.hits = std::mem::take(&mut page.hits);
        self.timeline = page.timeline;
        self.flame_area = page.flame;
        self.bar.paint(scene, palette, scale, b, self.content_h, self.scroll);
    }

    fn poll(&mut self) -> bool {
        self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.asked.is_some()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        // Over the flame graph, a zoomed one scrolls itself first.
        if self.mouse.is_some_and(|m| self.flame_area.is_some_and(|r| r.contains(m)))
            && self.flame.scroll(dy)
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
        let changed = hover != self.hover || on_timeline || was_on;
        self.hover = hover;
        self.mouse = Some(p);
        let flame = if self.flame_area.is_some_and(|r| r.contains(p)) {
            self.flame.hover(p)
        } else {
            self.flame.leave()
        };
        self.bar.hover(p) | changed | flame
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let on_flame =
            self.flame_area.is_some_and(|r| r.contains(p)) && self.flame.node_at(p).is_some();
        Some(if self.hit(p).is_some() || on_flame {
            ViewerCursor::Pointer
        } else {
            ViewerCursor::Arrow
        })
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::Tip> {
        self.flame_area.filter(|r| r.contains(p)).and_then(|_| self.flame.tooltip_at(p))
    }

    /// A click (as a tab or a modal window sends it: both come here).
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // On its scrollbar: the thumb taken (or the page jumped there); a drag follows.
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        if self.flame_area.is_some_and(|r| r.contains(p)) {
            self.flame.press(p);
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
        let Some(offset) = self.bar.drag(p) else { return false };
        let changed = offset != self.scroll;
        self.scroll = offset;
        changed
    }

    fn release(&mut self) -> bool {
        self.bar.release()
    }

    /// The keys: the flame graph's while the mouse is over it, else the page's scroll.
    fn edit_key(&mut self, key: ViewerKey) -> bool {
        if self.mouse.is_some_and(|m| self.flame_area.is_some_and(|r| r.contains(m)))
            && self.flame.key(key)
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
