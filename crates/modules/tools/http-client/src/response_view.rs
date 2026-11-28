//! A run's response, as a tab of its node in the Services window: a line saying how it went
//! (the status in its class's color, the time, the size, the type; a failure's reason),
//! then the body made readable ([`crate::format`]) and colored as code of its kind, the
//! rows in view drawn only; its menu: Open in Editor (the body as saved), Copy Body, Copy
//! as cURL. Running, it says so and follows the run (its runs wake the app; it's polled).

use std::sync::Arc;

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::code_tokens::{CodeLine, draw_code};
use ui_components::ds::{self, Ctx};
use ui_components::{AppRequest, ViewMenuEntry, Viewer, ViewerAction, ViewerPaint};
use ui_kit::draw_text;

use crate::format::{self, Kind};
use crate::pending::Pending;
use crate::runs::{Outcome, RunId, RunState, Runs};
use http_requests::vars::ResolvedRequest;

/// What of a run a tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// The response's body, readable.
    Body,
    /// The request's headers, then the response's.
    Headers,
    /// How long each part took, the redirects.
    Timeline,
    /// The exchange as sent and received (the request's body, the response's headers).
    Raw,
    /// What its scripts did: their tests, their logs (only for a request with scripts).
    Tests,
    /// Its page, shown by the system's web engine (only for an HTML response).
    Preview,
}

impl Part {
    pub const ALL: [Self; 4] = [Self::Body, Self::Headers, Self::Timeline, Self::Raw];

    /// What its tab is derived by (with its node's identity).
    pub fn key(self) -> &'static str {
        match self {
            Self::Body => "response",
            Self::Headers => "headers",
            Self::Timeline => "timeline",
            Self::Raw => "raw",
            Self::Tests => "tests",
            Self::Preview => "preview",
        }
    }

    pub fn title(self) -> String {
        t(match self {
            Self::Body => "http.response_tab",
            Self::Headers => "http.headers_tab",
            Self::Timeline => "http.timeline_tab",
            Self::Raw => "http.raw_tab",
            Self::Tests => "http.tests_tab",
            Self::Preview => "http.preview",
        })
    }

    /// The parts a run's tabs show: the four, its tests when it has scripts, its page when
    /// it's HTML.
    pub fn of(state: &RunState) -> Vec<Self> {
        let outcome = match state {
            RunState::Done(outcome) => Some(outcome),
            _ => None,
        };
        let scripted = outcome.is_some_and(|o| o.script.is_some());
        let page = outcome.is_some_and(|o| crate::page::shows(o));
        Self::ALL
            .into_iter()
            .chain(scripted.then_some(Self::Tests))
            .chain(page.then_some(Self::Preview))
            .collect()
    }
}

/// A request's header as shown: its secrets hidden ([`ResolvedRequest::masked`]); a header
/// that carries credentials hidden whole when a secret went into the request.
fn shown_header(request: &ResolvedRequest, header: &http_requests::Header) -> String {
    let carries = ["authorization", "cookie", "x-api-key", "proxy-authorization"];
    let value = if request.secret && carries.contains(&header.name.to_ascii_lowercase().as_str()) {
        "••••••".to_owned()
    } else {
        request.masked(&header.value)
    };
    format!("{}: {value}", header.name)
}

/// The lines of `part` of `outcome` (the body's: [`done`]'s), and the extension that colors
/// them.
fn part_lines(part: Part, outcome: &Outcome) -> (Vec<String>, &'static str) {
    let request = &outcome.request;
    let response = &outcome.response;
    let header = |h: &http_requests::Header| shown_header(request, h);
    match part {
        Part::Body | Part::Preview => (Vec::new(), "txt"),
        Part::Tests => {
            let mut lines = Vec::new();
            if let Some(run) = &outcome.script {
                for test in &run.tests {
                    lines.push(match &test.failure {
                        None => format!("✓ {}", test.name),
                        Some(why) => format!("✗ {} — {why}", test.name),
                    });
                }
                if let Some(why) = &run.error {
                    lines
                        .push(format!("✗ {}", i18n::tr!("http.script_failed", why = why.as_str())));
                }
                if lines.is_empty() {
                    lines.push(i18n::t("http.no_tests"));
                }
                if !run.logs.is_empty() {
                    lines.push(String::new());
                    lines.push(i18n::t("http.log_title"));
                    lines.extend(run.logs.iter().flat_map(|l| l.lines().map(str::to_owned)));
                }
            }
            (lines, "txt")
        }
        Part::Headers => {
            let mut lines =
                vec![format!("### {} {}", request.method, request.masked(&request.url))];
            lines.extend(request.headers.iter().map(header));
            lines.push(String::new());
            lines.push(format!("### {} {} {}", response.version, response.status, response.reason));
            lines.extend(response.headers.iter().map(|h| format!("{}: {}", h.name, h.value)));
            (lines, "http")
        }
        Part::Timeline => {
            let t = response.timings;
            let ms = |d: std::time::Duration| format!("{} ms", d.as_millis());
            let mut lines = vec![
                format!("{:<20}{}", i18n::t("http.waiting"), ms(t.headers)),
                format!(
                    "{:<20}{}",
                    i18n::t("http.download"),
                    ms(t.total.saturating_sub(t.headers))
                ),
                format!("{:<20}{}", i18n::t("http.total"), ms(t.total)),
            ];
            if !response.hops.is_empty() {
                lines.push(String::new());
                lines.push(i18n::t("http.redirects_title"));
                for hop in &response.hops {
                    lines.push(format!("  {} {} → {}", hop.status, hop.url, hop.location));
                }
            }
            (lines, "txt")
        }
        Part::Raw => {
            let mut lines =
                vec![format!("{} {} HTTP/1.1", request.method, request.masked(&request.url))];
            lines.extend(request.headers.iter().map(header));
            for piece in &request.body {
                lines.push(String::new());
                match piece {
                    http_requests::BodyPart::Text(text) => {
                        lines.extend(request.masked(text).lines().map(str::to_owned))
                    }
                    http_requests::BodyPart::File(path) => lines.push(format!("< {path}")),
                }
            }
            lines.push(String::new());
            lines.push(format!("{} {} {}", response.version, response.status, response.reason));
            lines.extend(response.headers.iter().map(|h| format!("{}: {}", h.name, h.value)));
            (lines, "http")
        }
    }
}

/// The most bytes of a body shown here (the rest: in the editor).
const SHOWN: usize = 2 * 1024 * 1024;

/// The most characters of a line drawn (a minified body is one long line: the rest is in
/// the editor).
const LINE_SHOWN: usize = 4_000;

/// Its status row's height: the request editor's tabs' beside it, so their lines meet.
pub const STATUS: f32 = 38.0;
/// The worker a response is made readable on.
pub const THREAD: &str = "http-view";

/// The mark at a wrapped row's end (its line goes on below).
const WRAP_END: &str = "↩";

/// Each of `lines`' first row when they wrap at `columns`, one more at the end (all of
/// them): a line no longer than the columns is one row, not read again.
fn first_rows(lines: &Lines, columns: usize) -> Vec<u32> {
    let mut firsts = Vec::with_capacity(lines.len() + 1);
    let mut parts = Vec::new();
    let mut at: u32 = 0;
    for i in 0..lines.len() {
        firsts.push(at);
        let line = lines.shown(i);
        let rows = if line.len() <= columns / 4 {
            1
        } else {
            ui_components::text_options::wrap_rows(line, columns, 4, &mut parts);
            parts.len() as u32
        };
        at = at.saturating_add(rows);
    }
    firsts.push(at);
    firsts
}

/// A text by its lines: one allocation, where each line starts.
#[derive(Debug, Default)]
struct Lines {
    text: String,
    starts: Vec<usize>,
}

impl Lines {
    fn new(text: String) -> Self {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(i, _)| i + 1).filter(|i| *i < text.len()));
        if text.is_empty() {
            starts.clear();
        }
        Self { text, starts }
    }

    fn len(&self) -> usize {
        self.starts.len()
    }

    /// Line `i` without its end.
    fn get(&self, i: usize) -> &str {
        let start = self.starts[i];
        let end = self.starts.get(i + 1).map_or(self.text.len(), |next| next - 1);
        self.text[start..end].strip_suffix('\r').unwrap_or(&self.text[start..end])
    }

    /// Line `i` as drawn: at most [`LINE_SHOWN`] characters.
    fn shown(&self, i: usize) -> &str {
        let line = self.get(i);
        line.char_indices().nth(LINE_SHOWN).map_or(line, |(at, _)| &line[..at])
    }
}

/// A finished run's part made readable (on [`THREAD`]).
#[derive(Debug, Default)]
struct Body {
    lines: Lines,
    extension: &'static str,
    /// More than [`SHOWN`]: the rest is in the editor.
    cut: bool,
}

/// What the view shows now.
#[derive(Debug, Default)]
enum Shown {
    #[default]
    Running,
    /// What came so far, while it goes (a stream of events, a long body).
    Streaming(Body),
    Failed(String),
    /// Its part being made readable until `body` is there.
    Done {
        outcome: Arc<Outcome>,
        body: Option<Body>,
    },
}

pub struct ResponseView {
    runs: Arc<Runs>,
    run: RunId,
    part: Part,
    /// The runs' generation and their live one, as last read.
    generation: (u64, u64),
    shown: Shown,
    scroll: f32,
    row_h: f32,
    body: Rect,
    action: Option<ViewerAction>,
    /// The part being made readable.
    preparing: Pending<Body>,
    /// What a menu row asked for, being made on a worker (a body read whole to copy, a page
    /// written for the browser).
    acting: Pending<Option<ViewerAction>>,
    /// Its page (Preview): the same identity for every response it shows (one engine,
    /// loaded again), the page once made (on a worker), where it goes.
    page_id: u128,
    page: Option<Arc<str>>,
    paging: Pending<Option<String>>,
    /// Lines wrap at the body's width ([`ui_components::TextOptions::word_wrap`]).
    wrap: bool,
    /// While wrapping: each line's first row, for the lines (their text's place and
    /// length) and the columns they were made for; one more at the end, all of them.
    wrapped: (Option<(usize, usize, usize)>, Vec<u32>),
}

impl std::fmt::Debug for ResponseView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResponseView").field("run", &self.run).finish_non_exhaustive()
    }
}

impl ResponseView {
    pub fn new(runs: Arc<Runs>, run: RunId, part: Part) -> Self {
        let mut view = Self {
            runs,
            run,
            part,
            generation: (u64::MAX, u64::MAX),
            shown: Shown::Running,
            scroll: 0.0,
            row_h: 18.0,
            body: Rect::default(),
            action: None,
            preparing: Pending::default(),
            acting: Pending::default(),
            page_id: kernel::id::new_uuid(),
            page: None,
            paging: Pending::default(),
            wrap: false,
            wrapped: (None, Vec::new()),
        };
        view.refresh();
        view
    }

    /// What it shows, read again when its runs changed. `true` if it did.
    fn refresh(&mut self) -> bool {
        // What came to it while it goes counts too (told apart from the runs' changes).
        let generation = (self.runs.generation(), self.runs.live_generation());
        if generation == self.generation {
            return false;
        }
        self.generation = generation;
        let Some(run) = self.runs.run(self.run) else {
            self.shown = Shown::Failed(t("http.run_gone"));
            return true;
        };
        let was_running = matches!(self.shown, Shown::Running | Shown::Streaming(_));
        self.shown = match run.state {
            RunState::Running => match self.runs.live(self.run) {
                // What came so far (it's small: its last part only), shown as it is.
                Some(text) => {
                    Shown::Streaming(Body { lines: Lines::new(text), extension: "txt", cut: false })
                }
                None => Shown::Running,
            },
            RunState::Failed(why) => Shown::Failed(why),
            RunState::Done(outcome) if was_running || !matches!(self.shown, Shown::Done { .. }) => {
                let part = self.part;
                let prepared = Arc::clone(&outcome);
                if part == Part::Preview {
                    // The page made on a worker (a large body is read from its file).
                    let paged = Arc::clone(&outcome);
                    self.paging.start(THREAD, move || crate::page::html_of(&paged));
                }
                self.preparing.start(THREAD, move || prepare(part, &prepared));
                Shown::Done { outcome, body: None }
            }
            // Already shown: the run's lighter form taken (its body left in its file by the
            // memory budget), so the heavy one goes; what's shown stays as it was made.
            RunState::Done(fresh) => {
                if let Shown::Done { outcome, .. } = &mut self.shown
                    && !Arc::ptr_eq(outcome, &fresh)
                {
                    *outcome = fresh;
                }
                return false;
            }
        };
        true
    }

    /// What was prepared on a worker, taken: the part readable, a body to copy.
    fn take_prepared(&mut self) -> bool {
        let mut changed = false;
        if let Some(prepared) = self.preparing.take()
            && let Shown::Done { body, .. } = &mut self.shown
        {
            *body = Some(prepared);
            changed = true;
        }
        if let Some(action) = self.acting.take() {
            self.action = action;
            changed = true;
        }
        if let Some(page) = self.paging.take() {
            self.page = page.map(Arc::from);
            changed = true;
        }
        changed
    }

    fn outcome(&self) -> Option<&Arc<Outcome>> {
        match &self.shown {
            Shown::Done { outcome, .. } => Some(outcome),
            _ => None,
        }
    }

    fn paint_status(&self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, row: Rect) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-0.5);
        let mid_base = ds::optical_baseline(row.min_y(), row.size.height, &font);
        let mut x = row.min_x() + px(12.0);
        let mut word =
            |layer: &mut Layer, text: &mut TextSystem, s: &str, color: Rgba, bold: bool| {
                let f = if bold { font.clone().with_weight(Font::SEMIBOLD) } else { font.clone() };
                let shaped = text.shape_line(s, &f);
                draw_text(layer, &shaped, x.round(), mid_base, color);
                x += shaped.width + px(14.0);
            };
        match &self.shown {
            Shown::Running => word(layer, text, &t("http.running"), p.text_muted, false),
            Shown::Streaming(body) => {
                word(layer, text, &t("http.receiving"), p.accent, true);
                word(layer, text, &format::size(body.lines.text.len() as u64), p.text_muted, false);
            }
            Shown::Failed(why) => word(layer, text, why, p.danger, true),
            Shown::Done { outcome, body } => {
                let r = &outcome.response;
                let color = match r.status {
                    200..300 => p.vcs_added,
                    300..400 => p.accent,
                    400..500 => p.warning,
                    _ => p.danger,
                };
                word(layer, text, &format!("{} {}", r.status, r.reason), color, true);
                let ms = r.timings.total.as_millis();
                word(layer, text, &tr!("http.took", ms = ms as i64), p.text_muted, false);
                word(layer, text, &format::size(r.body.len()), p.text_muted, false);
                if let Some(media) = r.media_type() {
                    word(layer, text, &media, p.text_muted, false);
                }
                if !r.hops.is_empty() {
                    let hops = tr!("http.redirects", count = r.hops.len() as i64);
                    word(layer, text, &hops, p.text_muted, false);
                }
                if body.as_ref().is_some_and(|b| b.cut) {
                    word(layer, text, &t("http.cut"), p.warning, false);
                }
            }
        }
    }
}

/// What a finished run's `part` shows, made readable (on a worker: a body read from its
/// file, formatted).
fn prepare(part: Part, outcome: &Outcome) -> Body {
    // The page shows itself (the web engine): no lines.
    if part == Part::Preview {
        return Body { lines: Lines::new(String::new()), extension: "txt", cut: false };
    }
    if part != Part::Body {
        let (lines, extension) = part_lines(part, outcome);
        return Body { lines: Lines::new(lines.join("\n")), extension, cut: false };
    }
    let response = &outcome.response;
    let kind = format::kind(response.media_type().as_deref());
    let extension = format::extension(response.media_type().as_deref());
    let (text, cut) = match (&response.body, kind) {
        (_, Kind::Image | Kind::Binary) => (t("http.binary_body"), false),
        (http_requests::transport::IncomingBody::Memory(bytes), _) => {
            let cut = bytes.len() > SHOWN;
            (format::readable(kind, &bytes[..bytes.len().min(SHOWN)]), cut)
        }
        (http_requests::transport::IncomingBody::File { path, len }, _) => {
            let head = read_head(path, SHOWN);
            (format::readable(kind, &head), *len as usize > SHOWN)
        }
    };
    Body { lines: Lines::new(text), extension, cut }
}

/// The whole body as text, to copy (on a worker: one in a file is read).
fn whole_body(outcome: &Outcome) -> String {
    String::from_utf8_lossy(&crate::runs::body_bytes(&outcome.response.body)).into_owned()
}

/// The first `most` bytes of `path`.
fn read_head(path: &std::path::Path, most: usize) -> Vec<u8> {
    use std::io::Read;
    let mut out = Vec::new();
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(most as u64).read_to_end(&mut out);
    }
    out
}

impl Viewer for ResponseView {
    /// What it made to show (the readable text, the page) let go: made again from the run
    /// when it's shown next (its body from its file, if it was left there).
    fn purge(&mut self) {
        self.shown = Shown::Running;
        self.page = None;
        self.generation = (u64::MAX, u64::MAX);
        self.preparing = Pending::default();
        self.paging = Pending::default();
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette, waker, .. } = ctx;
        self.preparing.set_waker(&waker);
        self.acting.set_waker(&waker);
        self.paging.set_waker(&waker);
        self.refresh();
        self.take_prepared();
        let ds = Ctx::new(palette, scale);
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, palette.editor_background));
        let status = Rect::new(b.min_x(), b.min_y(), b.size.width, px(STATUS));
        self.paint_status(layer, text, &ds, status);
        ds::divider(layer, &ds, b.min_x(), b.max_x(), status.max_y());
        let body = Rect::new(
            b.min_x(),
            status.max_y() + ds.line(),
            b.size.width,
            (b.max_y() - status.max_y()).max(0.0),
        );
        self.body = body;
        if self.part == Part::Preview {
            // Seen only while the page isn't over it (being made, hidden, no engine).
            let (title, detail) = match (&self.shown, &self.page) {
                (Shown::Done { .. }, Some(_)) => {
                    (t("http.preview_title"), t("http.preview_detail"))
                }
                (Shown::Done { .. }, None) => (t("http.preview_loading"), String::new()),
                _ => (t("http.running"), String::new()),
            };
            let detail = (!detail.is_empty()).then_some(detail);
            ds::empty_state(layer, text, &ds, body, &title, detail.as_deref());
            return;
        }
        let (Shown::Done { body: Some(Body { lines, extension, .. }), .. }
        | Shown::Streaming(Body { lines, extension, .. })) = &self.shown
        else {
            return;
        };
        let font = ds.code_font(-1.0);
        self.row_h = (font.size * 1.55).round();
        // Wrapping: each line's rows at the body's width (made again when either changes).
        let (left, right) = (body.min_x() + px(12.0), body.max_x() - px(8.0));
        let advance = text.metrics(&font).advance.max(1.0);
        let columns = if self.wrap {
            (((right - left) / advance).floor() as usize).saturating_sub(1)
        } else {
            0
        };
        let key = (lines.text.as_ptr() as usize, lines.text.len(), columns);
        if columns > 0 && self.wrapped.0 != Some(key) {
            self.wrapped = (Some(key), first_rows(lines, columns));
        }
        let firsts = if columns > 0 { self.wrapped.1.as_slice() } else { &[] };
        let first_of = |i: usize| firsts.get(i).map_or(i, |&r| r as usize);
        let content = first_of(lines.len()) as f32 * self.row_h + px(16.0);
        // Coming: its end followed.
        if matches!(self.shown, Shown::Streaming(_)) {
            self.scroll = f32::MAX;
        }
        self.scroll = self.scroll.clamp(0.0, (content - body.size.height).max(0.0));
        let layer = scene.push_layer(Some(body));
        let row = (self.scroll / self.row_h) as usize;
        let first = if firsts.is_empty() {
            row
        } else {
            firsts.partition_point(|&f| f as usize <= row).saturating_sub(1)
        };
        let bottom = self.scroll + body.size.height;
        let mut parts = Vec::new();
        for i in first..lines.len() {
            let top = body.min_y() + px(8.0) + first_of(i) as f32 * self.row_h - self.scroll;
            if top - body.min_y() > bottom - self.scroll + self.row_h {
                break;
            }
            let line = lines.shown(i);
            let spec = |baseline| CodeLine {
                source: line,
                extension,
                font: &font,
                x: left,
                baseline,
                palette,
                max_x: right,
            };
            if columns == 0 {
                draw_code(layer, text, spec(ds::optical_baseline(top, self.row_h, &font)));
                continue;
            }
            ui_components::text_options::wrap_rows(line, columns, 4, &mut parts);
            for (k, part) in parts.iter().enumerate() {
                let row_top = top + k as f32 * self.row_h;
                let baseline = ds::optical_baseline(row_top, self.row_h, &font);
                if parts.len() == 1 {
                    draw_code(layer, text, spec(baseline));
                    continue;
                }
                ui_components::code_tokens::draw_code_part(
                    layer,
                    text,
                    spec(baseline),
                    part.clone(),
                );
                // The line goes on below: the wrap's mark after this row's text.
                if k + 1 < parts.len() {
                    let width = text.shape_line(&line[part.clone()], &font).width;
                    let mark = text.shape_line(WRAP_END, &font);
                    let color = palette.text_muted.with_alpha(0.6);
                    ui_kit::draw_text(layer, &mark, left + width + px(4.0), baseline, color);
                }
            }
        }
    }

    /// Word wrap: the body's lines go on in rows under them.
    fn set_text_options(&mut self, options: &ui_components::TextOptions) {
        self.wrap = options.word_wrap;
    }

    fn poll(&mut self) -> bool {
        let refreshed = self.refresh();
        self.take_prepared() || refreshed
    }

    fn web_page(&self) -> Option<ui_components::WebPage> {
        if self.part != Part::Preview || self.body.size.height <= 0.0 {
            return None;
        }
        Some(ui_components::WebPage {
            id: self.page_id,
            source: ui_components::WebSource::Html(Arc::clone(self.page.as_ref()?)),
            area: self.body,
            freeze: false,
        })
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll = (self.scroll + dy).max(0.0);
        self.scroll != before
    }

    fn context_menu(&mut self, _p: Point) -> Option<Vec<ViewMenuEntry>> {
        let outcome = self.outcome()?;
        let mut entries = Vec::new();
        if outcome.saved.is_some() {
            entries.push(ViewMenuEntry::item("open", t("http.open_in_editor")).with_icon("file"));
        }
        let kind = format::kind(outcome.response.media_type().as_deref());
        if kind == Kind::Html && outcome.response.body.len() <= crate::page::LARGEST as u64 {
            entries.push(ViewMenuEntry::item("preview", t("http.preview")).with_icon("eye"));
            entries.push(
                ViewMenuEntry::item("open-in-browser", t("http.open_in_browser"))
                    .with_icon("globe"),
            );
        }
        if !matches!(kind, Kind::Image | Kind::Binary) {
            entries.push(ViewMenuEntry::item("copy-body", t("http.copy_body")).with_icon("copy"));
        }
        entries.push(ViewMenuEntry::item("copy-curl", t("http.copy_curl")).with_icon("terminal"));
        entries
            .push(ViewMenuEntry::item("copy-guzzle", t("http.copy_guzzle")).with_icon("file-code"));
        Some(entries)
    }

    fn command(&mut self, id: &str) -> bool {
        let action = match id {
            "open" => self
                .outcome()
                .and_then(|o| o.saved.clone())
                .map(|path| ViewerAction::OpenFile { path, line: None, column: None }),
            // The body as it came (not as shown: cut, indented), read on a worker.
            "copy-body" => {
                let Some(outcome) = self.outcome().cloned() else { return false };
                self.acting.start(THREAD, move || {
                    Some(ViewerAction::Request(AppRequest::CopyText(whole_body(&outcome))))
                });
                return true;
            }
            // The page in a tab, shown by the system's web engine.
            "preview" => {
                let Some(outcome) = self.outcome().cloned() else { return false };
                let folder = crate::job::own_folder(&self.runs);
                self.acting.start(THREAD, move || {
                    let html = crate::page::html_of(&outcome)?;
                    let url = outcome.response.url.clone();
                    let title = tr!("http.preview_tab", url = url.as_str());
                    let data = crate::preview_view::PreviewData { html: html.into(), url, folder };
                    Some(ViewerAction::OpenView(ui_components::OpenView::new(
                        title,
                        crate::preview_view::EXTENSION,
                        data,
                    )))
                });
                return true;
            }
            // Written with its `<base>` beside the project's responses, then opened.
            "open-in-browser" => {
                let Some(outcome) = self.outcome().cloned() else { return false };
                let folder = crate::job::own_folder(&self.runs);
                self.acting.start(THREAD, move || {
                    let html = crate::page::html_of(&outcome)?;
                    crate::page::open_in_browser(&folder, &html)
                });
                return true;
            }
            "copy-guzzle" => self.outcome().map(|o| {
                ViewerAction::Request(AppRequest::CopyText(http_requests::curl::to_guzzle(
                    &o.request,
                )))
            }),
            "copy-curl" => self.outcome().map(|o| {
                ViewerAction::Request(AppRequest::CopyText(http_requests::curl::to_curl(
                    &o.request,
                )))
            }),
            _ => return false,
        };
        self.action = action;
        self.action.is_some()
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn select_all(&mut self) -> bool {
        false
    }
}

#[cfg(test)]
#[path = "../tests/unit/response_view.rs"]
mod tests;
