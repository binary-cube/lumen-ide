//! A `.http` file's requests as a graph (plan 77g): a card per request (its method, its
//! name, its latest run: the status and the time, a pulse while it runs), an edge from a
//! request to each one using its response (the variable it goes by on it), laid out in
//! levels left to right (the shared layered layout). Play runs the whole file in the order
//! its chain allows; Stop stops it; while a request runs, a dot flows along the edges into
//! it. A card clicked: its request in the file. Pan, zoom, fit as the other diagrams.
//!
//! The file is read and laid out on a worker ([`THREAD`]), never on the UI thread; the
//! requests' states are read once per change of the runs, not per frame.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use http_requests::HttpFile;
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::diagram::{
    self, CardFonts, CardLook, Dir, EdgeStyle, Head, Header, Release, Viewport,
    layered::{self, Direction, EdgeSpec, Options},
};
use ui_components::ds::{self, Ctx, Pen};
use ui_components::{
    Viewer, ViewerAction, ViewerCursor, ViewerKind, ViewerPaint, ViewerProvider, WithHost,
};
use ui_kit::{draw_icon, draw_text};

use crate::pending::Pending;
use crate::runs::{RunState, Runs};

/// The extension its tab is opened by.
pub const EXTENSION: &str = "view-http-chain";
/// The toolbar's height (logical pixels).
const BAR: f32 = 40.0;
/// A card's size in the diagram (logical pixels).
const CARD: (f32, f32) = (240.0, diagram::HEADER_H + diagram::ROW_H + diagram::PAD);
/// The worker the file is read and laid out on.
pub const THREAD: &str = "http-graph";

/// What the tab is opened with.
#[derive(Debug, Clone)]
pub struct ChainData {
    pub runs: Arc<Runs>,
    pub file: PathBuf,
    pub text: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Play,
    Stop,
    Fit,
}

/// The graph laid out.
#[derive(Debug, Default)]
struct Laid {
    cards: Vec<Rect>,
    /// From, to, its points (diagram space), its label and where it goes.
    edges: Vec<(usize, usize, Vec<Point>, String, Point)>,
    bounds: Rect,
}

/// A file read and laid out (on [`THREAD`]).
#[derive(Debug, Default)]
struct Graph {
    parsed: HttpFile,
    laid: Laid,
    /// Each request's label, as its runs go by.
    labels: Vec<String>,
    /// What each card writes that its runs don't change: its title, its method's letter,
    /// its request line.
    texts: Vec<CardText>,
}

#[derive(Debug, Default)]
struct CardText {
    title: String,
    glyph: String,
    line: String,
}

/// A card's state as it's written: its words and their tone.
#[derive(Debug, Clone, PartialEq)]
struct Status {
    words: String,
    tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Muted,
    Running,
    Good,
    Bad,
}

impl Status {
    fn of(state: Option<&RunState>) -> Self {
        let (words, tone) = match state {
            None => (i18n::t("http.not_run_yet"), Tone::Muted),
            Some(RunState::Running) => (i18n::t("http.running"), Tone::Running),
            Some(RunState::Failed(why)) => (why.clone(), Tone::Bad),
            Some(RunState::Done(o)) => {
                let r = &o.response;
                let tone = if r.status < 400 { Tone::Good } else { Tone::Bad };
                (format!("{} · {} ms", r.status, r.timings.total.as_millis()), tone)
            }
        };
        Self { words, tone }
    }

    fn color(&self, p: &ui_components::Palette) -> Rgba {
        match self.tone {
            Tone::Muted => p.text_muted,
            Tone::Running => p.accent,
            Tone::Good => p.vcs_added,
            Tone::Bad => p.danger,
        }
    }
}

impl Graph {
    fn of(text: &str) -> Self {
        let parsed = http_requests::parse(text);
        let laid = lay_out(&parsed);
        let labels = parsed.requests.iter().map(|r| r.label()).collect();
        let texts = parsed
            .requests
            .iter()
            .map(|r| CardText {
                title: r.name.clone().unwrap_or_else(|| r.url.clone()),
                glyph: r.method.chars().take(1).collect(),
                line: format!("{} {}", r.method, r.url),
            })
            .collect();
        Self { parsed, laid, labels, texts }
    }
}

#[derive(Default)]
pub struct ChainView {
    data: Option<ChainData>,
    graph: Graph,
    /// The graph being laid out (the one shown stays until it's ready).
    building: Pending<Graph>,
    /// Each request's latest run, read when the runs change, and how its card says it.
    states: Vec<Option<RunState>>,
    statuses: Vec<Status>,
    /// The file's text as the editor had it at the last Play (it may have been edited since
    /// the tab opened): the graph laid out again from it.
    fresh: Arc<Mutex<Option<Arc<str>>>>,
    /// Its runs and its layouts wake the app (its paint's waker given them).
    followed: bool,
    viewport: Viewport,
    hits: Vec<(Rect, Tool)>,
    hover_tool: Option<Tool>,
    hover_card: Option<usize>,
    action: Option<ViewerAction>,
    opened: Option<Instant>,
    /// The runs' generation last drawn.
    generation: u64,
}

impl std::fmt::Debug for ChainView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChainView")
            .field("requests", &self.graph.parsed.requests.len())
            .finish_non_exhaustive()
    }
}

/// A variable's label on its edge: what of the response it takes.
fn edge_label(variable: &str) -> String {
    variable.split_once(".response.").map_or(variable, |(_, rest)| rest).to_owned()
}

/// `parsed` laid out: levels left to right, the edges routed square from a card's right side
/// to the next one's left (the shared lines are axis-aligned).
fn lay_out(parsed: &HttpFile) -> Laid {
    let edges = http_requests::chain::edges(parsed);
    let sizes = vec![CARD; parsed.requests.len()];
    let specs: Vec<EdgeSpec> = edges
        .iter()
        .map(|(from, to, _)| EdgeSpec { from: *from, to: *to, min_len: 1, label: None })
        .collect();
    let placed = layered::layout(
        &sizes,
        &specs,
        Options { direction: Direction::Right, node_gap: 36.0, rank_gap: 90.0 },
    );
    let cards = placed.nodes.clone();
    let (outs, ins) = ports(&cards, &edges);
    let mut routed = Vec::new();
    for (((from, to, variable), edge), (out_y, in_y)) in
        edges.into_iter().zip(&placed.edges).zip(outs.into_iter().zip(ins))
    {
        let (a, b) = (cards[from], cards[to]);
        let start = Point::new(a.max_x(), out_y);
        let end = Point::new(b.min_x(), in_y);
        // Through the layout's hidden points (in the gaps between the cards), square: each
        // step across at its middle, so a long edge goes around the cards between.
        let mut through = vec![start];
        through
            .extend(edge.points.iter().skip(1).take(edge.points.len().saturating_sub(2)).copied());
        through.push(end);
        let mut points = vec![start];
        for pair in through.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            if (p.y - q.y).abs() > 0.5 {
                let mid = (p.x + q.x) / 2.0;
                points.push(Point::new(mid, p.y));
                points.push(Point::new(mid, q.y));
            }
            points.push(q);
        }
        points.dedup_by(|a, b| (a.x - b.x).abs() < 0.5 && (a.y - b.y).abs() < 0.5);
        // Its label just before its arrow, above the line.
        let label = Point::new(end.x - 8.0, end.y - 12.0);
        routed.push((from, to, points, edge_label(&variable), label));
    }
    let bounds = diagram::bounds_of(cards.iter().copied());
    Laid { cards, edges: routed, bounds }
}

/// Where each edge leaves its card and reaches the other (their heights): a card's edges
/// spread along its side, ordered by the other end's height, so none share a point (nor their
/// labels, just before the arrow).
fn ports(cards: &[Rect], edges: &[(usize, usize, String)]) -> (Vec<f32>, Vec<f32>) {
    let centre = |i: usize| cards[i].min_y() + cards[i].size.height / 2.0;
    let spread = |card: usize, mine: &[usize], other: &dyn Fn(usize) -> usize| {
        let mut order = mine.to_vec();
        order.sort_by(|a, b| centre(other(*a)).total_cmp(&centre(other(*b))));
        let r = cards[card];
        let n = order.len() as f32;
        order
            .into_iter()
            .enumerate()
            .map(|(k, e)| (e, r.min_y() + r.size.height * (k as f32 + 1.0) / (n + 1.0)))
            .collect::<Vec<_>>()
    };
    let mut outs = vec![0.0; edges.len()];
    let mut ins = vec![0.0; edges.len()];
    for card in 0..cards.len() {
        let leaving: Vec<usize> = (0..edges.len()).filter(|e| edges[*e].0 == card).collect();
        for (e, y) in spread(card, &leaving, &|e| edges[e].1) {
            outs[e] = y;
        }
        let arriving: Vec<usize> = (0..edges.len()).filter(|e| edges[*e].1 == card).collect();
        for (e, y) in spread(card, &arriving, &|e| edges[e].0) {
            ins[e] = y;
        }
    }
    (outs, ins)
}

/// Where along `points` (a polyline) a fraction `t` (0–1) of its length is.
fn along(points: &[Point], t: f32) -> Point {
    let lengths: Vec<f32> =
        points.windows(2).map(|w| (w[1].x - w[0].x).hypot(w[1].y - w[0].y)).collect();
    let total: f32 = lengths.iter().sum();
    let mut left = total * t.clamp(0.0, 1.0);
    for (pair, len) in points.windows(2).zip(&lengths) {
        if left <= *len && *len > 0.0 {
            let f = left / len;
            return Point::new(
                pair[0].x + (pair[1].x - pair[0].x) * f,
                pair[0].y + (pair[1].y - pair[0].y) * f,
            );
        }
        left -= len;
    }
    points.last().copied().unwrap_or(Point::new(0.0, 0.0))
}

impl ChainView {
    /// The latest run's state of request `i` of the file.
    fn state_of(&self, i: usize) -> Option<&RunState> {
        self.states.get(i)?.as_ref()
    }

    fn running(&self) -> bool {
        self.states.iter().any(|s| matches!(s, Some(RunState::Running)))
    }

    /// The requests' states read again (the runs changed, or the graph did).
    fn read_states(&mut self) {
        self.states = match &self.data {
            Some(data) => data.runs.states(&data.file, &self.graph.labels),
            None => Vec::new(),
        };
        self.statuses = self.states.iter().map(|s| Status::of(s.as_ref())).collect();
    }

    /// The graph laid out, if it's ready: shown, fitted the first time.
    fn take_built(&mut self) -> bool {
        let Some(graph) = self.building.take() else { return false };
        self.graph = graph;
        self.viewport.fit_pending = true;
        self.read_states();
        true
    }

    /// `text` laid out on [`THREAD`]: the graph arrives by a poll (the one shown till then).
    fn build(&mut self, text: Arc<str>) {
        self.building.start(THREAD, move || Graph::of(&text));
    }

    /// The text a Play found changed: kept, and laid out again.
    fn take_fresh(&mut self) -> bool {
        let fresh = self.fresh.lock().unwrap_or_else(PoisonError::into_inner).take();
        let (Some(text), Some(data)) = (fresh, self.data.as_mut()) else { return false };
        data.text = Arc::clone(&text);
        self.build(text);
        true
    }

    fn press_tool(&mut self, tool: Tool) {
        let Some(data) = self.data.clone() else { return };
        match tool {
            // The file as the editor has it now (edited since the tab opened, perhaps): run,
            // and the graph laid out again from it if it changed.
            Tool::Play => {
                let fresh = Arc::clone(&self.fresh);
                self.action = Some(ViewerAction::WithHost(WithHost::once(move |host| {
                    let text: Arc<str> = match host.file_text(&data.file) {
                        Some(text) => text.into(),
                        None => Arc::clone(&data.text),
                    };
                    crate::job::run_graph(&data.runs, &data.file, &text);
                    if text != data.text {
                        *fresh.lock().unwrap_or_else(PoisonError::into_inner) = Some(text);
                    }
                })));
            }
            Tool::Stop => {
                data.runs.stop(&data.file);
            }
            Tool::Fit => self.viewport.fit_pending = true,
        }
    }
}

impl Viewer for ChainView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<ChainData>() else { return };
        self.data = Some(data.clone());
        self.viewport = Viewport::new(BAR);
        self.build(Arc::clone(&data.text));
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint { bounds: b, scale, text, scene, palette: p, icons, waker, .. } = ctx;
        // Woken by its runs and its layouts: told once.
        if !self.followed
            && let (Some(data), Some(waker)) = (&self.data, &waker)
        {
            data.runs.follow(waker);
            self.building.set_waker(&Some(Arc::clone(waker)));
            self.followed = true;
        }
        // Ready before the waker was known.
        self.take_built();
        let now = Instant::now();
        let opened = *self.opened.get_or_insert(now);
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.viewport.bounds = b;
        self.viewport.scale = scale;
        if std::mem::take(&mut self.viewport.fit_pending) {
            self.viewport.fit(self.graph.laid.bounds, None, 0.3, 1.4);
        }
        let layer = scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));
        let area = self.viewport.area();
        diagram::dot_grid(
            layer,
            p,
            area,
            self.viewport.to_screen(Point::new(0.0, 0.0)),
            self.viewport.k(),
        );
        let k = self.viewport.k();
        let fonts = CardFonts::new(p, k);
        // The edges, a dot flowing along those into a request that runs.
        let t = now.duration_since(opened).as_secs_f32();
        let graph = &self.graph;
        for (_, to, points, label, at) in &graph.laid.edges {
            let screen: Vec<Point> = points.iter().map(|pt| self.viewport.to_screen(*pt)).collect();
            let running = matches!(self.state_of(*to), Some(RunState::Running));
            let color = if running { p.accent } else { p.text_muted.with_alpha(0.7) };
            diagram::edge(
                layer,
                &screen,
                Dir::Right,
                k,
                EdgeStyle {
                    color,
                    dashed: false,
                    head: Head::Arrow { hollow: None },
                    width: EdgeStyle::LINE,
                },
            );
            if running {
                let dot = along(&screen, (t * 0.9).fract());
                let s = px(7.0) * self.viewport.zoom.max(0.5);
                layer.quad(
                    Quad::filled(Rect::new(dot.x - s / 2.0, dot.y - s / 2.0, s, s), p.accent)
                        .rounded(s / 2.0),
                );
            }
            let at = self.viewport.to_screen(*at);
            let font = fonts.sub.clone();
            let shaped = text.shape_line(label, &font);
            let pad = px(4.0);
            let h = (font.size * 1.5).round();
            // Ending at its point (right-aligned: just before the arrow).
            let w = shaped.width + pad * 2.0;
            let r = Rect::new(at.x - w, at.y - h / 2.0, w, h);
            layer.quad(Quad::filled(r, p.editor_background).rounded(px(4.0)));
            draw_text(
                layer,
                &shaped,
                r.min_x() + pad,
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                p.text_muted,
            );
        }
        // The cards.
        let pulse = 0.5 + 0.5 * (t * 4.0).sin();
        for (i, rect) in graph.laid.cards.iter().enumerate() {
            let Some(request) = graph.parsed.requests.get(i) else { continue };
            let r = self.viewport.rect_to_screen(*rect);
            let state = self.state_of(i);
            let look = CardLook { focus: false, hovered: self.hover_card == Some(i), muted: false };
            diagram::card_frame(layer, &ds, r, look, k, self.viewport.zoom, scale);
            let (Some(status), Some(card)) = (self.statuses.get(i), graph.texts.get(i)) else {
                continue;
            };
            let color = status.color(p);
            // A ring in the state's color (pulsing while it runs).
            let ring = if matches!(state, Some(RunState::Running)) {
                color.with_alpha(0.35 + 0.65 * pulse)
            } else {
                color.with_alpha(0.8)
            };
            if state.is_some() {
                let w = (2.0 * scale).round();
                layer.quad(
                    Quad::filled(r, Rgba::TRANSPARENT)
                        .rounded((10.0 * k).round())
                        .bordered(w, ring),
                );
            }
            let header = Header {
                glyph: &card.glyph,
                color: crate::format::method_color(&request.method, p),
                title: &card.title,
                subtitle: &status.words,
                muted: false,
            };
            let header_rect =
                diagram::card_header(Pen { layer, text, ds: &ds }, &fonts, header, r, k);
            let row = Rect::new(
                r.min_x() + px(10.0) * self.viewport.zoom,
                header_rect.max_y(),
                r.size.width,
                diagram::ROW_H * k,
            );
            let shown = ds::ellipsize(
                text,
                &card.line,
                &fonts.row,
                r.size.width - px(20.0) * self.viewport.zoom,
            );
            let shaped = text.shape_line(&shown, &fonts.row);
            draw_text(
                layer,
                &shaped,
                row.min_x(),
                ds::optical_baseline(row.min_y(), row.size.height, &fonts.row),
                p.text_muted,
            );
        }
        // The toolbar.
        let bar = diagram::toolbar(layer, &ds, b, BAR);
        let title = data_title(self.data.as_ref());
        let x = diagram::toolbar_title(text, layer, &ds, bar, bar.min_x() + px(14.0), &title)
            + px(16.0);
        self.hits.clear();
        let size = px(28.0);
        let mut x = x;
        let running = self.running();
        for (tool, icon, tint) in [
            (Tool::Play, "play", p.vcs_added),
            (Tool::Stop, "square", p.danger),
            (Tool::Fit, "maximize-2", p.text),
        ] {
            let r = Rect::new(x, bar.min_y() + (bar.size.height - size) / 2.0, size, size);
            let enabled = tool != Tool::Stop || running;
            if enabled && self.hover_tool == Some(tool) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(6.0)));
            }
            if let Some(id) = icons.id(icon) {
                let s = px(15.0);
                let ir = Rect::new(
                    (r.min_x() + (size - s) / 2.0).round(),
                    (r.min_y() + (size - s) / 2.0).round(),
                    s,
                    s,
                );
                draw_icon(
                    layer,
                    icons,
                    id,
                    ir,
                    if enabled { tint } else { p.text_muted.with_alpha(0.4) },
                );
            }
            self.hits.push((r, tool));
            x = r.max_x() + px(4.0);
        }
        if self.graph.parsed.requests.is_empty() {
            ds::empty_state(layer, text, &ds, area, &i18n::t("http.no_requests"), None);
        }
    }

    /// A request of the chain running: the loader's pace.
    fn is_loading(&self) -> bool {
        self.running()
    }

    /// Its graph got laid out, or its runs changed (a request started or ended).
    fn poll(&mut self) -> bool {
        self.take_fresh();
        let built = self.take_built();
        let generation = self.data.as_ref().map_or(0, |d| d.runs.generation());
        let changed = generation != self.generation;
        self.generation = generation;
        if changed {
            self.read_states();
        }
        built || changed
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.viewport.scroll(dx, dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.viewport.zoom_at(factor, at, 0.3, 2.5)
    }

    fn pointer_down(
        &mut self,
        p: Point,
        _clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        if let Some((_, tool)) = self.hits.iter().find(|(r, _)| r.contains(p)).copied() {
            self.press_tool(tool);
            return None;
        }
        self.viewport.press(p);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        self.viewport.drag(p)
    }

    fn click(&mut self, _p: Point) -> Option<ViewerAction> {
        let Release::Click(at) = self.viewport.release() else { return None };
        let point = self.viewport.to_diagram(at);
        let i = self.graph.laid.cards.iter().position(|r| r.contains(point))?;
        let data = self.data.as_ref()?;
        let line = self.graph.parsed.requests.get(i)?.line;
        Some(ViewerAction::OpenAt { path: data.file.clone(), line })
    }

    fn hover(&mut self, p: Point) -> bool {
        let tool = self.hits.iter().find(|(r, _)| r.contains(p)).map(|(_, t)| *t);
        let point = self.viewport.to_diagram(p);
        let card = self.graph.laid.cards.iter().position(|r| r.contains(point));
        let changed = tool != self.hover_tool || card != self.hover_card;
        self.hover_tool = tool;
        self.hover_card = card;
        changed
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        let on_tool = self.hits.iter().any(|(r, _)| r.contains(p));
        let on_card = self.graph.laid.cards.iter().any(|r| r.contains(self.viewport.to_diagram(p)));
        (on_tool || on_card).then_some(ViewerCursor::Pointer)
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }
}

fn data_title(data: Option<&ChainData>) -> String {
    let name = data
        .and_then(|d| d.file.file_name())
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    i18n::tr!("http.chain_title", file = name.as_str())
}

/// The graph's tab.
#[derive(Debug, Default)]
pub struct ChainProvider;

impl ViewerProvider for ChainProvider {
    fn id(&self) -> &'static str {
        "http-chain"
    }

    fn kind(&self, path: &std::path::Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &std::path::Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ChainView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/chain_view.rs"]
mod tests;
