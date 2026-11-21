//! The Remote tool window's "Changed" tab: the files changed here in a span of time, each
//! once (newest first) on a timeline — each day a dot on a line down its left side, named,
//! its files under it with their times —, and whether the server has each as it is or it's
//! still to upload. The span is chosen above the list: the last hour, the last 4 hours (at
//! first), today, the last 24 hours, the last 7 days, or days picked on a calendar; a search
//! beside it narrows the list by the files' paths.
//!
//! The local copy is walked on a worker (`remote-changed`): when the tab first shows, when
//! the span changes, and after the project's files change (a save, a download, the disk) —
//! only while the tab is painted (hidden, it reads nothing). A click opens the file; its
//! menu copies its path (or all of them) and shows its properties on the server.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;

use i18n::{t, tr};
use kernel::{Point, Rect};
use render_api::Quad;
use ui_components::control::{Control, Controls, Thumb};
use ui_components::date_range::{RangeAnswer, RangePart};
use ui_components::ds::{self, Ctx, Tone, radius};
use ui_components::search_field::SearchField;
use ui_components::{
    AppRequest, Date, DateRangePicker, EdgeScrollbar, KeyReach, PickerEvent, ViewMenuEntry, Viewer,
    ViewerAction, ViewerCursor, ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};
use ui_kit::draw_text;

use crate::local_time;
use crate::session::RecentFile;

mod search;
mod timeline;

use timeline::{Item, Stamp};

/// The extension of the view (in the Remote tool window).
pub const EXTENSION: &str = "view-remote-changed";

/// What the view is opened with: the remote project, its events (it follows the files'
/// changes through them), and its queries (the files' icons).
#[derive(Clone)]
pub struct ChangedData {
    pub project: kernel::ProjectId,
    pub events: kernel::EventBus,
    pub queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
}

#[derive(Debug)]
pub(crate) struct ChangedProvider;

impl ViewerProvider for ChangedProvider {
    fn id(&self) -> &'static str {
        "remote.changed"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ChangedFiles::default()))
    }
}

/// The span of time shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    /// The last hours, up to now.
    Hours(u32),
    /// Since today's local midnight.
    Today,
    /// Days picked (both whole, local).
    Days(Date, Date),
}

impl Span {
    /// The quick choices above the list, in order (the last 4 hours at first).
    pub const QUICK: [Span; 5] =
        [Span::Hours(1), Span::Hours(4), Span::Today, Span::Hours(24), Span::Hours(24 * 7)];
    pub const DEFAULT: Span = Span::Hours(4);

    /// When it starts and ends (`None`: now), at `now` (seconds since the epoch).
    pub fn bounds(self, now: i64) -> (i64, Option<i64>) {
        match self {
            Span::Hours(h) => (now.saturating_sub(i64::from(h) * 3600), None),
            Span::Today => (local_time::day_start(local_time::today()).unwrap_or(now), None),
            Span::Days(a, b) => {
                let start = local_time::day_start(a).unwrap_or(0);
                (start, local_time::day_start(b.add_days(1)))
            }
        }
    }

    fn label(self) -> String {
        match self {
            Span::Hours(h) if h >= 48 && h % 24 == 0 => tr!("remote.changed_days", count = h / 24),
            Span::Hours(h) => tr!("remote.changed_hours", count = h),
            Span::Today => t("remote.changed_today"),
            Span::Days(a, b) => {
                ui_components::DateLocale::current().range((a, b), local_time::today())
            }
        }
    }
}

/// The places of its parts (see [`ChangedFiles::with_controls`]).
const CALENDAR: usize = 0;
const SEARCH: usize = 1;

/// What can be pressed above the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Chip {
    Quick(usize),
    /// The days, on a calendar.
    Pick,
}

/// The rows' menu.
const COPY_PATH: &str = "remote.changed.copy_path";
const COPY_ALL: &str = "remote.changed.copy_all";
const PROPERTIES: &str = "remote.changed.properties";

pub struct ChangedFiles {
    project: Option<kernel::ProjectId>,
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    span: Span,
    /// The files as last read (for the span read), each with its stamp (same order).
    rows: Vec<RecentFile>,
    stamps: Vec<Stamp>,
    read_for: Option<Span>,
    /// Counts the walks taken in (what the list was made for).
    generation: u64,
    /// A walk going on, and the span it's for.
    reading: Option<(Span, Receiver<Read>)>,
    /// The project's files changed since the last walk (heard on its events).
    stale: Arc<AtomicBool>,
    subscriptions: Vec<kernel::Subscription>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    chips: Vec<(Rect, Chip)>,
    hover: Option<Chip>,
    picker: Option<DateRangePicker>,
    /// Where "Days…" is (the calendar opens under it).
    pick_rect: Rect,
    /// What's searched for in the paths (the app's search field: it has the keys once
    /// clicked into), and where it's laid out.
    query: SearchField,
    field: Rect,
    scroll: f32,
    max_scroll: f32,
    bar: EdgeScrollbar,
    /// Its controls' state of use (see [`ChangedFiles::with_controls`]).
    controls: Controls,
    /// The last press closed the calendar by landing outside it.
    closed_outside: bool,
    list: Rect,
    /// The rows shown (the span's, the search's), newest first: what the menu copies.
    shown: Vec<usize>,
    /// The span's rows (before the search) they were made from.
    in_span: usize,
    /// The list: the days and their files, and where each starts (logical, see
    /// [`timeline::tops`]).
    items: Vec<Item>,
    tops: Vec<f32>,
    scale: f32,
    /// What `shown` and `items` were made for: the walk, the span, the search, the span's rows.
    made_for: Option<(u64, Span, String, usize)>,
    hovered_row: Option<usize>,
    menu: Option<String>,
    action: Option<ViewerAction>,
    /// Made-up files shown without a project (snapshots).
    #[cfg(feature = "snapshot")]
    sample: bool,
}

impl Default for ChangedFiles {
    fn default() -> Self {
        Self {
            project: None,
            queries: None,
            span: Span::DEFAULT,
            rows: Vec::new(),
            stamps: Vec::new(),
            read_for: None,
            generation: 0,
            reading: None,
            stale: Arc::default(),
            subscriptions: Vec::new(),
            waker: None,
            chips: Vec::new(),
            hover: None,
            picker: None,
            pick_rect: Rect::default(),
            query: SearchField::default(),
            field: Rect::default(),
            scroll: 0.0,
            max_scroll: 0.0,
            bar: EdgeScrollbar::default(),
            controls: Controls::default(),
            closed_outside: false,
            list: Rect::default(),
            shown: Vec::new(),
            in_span: 0,
            items: Vec::new(),
            tops: Vec::new(),
            scale: 1.0,
            made_for: None,
            hovered_row: None,
            menu: None,
            action: None,
            #[cfg(feature = "snapshot")]
            sample: false,
        }
    }
}

impl std::fmt::Debug for ChangedFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChangedFiles")
            .field("span", &self.span)
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}

/// A walk's files, with their stamps.
type Read = (Vec<RecentFile>, Vec<Stamp>);

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The rows of `rows` in `span` at `now` (what was read may have aged out since: a span up
/// to now moves on).
pub fn in_span(rows: &[RecentFile], span: Span, now: i64) -> Vec<usize> {
    let (from, to) = span.bounds(now);
    let inside = |r: &RecentFile| r.modified >= from && to.is_none_or(|to| r.modified < to);
    rows.iter().enumerate().filter(|(_, r)| inside(r)).map(|(i, _)| i).collect()
}

impl ChangedFiles {
    /// A walk of the local copy for the span (when none runs): on a worker, woken when done.
    fn read(&mut self) {
        let Some(state) = self.project.and_then(crate::session::session) else { return };
        let span = self.span;
        let (from, to) = span.bounds(now());
        let waker = self.waker.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("remote-changed", move || {
            let rows = state.changed_between(from, to);
            let stamps = rows.iter().map(Stamp::of).collect();
            let _ = tx.send((rows, stamps));
            if let Some(wake) = waker {
                wake();
            }
        });
        self.stale.store(false, Ordering::Relaxed);
        self.reading = Some((span, rx));
    }

    /// The walk's files taken in: `true` if they came.
    fn take_read(&mut self) -> bool {
        let Some((span, rx)) = &self.reading else { return false };
        let Ok((rows, stamps)) = rx.try_recv() else { return false };
        self.read_for = Some(*span);
        (self.rows, self.stamps) = (rows, stamps);
        self.generation += 1;
        self.reading = None;
        true
    }

    /// Read again when what's shown isn't the span's, or the files changed since.
    fn read_if_needed(&mut self) {
        if self.reading.is_some() {
            return;
        }
        if self.read_for != Some(self.span) || self.stale.load(Ordering::Relaxed) {
            self.read();
        }
    }

    fn choose(&mut self, span: Span) {
        if span != self.span {
            self.span = span;
            self.scroll = 0.0;
            self.hovered_row = None;
        }
    }

    fn chip_at(&self, p: Point) -> Option<Chip> {
        self.chips.iter().find(|(r, _)| r.contains(p)).map(|(_, c)| *c)
    }

    /// The file (an index in `rows`) under `p` (a day's row has none).
    fn row_at(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) {
            return None;
        }
        let scale = self.scale.max(0.1);
        let scroll = self.scroll.round() / scale;
        // Under the day pinned at the top: its heading, not the file passing beneath.
        if let Some(pinned) = timeline::pinned(&self.items, &self.tops, scroll)
            && (p.y - self.list.min_y()) / scale < timeline::DAY_H - pinned.pushed
        {
            return None;
        }
        let y = (p.y - self.list.min_y()) / scale + scroll;
        match self.items.get(timeline::item_at(&self.tops, y)?)? {
            Item::File(i) => Some(*i),
            Item::Day { .. } => None,
        }
    }

    /// The list for the span's rows `span_rows` (newest first), made again only when the
    /// walk, the span, the search or those rows changed.
    fn make_list(&mut self, span_rows: Vec<usize>) {
        let key = (self.generation, self.span, self.query.text().to_owned(), span_rows.len());
        if self.made_for.as_ref() == Some(&key) {
            return;
        }
        let words = timeline::words(self.query.text());
        self.in_span = span_rows.len();
        self.shown = span_rows
            .into_iter()
            .filter(|&i| self.stamps.get(i).is_some_and(|s| timeline::matches(&s.lower, &words)))
            .collect();
        self.items = timeline::items(&self.shown, &self.stamps);
        self.tops = timeline::tops(&self.items);
        self.made_for = Some(key);
    }

    /// The search changed: the list from its top.
    fn searched(&mut self) {
        self.scroll = 0.0;
        self.hovered_row = None;
    }

    /// Its controls, the topmost first, with their state of use: the calendar while it's
    /// open (over everything), the search, the list's scrollbar. What they said is done
    /// after: a range chosen, the calendar closed by a press outside it, the search changed
    /// or left (the keys go nowhere then).
    fn with_controls<R: Default>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut f = Some(f);
        let mut said = None;
        // Called once, always: what `f` said.
        self.use_parts(&mut |c, parts| said = f.take().map(|f| f(c, parts)));
        said.unwrap_or_default()
    }

    /// [`Self::with_controls`] without a result (not generic: it calls itself to let the
    /// keys go).
    fn use_parts(&mut self, f: &mut dyn FnMut(&mut Controls, &mut [&mut dyn Control])) {
        let (mut answer, mut edited, mut left) = (None, false, false);
        let open = self.picker.is_some();
        {
            // While it's closed, its place takes nothing.
            let mut calendar =
                self.picker.as_mut().map(|picker| RangePart { picker, answer: &mut answer });
            let mut search =
                search::SearchPart { field: &mut self.query, edited: &mut edited, left: &mut left };
            let mut list = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
            let mut parts: [&mut dyn Control; 3] = [&mut calendar, &mut search, &mut list];
            f(&mut self.controls, &mut parts);
        }
        match answer {
            Some(RangeAnswer::Chose(event)) => self.picker_event(event),
            Some(RangeAnswer::Outside) => {
                self.picker = None;
                self.closed_outside = true;
            }
            None => {}
        }
        if edited {
            self.searched();
        }
        if left || (open && self.picker.is_none()) {
            self.use_parts(&mut |c, parts| c.focus(parts, None));
        }
    }

    /// The search has the keys.
    fn typing(&self) -> bool {
        self.controls.focused() == Some(SEARCH)
    }

    fn open_picker(&mut self) {
        let today = local_time::today();
        let presets = ui_components::date_range::standard_presets(today, false);
        let range = match self.span {
            Span::Days(a, b) => Some((a, b)),
            _ => None,
        };
        let mut picker = DateRangePicker::new(today, presets, range);
        picker.max = Some(today);
        self.picker = Some(picker);
        // Open, it has the keys (its fields, its days).
        self.with_controls(|c, parts| c.focus(parts, Some(CALENDAR)));
    }

    fn picker_event(&mut self, event: PickerEvent) {
        let picker = self.picker.take();
        match event {
            PickerEvent::Preset(i) => {
                let range = picker.and_then(|p| p.presets.get(i).and_then(|p| p.range));
                if let Some((a, b)) = range {
                    self.choose(Span::Days(a, b));
                }
            }
            PickerEvent::Apply(a, b) => self.choose(Span::Days(a, b)),
            PickerEvent::Cancel => {}
        }
    }

    /// The paths of the rows shown, one a line.
    fn all_paths(&self) -> String {
        let paths: Vec<&str> = self.shown.iter().map(|&i| self.rows[i].path.as_str()).collect();
        paths.join("\n")
    }

    /// The search and the span's choices above the list (on one line when there's room,
    /// else the search on its own above them), how many on the right. Returns its height.
    fn paint_header(&mut self, pen: &mut ui_components::LayerPainter<'_>, b: Rect) -> f32 {
        let (layer, text, icons, ds) = (&mut *pen.layer, &mut *pen.text, pen.icons, &pen.ds);
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let line_h = px(HEADER_H);
        let labels: Vec<String> = Span::QUICK.iter().map(|s| s.label()).collect();
        let pick = match self.span {
            Span::Days(..) => self.span.label(),
            _ => t("remote.changed_pick"),
        };
        let chips_w: f32 =
            labels.iter().map(|l| ds::choice_width(text, ds, l) + px(6.0)).sum::<f32>()
                + px(6.0)
                + ds::choice_width(text, ds, &pick);
        let room = b.size.width - px(16.0) * 2.0;
        let one_line = px(FIELD_MIN) + px(12.0) + chips_w <= room;
        let field_w = if one_line {
            px(FIELD_MAX).min(room - chips_w - px(12.0))
        } else {
            px(FIELD_MAX * 1.5).min(room)
        };
        let first_mid = b.min_y() + line_h / 2.0;
        let h = px(28.0);
        self.field = Rect::new(b.min_x() + px(16.0), (first_mid - h / 2.0).round(), field_w, h);
        self.paint_search(layer, text, icons, ds);
        let (mut x, mid) = if one_line {
            (self.field.max_x() + px(12.0), first_mid)
        } else {
            (b.min_x() + px(16.0), first_mid + line_h)
        };
        self.chips.clear();
        for (i, (span, label)) in Span::QUICK.iter().zip(&labels).enumerate() {
            let chip = Chip::Quick(i);
            let spec = ds::ChoiceSpec {
                x,
                mid,
                label,
                selected: self.span == *span,
                hovered: self.hover == Some(chip),
            };
            let r = ds::choice(layer, text, ds, spec);
            self.chips.push((r, chip));
            x = r.max_x() + px(6.0);
        }
        // The days picked (their range once chosen), on a calendar.
        let spec = ds::ChoiceSpec {
            x: x + px(6.0),
            mid,
            label: &pick,
            selected: matches!(self.span, Span::Days(..)),
            hovered: self.hover == Some(Chip::Pick) || self.picker.is_some(),
        };
        let r = ds::choice(layer, text, ds, spec);
        self.chips.push((r, Chip::Pick));
        self.pick_rect = r;
        let header_h = if one_line { line_h } else { line_h * 2.0 };
        // How many, on the right (none while the first walk goes on).
        if self.read_for.is_some() {
            let count = if self.query.text().trim().is_empty() {
                tr!("remote.changed_count", count = self.shown.len())
            } else {
                tr!("remote.changed_found", found = self.shown.len(), count = self.in_span)
            };
            let small = ds.ui_font(-1.0);
            let shaped = text.shape_line(&count, &small);
            let right = b.max_x() - px(16.0);
            if right - shaped.width > r.max_x() + px(12.0) {
                let base = ds::optical_baseline(mid - line_h / 2.0, line_h, &small);
                draw_text(layer, &shaped, right - shaped.width, base, p.text_muted);
            }
        }
        header_h
    }

    /// The search field in `self.field`: the app's (pill, magnifier, what's typed, its ×;
    /// the caret while it has the keys).
    fn paint_search(
        &mut self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let typing = self.typing();
        let placeholder = t("remote.changed_search");
        self.query.paint(layer, text, icons, ds, (self.field, typing), &placeholder);
    }

    /// The list: the fine line down its left side, each day's ring and name (its date
    /// beside it), its files under it — a small dot on the line, their time, icon, path (the
    /// folder faint, the name plain) and whether the server has them.
    fn paint_rows(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let mono = ds.code_font(-1.0);
        self.scale = ds.scale;
        let end = self.tops.last().copied().unwrap_or(0.0) + timeline::PAD_BOTTOM;
        self.max_scroll = (px(end) - self.list.size.height).max(0.0);
        self.scroll = self.scroll.min(self.max_scroll);
        let scroll = self.scroll.round();
        let rows = ctx.scene.push_layer(Some(self.list));
        let y0 = self.list.min_y() - scroll;
        let at = |logical: f32| (y0 + px(logical)).round();
        let first = timeline::item_at(&self.tops, scroll / ds.scale).unwrap_or(0);
        let lane = (self.list.min_x() + px(26.0)).round();
        let line_w = px(1.0).max(1.0);
        let faint = p.text.with_alpha(0.16);
        let today = local_time::today();
        // The times' column as wide as a time is (the code font: every one as wide).
        let time_w = ctx.text.shape_line("00:00", &mono).width;
        let (to_upload, on_server) = (t("remote.changed_to_upload"), t("remote.changed_sent"));
        // Each row's middle (a day's: under its air), where the line passes by its dot.
        let middle = |n: usize| {
            let bottom = self.tops[n + 1];
            let h = if matches!(self.items[n], Item::Day { .. }) {
                timeline::DAY_H
            } else {
                timeline::FILE_H
            };
            bottom - h / 2.0
        };
        let last = self.items.len().saturating_sub(1);
        for n in first..self.items.len() {
            let (top, bottom) = (at(self.tops[n]), at(self.tops[n + 1]));
            if top > self.list.max_y() {
                break;
            }
            let mid = at(middle(n));
            let from = if n == 0 { mid } else { top };
            let to = if n == last { mid } else { bottom };
            timeline::line(rows, lane, (from, to), line_w, faint);
            let i = match self.items[n] {
                Item::Day { day, count } => {
                    let row_top = mid - px(timeline::DAY_H) / 2.0;
                    let heading = DayHeading { day, count, today, lane, top: row_top };
                    heading.paint(rows, ctx.text, ds);
                    continue;
                }
                Item::File(i) => i,
            };
            let row_h = bottom - top;
            let row = &self.rows[i];
            let r = Rect::new(
                lane + px(12.0),
                top,
                self.list.max_x() - px(6.0) - lane - px(12.0),
                row_h,
            );
            ds::RowHighlight::new(false, self.hovered_row == Some(i))
                .hover(ds::HoverFill::Faint(ROW_HOVER))
                .paint(rows, ds, r);
            timeline::dot(rows, Point::new(lane, mid), px(2.0), None, p.text.with_alpha(0.28));
            let mut x = lane + px(20.0);
            let time = self.stamps.get(i).map_or("", |s| s.time.as_str());
            let shaped = ctx.text.shape_line(time, &mono);
            let base = ds::optical_baseline(top, row_h, &mono);
            draw_text(rows, &shaped, x + time_w - shaped.width, base, p.text_muted);
            x += time_w + px(14.0);
            let name = row.path.rsplit('/').next().unwrap_or(&row.path);
            let icon = self.queries.as_ref().map_or("file", |q| q.file_icon(name).0);
            if let Some(id) = ctx.icons.id(icon) {
                let s = px(14.0);
                let at = Rect::new(x, top + (row_h - s) / 2.0, s, s);
                ui_kit::draw_icon(rows, ctx.icons, id, at, p.text_muted);
            }
            x += px(22.0);
            let (label, tone) = if row.sent {
                (on_server.as_str(), Tone::Neutral)
            } else {
                (to_upload.as_str(), Tone::Warning)
            };
            let badge_font = ds.ui_font(-1.5);
            let badge_w = ctx.text.shape_line(label, &badge_font).width + px(14.0);
            let right = self.list.max_x() - px(18.0);
            let room = (right - badge_w - px(12.0) - x).max(px(60.0));
            // The folder faint, the name plain (as the path is shown, shortened or not).
            let shown = ds::ellipsize_path(ctx.text, &row.path, &font, room);
            let cut = shown.rfind('/').map_or(0, |i| i + 1);
            let base = ds::optical_baseline(top, row_h, &font);
            let folder = ctx.text.shape_line(&shown[..cut], &font);
            draw_text(rows, &folder, x, base, p.text_muted);
            let file = ctx.text.shape_line(&shown[cut..], &font);
            draw_text(rows, &file, x + folder.width, base, p.text);
            let spec = ds::BadgeSpec { label, x: right, mid, align_right: true, tone };
            ds::badge(rows, ctx.text, ds, spec);
        }
        // On a layer of its own: over the rows' words, not only their quads.
        let pinned = ctx.scene.push_layer(Some(self.list));
        self.paint_pinned_day(pinned, ctx.text, ds, (lane, line_w, today));
        let content = self.max_scroll + self.list.size.height;
        self.bar.paint(ctx.scene, p, ctx.scale, self.list, content, scroll);
    }

    /// The day being scrolled through, pinned at the list's top once its own heading has
    /// gone above it (its files still passing under it); the next day's heading, coming up,
    /// pushes it out.
    fn paint_pinned_day(
        &self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
        (lane, line_w, today): (f32, f32, Date),
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let scroll = self.scroll.round() / ds.scale;
        let Some(pinned) = timeline::pinned(&self.items, &self.tops, scroll) else { return };
        let Item::Day { day, count } = self.items[pinned.day] else { return };
        let h = px(timeline::DAY_H);
        let top = (self.list.min_y() - px(pinned.pushed)).round();
        let band = Rect::new(self.list.min_x(), top, self.list.size.width, h);
        layer.quad(Quad::filled(band, p.editor_background));
        // The line passes behind it, as it does behind every heading.
        timeline::line(layer, lane, (band.min_y(), band.max_y()), line_w, p.text.with_alpha(0.16));
        DayHeading { day, count, today, lane, top }.paint(layer, text, ds);
        ds::divider_card(layer, ds, band.min_x(), band.max_x(), band.max_y() - ds.line());
    }
}

/// A day's heading on the line: its ring, its name, its date and how many of its files
/// are shown, in a row of [`timeline::DAY_H`] from `top`.
struct DayHeading {
    day: Date,
    count: usize,
    today: Date,
    lane: f32,
    top: f32,
}

impl DayHeading {
    fn paint(
        &self,
        layer: &mut render_api::Layer,
        text: &mut text_layout::TextSystem,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let h = px(timeline::DAY_H);
        let mid = (self.top + h / 2.0).round();
        let ring = Some((px(1.6), p.editor_background));
        timeline::dot(layer, Point::new(self.lane, mid), px(5.0), ring, p.accent);
        let day_font = ds.ui_font(0.0).with_weight(text_layout::Font::SEMIBOLD);
        let quiet = ds.ui_font(-0.5);
        let (name, date) = local_time::day_heading(self.day, self.today);
        let x = self.lane + px(20.0);
        let base = ds::optical_baseline(self.top, h, &day_font);
        let name = text.shape_line(&name, &day_font);
        draw_text(layer, &name, x, base, p.text);
        let more = format!("{date} · {}", tr!("remote.changed_count", count = self.count));
        let more = text.shape_line(&more, &quiet);
        draw_text(layer, &more, x + name.width + px(10.0), base, p.text_muted);
    }
}

/// The header's line height, and the search field's width (logical pixels).
const HEADER_H: f32 = 44.0;
/// A row under the mouse: the text's alpha over it.
const ROW_HOVER: f32 = 0.05;
const FIELD_MIN: f32 = 180.0;
const FIELD_MAX: f32 = 280.0;

impl Viewer for ChangedFiles {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        #[cfg(feature = "snapshot")]
        if let Some(sample) = data.downcast_ref::<Sample>() {
            self.show_sample(sample);
            return;
        }
        let Some(data) = data.downcast_ref::<ChangedData>() else { return };
        self.project = Some(data.project);
        self.queries.clone_from(&data.queries);
        // The project's files changed (saved, renamed, removed, brought, written by another
        // program; a transfer ended: whether the server has it): read again when next shown.
        let heard = |stale: &Arc<AtomicBool>| {
            let stale = Arc::clone(stale);
            move || stale.store(true, Ordering::Relaxed)
        };
        let events = &data.events;
        let (a, b, c, d, e, f) = (
            heard(&self.stale),
            heard(&self.stale),
            heard(&self.stale),
            heard(&self.stale),
            heard(&self.stale),
            heard(&self.stale),
        );
        self.subscriptions = vec![
            events.subscribe(move |_: &document::events::FileSaved| a()),
            events.subscribe(move |_: &workspace::events::FileRenamed| b()),
            events.subscribe(move |_: &workspace::events::EntriesDeleted| c()),
            events.subscribe(move |_: &workspace::events::EntriesChanged| d()),
            events.subscribe(move |_: &workspace::events::FilesWritten| e()),
            events.subscribe(move |_: &remote::events::TransferFinished| f()),
        ];
        self.read_for = None;
    }

    fn poll(&mut self) -> bool {
        self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.reading.is_some()
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // Its controls first: the calendar (a press outside closes it), the search (its ×,
        // or the keys to it; a click elsewhere takes them back), the scrollbar's thumb.
        let open = self.picker.is_some();
        let pressed = self.with_controls(|c, parts| c.press(parts, p, 1));
        if open {
            // On "Days…" itself: only closed; on another chip: it goes on to it.
            if std::mem::take(&mut self.closed_outside) && self.chip_at(p) == Some(Chip::Pick) {
                return None;
            }
            if self.picker.is_some() || self.chip_at(p).is_none() {
                return None;
            }
            return self.click(p);
        }
        if pressed.is_some() {
            return None;
        }
        match self.chip_at(p) {
            Some(Chip::Quick(i)) => {
                self.choose(Span::QUICK[i]);
                return None;
            }
            Some(Chip::Pick) => {
                self.open_picker();
                return None;
            }
            None => {}
        }
        let i = self.row_at(p)?;
        let root = self.project.and_then(crate::session::session)?.saved.local.clone();
        Some(ViewerAction::OpenFile {
            path: root.join(&self.rows[i].path),
            line: None,
            column: None,
        })
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        let mut changed = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        // Over the open calendar: nothing under it lights up.
        if self.picker.as_ref().is_some_and(|picker| picker.rect().contains(p)) {
            return changed;
        }
        let (chip, row) = (self.chip_at(p), self.row_at(p));
        changed |= chip != self.hover || row != self.hovered_row;
        (self.hover, self.hovered_row) = (chip, row);
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The calendar's: the text's over its fields, a hand elsewhere on it (its shared
        // control says a hand only over what it takes, the arrow elsewhere).
        if let Some(picker) = &self.picker
            && picker.rect().contains(p)
        {
            return Some(if picker.over_field(p) {
                ViewerCursor::Text
            } else {
                ViewerCursor::Pointer
            });
        }
        if let Some(cursor) = self.controls.cursor_at(p) {
            return Some(cursor);
        }
        (self.chip_at(p).is_some() || self.row_at(p).is_some()).then_some(ViewerCursor::Pointer)
    }

    fn wants_text(&self) -> bool {
        self.picker.is_some() || self.typing()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if self.with_controls(|c, parts| c.text(parts, typed)).is_some() {
            return true;
        }
        // Space on the calendar's days picks the one with the keys (its control doesn't
        // take it yet).
        self.picker.as_mut().is_some_and(|picker| typed == " " && picker.space())
    }

    /// Read from `&self`: the calendar's own (its part is made over it mutably), else the
    /// search lends its field by its place.
    fn selected_text(&self) -> Option<String> {
        if let Some(picker) = &self.picker {
            return picker.selected_text();
        }
        self.controls.selected_text_with(|i| (i == SEARCH).then_some(&self.query as &dyn Control))
    }

    /// The part with the keys takes them (the calendar all of them, Esc closing it; the
    /// search its own, Esc emptying it, then giving the keys back); with none, none is used.
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.controls.focused().is_none() {
            return false;
        }
        self.with_controls(|c, parts| c.key(parts, key, reach)).is_some()
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        let i = self.row_at(p)?;
        self.menu = Some(self.rows[i].path.clone());
        Some(vec![
            ViewMenuEntry::item(COPY_PATH, t("remote.changed_copy_path")).with_icon("copy"),
            ViewMenuEntry::item(COPY_ALL, t("remote.changed_copy_all")).with_icon("list"),
            ViewMenuEntry::Separator,
            ViewMenuEntry::item(PROPERTIES, t("remote.menu_properties")).with_icon("info"),
        ])
    }

    fn command(&mut self, id: &str) -> bool {
        // The part with the keys: the calendar's fields, the search.
        if self.with_controls(|c, parts| c.command(parts, id)).is_some() {
            return true;
        }
        // While the calendar is open, the commands are its only.
        if self.picker.is_some() {
            return false;
        }
        match id {
            COPY_PATH => {
                let Some(path) = self.menu.take() else { return false };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(path)));
            }
            COPY_ALL => {
                self.menu = None;
                let all = self.all_paths();
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(all)));
            }
            PROPERTIES => {
                let (Some(project), Some(path)) = (self.project, self.menu.take()) else {
                    return false;
                };
                self.action = Some(crate::properties_view::action(project, vec![path]));
            }
            _ => return false,
        }
        true
    }

    /// A fixed tab of the Remote window: never closed.
    fn closable(&self) -> bool {
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        // Only while it's shown: the files read for the span, again after they changed.
        self.take_read();
        self.read_if_needed();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));
        #[cfg(feature = "snapshot")]
        let remote = self.sample || self.project.and_then(crate::session::session).is_some();
        #[cfg(not(feature = "snapshot"))]
        let remote = self.project.and_then(crate::session::session).is_some();
        if !remote {
            ds::empty_state(layer, ctx.text, &ds, b, &t("remote.not_remote"), None);
            return;
        }
        // What was read for this span, still in it (a span up to now moves on), searched.
        let span_rows = if self.read_for == Some(self.span) {
            in_span(&self.rows, self.span, now())
        } else {
            Vec::new()
        };
        self.make_list(span_rows);
        let mut pen = ui_components::LayerPainter::new(layer, ctx.text, ctx.icons, ds);
        let header_h = self.paint_header(&mut pen, b);
        ds::divider_card(layer, &ds, b.min_x(), b.max_x(), b.min_y() + header_h);
        self.list =
            Rect::new(b.min_x(), b.min_y() + header_h, b.size.width, b.size.height - header_h);
        if self.read_for != Some(self.span) {
            let label = t("remote.changed_reading");
            let at = self.list.min_y() + px(24.0);
            ds::loader(
                layer,
                ctx.text,
                &ds,
                self.list.min_x() + px(16.0),
                at,
                &ds::Loader::with_text(&label),
            );
        } else if self.shown.is_empty() {
            ds::empty_state(layer, ctx.text, &ds, self.list, &t("remote.changed_none"), None);
        } else {
            self.paint_rows(&mut ctx, &ds);
        }
        // The calendar, over everything.
        if let Some(picker) = &mut self.picker {
            picker.paint(ctx.scene, ctx.text, ctx.icons, &ds, self.pick_rect, b);
        }
    }
}

/// Made-up files for a snapshot: over the last days, some still to upload, a search typed.
#[cfg(feature = "snapshot")]
struct Sample {
    query: String,
    /// Scrolled into the list (physical pixels): a day pinned at its top.
    scroll: f32,
}

#[cfg(feature = "snapshot")]
impl ChangedFiles {
    fn show_sample(&mut self, sample: &Sample) {
        let now = now();
        let paths = [
            "admin/assets/ai/analyze/image/studio/css/image-studio.css",
            "admin/assets/ai/analyze/image/studio/js/components/generate-wizard.js",
            "admin/draw/body.web/analyze/image/views/generate/step-progress.php",
            "admin/.htaccess",
            "admin/draw/body.categorii.php",
            "admin/control/api/ai/analyze/image-studio/README.md",
            "class/DB/table.ProdusImagine.php",
            "admin/control/api/ai/analyze/image-studio/src/Api/ApiUrls.php",
            "admin/control/api/ai/analyze/image-studio/src/Jobs/JobService.php",
            "admin/draw/body.web/analyze/image/studio.php",
            "public/index.php",
            "composer.json",
        ];
        let ages =
            [600, 640, 700, 1500, 1700, 86_400, 86_500, 87_000, 88_000, 92_000, 260_000, 262_000];
        self.rows = paths
            .iter()
            .zip(ages)
            .enumerate()
            .map(|(i, (path, age))| RecentFile {
                path: (*path).to_owned(),
                modified: now - age,
                size: 1,
                sent: i % 4 != 1,
            })
            .collect();
        self.stamps = self.rows.iter().map(Stamp::of).collect();
        self.span = Span::Hours(24 * 7);
        self.read_for = Some(self.span);
        self.generation += 1;
        // Scrolled: more files a day, so the days pass under the top.
        if sample.scroll > 0.0 {
            let more: Vec<RecentFile> = self
                .rows
                .iter()
                .flat_map(|r| {
                    (1..4).map(|k| RecentFile { modified: r.modified - k * 120, ..r.clone() })
                })
                .collect();
            self.rows.extend(more);
            self.rows.sort_by_key(|r| std::cmp::Reverse(r.modified));
            self.stamps = self.rows.iter().map(Stamp::of).collect();
        }
        self.scroll = sample.scroll;
        self.query.set_text(&sample.query);
        let typing = (!sample.query.is_empty()).then_some(SEARCH);
        self.with_controls(|c, parts| c.focus(parts, typing));
        self.sample = true;
    }
}

/// Opens the tab with made-up files (`search`: a search typed).
#[cfg(feature = "snapshot")]
pub fn snapshot(host: &mut dyn workbench_model::WorkbenchHost, which: &str) -> bool {
    let query = if which == "search" { "image php".to_owned() } else { String::new() };
    let scroll = which.strip_prefix("scrolled").map_or(0.0, |at| at.parse().unwrap_or(900.0));
    host.open_view(&t("remote.changed_tab"), EXTENSION, &Sample { query, scroll })
}

#[cfg(test)]
#[path = "../tests/unit/changed_view.rs"]
mod tests;
