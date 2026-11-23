//! A log ("Output", in the Services window): lines with their time, their kind (info, a
//! statement, a result, a warning, an error, a notice) and their text — a statement's
//! colored as code of its language, after its prefix (`shop.main>`) —, an error's a link
//! (a click goes where it points: the console, at the error's place).
//!
//! A [`Log`] is shared: whoever writes it (a console's runner, from its thread) pushes
//! lines while the window shows it or not; at most [`Log::limit`] lines are kept, the
//! oldest leaving. A [`LogView`] is the [`Viewer`] drawing one: its side buttons wrap the
//! lines, keep the end in view, copy, clear. Its rows are laid out only when the lines,
//! the width or the wrapping change (new lines only appended), in a monospace font, so a
//! wrap is a count of characters, never a measure; a frame draws only the rows in view.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};

use icons::Icons;
use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_kit::draw_text;

use crate::ds::{self, Ctx};
use crate::palette::Palette;
use crate::scrollbar::EdgeScrollbar;
use crate::tree::TreeList;
use crate::viewer::{
    AppRequest, ViewMemory, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerPaint,
};

/// How many lines a log keeps by default (a console session's).
pub const LIMIT: usize = 10_000;

/// How much a log's lines hold at most (a dump's statements sent one by one): the oldest
/// leave past it, as past [`LIMIT`] — the last one always stays.
pub const BYTE_LIMIT: usize = 32 * 1024 * 1024;

/// What a line says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LogKind {
    Info,
    /// A statement sent (its text colored as code).
    Statement,
    /// What a statement gave (`12 rows retrieved…`).
    Result,
    Warning,
    Error,
    /// What the server said along the way (a PostgreSQL notice).
    Notice,
}

/// A line as written.
#[derive(Debug, Clone, PartialEq)]
pub struct LogLine {
    /// Its time, as shown (`2026-10-31 10:00:23.698`); empty: none.
    pub time: String,
    pub kind: LogKind,
    /// Before the text, dimmed (`shop.main>` before a statement); empty: none.
    pub prefix: String,
    /// Its text (several lines: shown on several rows).
    pub text: String,
    /// The text is code of the files with this extension (`sql`): colored as such.
    pub code: Option<&'static str>,
    /// What a click on its text does (an error: the cursor to its place).
    pub link: Option<ViewerAction>,
}

impl LogLine {
    pub fn new(time: impl Into<String>, kind: LogKind, text: impl Into<String>) -> Self {
        Self {
            time: time.into(),
            kind,
            prefix: String::new(),
            text: text.into(),
            code: None,
            link: None,
        }
    }

    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// The same line, its text colored as code of the files with `extension`.
    pub fn as_code(mut self, extension: &'static str) -> Self {
        self.code = Some(extension);
        self
    }

    pub fn with_link(mut self, link: ViewerAction) -> Self {
        self.link = Some(link);
        self
    }

    /// As copied: `[time] prefix text`.
    pub fn plain(&self) -> String {
        let mut out = String::new();
        if !self.time.is_empty() {
            out.push('[');
            out.push_str(&self.time);
            out.push_str("] ");
        }
        if !self.prefix.is_empty() {
            out.push_str(&self.prefix);
            out.push(' ');
        }
        out.push_str(&self.text);
        out
    }
}

/// A line kept: its prefix and text as one body (`prefix text`), tabs made spaces.
#[derive(Debug, Clone)]
struct Entry {
    time: String,
    kind: LogKind,
    /// Where the text starts in the body (after the prefix and its space).
    text_start: usize,
    body: String,
    code: Option<&'static str>,
    link: Option<ViewerAction>,
}

impl Entry {
    fn of(line: LogLine) -> Self {
        let text = line.text.replace('\t', "    ").replace("\r\n", "\n").replace('\r', "\n");
        let (body, text_start) = if line.prefix.is_empty() {
            (text, 0)
        } else {
            let start = line.prefix.len() + 1;
            (format!("{} {text}", line.prefix), start)
        };
        Self {
            time: line.time,
            kind: line.kind,
            text_start,
            body,
            code: line.code,
            link: line.link,
        }
    }

    fn line(&self) -> LogLine {
        LogLine {
            time: self.time.clone(),
            kind: self.kind,
            prefix: self.body[..self.text_start.saturating_sub(1)].to_owned(),
            text: self.body[self.text_start..].to_owned(),
            code: self.code,
            link: self.link.clone(),
        }
    }

    /// The columns before its body: `[time] `.
    fn indent(&self) -> usize {
        if self.time.is_empty() { 0 } else { self.time.chars().count() + 3 }
    }

    fn bytes(&self) -> usize {
        self.time.len() + self.body.len() + std::mem::size_of::<Self>()
    }
}

#[derive(Debug)]
struct Lines {
    entries: VecDeque<Entry>,
    /// How many left from the front (the first one's number).
    dropped: u64,
    /// Bumped by every change.
    generation: u64,
    /// Bumped when it's cleared (what was laid out is gone).
    epoch: u64,
    limit: usize,
    bytes: usize,
}

/// A log's lines, shared by whoever writes it and the views showing it.
#[derive(Debug, Clone)]
pub struct Log(Arc<Mutex<Lines>>);

impl Default for Log {
    fn default() -> Self {
        Self::new()
    }
}

impl Log {
    /// A log keeping [`LIMIT`] lines.
    pub fn new() -> Self {
        Self::with_limit(LIMIT)
    }

    /// A log keeping `limit` lines at most (at least one).
    pub fn with_limit(limit: usize) -> Self {
        Self(Arc::new(Mutex::new(Lines {
            entries: VecDeque::new(),
            dropped: 0,
            generation: 0,
            epoch: 0,
            limit: limit.max(1),
            bytes: 0,
        })))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Lines> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Adds a line at the end (the oldest leaves past the limit).
    pub fn push(&self, line: LogLine) {
        let mut lines = self.lock();
        let entry = Entry::of(line);
        lines.bytes += entry.bytes();
        lines.entries.push_back(entry);
        while lines.entries.len() > lines.limit
            || (lines.bytes > BYTE_LIMIT && lines.entries.len() > 1)
        {
            if let Some(gone) = lines.entries.pop_front() {
                lines.bytes = lines.bytes.saturating_sub(gone.bytes());
            }
            lines.dropped += 1;
        }
        lines.generation += 1;
    }

    /// Forgets every line.
    pub fn clear(&self) {
        let mut lines = self.lock();
        lines.dropped += lines.entries.len() as u64;
        lines.entries.clear();
        lines.bytes = 0;
        lines.generation += 1;
        lines.epoch += 1;
    }

    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn limit(&self) -> usize {
        self.lock().limit
    }

    /// What its lines hold (bytes, an estimate).
    pub fn memory(&self) -> usize {
        self.lock().bytes
    }

    /// Its lines, oldest first.
    pub fn lines(&self) -> Vec<LogLine> {
        self.lock().entries.iter().map(Entry::line).collect()
    }

    /// Every line as copied, one per row.
    pub fn text(&self) -> String {
        let lines = self.lock();
        let mut out = String::new();
        for entry in &lines.entries {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&entry.line().plain());
        }
        out
    }
}

/// A row on screen: a piece of line `line` (its number), `start..end` of its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Row {
    line: u64,
    start: usize,
    end: usize,
    /// The line's first row (its time is drawn).
    first: bool,
}

/// The rows, for a log's lines at a width.
#[derive(Debug, Default)]
struct Layout {
    epoch: u64,
    wrap: bool,
    cols: usize,
    /// The number of the next line to lay out.
    next: u64,
    rows: VecDeque<Row>,
    /// The widest row (columns, with its indent): how far it scrolls sideways.
    widest: usize,
    /// A piece's rows when it wraps (scratch, reused).
    parts: Vec<std::ops::Range<usize>>,
}

impl Layout {
    /// Brings it up to `lines` at `cols` columns (`wrap`): the new lines appended, the
    /// gone ones removed; everything again when the width, the wrapping or the epoch
    /// changed.
    fn update(&mut self, lines: &Lines, wrap: bool, cols: usize) {
        if self.epoch != lines.epoch || self.wrap != wrap || (wrap && self.cols != cols) {
            *self = Self { epoch: lines.epoch, wrap, cols, next: lines.dropped, ..Self::default() };
        }
        self.cols = cols;
        while self.rows.front().is_some_and(|r| r.line < lines.dropped) {
            self.rows.pop_front();
        }
        self.next = self.next.max(lines.dropped);
        let end = lines.dropped + lines.entries.len() as u64;
        while self.next < end {
            let entry = &lines.entries[(self.next - lines.dropped) as usize];
            self.lay(self.next, entry);
            self.next += 1;
        }
    }

    fn lay(&mut self, line: u64, entry: &Entry) {
        let indent = entry.indent();
        let room = self.cols.saturating_sub(indent).max(8);
        let mut first = true;
        let mut start = 0;
        for piece in entry.body.split('\n') {
            let end = start + piece.len();
            if self.wrap {
                // The rows every view wraps text into (`text_options`), the same breaks.
                crate::text_options::wrap_rows(piece, room, 4, &mut self.parts);
                for part in &self.parts {
                    let (a, b) = (part.start, part.end);
                    self.rows.push_back(Row { line, start: start + a, end: start + b, first });
                    first = false;
                }
            } else {
                self.rows.push_back(Row { line, start, end, first });
                first = false;
            }
            self.widest = self.widest.max(indent + piece.chars().count());
            start = end + 1;
        }
    }
}

/// A side button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Wrap,
    Follow,
    Copy,
    Clear,
}

impl Button {
    const ALL: [Self; 4] = [Self::Wrap, Self::Follow, Self::Copy, Self::Clear];

    fn icon(self) -> &'static str {
        match self {
            Self::Wrap => "text-wrap",
            Self::Follow => "arrow-down-to-line",
            Self::Copy => "copy",
            Self::Clear => "trash",
        }
    }

    fn label(self) -> String {
        i18n::t(match self {
            Self::Wrap => "log.wrap",
            Self::Follow => "log.follow",
            Self::Copy => "log.copy",
            Self::Clear => "log.clear",
        })
    }

    fn command(self) -> &'static str {
        match self {
            Self::Wrap => "log.wrap",
            Self::Follow => "log.follow",
            Self::Copy => "log.copy",
            Self::Clear => "log.clear",
        }
    }
}

/// What's under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Button(Button),
    /// A line's row; on its link's text.
    Line(u64, bool),
}

/// Logical pixels: the side buttons' strip, a button, the text's inset.
const STRIP: f32 = 36.0;
const BUTTON: f32 = ds::button_size::MEDIUM;
const INSET: f32 = 10.0;
const TOP: f32 = 6.0;

/// The view of a [`Log`] (see the module's doc).
pub struct LogView {
    log: Log,
    layout: Layout,
    wrap: bool,
    /// The end is kept in view as lines come.
    follow: bool,
    scroll: f32,
    hscroll: f32,
    /// The lines selected (their numbers): the anchor, the one moved.
    selection: Option<(u64, u64)>,
    hover: Option<Hit>,
    /// The log's generation last drawn.
    seen: u64,
    bar: EdgeScrollbar,
    selecting: bool,
    action: Option<ViewerAction>,
    // Where it was last drawn: its area, the text's, the buttons', a row's height, a
    // character's advance, the scale.
    bounds: Rect,
    text_area: Rect,
    buttons: Vec<(Rect, Button)>,
    row_height: f32,
    advance: f32,
    scale: f32,
    /// The code entry last drawn, its pieces' lengths and colors: a wrapped statement's rows
    /// draw from it, tokenized once, not once per row and frame.
    code_runs: CodeRuns,
}

/// A code entry's colored pieces, for its line in an epoch and a palette.
#[derive(Debug, Default)]
struct CodeRuns {
    key: Option<(u64, u64)>,
    palette: Option<Palette>,
    runs: Vec<(usize, Rgba)>,
}

impl std::fmt::Debug for LogView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogView")
            .field("wrap", &self.wrap)
            .field("follow", &self.follow)
            .field("rows", &self.layout.rows.len())
            .finish_non_exhaustive()
    }
}

impl LogView {
    /// A view of `log`, its end kept in view, its lines not wrapped.
    pub fn new(log: Log) -> Self {
        Self {
            log,
            layout: Layout::default(),
            wrap: false,
            follow: true,
            scroll: 0.0,
            hscroll: 0.0,
            selection: None,
            hover: None,
            seen: u64::MAX,
            bar: EdgeScrollbar::default(),
            selecting: false,
            action: None,
            bounds: Rect::default(),
            text_area: Rect::default(),
            buttons: Vec::new(),
            row_height: 20.0,
            advance: 8.0,
            scale: 1.0,
            code_runs: CodeRuns::default(),
        }
    }

    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn wraps(&self) -> bool {
        self.wrap
    }

    pub fn set_wrap(&mut self, wrap: bool) {
        self.wrap = wrap;
        self.hscroll = 0.0;
    }

    /// It keeps the end in view.
    pub fn follows(&self) -> bool {
        self.follow
    }

    pub fn set_follow(&mut self, follow: bool) {
        self.follow = follow;
        if follow {
            self.scroll = f32::MAX;
        }
    }

    /// The rows laid out (after a paint), as `(line number, text)`: for tests.
    pub fn rows(&self) -> Vec<(u64, String)> {
        let lines = self.log.lock();
        self.layout
            .rows
            .iter()
            .filter_map(|r| {
                let entry = lines.entries.get(r.line.checked_sub(lines.dropped)? as usize)?;
                Some((r.line, entry.body[r.start..r.end].to_owned()))
            })
            .collect()
    }

    /// How far it's scrolled, and the most it can be (physical pixels).
    pub fn scroll_range(&self) -> (f32, f32) {
        (self.scroll, self.max_scroll())
    }

    /// Where side button `command` (`log.wrap`, `log.follow`, `log.copy`, `log.clear`)
    /// was drawn.
    pub fn button_rect(&self, command: &str) -> Option<Rect> {
        self.buttons.iter().find(|(_, b)| b.command() == command).map(|(r, _)| *r)
    }

    /// Where line `line`'s first row was drawn (scrolled), if it's laid out.
    pub fn line_rect(&self, line: u64) -> Option<Rect> {
        let i = self.layout.rows.iter().position(|r| r.line == line)?;
        let top = self.text_area.min_y() + i as f32 * self.row_height - self.scroll;
        Some(Rect::new(self.text_area.min_x(), top, self.text_area.size.width, self.row_height))
    }

    fn content_height(&self) -> f32 {
        self.layout.rows.len() as f32 * self.row_height + (TOP * self.scale).round()
    }

    fn max_scroll(&self) -> f32 {
        (self.content_height() - self.text_area.size.height).max(0.0)
    }

    fn max_hscroll(&self) -> f32 {
        if self.wrap {
            return 0.0;
        }
        let wide = self.layout.widest as f32 * self.advance + 2.0 * (INSET * self.scale).round();
        (wide - self.text_area.size.width).max(0.0)
    }

    fn at_end(&self) -> bool {
        self.scroll >= self.max_scroll() - 0.5
    }

    /// The lines selected, as copied (`None`: none).
    fn selection_text(&self) -> Option<String> {
        let (a, b) = self.selection?;
        let (from, to) = (a.min(b), a.max(b));
        let lines = self.log.lock();
        let text: Vec<String> = (from..=to)
            .filter_map(|n| n.checked_sub(lines.dropped))
            .filter_map(|i| lines.entries.get(i as usize))
            .map(|e| e.line().plain())
            .collect();
        (!text.is_empty()).then(|| text.join("\n"))
    }

    /// Runs one of its own commands (a side button, a menu row).
    fn run(&mut self, id: &str) -> bool {
        match id {
            "log.wrap" => self.set_wrap(!self.wrap),
            "log.follow" => self.set_follow(!self.follow),
            "log.copy" => {
                let text = self.selection_text().unwrap_or_else(|| self.log.text());
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
            }
            "log.select_all" => {
                let lines = self.log.lock();
                let (first, len) = (lines.dropped, lines.entries.len() as u64);
                drop(lines);
                self.selection = (len > 0).then(|| (first, first + len - 1));
            }
            "log.clear" => {
                self.log.clear();
                self.selection = None;
                self.scroll = 0.0;
                self.hscroll = 0.0;
            }
            _ => return false,
        }
        true
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        if let Some((_, b)) = self.buttons.iter().find(|(r, _)| r.contains(p)) {
            return Some(Hit::Button(*b));
        }
        if !self.text_area.contains(p) {
            return None;
        }
        let list = TreeList::new(self.row_height, self.layout.rows.len());
        let top = (TOP * self.scale).round();
        let row = list.row_at(p.y - self.text_area.min_y() - top + self.scroll)?;
        let row = self.layout.rows[row];
        let lines = self.log.lock();
        let entry = lines.entries.get(row.line.checked_sub(lines.dropped)? as usize)?;
        // On its link: over its text (not the time, nor the empty rest of the row).
        let on_link = entry.link.is_some() && {
            let x0 = self.text_area.min_x() + (INSET * self.scale).round() - self.hscroll;
            let from = x0 + entry.indent() as f32 * self.advance;
            let to = from + entry.body[row.start..row.end].chars().count() as f32 * self.advance;
            p.x >= from && p.x <= to
        };
        Some(Hit::Line(row.line, on_link))
    }

    fn link_of(&self, line: u64) -> Option<ViewerAction> {
        let lines = self.log.lock();
        lines.entries.get(line.checked_sub(lines.dropped)? as usize)?.link.clone()
    }

    /// The selection moved to line `line` (kept in view); `extend`: from the anchor.
    fn select(&mut self, line: u64, extend: bool) {
        let anchor = match self.selection {
            Some((a, _)) if extend => a,
            _ => line,
        };
        self.selection = Some((anchor, line));
        if let Some(i) = self.layout.rows.iter().position(|r| r.line == line) {
            let top = i as f32 * self.row_height;
            let view = self.text_area.size.height - (TOP * self.scale).round();
            if top < self.scroll {
                self.scroll = top;
            } else if view > 0.0 && top + self.row_height > self.scroll + view {
                self.scroll = top + self.row_height - view;
            }
            self.follow = false;
        }
    }

    // ─── Painting ───

    fn paint_buttons(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let b = self.bounds;
        let strip = Rect::new(b.min_x(), b.min_y(), px(STRIP), b.size.height);
        layer.quad(Quad::filled(
            Rect::new(strip.max_x(), b.min_y(), ds.line(), b.size.height),
            p.border,
        ));
        self.buttons.clear();
        let size = px(BUTTON);
        let x = (strip.min_x() + (strip.size.width - size) / 2.0).round();
        let mut y = b.min_y() + px(TOP);
        for button in Button::ALL {
            let r = Rect::new(x, y, size, size);
            let on = match button {
                Button::Wrap => self.wrap,
                Button::Follow => self.follow,
                _ => false,
            };
            let hovered = self.hover == Some(Hit::Button(button));
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon(button.icon())
                .rest_tone(ds::Tone::Neutral)
                .toggle(on)
                .hovered(hovered);
            ds::button(layer, text, icons, ds, r, spec);
            self.buttons.push((r, button));
            y += size + px(4.0);
            // Copy and clear apart from the toggles.
            if button == Button::Follow {
                ds::divider(layer, ds, x + px(5.0), x + size - px(5.0), y);
                y += px(5.0);
            }
        }
    }

    fn paint_rows(&mut self, layer: &mut Layer, text: &mut TextSystem, font: &Font, ds: &Ctx<'_>) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let area = self.text_area;
        let h = self.row_height;
        let top = area.min_y() + px(TOP) - self.scroll;
        let metrics = text.metrics(font);
        let x0 = area.min_x() + px(INSET) - self.hscroll;
        let range = TreeList::new(h, self.layout.rows.len())
            .visible((self.scroll - px(TOP)).max(0.0), area.size.height);
        let lines = self.log.lock();
        let selected = self.selection.map(|(a, b)| a.min(b)..=a.max(b));
        let hover_link = match self.hover {
            Some(Hit::Line(line, true)) => Some(line),
            _ => None,
        };
        // Made once per frame, not per row.
        let prefix_font = font.clone().with_weight(Font::SEMIBOLD);
        let mut time_label = String::new();
        for i in range {
            let row = self.layout.rows[i];
            let Some(entry) =
                row.line.checked_sub(lines.dropped).and_then(|n| lines.entries.get(n as usize))
            else {
                continue;
            };
            let y = top + i as f32 * h;
            if selected.as_ref().is_some_and(|s| s.contains(&row.line)) {
                layer.quad(Quad::filled(
                    Rect::new(area.min_x(), y, area.size.width, h),
                    p.selection,
                ));
            }
            let baseline = ui_kit::centered_baseline(y, h, &metrics);
            if row.first && !entry.time.is_empty() {
                time_label.clear();
                time_label.push('[');
                time_label.push_str(&entry.time);
                time_label.push(']');
                let shaped = text.shape_line(&time_label, font);
                draw_text(layer, &shaped, x0, baseline, p.text_muted.with_alpha(0.8));
            }
            let body_x = x0 + entry.indent() as f32 * self.advance;
            // The prefix's part of the row.
            let prefix_end = entry.text_start.saturating_sub(1);
            if row.start < prefix_end {
                let piece = &entry.body[row.start..row.end.min(prefix_end)];
                let shaped = text.shape_line(piece, &prefix_font);
                draw_text(layer, &shaped, body_x, baseline, p.text_muted);
            }
            // The text's part.
            let from = row.start.max(entry.text_start);
            if from >= row.end {
                continue;
            }
            let x = body_x + entry.body[row.start..from].chars().count() as f32 * self.advance;
            let color = kind_color(entry.kind, p);
            match entry.code {
                Some(extension) => {
                    let source = &entry.body[entry.text_start..];
                    let (a, b) = (from - entry.text_start, row.end - entry.text_start);
                    let cache = &mut self.code_runs;
                    let key = Some((lines.epoch, row.line));
                    if cache.key != key || cache.palette.as_ref() != Some(p) {
                        cache.runs.clear();
                        cache.runs.extend(
                            crate::code_tokens::runs(source, extension, p)
                                .into_iter()
                                .map(|(piece, color)| (piece.len(), color)),
                        );
                        cache.key = key;
                        cache.palette = Some(p.clone());
                    }
                    let mut at = 0;
                    let mut cx = x;
                    for &(len, color) in &cache.runs {
                        let (s, e) = (at, at + len);
                        at = e;
                        let (s, e) = (s.max(a), e.min(b));
                        if s >= e {
                            continue;
                        }
                        let part = &source[s..e];
                        // A newline inside a token is past this row.
                        let part = part.split('\n').next().unwrap_or("");
                        let shaped = text.shape_line(part, font);
                        draw_text(layer, &shaped, cx, baseline, color);
                        cx += part.chars().count() as f32 * self.advance;
                    }
                }
                None => {
                    let piece = &entry.body[from..row.end];
                    let shaped = text.shape_line(piece, font);
                    draw_text(layer, &shaped, x, baseline, color);
                    if hover_link == Some(row.line) {
                        let under = Rect::new(
                            x,
                            baseline + px(2.0),
                            piece.chars().count() as f32 * self.advance,
                            ds.line(),
                        );
                        layer.quad(Quad::filled(under, color));
                    }
                }
            }
        }
    }
}

/// A line's text color, by its kind.
fn kind_color(kind: LogKind, p: &Palette) -> Rgba {
    match kind {
        LogKind::Info | LogKind::Statement => p.text,
        LogKind::Result => p.text.with_alpha(0.86),
        LogKind::Warning => p.warning,
        LogKind::Error => p.danger,
        LogKind::Notice => p.text_muted,
    }
}

impl Viewer for LogView {
    /// Word wrap as every view shows text (its own button still turns it for this log).
    fn set_text_options(&mut self, options: &crate::TextOptions) {
        self.set_wrap(options.word_wrap);
    }

    fn memory(&self) -> ViewMemory {
        let rows = self.layout.rows.len() * std::mem::size_of::<Row>();
        ViewMemory { ram: rows as u64, gpu: 0 }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        self.bounds = b;
        self.scale = ctx.scale;
        let font = Font::new(p.code_font.clone(), px(p.code_font_size - 0.5));
        self.advance = text_advance(ctx.text, &font);
        self.row_height = (font.size * 1.6).round().max(1.0);
        let strip = px(STRIP) + ds.line();
        self.text_area =
            Rect::new(b.min_x() + strip, b.min_y(), (b.size.width - strip).max(0.0), b.size.height);
        let base = ctx.scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background));
        self.paint_buttons(base, ctx.text, ctx.icons, &ds);
        // The rows: brought up to the lines (only the new ones laid out).
        let room = self.text_area.size.width - 2.0 * px(INSET);
        let cols = (room / self.advance.max(1.0)).floor().max(1.0) as usize;
        {
            let lines = self.log.lock();
            self.layout.update(&lines, self.wrap, cols);
            self.seen = lines.generation;
            if let Some((a, b)) = self.selection
                && a.min(b) < lines.dropped
            {
                self.selection = None;
            }
        }
        if self.follow {
            self.scroll = self.max_scroll();
        }
        self.scroll = self.scroll.clamp(0.0, self.max_scroll());
        self.hscroll = self.hscroll.clamp(0.0, self.max_hscroll());
        if self.layout.rows.is_empty() {
            let area = Rect::new(
                self.text_area.min_x(),
                self.text_area.min_y(),
                self.text_area.size.width,
                (self.text_area.size.height * 0.8).max(0.0),
            );
            ds::empty_state(base, ctx.text, &ds, area, &i18n::t("log.empty"), None);
            return;
        }
        let layer = ctx.scene.push_layer(Some(self.text_area));
        self.paint_rows(layer, ctx.text, &font, &ds);
        let content = self.content_height();
        self.bar.paint(ctx.scene, p, ctx.scale, self.text_area, content, self.scroll);
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        let (before, hbefore) = (self.scroll, self.hscroll);
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll());
        self.hscroll = (self.hscroll + dx).clamp(0.0, self.max_hscroll());
        if dy != 0.0 {
            // Up: the end isn't followed any more; back at the end: it is again.
            self.follow = self.at_end();
        }
        self.scroll != before || self.hscroll != hbefore
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            self.follow = self.at_end();
            return None;
        }
        match self.hit(p) {
            Some(Hit::Button(button)) => {
                self.run(button.command());
                self.action.take()
            }
            Some(Hit::Line(line, true)) if clicks == 1 => {
                self.selection = Some((line, line));
                self.link_of(line)
            }
            Some(Hit::Line(line, _)) => {
                self.selection = Some((line, line));
                self.selecting = true;
                None
            }
            None => {
                self.selection = None;
                None
            }
        }
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            self.follow = self.at_end();
            return changed;
        }
        if !self.selecting {
            return false;
        }
        let y = p.y.clamp(self.text_area.min_y(), self.text_area.max_y() - 1.0);
        let at = Point::new(self.text_area.min_x() + 1.0, y);
        if let Some(Hit::Line(line, _)) = self.hit(at) {
            let anchor = self.selection.map_or(line, |(a, _)| a);
            let changed = self.selection != Some((anchor, line));
            self.selection = Some((anchor, line));
            return changed;
        }
        false
    }

    fn release(&mut self) -> bool {
        self.selecting = false;
        self.bar.release()
    }

    fn hover(&mut self, p: Point) -> bool {
        let bar = self.bar.hover(p);
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed || bar
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        match self.hit(p) {
            Some(Hit::Button(_) | Hit::Line(_, true)) => Some(ViewerCursor::Pointer),
            _ => None,
        }
    }

    fn tooltip_at(&self, p: Point) -> Option<crate::tooltip::Tip> {
        let (rect, button) = self.buttons.iter().find(|(r, _)| r.contains(p))?;
        Some(crate::tooltip::Tip::new(*rect, button.label()))
    }

    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if let Some(Hit::Line(line, _)) = self.hit(p) {
            let inside = self.selection.is_some_and(|(a, b)| (a.min(b)..=a.max(b)).contains(&line));
            if !inside {
                self.selection = Some((line, line));
            }
        }
        let item = |id: &str, key: &str, icon: &'static str| {
            ViewMenuEntry::item(id, i18n::t(key)).with_icon(icon)
        };
        Some(vec![
            item("log.copy", "log.copy", "copy"),
            ViewMenuEntry::item("log.select_all", i18n::t("log.select_all")),
            ViewMenuEntry::Separator,
            item("log.wrap", if self.wrap { "log.unwrap" } else { "log.wrap" }, "text-wrap"),
            item(
                "log.follow",
                if self.follow { "log.unfollow" } else { "log.follow" },
                "arrow-down-to-line",
            ),
            ViewMenuEntry::Separator,
            item("log.clear", "log.clear", "trash"),
        ])
    }

    fn select_all(&mut self) -> bool {
        self.run("log.select_all")
    }

    fn selected_text(&self) -> Option<String> {
        self.selection_text()
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        let (first, len) = {
            let lines = self.log.lock();
            (lines.dropped, lines.entries.len() as u64)
        };
        if len == 0 {
            return false;
        }
        let last = first + len - 1;
        let at = self.selection.map(|(_, b)| b);
        let page = ((self.text_area.size.height / self.row_height).floor() as u64).max(1);
        match key {
            ViewerKey::Down => self.select(at.map_or(first, |l| (l + 1).min(last)), false),
            ViewerKey::Up => {
                self.select(at.map_or(last, |l| l.saturating_sub(1).max(first)), false)
            }
            ViewerKey::PageDown => self.select(at.map_or(first, |l| (l + page).min(last)), false),
            ViewerKey::PageUp => {
                self.select(at.map_or(first, |l| l.saturating_sub(page).max(first)), false);
            }
            ViewerKey::Home => {
                self.scroll = 0.0;
                self.follow = false;
            }
            ViewerKey::End => self.set_follow(true),
            ViewerKey::Escape => return self.selection.take().is_some(),
            ViewerKey::Enter => {
                let Some(link) = at.and_then(|l| self.link_of(l)) else { return false };
                self.action = Some(link);
            }
            _ => return false,
        }
        true
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "copy" => {
                let Some(text) = self.selection_text() else { return false };
                self.action = Some(ViewerAction::Request(AppRequest::CopyText(text)));
                true
            }
            "select_all" => self.run("log.select_all"),
            id => self.run(id),
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn poll(&mut self) -> bool {
        self.log.generation() != self.seen
    }

    fn purge(&mut self) {
        // Laid out again at the next paint.
        self.layout = Layout::default();
    }
}

/// A character's advance in the (monospace) `font`.
fn text_advance(text: &mut TextSystem, font: &Font) -> f32 {
    let shaped = text.shape_line("0000000000", font);
    (shaped.width / 10.0).max(1.0)
}

#[cfg(test)]
#[path = "../tests/unit/log_view.rs"]
mod tests;
