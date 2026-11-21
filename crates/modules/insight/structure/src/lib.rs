//! "Structure" module: a side panel with the declaration at the cursor (its colored header,
//! visibility, class, lines), where it is used in the project, the problems in it and
//! the tree of declarations in the file. Everything is clickable: jumps to that location.
//!
//! The panel reads nothing on its own: the app gives it a [`PanelContext`] only when the
//! file, the text or the cursor changes; rows are prepared here once, and drawing only
//! lays them out.

mod usages;

use std::ops::Range;
use std::sync::Arc;

use code_intelligence::{Symbol, SymbolKind, Visibility};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use i18n::{t, tr};
use kernel::cache::{GenerationalCache, Memo};
use kernel::{Point, Rect, Rgba};
use languages::IconSvg;
use render_api::{Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_components::control::{Control, Controls, Thumb};
use ui_components::ds::{self, Ctx};
use ui_components::{
    Dock, PanelContext, Problem, ToolContent, ToolPanel, ToolPanelAction, ToolWindowProvider,
    Usage, ViewerPaint,
};
use ui_kit::draw_text;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("panel.structure"),
    name: "Structure",
    description: "The panel with the declaration at the cursor, its usages and the file's structure",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "braces", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct StructureModule;

impl Module for StructureModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(IconSvg { name: "structure", svg: include_str!("../svg/structure.svg") });
        // Its usages' search: the thread, the files read.
        ctx.contribute(workbench_model::ModuleThreads(&["usages"]));
        ctx.contribute(workbench_model::ModuleCounters {
            disk: Some(&usages::DISK),
            traffic: None,
        });
        ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(Provider));
        // Its toggle, with its shortcut (⌘7 / Alt+7).
        ctx.contribute(workbench_model::CommandContribution {
            spec: command_model::CommandSpec {
                id: TOGGLE,
                label: "Structure",
                // In View › Tool Windows: its tool window's row.
                menu: None,
                shortcuts: &["CmdOrCtrl+7", "Alt+7"],
            },
            icon: Some("structure"),
            handler: Arc::new(workbench_model::ToggleTool("structure")),
        });
        Ok(())
    }
}

/// Shows / hides its panel.
pub const TOGGLE: command_model::CommandId = command_model::CommandId("view.toggle_structure");

#[derive(Debug)]
struct Provider;

/// Its window: a panel on the right, toggled by the app's "Structure" command.
impl ToolWindowProvider for Provider {
    fn id(&self) -> &'static str {
        "structure"
    }

    fn icon(&self) -> &'static str {
        "structure"
    }

    fn title(&self) -> String {
        t("structure.title")
    }

    fn dock(&self) -> Dock {
        Dock::Right
    }

    fn order(&self) -> u16 {
        10
    }

    /// Its group on the strip: what the code holds.
    fn group(&self) -> &'static str {
        "code"
    }

    fn toggle_command(&self) -> Option<&'static str> {
        Some(TOGGLE.0)
    }

    fn content(&self) -> ToolContent {
        ToolContent::Panel
    }

    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        Some(Box::new(StructurePanel::default()))
    }
}

/// How many usages are shown (the rest: "N more places").
const MAX_USAGES: usize = 8;
/// The declaration's block under the mouse: its fill's alpha of the text.
const DECLARATION_HOVER: f32 = 0.035;

/// The declaration at the cursor.
#[derive(Debug, Default)]
struct AtCursor {
    /// The header, in colored pieces (`None` = the text color).
    runs: Vec<(String, Option<Rgba>)>,
    meta: String,
    offset: usize,
}

#[derive(Debug)]
struct Node {
    label: String,
    glyph: &'static str,
    depth: usize,
    offset: usize,
    /// The symbol's index in the file (to mark the one at the cursor).
    symbol: usize,
    active: bool,
    private: bool,
    /// What's written on the declaration (PHP's attributes), dimmed after its name.
    note: Option<String>,
}

#[derive(Default)]
pub struct StructurePanel {
    at_cursor: Option<AtCursor>,
    /// `None`: still searching (or nothing to search for, see `at_cursor`).
    usages: Option<Vec<Usage>>,
    problems: Vec<Problem>,
    nodes: Vec<Node>,
    /// What the nodes were built from (the file's path and symbols, hashed): moving the
    /// cursor only moves the active row, the outline isn't rebuilt.
    outline_key: Option<u64>,
    active: Option<usize>,
    scroll_to_active: bool,
    scroll: f32,
    content_h: f32,
    bounds: Rect,
    /// Its scrollbar, hidden at rest (over its right padding).
    bar: ui_components::EdgeScrollbar,
    /// Its controls' state of use (see [`StructurePanel::with_controls`]).
    controls: Controls,
    targets: Vec<(Rect, ToolPanelAction)>,
    hover: Option<usize>,
    /// Bumped by every `update()`: what the header's wrapped lines were made from.
    header_generation: u64,
    /// The header's lines, for (`header_generation`, how many characters fit on a line).
    header_lines: Memo<(u64, usize), Vec<Line>>,
    /// The outline rows' shortened labels.
    labels: RowLabels,
    /// Their notes, shortened to what's left after the label.
    notes: RowLabels,
    /// The section titles in capitals, per interface language.
    titles: Memo<String, Titles>,
    /// The project's queries (its files, where a declaration is used).
    queries: Option<Arc<dyn workbench_model::ProjectQueries>>,
    /// The usages of the declaration at the cursor, searched in the background.
    finder: usages::Usages,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
}

/// A header line: its colored pieces (`None` = the text color).
type Line = Vec<(String, Option<Rgba>)>;

/// The section titles, in capitals: at the cursor, used in, problems, the file's structure.
type Titles = [String; 4];

fn section_titles() -> Titles {
    ["structure.at_cursor", "structure.used_in", "structure.problems", "structure.outline"]
        .map(|key| t(key).to_uppercase())
}

/// How many shortened labels are kept (a generation; the rows in view, a few times over).
const LABELS: usize = 512;

/// The outline rows' labels shortened to their room, per (row, room); made only for the
/// rows drawn, for the outline and the font they were made with.
#[derive(Debug)]
struct RowLabels {
    of: Option<(Option<u64>, Font)>,
    labels: GenerationalCache<(usize, u32), String>,
}

impl Default for RowLabels {
    fn default() -> Self {
        Self { of: None, labels: GenerationalCache::new(LABELS) }
    }
}

impl RowLabels {
    /// Forgets the labels if the outline or the font changed.
    fn follow(&mut self, outline: Option<u64>, font: &Font) {
        if self.of.as_ref().is_none_or(|(o, f)| *o != outline || f != font) {
            self.labels.clear();
            self.of = Some((outline, font.clone()));
        }
    }

    /// Row `row`'s `label`, shortened to `room` in `font` (the one given to `follow`).
    fn get(
        &mut self,
        text: &mut TextSystem,
        row: usize,
        label: &str,
        font: &Font,
        room: f32,
    ) -> &str {
        let key = (row, room.to_bits());
        if self.labels.get(&key).is_none() {
            self.labels.insert(key, ds::ellipsize(text, label, font, room));
        }
        self.labels.get(&key).map_or("", String::as_str)
    }
}

/// What the outline depends on: the file and its symbols (names, kinds, positions).
fn outline_key(path: Option<&std::path::Path>, file: &code_intelligence::FileSymbols) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    for s in &file.symbols {
        (s.offset, s.range.end, s.name.as_ref(), s.detail.as_deref(), s.note.as_deref())
            .hash(&mut hasher);
        (std::mem::discriminant(&s.kind), std::mem::discriminant(&s.visibility)).hash(&mut hasher);
    }
    hasher.finish()
}

fn glyph(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Class => "C",
        SymbolKind::Interface => "I",
        SymbolKind::Trait => "T",
        SymbolKind::Enum => "E",
        SymbolKind::Function | SymbolKind::Method => "ƒ",
        SymbolKind::Property => "p",
        SymbolKind::Constant | SymbolKind::ClassConstant => "k",
        SymbolKind::EnumCase => "e",
        SymbolKind::CssClass => ".",
        SymbolKind::CssId => "#",
        SymbolKind::CssVariable => "–",
        SymbolKind::Mixin => "@",
        SymbolKind::Variable => "v",
        SymbolKind::Section => "§",
    }
}

/// Meta rows: `public · Car · lines 11–13`.
fn meta(s: &Symbol, line_of: &dyn Fn(usize) -> u32) -> String {
    let mut parts = Vec::new();
    if s.kind.is_member() {
        parts.push(
            match s.visibility {
                Visibility::Public => "public",
                Visibility::Protected => "protected",
                Visibility::Private => "private",
            }
            .to_owned(),
        );
        if s.is_static {
            parts.push("static".to_owned());
        }
    }
    if let Some(container) = &s.container {
        parts.push(code_intelligence::short_name(container).to_owned());
    }
    let start = line_of(s.range.start) + 1;
    let end = line_of(s.range.end.saturating_sub(1).max(s.range.start)) + 1;
    let count = (end - start + 1) as i64;
    parts.push(tr!("structure.lines", count = count, start = start as i64, end = end as i64));
    parts.join(" · ")
}

impl std::fmt::Debug for StructurePanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StructurePanel").field("nodes", &self.nodes.len()).finish_non_exhaustive()
    }
}

impl StructurePanel {
    /// The usages of the declaration at the cursor: searched when it's another one; none
    /// outside declarations.
    fn search_usages(&mut self, ctx: &PanelContext<'_>) {
        let Some(queries) = self.queries.clone() else {
            self.usages = None;
            return;
        };
        let root = queries.root();
        let current = ctx.file.and_then(|f| {
            let i = *code_intelligence::scope_at(f, ctx.cursor).last()?;
            Some((&f.symbols[i], f))
        });
        let query = current.map(|(s, f)| code_intelligence::usages::UsageQuery {
            name: s.name.to_string(),
            kind: s.kind,
            language: f.language.to_string(),
            declared_in: ctx.path.and_then(|p| p.strip_prefix(&root).ok()).map(Into::into),
            declared_line: s.line,
        });
        let outside = query.is_none();
        self.finder.request(&queries, query, self.waker.clone());
        self.usages =
            if outside { Some(Vec::new()) } else { self.finder.result().map(<[Usage]>::to_vec) };
    }
}

impl ToolPanel for StructurePanel {
    fn update(&mut self, ctx: &PanelContext<'_>) {
        let previous = self.active;
        self.header_generation = self.header_generation.wrapping_add(1);
        self.at_cursor = None;
        self.active = None;
        self.search_usages(ctx);
        let Some(file) = ctx.file else {
            self.nodes.clear();
            self.outline_key = None;
            self.problems = ctx.problems.to_vec();
            return;
        };
        let chain = code_intelligence::scope_at(file, ctx.cursor);
        let current = chain.last().copied();
        if let Some(i) = current {
            let s = &file.symbols[i];
            let runs = match ctx.header {
                Some((header, colors)) => runs(header, colors),
                None => vec![(code_intelligence::label(s, ctx.sigil), None)],
            };
            self.at_cursor = Some(AtCursor { runs, meta: meta(s, ctx.line_of), offset: s.offset });
        }
        // Problems of the declaration at the cursor (or of the file, outside declarations).
        let scope = current.map(|i| file.symbols[i].range.clone());
        self.problems = ctx
            .problems
            .iter()
            .filter(|p| {
                scope.as_ref().is_none_or(|r| r.start <= p.range.start && p.range.start <= r.end)
            })
            .cloned()
            .collect();
        let key = outline_key(ctx.path, file);
        if self.outline_key != Some(key) {
            self.outline_key = Some(key);
            self.nodes.clear();
            for item in code_intelligence::outline(file) {
                let s = &file.symbols[item.index];
                self.nodes.push(Node {
                    label: code_intelligence::label(s, ctx.sigil),
                    glyph: glyph(s.kind),
                    depth: item.depth,
                    offset: s.offset,
                    symbol: item.index,
                    active: false,
                    private: s.visibility == Visibility::Private && s.kind.is_member(),
                    note: s.note.clone().map(String::from),
                });
            }
        }
        // The cursor only moves the active row.
        for (row, node) in self.nodes.iter_mut().enumerate() {
            node.active = Some(node.symbol) == current;
            if node.active {
                self.active = Some(row);
            }
        }
        if self.active != previous && self.active.is_some() {
            self.scroll_to_active = true;
        }
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(view) = data.downcast_ref::<workbench_model::ProjectView>() {
            self.queries.clone_from(&view.queries);
        }
    }

    /// The usages found in the background.
    fn poll(&mut self) -> bool {
        if !self.finder.poll() {
            return false;
        }
        self.usages = self.finder.result().map(<[Usage]>::to_vec);
        true
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.bounds = ctx.bounds;
        let ds = Ctx::new(ctx.palette, ctx.scale);
        // The active structure row must be visible: a "dry" pass finds where it
        // lands, then it is drawn with the corrected scroll.
        if self.scroll_to_active {
            self.scroll_to_active = false;
            let mut dry = Scene::default();
            let active = self.layout(&ds, ctx.text, &mut dry);
            if let Some(r) = active {
                let view = ctx.bounds;
                let margin = ds.px(40.0);
                if r.min_y() < view.min_y() + margin || r.max_y() > view.max_y() - margin {
                    self.scroll += r.min_y() - (view.min_y() + view.size.height / 3.0);
                }
                self.clamp_scroll();
            }
        }
        self.layout(&ds, ctx.text, ctx.scene);
        self.bar.set_auto_hiding(true);
        let (b, content) = (self.bounds, self.content_h);
        self.bar.paint(ctx.scene, ctx.palette, ctx.scale, b, content, self.scroll.round());
        // What the scrollbar wants of the frames (its fade), for the questions before the next.
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some()
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn scroll(&mut self, dy: f32) -> bool {
        let before = self.scroll;
        self.scroll += dy;
        self.clamp_scroll();
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        // Its scrollbar (when shown): scrolled there.
        if self.with_controls(|c, parts| c.press(parts, p, 1)).is_some() {
            return None;
        }
        self.targets.iter().find(|(r, _)| r.contains(p)).map(|(_, a)| a.clone())
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.targets.iter().position(|(r, _)| r.contains(p));
        let changed = hover != self.hover;
        self.hover = hover;
        self.with_controls(|c, parts| c.hover(parts, Some(p))) | changed
    }

    fn clickable(&self, p: Point) -> bool {
        self.targets.iter().any(|(r, _)| r.contains(p))
    }
}

/// The header in colored pieces (the colors cover only part of the text).
fn runs(header: &str, colors: &[(Range<usize>, Rgba)]) -> Vec<(String, Option<Rgba>)> {
    let mut out = Vec::new();
    let mut at = 0;
    for (range, color) in colors {
        let (start, end) = (range.start.max(at), range.end.min(header.len()));
        if start >= end || !header.is_char_boundary(start) || !header.is_char_boundary(end) {
            continue;
        }
        if start > at {
            out.push((header[at..start].to_owned(), None));
        }
        out.push((header[start..end].to_owned(), Some(*color)));
        at = end;
    }
    if at < header.len() {
        out.push((header[at..].to_owned(), None));
    }
    out
}

impl StructurePanel {
    /// Its controls (the scrollbar with the scroll it moves), the topmost first.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let mut outline = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut parts: [&mut dyn Control; 1] = [&mut outline];
        f(&mut self.controls, &mut parts)
    }

    fn clamp_scroll(&mut self) {
        let max = (self.content_h - self.bounds.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    fn target(&mut self, r: Rect, action: ToolPanelAction) -> bool {
        self.targets.push((r, action));
        self.hover == Some(self.targets.len() - 1)
    }

    /// Lays out and draws the content; returns the rect of the active structure row.
    fn layout(&mut self, ctx: &Ctx<'_>, text: &mut TextSystem, scene: &mut Scene) -> Option<Rect> {
        self.targets.clear();
        let b = self.bounds;
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(b));
        let pad = px(16.0);
        // Whole pixels, so text and boxes scroll in step (no jitter).
        let top = b.min_y() + px(14.0) - self.scroll.round();
        // Lent to the sections while they draw (they need `&mut self`), then put back.
        let mut titles = std::mem::take(&mut self.titles);
        let s = Sheet {
            ctx: *ctx,
            x0: b.min_x() + pad,
            width: (b.size.width - 2.0 * pad).max(0.0),
            body: ctx.ui_font(-0.5),
            small: ctx.ui_font(-1.5),
            code: ctx.code_font(-0.5),
            titles: titles.get_or_update(i18n::locale(), section_titles),
        };
        let mut y = self.paint_at_cursor(layer, text, &s, top);
        if self.at_cursor.is_some() {
            y = self.paint_used_in(layer, text, &s, y);
        }
        if !self.problems.is_empty() {
            y = self.paint_problems(layer, text, &s, y);
        }
        let (y, active) = self.paint_outline(layer, text, &s, y);
        self.titles = titles;
        self.content_h = y - top + px(16.0);
        active
    }

    /// "At cursor": the declaration's header (wrapped, colored) and its meta line, or
    /// that there is none; returns the `y` below it.
    fn paint_at_cursor(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        s: &Sheet<'_>,
        mut y: f32,
    ) -> f32 {
        let (ctx, p, x0, width) = (&s.ctx, s.ctx.palette, s.x0, s.width);
        let px = |v: f32| ctx.px(v);
        y = section(layer, text, ctx, x0, y, &s.titles[0]);
        let Some(at) = &self.at_cursor else {
            let shaped = text.shape_line(&t("structure.no_symbol"), &s.small);
            draw_text(
                layer,
                &shaped,
                x0,
                ds::optical_baseline(y, px(20.0), &s.small),
                p.text_muted,
            );
            return y + px(20.0);
        };
        let start = y;
        let max = line_chars(text, &s.code, width);
        let lines =
            self.header_lines.get_or_update((self.header_generation, max), || wrap(&at.runs, max));
        let line_h = px(20.0);
        for line in lines {
            let mut x = x0;
            let base = ds::optical_baseline(y, line_h, &s.code);
            for (piece, color) in line {
                let shaped = text.shape_line(piece, &s.code);
                draw_text(layer, &shaped, x, base, color.unwrap_or(p.text));
                x += shaped.width;
            }
            y += line_h;
        }
        let meta = ds::ellipsize(text, &at.meta, &s.small, width);
        let shaped = text.shape_line(&meta, &s.small);
        y += px(2.0);
        draw_text(layer, &shaped, x0, ds::optical_baseline(y, px(18.0), &s.small), p.text_muted);
        y += px(18.0);
        let r = Rect::new(x0 - px(6.0), start - px(2.0), width + px(12.0), y - start + px(4.0));
        let offset = at.offset;
        let hovered = self.target(r, ToolPanelAction::Reveal(offset));
        ds::RowHighlight::new(false, hovered)
            .radius(ds::radius::SM)
            .hover(ds::HoverFill::Faint(DECLARATION_HOVER))
            .paint(layer, ctx, r);
        y
    }

    /// "Used in": a card per usage (file, folder, line), at most `MAX_USAGES`, or that the
    /// search runs / found none; returns the `y` below it.
    fn paint_used_in(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        s: &Sheet<'_>,
        mut y: f32,
    ) -> f32 {
        let (ctx, x0) = (&s.ctx, s.x0);
        y += ctx.px(18.0);
        y = section(layer, text, ctx, x0, y, &s.titles[1]);
        // Lent while the cards draw (they need `&mut self`), then put back.
        let usages = self.usages.take();
        let y = match &usages {
            None => muted_line(layer, text, ctx, x0, y, &t("structure.searching")),
            Some(usages) if usages.is_empty() => {
                muted_line(layer, text, ctx, x0, y, &t("structure.no_usages"))
            }
            Some(usages) => {
                for usage in usages.iter().take(MAX_USAGES) {
                    y = self.paint_usage(layer, text, s, y, usage);
                }
                let total = usages.len();
                if total > MAX_USAGES {
                    let more = tr!("structure.more", count = (total - MAX_USAGES) as i64);
                    y = muted_line(layer, text, ctx, x0, y, &more);
                }
                y
            }
        };
        self.usages = usages;
        y
    }

    /// A usage's card: its file name, its folder and its line; returns the `y` below it.
    fn paint_usage(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        s: &Sheet<'_>,
        y: f32,
        usage: &Usage,
    ) -> f32 {
        let (ctx, p) = (&s.ctx, s.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let card = Rect::new(s.x0, y, s.width, px(32.0));
        let hovered =
            self.target(card, ToolPanelAction::Open { path: usage.path.clone(), line: usage.line });
        let fill = if hovered { 0.07 } else { 0.04 };
        layer.quad(Quad::filled(card, p.text.with_alpha(fill)).rounded(px(ds::radius::SM)));
        let line = text.shape_line(&format!(":{}", usage.line + 1), &s.code);
        let lx = card.max_x() - px(10.0) - line.width;
        draw_text(
            layer,
            &line,
            lx,
            ds::optical_baseline(card.min_y(), card.size.height, &s.code),
            p.text_muted,
        );
        let name =
            usage.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = usage.path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let room = lx - card.min_x() - px(20.0);
        let name = ds::ellipsize(text, &name, &s.body, room);
        let shaped = text.shape_line(&name, &s.body);
        let base = ds::optical_baseline(card.min_y(), card.size.height, &s.body);
        draw_text(layer, &shaped, card.min_x() + px(10.0), base, p.text);
        let rest = room - shaped.width - px(8.0);
        if !dir.is_empty() && rest > px(30.0) {
            let dir = ds::ellipsize(text, &dir, &s.small, rest);
            let d = text.shape_line(&dir, &s.small);
            let x = card.min_x() + px(10.0) + shaped.width + px(8.0);
            draw_text(layer, &d, x, base, p.text_muted.with_alpha(0.8));
        }
        y + card.size.height + px(6.0)
    }

    /// "Problems": a card per problem (at most 6), its message and line; returns the `y`
    /// below them.
    fn paint_problems(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        s: &Sheet<'_>,
        mut y: f32,
    ) -> f32 {
        let (ctx, p, x0) = (&s.ctx, s.ctx.palette, s.x0);
        let px = |v: f32| ctx.px(v);
        y += px(18.0);
        y = section(layer, text, ctx, x0, y, &s.titles[2]);
        // Lent while the cards draw (they need `&mut self`), then put back.
        let problems = std::mem::take(&mut self.problems);
        for problem in problems.iter().take(6) {
            let card = Rect::new(x0, y, s.width, px(34.0));
            let hovered = self.target(card, ToolPanelAction::Reveal(problem.range.start));
            let fill = if hovered { 0.13 } else { 0.08 };
            layer.quad(Quad::filled(card, p.danger.with_alpha(fill)).rounded(px(ds::radius::MD)));
            let line = text.shape_line(&format!(":{}", problem.line + 1), &s.code);
            let lx = card.max_x() - px(10.0) - line.width;
            draw_text(
                layer,
                &line,
                lx,
                ds::optical_baseline(card.min_y(), card.size.height, &s.code),
                p.danger.with_alpha(0.7),
            );
            // On one line: a message may carry a newline (code written over lines).
            let flat = kernel::text::one_line(&problem.message);
            let message = ds::ellipsize(text, &flat, &s.body, lx - card.min_x() - px(20.0));
            let shaped = text.shape_line(&message, &s.body);
            draw_text(
                layer,
                &shaped,
                card.min_x() + px(12.0),
                ds::optical_baseline(card.min_y(), card.size.height, &s.body),
                p.danger,
            );
            y += card.size.height + px(6.0);
        }
        self.problems = problems;
        y
    }

    /// "File structure": the outline's rows in view (indented, the active one marked);
    /// returns the `y` below all of them and the active row's rect.
    fn paint_outline(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        s: &Sheet<'_>,
        mut y: f32,
    ) -> (f32, Option<Rect>) {
        let (ctx, p, x0, b) = (&s.ctx, s.ctx.palette, s.x0, self.bounds);
        let px = |v: f32| ctx.px(v);
        y += px(18.0);
        y = section(layer, text, ctx, x0, y, &s.titles[3]);
        if self.nodes.is_empty() {
            y = muted_line(layer, text, ctx, x0, y, &t("structure.empty"));
        }
        let code = &s.code;
        let row_h = px(26.0);
        let mut active = None;
        // Only the rows in view are drawn (big files have hundreds); their click targets are
        // collected on the side, so the nodes are read in place (no per-frame copies).
        let mut targets = Vec::new();
        self.labels.follow(self.outline_key, code);
        self.notes.follow(self.outline_key, &s.small);
        for (row, node) in self.nodes.iter().enumerate() {
            let r = Rect::new(x0 - px(4.0), y, s.width + px(8.0), row_h);
            y += row_h + px(1.0);
            if r.max_y() < b.min_y() || r.min_y() > b.max_y() {
                if node.active {
                    active = Some(r);
                }
                continue;
            }
            let hovered = self.hover == Some(self.targets.len() + targets.len());
            targets.push((r, ToolPanelAction::Reveal(node.offset)));
            ds::RowHighlight::new(node.active, hovered).radius(ds::radius::SM).paint(layer, ctx, r);
            if node.active {
                active = Some(r);
            }
            let x = r.min_x() + px(10.0) + node.depth as f32 * px(16.0);
            let base = ds::optical_baseline(r.min_y(), row_h, code);
            let g = text.shape_line(node.glyph, code);
            let glyph_color = if node.active { p.accent } else { p.text_muted };
            draw_text(layer, &g, x, base, glyph_color);
            let lx = x + px(18.0);
            let room = r.max_x() - lx - px(6.0);
            let label = self.labels.get(text, row, &node.label, code, room);
            let shaped = text.shape_line(label, code);
            let color = if node.active {
                p.accent
            } else if node.private {
                p.text_muted
            } else {
                p.text
            };
            draw_text(layer, &shaped, lx, base, color);
            // Its note (`AsCommand('app:x')`), in what's left of the row.
            let nx = lx + shaped.width + px(8.0);
            let left = r.max_x() - nx - px(6.0);
            if let Some(note) = node.note.as_deref().filter(|_| left > px(24.0)) {
                let note = self.notes.get(text, row, note, &s.small, left);
                let shaped = text.shape_line(note, &s.small);
                let base = ds::optical_baseline(r.min_y(), row_h, &s.small);
                draw_text(layer, &shaped, nx, base, p.text_muted);
            }
        }
        self.targets.extend(targets);
        (y, active)
    }
}

/// What the panel's sections draw with: the column (its left edge and width) and the fonts.
struct Sheet<'a> {
    ctx: Ctx<'a>,
    x0: f32,
    width: f32,
    body: Font,
    small: Font,
    code: Font,
    titles: &'a Titles,
}

/// A section title (already in capitals; small, dimmed); returns the `y` below it.
fn section(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    x: f32,
    y: f32,
    title: &str,
) -> f32 {
    let font = ctx.ui_font(-2.5).with_weight(Font::SEMIBOLD);
    let shaped = text.shape_line(title, &font);
    let h = ctx.px(22.0);
    draw_text(layer, &shaped, x, ds::optical_baseline(y, h, &font), ctx.palette.text_muted);
    y + h + ctx.px(4.0)
}

fn muted_line(
    layer: &mut Layer,
    text: &mut TextSystem,
    ctx: &Ctx<'_>,
    x: f32,
    y: f32,
    s: &str,
) -> f32 {
    let font = ctx.ui_font(-1.5);
    let shaped = text.shape_line(s, &font);
    let h = ctx.px(22.0);
    draw_text(layer, &shaped, x, ds::optical_baseline(y, h, &font), ctx.palette.text_muted);
    y + h
}

/// How many characters of the (monospace) `font` fit on a line of `width` (at least 8).
fn line_chars(text: &mut TextSystem, font: &Font, width: f32) -> usize {
    let cell = text.shape_line("M", font).width.max(1.0);
    ((width / cell).floor() as usize).max(8)
}

/// Splits colored pieces into lines of at most `max` characters (the code font is
/// monospace: breaks by character count, preferably after `, ` or a space). At most 4 lines.
fn wrap(runs: &[(String, Option<Rgba>)], max: usize) -> Vec<Line> {
    let chars: Vec<(char, Option<Rgba>)> =
        runs.iter().flat_map(|(s, c)| s.chars().map(move |ch| (ch, *c))).collect();
    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() && lines.len() < 4 {
        let mut end = (start + max).min(chars.len());
        if end < chars.len()
            && let Some(space) = (start + 1..end).rev().find(|&i| chars[i].0 == ' ')
            && space > start + max / 3
        {
            end = space + 1;
        }
        let mut line: Line = Vec::new();
        for &(ch, color) in &chars[start..end] {
            match line.last_mut() {
                Some((s, c)) if *c == color => s.push(ch),
                _ => line.push((ch.to_string(), color)),
            }
        }
        lines.push(line);
        start = end;
    }
    lines
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
