//! An icicle (a flame graph drawn top down: [`Icicle`]): a tree's nodes as bars, a level a
//! row, each as wide as its value in all (a profile's step: its time with its children's)
//! within its parent, its children under it from the left — so what's left of a parent
//! uncovered is its own part. Its color is the caller's (a heat), a node may be outlined (a
//! problem) and carry a sign. Children worth more than their parent (rounding, parallel
//! workers) are scaled into it, marked approximate; bars under [`MIN_PX`] next to one
//! another are joined into one "…".
//!
//! A click chooses a node and zooms into it (its subtree the whole width); its band on top
//! says the way down (each step clickable, ← back) and what width and color mean; a time
//! axis says where the values fall. Keys: ↑ the parent, ↓ the widest child, ← → its
//! siblings, ↵ zooms in, ⌫ / Esc back, Home all. The bars are laid out once per width,
//! nodes and zoom ([`kernel::Memo`]); a frame draws only the rows in view.

use icons::Icons;
use kernel::{Memo, Point, Rect, Rgba};
use render_api::{Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{draw_icon, draw_text};

use crate::ds::{self, Ctx, radius};
use crate::palette::Palette;
use crate::severity::contrast;
use crate::tooltip::Tip;
use crate::tree_table::Ink;
use crate::viewer::ViewerKey;

/// Bars narrower than this (logical pixels) are joined with their narrow neighbours.
pub const MIN_PX: f32 = 2.0;
/// A level's row and the band on top (logical pixels).
pub const ROW_H: f32 = 24.0;
pub const BAND_H: f32 = crate::tree_table::HEADER_H;
/// The time axis under the band (logical pixels).
pub const AXIS_H: f32 = 20.0;

/// A node: its parent, its value in all and its own, what it says (its label, a detail
/// after it), its fill, an outline and a sign (a problem), its tooltip.
#[derive(Debug, Clone, PartialEq)]
pub struct IcicleNode {
    pub parent: Option<usize>,
    pub value: f64,
    pub self_value: f64,
    pub label: String,
    pub detail: String,
    pub tone: Ink,
    pub mark: Option<Ink>,
    pub sign: Option<&'static str>,
    pub tip: String,
}

/// A bar laid out: its node (`None`: `merged` narrow ones joined), its level under the one
/// zoomed into, where it starts and its width (pixels), its value, scaled into its parent.
#[derive(Debug, Clone, PartialEq)]
pub struct IcicleCell {
    pub node: Option<usize>,
    pub merged: Vec<usize>,
    pub depth: usize,
    pub x: f32,
    pub w: f32,
    pub value: f64,
    pub approx: bool,
}

/// What the band and the merged bars say (the caller's words).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IcicleLabels {
    /// The first crumb (the whole tree).
    pub all: String,
    pub back: String,
    /// What width and color mean.
    pub legend: String,
    /// A merged bar's tooltip, `{count}` its nodes.
    pub merged: String,
    /// Said when nothing has a value.
    pub empty: String,
    /// Added to the tooltip of a bar scaled into its parent.
    pub approx: String,
}

/// The bars of `nodes` in `width` pixels: from `focus` (the whole width) or every root (by
/// their values), each node's children under it from its left, as wide as their values —
/// scaled into it when they're worth more (`approx`) —, the narrow ones (under `min`) next
/// to one another joined into one bar and not followed down.
pub fn layout(
    nodes: &[IcicleNode],
    children: &[Vec<usize>],
    roots: &[usize],
    focus: Option<usize>,
    width: f32,
    min: f32,
) -> Vec<IcicleCell> {
    let mut out = Vec::new();
    let tops: Vec<usize> = match focus {
        Some(f) if f < nodes.len() => vec![f],
        _ => roots.iter().copied().filter(|&r| r < nodes.len()).collect(),
    };
    let total: f64 = tops.iter().map(|&r| nodes[r].value.max(0.0)).sum();
    if total <= 0.0 || width <= 0.0 {
        return out;
    }
    let mut stack: Vec<(usize, f32, f32, usize)> = Vec::new();
    // A row of siblings placed from `x0` with `k` pixels per unit.
    let place = |row: &[usize],
                 x0: f32,
                 k: f64,
                 depth: usize,
                 approx: bool,
                 out: &mut Vec<IcicleCell>,
                 stack: &mut Vec<(usize, f32, f32, usize)>| {
        let mut x = x0;
        let mut narrow: Vec<usize> = Vec::new();
        let mut narrow_x = x;
        let flush = |narrow: &mut Vec<usize>, narrow_x: f32, x: f32, out: &mut Vec<IcicleCell>| {
            if narrow.is_empty() {
                return;
            }
            let value = narrow.iter().map(|&i| nodes[i].value.max(0.0)).sum();
            let one = narrow.len() == 1;
            out.push(IcicleCell {
                node: one.then(|| narrow[0]),
                merged: if one { Vec::new() } else { std::mem::take(narrow) },
                depth,
                x: narrow_x,
                w: x - narrow_x,
                value,
                approx,
            });
            narrow.clear();
        };
        for &i in row {
            let w = (nodes[i].value.max(0.0) * k) as f32;
            if w < min {
                if narrow.is_empty() {
                    narrow_x = x;
                }
                narrow.push(i);
                x += w;
                continue;
            }
            flush(&mut narrow, narrow_x, x, out);
            out.push(IcicleCell {
                node: Some(i),
                merged: Vec::new(),
                depth,
                x,
                w,
                value: nodes[i].value,
                approx,
            });
            stack.push((i, x, w, depth));
            x += w;
        }
        flush(&mut narrow, narrow_x, x, out);
    };
    place(&tops, 0.0, f64::from(width) / total, 0, false, &mut out, &mut stack);
    while let Some((i, x, w, depth)) = stack.pop() {
        let kids = &children[i];
        if kids.is_empty() {
            continue;
        }
        let sum: f64 = kids.iter().map(|&c| nodes[c].value.max(0.0)).sum();
        let own = nodes[i].value.max(0.0);
        // Worth more than their parent: scaled into it.
        let approx = sum > own * (1.0 + 1e-9) && sum > 0.0;
        let basis = if approx { sum } else { own };
        if basis <= 0.0 {
            continue;
        }
        place(kids, x, f64::from(w) / basis, depth + 1, approx, &mut out, &mut stack);
    }
    out.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.x.total_cmp(&b.x)));
    out
}

/// What a click on the icicle meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcicleEvent {
    /// A node chosen (and zoomed into when it has children).
    Selected(usize),
    /// The zoom changed (a crumb, back).
    Zoomed,
}

/// A part of it under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hit {
    Cell(usize),
    Back,
    Crumb(Option<usize>),
}

/// An icicle (see the module's doc).
#[derive(Debug, Default)]
pub struct Icicle {
    nodes: Vec<IcicleNode>,
    children: Vec<Vec<usize>>,
    roots: Vec<usize>,
    generation: u64,
    focus: Option<usize>,
    history: Vec<Option<usize>>,
    selected: Option<usize>,
    hover: Option<Hit>,
    cells: Memo<(u64, u32, Option<usize>), Vec<IcicleCell>>,
    labels: IcicleLabels,
    format: Option<fn(f64) -> String>,
    // The geometry last drawn (physical pixels).
    scale: f32,
    bounds: Rect,
    chart: Rect,
    row_h: f32,
    scroll: f32,
    crumbs: Vec<(Rect, Hit)>,
}

impl Icicle {
    pub fn new() -> Self {
        Self { scale: 1.0, ..Self::default() }
    }

    /// Its nodes replaced (each one's parent before it or not: any order), the zoom and the
    /// choice kept when they're still nodes.
    pub fn set_nodes(&mut self, nodes: Vec<IcicleNode>) {
        let n = nodes.len();
        let mut children = vec![Vec::new(); n];
        let mut roots = Vec::new();
        for (i, node) in nodes.iter().enumerate() {
            match node.parent.filter(|&p| p < n && p != i) {
                Some(p) => children[p].push(i),
                None => roots.push(i),
            }
        }
        self.nodes = nodes;
        self.children = children;
        self.roots = roots;
        self.generation += 1;
        if self.focus.is_some_and(|f| f >= n) {
            self.focus = None;
            self.history.clear();
        }
        if self.selected.is_some_and(|s| s >= n) {
            self.selected = None;
        }
    }

    pub fn set_labels(&mut self, labels: IcicleLabels) {
        self.labels = labels;
    }

    /// How values are written (the axis, the tooltips of merged bars).
    pub fn set_format(&mut self, format: fn(f64) -> String) {
        self.format = Some(format);
    }

    pub fn nodes(&self) -> &[IcicleNode] {
        &self.nodes
    }

    /// The node zoomed into (`None`: the whole tree).
    pub fn focus(&self) -> Option<usize> {
        self.focus
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The way from the top to the node zoomed into.
    pub fn path(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut at = self.focus;
        while let Some(i) = at {
            out.push(i);
            at = self.nodes.get(i).and_then(|n| n.parent);
        }
        out.reverse();
        out
    }

    /// Zoomed into `node` (`None`: all); `true` if it changed.
    pub fn zoom_to(&mut self, node: Option<usize>) -> bool {
        let node = node.filter(|&n| n < self.nodes.len());
        if node == self.focus {
            return false;
        }
        self.history.push(self.focus);
        self.focus = node;
        self.scroll = 0.0;
        true
    }

    /// Back to the zoom before; `false`: none.
    pub fn back(&mut self) -> bool {
        match self.history.pop() {
            Some(focus) => {
                self.focus = focus;
                self.scroll = 0.0;
                true
            }
            None if self.focus.is_some() => {
                self.focus = None;
                true
            }
            None => false,
        }
    }

    /// `node` is `ancestor` or under it.
    fn is_under(&self, node: usize, ancestor: usize) -> bool {
        let mut at = Some(node);
        while let Some(i) = at {
            if i == ancestor {
                return true;
            }
            at = self.nodes.get(i).and_then(|n| n.parent);
        }
        false
    }

    /// `node` chosen (from another view too): out of the zoom, the zoom goes back to the
    /// whole tree.
    pub fn select(&mut self, node: Option<usize>) {
        let node = node.filter(|&n| n < self.nodes.len());
        if let (Some(n), Some(f)) = (node, self.focus)
            && !self.is_under(n, f)
        {
            self.history.push(self.focus);
            self.focus = None;
        }
        self.selected = node;
    }

    /// The bars for `width` pixels (worked out once per nodes, width and zoom).
    pub fn cells(&mut self, width: f32) -> &[IcicleCell] {
        let key = (self.generation, width.to_bits(), self.focus);
        let (nodes, children, roots, focus) =
            (&self.nodes, &self.children, &self.roots, self.focus);
        let min = MIN_PX * self.scale;
        self.cells.get_or_update(key, || layout(nodes, children, roots, focus, width, min))
    }

    fn cells_cached(&self) -> &[IcicleCell] {
        self.cells.value().map_or(&[], Vec::as_slice)
    }

    /// A bar's rectangle on screen.
    fn cell_rect(&self, c: &IcicleCell) -> Rect {
        let y = self.chart.min_y() + c.depth as f32 * self.row_h - self.scroll;
        Rect::new(self.chart.min_x() + c.x, y, c.w, self.row_h)
    }

    fn hit(&self, p: Point) -> Option<Hit> {
        if let Some((_, h)) = self.crumbs.iter().find(|(r, _)| r.contains(p)) {
            return Some(*h);
        }
        if !self.chart.contains(p) {
            return None;
        }
        let cells = self.cells_cached();
        cells.iter().position(|c| self.cell_rect(c).contains(p)).map(Hit::Cell)
    }

    /// The node whose bar is at `p` (a merged one: none).
    pub fn node_at(&self, p: Point) -> Option<usize> {
        match self.hit(p)? {
            Hit::Cell(i) => self.cells_cached().get(i)?.node,
            _ => None,
        }
    }

    // ─── Input ───

    /// A press at `p`: a bar chosen and zoomed into (when it has children), a crumb, back.
    pub fn press(&mut self, p: Point) -> Option<IcicleEvent> {
        match self.hit(p)? {
            Hit::Back => self.back().then_some(IcicleEvent::Zoomed),
            Hit::Crumb(node) => self.zoom_to(node).then_some(IcicleEvent::Zoomed),
            Hit::Cell(i) => {
                let cell = self.cells_cached().get(i)?.clone();
                match cell.node {
                    Some(n) => {
                        self.selected = Some(n);
                        if !self.children[n].is_empty() {
                            self.zoom_to(Some(n));
                        }
                        Some(IcicleEvent::Selected(n))
                    }
                    // A merged bar: its parent zoomed into, so they're wide enough.
                    None => {
                        let parent = cell.merged.first().and_then(|&m| self.nodes[m].parent);
                        self.zoom_to(parent).then_some(IcicleEvent::Zoomed)
                    }
                }
            }
        }
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hit = self.hit(p);
        let changed = hit != self.hover;
        self.hover = hit;
        changed
    }

    pub fn leave(&mut self) -> bool {
        self.hover.take().is_some()
    }

    /// Scrolled down the levels (physical pixels).
    pub fn scroll(&mut self, dy: f32) -> bool {
        let depth = self.cells_cached().iter().map(|c| c.depth + 1).max().unwrap_or(0);
        let max = (depth as f32 * self.row_h - self.chart.size.height).max(0.0);
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, max);
        self.scroll != before
    }

    /// The keys: ↑ the parent, ↓ the widest child, ← → the siblings, ↵ zooms into the one
    /// chosen, ⌫ / Esc back, Home all. `true` if used.
    pub fn key(&mut self, key: ViewerKey) -> bool {
        let n = self.nodes.len();
        if n == 0 {
            return false;
        }
        let top = |s: &Self| s.focus.map_or_else(|| s.roots.clone(), |f| vec![f]);
        let next = match (key, self.selected) {
            (ViewerKey::Escape | ViewerKey::Backspace, _) => return self.back(),
            (ViewerKey::Home, _) => return self.zoom_to(None),
            (ViewerKey::Enter, Some(s)) => {
                return !self.children[s].is_empty() && self.zoom_to(Some(s));
            }
            (_, None) => top(self).first().copied(),
            (ViewerKey::Up, Some(s)) => {
                let parent = self.nodes[s].parent;
                if self.focus == Some(s) {
                    // Above the zoom: zoomed out a level.
                    self.zoom_to(parent);
                }
                parent.or(Some(s))
            }
            (ViewerKey::Down, Some(s)) => self.children[s]
                .iter()
                .copied()
                .max_by(|a, b| self.nodes[*a].value.total_cmp(&self.nodes[*b].value))
                .or(Some(s)),
            (ViewerKey::Left | ViewerKey::Right, Some(s)) => {
                let siblings = match self.nodes[s].parent {
                    Some(p) if self.focus != Some(s) => self.children[p].clone(),
                    _ => top(self),
                };
                let at = siblings.iter().position(|&x| x == s).unwrap_or(0);
                let to = if key == ViewerKey::Left {
                    at.checked_sub(1)
                } else {
                    Some(at + 1).filter(|&i| i < siblings.len())
                };
                to.map(|i| siblings[i]).or(Some(s))
            }
            _ => return false,
        };
        let changed = next != self.selected;
        self.select(next);
        changed
    }

    /// The tooltip at `p`: a bar's exact figures (a merged one's count), a crumb's name.
    pub fn tooltip_at(&self, p: Point) -> Option<Tip> {
        match self.hit(p)? {
            Hit::Cell(i) => {
                let c = self.cells_cached().get(i)?;
                let r = self.cell_rect(c);
                match c.node {
                    Some(n) if c.approx && !self.labels.approx.is_empty() => {
                        Some(Tip::new(r, format!("{}\n{}", self.nodes[n].tip, self.labels.approx)))
                    }
                    Some(n) => Some(Tip::new(r, self.nodes[n].tip.clone())),
                    None => {
                        let mut tip =
                            self.labels.merged.replace("{count}", &c.merged.len().to_string());
                        if let Some(format) = self.format {
                            tip = format!("{tip} · {}", format(c.value));
                        }
                        Some(Tip::new(r, tip))
                    }
                }
            }
            Hit::Back => {
                let r = self.crumbs.iter().find(|(_, h)| *h == Hit::Back)?.0;
                Some(Tip::new(r, self.labels.back.clone()))
            }
            Hit::Crumb(node) => {
                let r = self.crumbs.iter().find(|(_, h)| *h == Hit::Crumb(node))?.0;
                let label =
                    node.map_or_else(|| self.labels.all.clone(), |n| self.nodes[n].tip.clone());
                Some(Tip::new(r, label))
            }
        }
    }

    // ─── Drawing ───

    /// Draws it in `bounds`: the band (back, the crumbs, the legend), the axis, the bars.
    pub fn paint(&mut self, ctx: IciclePaint<'_>) {
        let IciclePaint { bounds: b, scale, text, scene, palette: p, icons } = ctx;
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        self.scale = scale;
        self.bounds = b;
        self.row_h = px(ROW_H).round();
        let pad = px(10.0);
        let band = Rect::new(b.min_x(), b.min_y(), b.size.width, px(BAND_H).round());
        let axis = Rect::new(
            b.min_x() + pad,
            band.max_y(),
            (b.size.width - 2.0 * pad).max(0.0),
            px(AXIS_H),
        );
        self.chart = Rect::new(
            axis.min_x(),
            axis.max_y() + px(2.0),
            axis.size.width,
            (b.max_y() - axis.max_y() - px(2.0)).max(0.0),
        );
        self.crumbs.clear();
        let base = scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background));
        // The band: back, the crumbs, the legend on the right.
        base.quad(Quad::filled(band, p.text.with_alpha(0.025)));
        ds::divider(base, &ds, band.min_x(), band.max_x(), band.max_y() - ds.line());
        let font = ds.ui_font(-1.0);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-2.0);
        let mid = band.min_y() + band.size.height / 2.0;
        let mut x = band.min_x() + px(6.0);
        if self.focus.is_some() {
            let r = Rect::new(x, mid - px(11.0), px(24.0), px(22.0));
            if self.hover == Some(Hit::Back) {
                base.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::SM)));
            }
            if let Some(id) = icons.id("arrow-left") {
                let s = px(14.0);
                draw_icon(
                    base,
                    icons,
                    id,
                    Rect::new((r.min_x() + px(5.0)).round(), (mid - s / 2.0).round(), s, s),
                    p.text,
                );
            }
            self.crumbs.push((r, Hit::Back));
            x = r.max_x() + px(4.0);
        }
        // The legend first (it keeps its room), then the crumbs in what's left.
        let legend = text.shape_line(&self.labels.legend, &small);
        let ramp_w = px(60.0);
        let legend_w = legend.width + ramp_w + px(16.0);
        let show_legend = band.size.width - legend_w > px(220.0);
        let crumbs_end =
            if show_legend { band.max_x() - legend_w - px(12.0) } else { band.max_x() - px(8.0) };
        if show_legend {
            let lx = band.max_x() - px(10.0) - legend.width;
            draw_text(
                base,
                &legend,
                lx.round(),
                ds::optical_baseline(band.min_y(), band.size.height, &small),
                p.text_muted,
            );
            // The heat ramp: faint to intense.
            let steps = 12;
            let rx = lx - px(8.0) - ramp_w;
            let h = px(8.0).round();
            for i in 0..steps {
                let t = i as f32 / (steps - 1) as f32;
                let w = ramp_w / steps as f32;
                base.quad(Quad::filled(
                    Rect::new((rx + w * i as f32).round(), (mid - h / 2.0).round(), w.ceil(), h),
                    p.heat(t),
                ));
            }
        }
        let mut path: Vec<Option<usize>> = vec![None];
        path.extend(self.path().into_iter().map(Some));
        let sep = text.shape_line("›", &font);
        for (k, node) in path.iter().enumerate() {
            let last = k + 1 == path.len();
            let label = match node {
                None => self.labels.all.clone(),
                Some(n) => self.nodes[*n].label.clone(),
            };
            let f = if last { &strong } else { &font };
            let room = crumbs_end - x;
            if room < px(30.0) {
                break;
            }
            let shown = ds::ellipsize(text, &label, f, room.min(px(220.0)));
            let shaped = text.shape_line(&shown, f);
            let r = Rect::new(x, mid - px(11.0), shaped.width + px(12.0), px(22.0));
            if !last && self.hover == Some(Hit::Crumb(*node)) {
                base.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::SM)));
            }
            draw_text(
                base,
                &shaped,
                (r.min_x() + px(6.0)).round(),
                ds::optical_baseline(band.min_y(), band.size.height, f),
                if last { p.text } else { p.text_muted },
            );
            if !last {
                self.crumbs.push((r, Hit::Crumb(*node)));
                draw_text(
                    base,
                    &sep,
                    (r.max_x() + px(2.0)).round(),
                    ds::optical_baseline(band.min_y(), band.size.height, &font),
                    p.text_muted.with_alpha(0.7),
                );
                x = r.max_x() + sep.width + px(4.0);
            }
        }
        let width = self.chart.size.width;
        let cells = self.cells(width).to_vec();
        if cells.is_empty() {
            ds::empty_state(base, text, &ds, self.chart, &self.labels.empty, None);
            return;
        }
        // The axis: nice ticks over the zoomed value.
        let total: f64 = match self.focus {
            Some(f) => self.nodes[f].value,
            None => self.roots.iter().map(|&r| self.nodes[r].value.max(0.0)).sum(),
        };
        if let Some(format) = self.format
            && total > 0.0
        {
            let step = nice_step(total / 5.0);
            let tick_font = ds.ui_font(-2.5);
            let mut v = 0.0;
            let mut last_end = f32::MIN;
            while v <= total * 1.0001 {
                let tx = axis.min_x() + (v / total) as f32 * width;
                base.quad(Quad::filled(
                    Rect::new(tx.round(), axis.max_y() - px(5.0), ds.line(), px(5.0)),
                    p.border,
                ));
                let label =
                    text.shape_line(&if v == 0.0 { "0".to_owned() } else { format(v) }, &tick_font);
                let lx = (tx - label.width / 2.0).clamp(axis.min_x(), axis.max_x() - label.width);
                if lx > last_end + px(8.0) {
                    draw_text(
                        base,
                        &label,
                        lx.round(),
                        (axis.min_y() + axis.size.height * 0.62).round(),
                        p.text_muted,
                    );
                    last_end = lx + label.width;
                }
                v += step;
            }
            ds::divider(base, &ds, axis.min_x(), axis.max_x(), axis.max_y() - ds.line());
        }
        // The bars in view.
        let layer = scene.push_layer(Some(self.chart));
        let label_font = ds.ui_font(-1.5);
        let detail_font = ds.ui_font(-2.0);
        let gap = px(1.0).max(1.0);
        let selected = self.selected;
        let hovered = match self.hover {
            Some(Hit::Cell(i)) => Some(i),
            _ => None,
        };
        for (i, c) in cells.iter().enumerate() {
            let full = self.cell_rect(c);
            if full.max_y() < self.chart.min_y() || full.min_y() > self.chart.max_y() {
                continue;
            }
            let r = Rect::new(
                full.min_x() + gap / 2.0,
                full.min_y() + gap / 2.0,
                (full.size.width - gap).max(1.0),
                full.size.height - gap,
            );
            let rad = px(3.0).min(r.size.width / 2.0);
            let (fill, mark) = match c.node {
                Some(n) => (self.nodes[n].tone.color(p), self.nodes[n].mark),
                None => (p.text.with_alpha(0.14), None),
            };
            let mut quad = Quad::filled(r, fill).rounded(rad);
            if c.node.is_some() && c.node == selected {
                quad = quad.bordered(px(2.0).round(), p.accent);
            } else if let Some(m) = mark {
                quad = quad.bordered(px(1.5).round().max(1.0), m.color(p));
            }
            layer.quad(quad);
            if hovered == Some(i) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.12)).rounded(rad));
            }
            let Some(n) = c.node else {
                if r.size.width > px(16.0) {
                    let dots = text.shape_line("…", &label_font);
                    draw_text(
                        layer,
                        &dots,
                        (r.min_x() + (r.size.width - dots.width) / 2.0).round(),
                        ds::optical_baseline(r.min_y(), r.size.height, &label_font),
                        p.text_muted,
                    );
                }
                continue;
            };
            let node = &self.nodes[n];
            // Its words, readable on its fill.
            let (dark, light) = (Rgba::rgb(0x1c1b1a), Rgba::rgb(0xf4f2ee));
            let ink = if contrast(dark, fill) >= contrast(light, fill) { dark } else { light };
            let mut lx = r.min_x() + px(6.0);
            let room_end = r.max_x() - px(6.0);
            if let (Some(sign), Some(m)) = (node.sign, node.mark)
                && room_end - lx > px(40.0)
                && let Some(id) = icons.id(sign)
            {
                let s = px(12.0);
                let ir =
                    Rect::new(lx.round(), (r.min_y() + (r.size.height - s) / 2.0).round(), s, s);
                // On a disc of the editor's background: the severity's color stays readable.
                layer.quad(
                    Quad::filled(
                        diagram_inflate(ir, px(2.0)),
                        p.editor_background.with_alpha(0.85),
                    )
                    .rounded(s),
                );
                draw_icon(layer, icons, id, ir, m.color(p));
                lx += s + px(6.0);
            }
            let room = room_end - lx;
            if room < px(14.0) {
                continue;
            }
            let label = ds::ellipsize(text, &node.label, &label_font, room);
            let shaped = text.shape_line(&label, &label_font);
            let base_y = ds::optical_baseline(r.min_y(), r.size.height, &label_font);
            draw_text(layer, &shaped, lx.round(), base_y, ink);
            if label == node.label && !node.detail.is_empty() {
                let detail = text.shape_line(&node.detail, &detail_font);
                if shaped.width + px(8.0) + detail.width <= room {
                    draw_text(
                        layer,
                        &detail,
                        (lx + shaped.width + px(8.0)).round(),
                        base_y,
                        ink.with_alpha(0.75),
                    );
                }
            }
        }
    }
}

/// `r` grown by `by` on every side.
fn diagram_inflate(r: Rect, by: f32) -> Rect {
    crate::diagram::inflate(r, by)
}

/// A step of 1, 2 or 5 times a power of ten, at least `raw`.
pub fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let f = raw / power;
    let nice = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// What an icicle is drawn with.
pub struct IciclePaint<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
}

impl std::fmt::Debug for IciclePaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IciclePaint").field("bounds", &self.bounds).finish_non_exhaustive()
    }
}

#[cfg(test)]
#[path = "../tests/unit/icicle.rs"]
mod tests;
