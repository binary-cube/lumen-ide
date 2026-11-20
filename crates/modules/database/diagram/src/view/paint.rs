//! The diagram drawn: the toolbar (←, the title, the source, the filter, the neighbors'
//! depth, the legend), then the lines in view, the cards in view over them, the labels of
//! the lines lit (the selected table's, the hovered one's, the line selected or under the
//! mouse) placed where they cover no card, and a line's popover over everything.

use database::TableKind;
use database::diagram::{Cardinality, GraphTable, Link, Origin, SchemaGraph};
use database::relations::{Confidence, RelationKinds, VirtualKey};
use kernel::Rgba;
use render_api::Quad;
use text_layout::{Font, TextSystem};
use ui_components::diagram::{
    self, CardFonts, CardLook, Dir, EdgeStyle, Head, Header, ROW_H, Row, clamp_into, clear_of,
    contains_rect,
};
use ui_components::ds::{self, Ctx, Pen, Tone, radius};
use ui_components::{Palette, ViewerPaint};
use ui_kit::{TextInputStyle, draw_icon, draw_text};

use super::*;

/// Below this zoom the cards show their headers only (their rows wouldn't be read).
const ROWS_FROM: f32 = 0.32;
/// The labels drawn at most (the lines of one table).
const LABELS: usize = 40;
/// Below this zoom only the label of the line under the mouse or selected is drawn.
const LABEL_ZOOM: f32 = 0.45;
/// The labels' text at 100% (logical pixels below the interface's), and the smallest it
/// gets as the diagram zooms out (logical pixels).
const LABEL_FONT: f32 = 2.5;
const LABEL_MIN: f32 = 9.0;

/// The labels placed for the lines lit, and what they were placed for: found again only
/// when the layout, the lines lit or the zoom change (never each frame).
#[derive(Debug, Default)]
pub(crate) struct Labels {
    key: Option<LabelKey>,
    items: Vec<Label>,
    /// The cards (diagram space, with a margin): what a label mustn't cover.
    cards: Vec<Rect>,
}

#[derive(Debug, Clone, PartialEq)]
struct LabelKey {
    /// The layout's generation.
    layout: u64,
    /// The lines labelled (their place in the layout's edges), each whole or short.
    lines: Vec<(usize, bool)>,
    font: u32,
    k: u32,
}

/// A line's label: its text, where it may go (diagram space, best first), which of those
/// cover no card, the one chosen (none: shown only when its line is asked for).
#[derive(Debug, Clone)]
struct Label {
    edge: usize,
    link: usize,
    text: String,
    whole: bool,
    spots: Vec<Rect>,
    clear: Vec<bool>,
    chosen: Option<usize>,
}

impl RelationsView {
    pub(super) fn paint_view(&mut self, ctx: ViewerPaint<'_>) {
        self.view.bounds = ctx.bounds;
        self.view.scale = ctx.scale;
        if self.view.zoom == 0.0 {
            self.view.zoom = 1.0;
        }
        self.update_insets();
        if self.view.fit_pending && !self.layout.layout.cards.is_empty() {
            let focus = self
                .selected
                .or(self.focus)
                .and_then(|t| self.layout.card_of(t))
                .map(|c| self.layout.layout.cards[c].rect);
            self.view.fit(self.layout.layout.bounds, focus, FIT_MIN, FIT_MAX);
        }
        self.hits.clear();
        self.tips.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let b = ctx.bounds;
        let base = ctx.scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background).rounded(ds.px(radius::CARD)));
        let area = self.view.area();
        let message = match (&self.graph, &self.error) {
            (_, Some(error)) => Some((t("relations.failed_title"), Some(failed(error)))),
            (None, None) => {
                let progress = self.reading.as_ref().map(|r| {
                    (r.progress.0.load(Ordering::Relaxed), r.progress.1.load(Ordering::Relaxed))
                });
                let detail = progress.filter(|(_, total)| *total > 0).map(|(done, total)| {
                    tr!("relations.read_progress", done = done, total = total)
                });
                Some((t("relations.reading"), detail))
            }
            (Some(graph), None) if graph.tables.is_empty() => {
                Some((t("relations.no_tables"), None))
            }
            (Some(_), None) if self.shown.tables.is_empty() && self.laying.is_none() => {
                Some((t("relations.no_match"), None))
            }
            _ => None,
        };
        if let Some((title, detail)) = message {
            let layer = ctx.scene.push_layer(Some(b));
            ds::empty_state(layer, ctx.text, &ds, area, &title, detail.as_deref());
        } else if self.graph.is_some() {
            // The graph the layout was made from (a decision's new one is laid out next).
            let graph = Arc::clone(&self.drawn);
            self.paint_diagram(&graph, &ds, ctx.text, ctx.scene, ctx.icons, area);
            self.paint_controls(&ds, ctx.text, ctx.scene, area);
            if self.list_open {
                self.paint_list(&ds, ctx.text, ctx.scene, area);
            }
            self.paint_popover(&ds, ctx.text, ctx.scene, area);
        }
        self.paint_toolbar(&ds, ctx.text, ctx.scene, ctx.icons, b, ctx.focused);
    }

    /// The tables touched by the lines shown with their labels: the one selected, the one
    /// under the mouse.
    fn lit(&self) -> [Option<usize>; 2] {
        [self.selected, self.hover.and_then(Hit::table)]
    }

    fn paint_diagram(
        &mut self,
        graph: &SchemaGraph,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
        area: Rect,
    ) {
        let p = ds.palette;
        let k = self.view.k();
        let layout = Arc::clone(&self.layout);
        // What of the diagram is in view (a line or a card outside it isn't drawn).
        let (a, z) = (
            self.view.to_diagram(Point::new(area.min_x(), area.min_y())),
            self.view.to_diagram(Point::new(area.max_x(), area.max_y())),
        );
        let seen = Rect::new(a.x - 4.0, a.y - 4.0, z.x - a.x + 8.0, z.y - a.y + 8.0);
        let lit = self.lit();
        // The lines asked for: the one selected, the one under the mouse.
        let picked = self.edge.as_ref().and_then(|e| e.find(graph));
        let hovered = match self.hover {
            Some(Hit::Edge(l)) => Some(l),
            _ => None,
        };
        let asked = |l: usize| Some(l) == picked || Some(l) == hovered;
        let touches = |l: &Link| lit.iter().flatten().any(|&t| l.from == t || l.to == t);

        let edges = scene.push_layer(Some(area));
        // Those lit: (their place in the edges, their color), the lines asked for first.
        let mut labelled: Vec<(usize, Rgba)> = Vec::new();
        for (e, edge) in layout.layout.edges.iter().enumerate() {
            let Some(link) = graph.links.get(edge.kind) else { continue };
            let hot = touches(link) || asked(edge.kind);
            if !hot && edge.bounds().intersect(&seen).is_none() {
                continue;
            }
            let style = edge_style(link.origin, hot, p);
            let color = style.color;
            let points: Vec<Point> = edge.points.iter().map(|&q| self.view.to_screen(q)).collect();
            let mut wide = style;
            if asked(edge.kind) {
                // The line asked for, thicker under its own.
                wide.color = color.with_alpha(0.25);
                wide.dashed = false;
                wide.head = Head::None;
                let w = (5.0 * k).max(3.0 * self.view.scale);
                for pair in points.windows(2) {
                    diagram::segment(edges, pair[0], pair[1], w, false, wide.color);
                }
            }
            diagram::edge(edges, &points, edge.dir, k, style);
            if hot && (asked(edge.kind) || labelled.len() < LABELS) {
                labelled.push((e, color));
            }
        }
        labelled.sort_by_key(|&(e, _)| {
            let link = layout.layout.edges[e].kind;
            (Some(link) != picked, Some(link) != hovered)
        });

        let fonts = CardFonts::new(p, k);
        let rows = self.view.zoom >= ROWS_FROM;
        let layer = scene.push_layer(Some(area));
        for (c, card) in layout.layout.cards.iter().enumerate() {
            if card.rect.intersect(&seen).is_none() {
                continue;
            }
            let Some(table) = graph.tables.get(card.node) else { continue };
            let r = self.view.rect_to_screen(card.rect);
            let t = card.node;
            let hovered = self.hover.and_then(Hit::table) == Some(t);
            let look = CardLook { focus: self.selected == Some(t), hovered, muted: table.external };
            diagram::card_frame(layer, ds, r, look, k, self.view.zoom, self.view.scale);
            let header =
                card_header(Pen { layer, text, ds }, &fonts, self.several_schemas, table, r, k);
            self.hits.push((header, Hit::Card(t)));
            if !rows || card.shown + card.more == 0 {
                continue;
            }
            diagram::header_divider(layer, ds, r, header);
            let row_h = ROW_H * k;
            let mut y = header.max_y() + 4.0 * k;
            for &column in &layout.rows[c] {
                let hit = Hit::Row(t, column);
                let rect = self.paint_row(
                    Pen { layer, text, ds },
                    icons,
                    &fonts,
                    (graph, t, column),
                    (r, y, row_h, k),
                    self.hover == Some(hit),
                );
                self.hits.push((rect, hit));
                y += row_h;
            }
            if card.more > 0 {
                let more = tr!("relations.more", count = card.more);
                diagram::more_row(Pen { layer, text, ds }, &fonts.sub, r, y, row_h, k, &more);
            }
        }

        // Over the cards: how many rows meet at each end, and what joins them.
        let labels = scene.push_layer(Some(area));
        if k >= 0.45 {
            for &(e, color) in &labelled {
                let edge = &layout.layout.edges[e];
                let Some(link) = graph.links.get(edge.kind) else { continue };
                let points: Vec<Point> =
                    edge.points.iter().map(|&q| self.view.to_screen(q)).collect();
                let pen = Pen { layer: labels, text, ds };
                cardinality(pen, &points, edge.dir, k, link.cardinality, color);
            }
        }
        // Labels: the lines asked for whole, the others short; zoomed far out, only those
        // asked for.
        let lines: Vec<(usize, bool)> = labelled
            .iter()
            .map(|&(e, _)| (e, asked(layout.layout.edges[e].kind)))
            .filter(|&(_, whole)| whole || self.view.zoom >= LABEL_ZOOM)
            .collect();
        let font = Font::new(
            p.ui_font.clone(),
            ((p.ui_font_size - LABEL_FONT) * k).round().max((LABEL_MIN * ds.scale).round()),
        );
        self.place_labels(graph, &layout, lines, &font, text, ds);
        let colors: std::collections::HashMap<usize, Rgba> = labelled.iter().copied().collect();
        let visible = self.view.visible();
        let mut drawn: Vec<Rect> = Vec::new();
        let items = std::mem::take(&mut self.labels.items);
        // The line whose popover is open: the popover says it.
        let told = self.popover.as_ref().and_then(|(e, _)| e.find(graph));
        for item in items.iter().filter(|i| Some(i.link) != told) {
            let screen = |r: Rect| self.view.rect_to_screen(r);
            let free = |r: Rect, drawn: &[Rect]| contains_rect(visible, r) && clear_of(r, drawn);
            let mut at = item.chosen.map(|i| screen(item.spots[i])).filter(|&r| free(r, &drawn));
            if at.is_none() && item.chosen.is_some() {
                // Out of view: another of its spots that is in it, else kept in view.
                at = item
                    .spots
                    .iter()
                    .zip(&item.clear)
                    .filter(|(_, clear)| **clear)
                    .map(|(&r, _)| screen(r))
                    .find(|&r| free(r, &drawn));
                if at.is_none() {
                    let r = clamp_into(screen(item.spots[item.chosen.unwrap_or(0)]), visible);
                    let (a, z) = (
                        self.view.to_diagram(Point::new(r.min_x(), r.min_y())),
                        self.view.to_diagram(Point::new(r.max_x(), r.max_y())),
                    );
                    let back = Rect::new(a.x, a.y, z.x - a.x, z.y - a.y);
                    let edge = layout.layout.edges[item.edge].bounds();
                    let near = diagram::inflate(edge, 40.0).intersect(&back).is_some();
                    if near && clear_of(back, &self.labels.cards) && clear_of(r, &drawn) {
                        at = Some(r);
                    }
                }
            }
            // The line asked for: its label shown whatever it covers.
            if at.is_none() && item.whole {
                let best = item.chosen.or_else(|| item.clear.iter().position(|c| *c));
                let r = screen(item.spots[best.unwrap_or(0)]);
                at = Some(clamp_into(r, visible));
            }
            let Some(r) = at else { continue };
            let color = colors.get(&item.edge).copied().unwrap_or(p.text_muted);
            label(Pen { layer: labels, text, ds }, r, &item.text, &font, color, item.whole);
            drawn.push(r);
            self.hits.push((r, Hit::Edge(item.link)));
        }
        self.labels.items = items;
    }

    /// The labels of `lines` placed (diagram space) unless they were for the same layout,
    /// lines and zoom.
    fn place_labels(
        &mut self,
        graph: &SchemaGraph,
        layout: &DiagramLayout,
        lines: Vec<(usize, bool)>,
        font: &Font,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
    ) {
        let k = self.view.k();
        let key = LabelKey {
            layout: self.layout_generation,
            lines,
            font: font.size.to_bits(),
            k: k.to_bits(),
        };
        if self.labels.key.as_ref() == Some(&key) {
            return;
        }
        // A card's margin: a label never touches it.
        let margin = 4.0;
        if self.labels.key.as_ref().is_none_or(|old| old.layout != key.layout) {
            self.labels.cards =
                layout.layout.cards.iter().map(|c| diagram::inflate(c.rect, margin)).collect();
        }
        let mut items = Vec::with_capacity(key.lines.len());
        for &(e, whole) in &key.lines {
            let edge = &layout.layout.edges[e];
            let Some(link) = graph.links.get(edge.kind) else { continue };
            let label = if whole { whole_label(graph, link) } else { short_label(graph, link) };
            let shaped = text.shape_line(&label, font);
            let (w, h) = (shaped.width + font.size * 1.1, (font.size * 1.75).round());
            let spots = diagram::label_spots(&edge.points, (w / k, h / k), ds.px(5.0) / k);
            let clear = spots.iter().map(|&r| clear_of(r, &self.labels.cards)).collect();
            items.push(Label {
                edge: e,
                link: edge.kind,
                text: label,
                whole,
                spots,
                clear,
                chosen: None,
            });
        }
        let spots: Vec<Vec<Rect>> = items.iter().map(|i| i.spots.clone()).collect();
        for (item, chosen) in
            items.iter_mut().zip(diagram::place_labels(&spots, &self.labels.cards))
        {
            item.chosen = chosen;
        }
        self.labels.items = items;
        self.labels.key = Some(key);
    }

    /// A column's row: its key's mark (the primary key's, a foreign key's), its name, its
    /// type (or the table it points to). Returns its rectangle.
    fn paint_row(
        &self,
        pen: Pen<'_, '_>,
        icons: &icons::Icons,
        fonts: &CardFonts,
        (graph, t, column): (&SchemaGraph, usize, usize),
        (r, y, row_h, k): (Rect, f32, f32, f32),
        hovered: bool,
    ) -> Rect {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let table = &graph.tables[t];
        let Some(c) = table.structure.columns.get(column) else { return Rect::default() };
        // The link it starts (among those shown), its color its kind's.
        let link = self
            .row_links
            .get(&t)
            .and_then(|columns| columns.get(c.name.as_str()))
            .and_then(|&l| graph.links.get(l));
        let points_to = link.and_then(|l| graph.tables.get(l.to));
        let tint = link.map_or(p.accent, |l| match l.origin {
            Origin::Real => p.accent,
            _ => edge_style(l.origin, true, p).color,
        });
        let target = points_to.map(|to| format!("→ {}", to.table().name));
        let declared = c.declared_type.to_lowercase();
        let detail = match &target {
            Some(target) => Some((target.as_str(), tint)),
            None if declared.is_empty() => None,
            None => Some((declared.as_str(), p.text_muted)),
        };
        let keyed = c.in_key || points_to.is_some();
        let row = Row {
            mark: "",
            mark_color: p.text_muted,
            label: &c.name,
            color: if keyed { p.text } else { p.text.with_alpha(0.8) },
            detail,
            hovered,
        };
        let (rect, _) = diagram::card_row(Pen { layer, text, ds }, &fonts.row, r, y, row_h, k, row);
        let icon = if c.in_key {
            Some(("key", p.terminal_ansi[3]))
        } else if points_to.is_some() {
            Some(("key-foreign", tint))
        } else {
            None
        };
        if let Some((name, color)) = icon
            && let Some(id) = icons.id(name)
        {
            let size = (11.0 * k).round().max(4.0);
            let at = Rect::new(r.min_x() + 11.0 * k, y + (row_h - size) / 2.0, size, size);
            draw_icon(layer, icons, id, at, color);
        }
        rect
    }

    fn paint_toolbar(
        &mut self,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
        b: Rect,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let bar = diagram::toolbar(layer, ds, b, TOOLBAR_H);
        self.hits.push((bar, Hit::Bar));
        let mid = bar.min_y() + bar.size.height / 2.0;
        let mut x = bar.min_x() + px(20.0);
        if !self.history.is_empty() {
            let r = Rect::new(x - px(6.0), mid - px(14.0), px(28.0), px(28.0));
            diagram::back_button(Pen { layer, text, ds }, r, self.hover == Some(Hit::Back));
            self.hits.push((r, Hit::Back));
            self.tip(Hit::Back, r, "relations.tip_back", Some("database.diagram_back"));
            x = r.max_x() + px(10.0);
        }
        x = diagram::toolbar_title(text, layer, ds, bar, x, &t("relations.title")) + px(12.0);
        if let Some(request) = &self.request {
            let name = match &request.schema {
                Some(schema) => format!("{} · {schema}", request.access.name()),
                None => request.access.name(),
            };
            let spec =
                ds::BadgeSpec { label: &name, x, mid, align_right: false, tone: Tone::Accent };
            x = ds::badge(layer, text, ds, spec).max_x() + px(16.0);
        }

        // The legend, on the right (as room allows): the kinds of lines drawn.
        let small = ds.ui_font(-1.5);
        let mut right = bar.max_x() - px(20.0);
        let legend = t("relations.legend_key");
        let shaped = text.shape_line(&legend, &small);
        let legend_w = shaped.width + px(40.0);
        let extra: Vec<(String, EdgeStyle, String)> = [
            (1, "relations.legend_virtual", "relations.tip_legend_virtual", Origin::Virtual),
            (
                2,
                "relations.legend_inferred",
                "relations.tip_legend_inferred",
                Origin::Inferred(Confidence::High),
            ),
        ]
        .into_iter()
        .filter(|(i, ..)| self.kinds_drawn[*i])
        .map(|(_, key, tip, origin)| (t(key), edge_style(origin, true, p), t(tip)))
        .collect();

        // The filter.
        let field_w = px(230.0);
        if x + field_w + px(200.0) < right {
            let f = Rect::new(x, mid - px(16.0), field_w, px(32.0));
            self.paint_filter(Pen { layer, text, ds }, icons, f, focused && self.typing);
            x = f.max_x() + px(16.0);
        }
        // A table's neighbors: how far, and every table again.
        if let Some(name) = self
            .focus
            .and_then(|f| self.graph.as_ref()?.tables.get(f))
            .map(|t| t.table().name.clone())
        {
            let caption = text.shape_line(&tr!("relations.neighbors_of", table = name), &small);
            if x + caption.width < right - px(220.0) {
                draw_text(
                    layer,
                    &caption,
                    x,
                    ds::optical_baseline(bar.min_y(), bar.size.height, &small),
                    p.text_muted,
                );
                x += caption.width + px(10.0);
            }
            let labels: Vec<String> = DEPTHS.iter().map(ToString::to_string).collect();
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            let width = ds::segmented_width(text, ds, &labels);
            let hovered = match self.hover {
                Some(Hit::Depth(d)) => DEPTHS.iter().position(|&x| x == d),
                _ => None,
            };
            let selected = DEPTHS.iter().position(|&d| d == self.depth).unwrap_or(0);
            let spec =
                ds::SegmentedSpec { right: x + width, mid, labels: &labels, selected, hovered };
            for (seg, d) in ds::segmented(layer, text, ds, spec).into_iter().zip(DEPTHS) {
                self.hits.push((seg, Hit::Depth(d)));
                let tip = Tip::new(seg, tr!("relations.tip_depth", count = d));
                self.tips.push((Hit::Depth(d), tip));
            }
            x += width + px(12.0);
            let all = text.shape_line(&t("relations.show_all"), &small);
            let r = Rect::new(x - px(6.0), mid - px(14.0), all.width + px(12.0), px(28.0));
            if self.hover == Some(Hit::ShowAll) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.08)).rounded(px(radius::MD)));
            }
            draw_text(
                layer,
                &all,
                x,
                ds::optical_baseline(bar.min_y(), bar.size.height, &small),
                p.accent,
            );
            self.hits.push((r, Hit::ShowAll));
            x = r.max_x() + px(16.0);
        }
        // The kinds besides the engine's keys, right to left, while they fit.
        let mut room = right - legend_w - x;
        for (label, style, tip) in extra.iter().rev() {
            let item = text.shape_line(label, &small);
            let w = item.width + px(40.0) + px(16.0);
            if room < w {
                break;
            }
            room -= w;
            right -= item.width;
            draw_text(
                layer,
                &item,
                right,
                ds::optical_baseline(bar.min_y(), bar.size.height, &small),
                p.text_muted,
            );
            let x1 = right - px(8.0);
            diagram::legend_sample(layer, ds, x1 - px(26.0), x1, mid, *style);
            let area = Rect::new(x1 - px(26.0), mid - px(12.0), item.width + px(34.0), px(24.0));
            self.tips.push((Hit::Bar, Tip::new(area, tip.clone())));
            right = x1 - px(26.0) - px(16.0);
        }
        if right - legend_w > x {
            right -= shaped.width;
            draw_text(
                layer,
                &shaped,
                right,
                ds::optical_baseline(bar.min_y(), bar.size.height, &small),
                p.text_muted,
            );
            let x1 = right - px(8.0);
            let x0 = x1 - px(26.0);
            let area = Rect::new(x0, mid - px(12.0), shaped.width + px(34.0), px(24.0));
            self.tips.push((Hit::Bar, Tip::new(area, t("relations.tip_legend_key"))));
            let style = EdgeStyle {
                color: p.text_muted,
                dashed: false,
                head: Head::Arrow { hollow: None },
                width: EdgeStyle::LINE,
            };
            diagram::legend_sample(layer, ds, x0, x1, mid, style);
        }
    }

    /// A control's tooltip (`key` translated), with command `command`'s keys when it has
    /// any.
    fn tip(&mut self, hit: Hit, anchor: Rect, key: &str, command: Option<&str>) {
        let keys = command.and_then(ui_components::shortcuts::label);
        self.tips.push((hit, Tip::new(anchor, t(key)).with_shortcut(keys)));
    }

    /// The filter's pill: the magnifier, the text (or what it's for), × once there's text.
    fn paint_filter(&mut self, pen: Pen<'_, '_>, icons: &icons::Icons, f: Rect, typing: bool) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let field = ds::search_field(layer, icons, ds, f, typing);
        self.hits.push((f, Hit::Field));
        self.tip(Hit::Field, f, "relations.tip_filter", Some("database.diagram_find"));
        let mut right = field.max_x();
        if !self.filter.text().is_empty() {
            let s = px(18.0);
            let r = Rect::new(
                f.max_x() - px(8.0) - s,
                (f.min_y() + (f.size.height - s) / 2.0).round(),
                s,
                s,
            );
            let hovered = self.hover == Some(Hit::Clear);
            layer.quad(
                Quad::filled(r, p.text.with_alpha(if hovered { 0.12 } else { 0.06 }))
                    .rounded(s / 2.0),
            );
            if let Some(id) = icons.id("x") {
                let x = px(9.0);
                let ir = Rect::new(
                    (r.min_x() + (s - x) / 2.0).round(),
                    (r.min_y() + (s - x) / 2.0).round(),
                    x,
                    x,
                );
                draw_icon(layer, icons, id, ir, p.text_muted);
            }
            self.hits.push((r, Hit::Clear));
            self.tip(Hit::Clear, r, "relations.tip_clear", None);
            right = r.min_x() - px(6.0);
        }
        let style = TextInputStyle {
            font: Font::new(p.ui_font.clone(), px(p.ui_font_size - 0.5)),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: 0.0,
            border_width: 0.0,
            padding: 0.0,
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let area =
            Rect::new(field.min_x(), f.min_y(), (right - field.min_x()).max(0.0), f.size.height);
        self.filter.paint(layer, text, area, typing, &t("relations.filter"), &style);
    }
}

/// How a link is drawn: the engine's keys plain (in the accent when lit), the accepted ones
/// in their own color with a hollow arrow, those found from names dashed, as strong as
/// they're sure.
fn edge_style(origin: Origin, hot: bool, p: &Palette) -> EdgeStyle {
    match origin {
        Origin::Real => EdgeStyle {
            color: if hot { p.accent } else { p.text_muted.with_alpha(0.75) },
            dashed: false,
            head: Head::Arrow { hollow: None },
            width: EdgeStyle::LINE,
        },
        Origin::Virtual => EdgeStyle {
            color: p.terminal_ansi[2].with_alpha(if hot { 1.0 } else { 0.8 }),
            dashed: false,
            head: Head::Arrow { hollow: Some(p.editor_background) },
            width: EdgeStyle::LINE,
        },
        Origin::Inferred(sure) => {
            let alpha = match (hot, sure) {
                (true, _) => 1.0,
                (_, Confidence::High) => 0.85,
                (_, Confidence::Medium) => 0.65,
                (_, Confidence::Low) => 0.45,
            };
            EdgeStyle {
                color: p.terminal_ansi[3].with_alpha(alpha),
                dashed: true,
                head: Head::Arrow { hollow: None },
                width: EdgeStyle::LINE,
            }
        }
    }
}

/// How sure a relation found from names is, in words.
fn confidence_word(confidence: Confidence) -> String {
    t(match confidence {
        Confidence::High => "relations.sure_high",
        Confidence::Medium => "relations.sure_medium",
        Confidence::Low => "relations.sure_low",
    })
}

/// A key as read in the list: `reviews.productId → products.id`.
fn key_label(key: &VirtualKey) -> String {
    let side = |table: &database::TableRef, columns: &[String]| match columns {
        [one] => format!("{}.{one}", table.name),
        many => format!("{}.({})", table.name, many.join(", ")),
    };
    format!("{} → {}", side(&key.table, &key.columns), side(&key.target, &key.references))
}

/// A button of text ending at `right` on `mid` (its hover shown). Returns its area.
fn text_button(
    pen: Pen<'_, '_>,
    right: f32,
    mid: f32,
    label: &str,
    color: Rgba,
    hovered: bool,
) -> Rect {
    let Pen { layer, text, ds } = pen;
    let font = ds.ui_font(-1.5).with_weight(ds::BUTTON_WEIGHT);
    let shaped = text.shape_line(label, &font);
    let (w, h) = (shaped.width + ds.px(16.0), ds.px(24.0));
    let r = Rect::new((right - w).round(), (mid - h / 2.0).round(), w.round(), h);
    let fill = color.with_alpha(if hovered { 0.18 } else { 0.10 });
    layer.quad(Quad::filled(r, fill).rounded(ds.px(radius::SM)));
    draw_text(
        layer,
        &shaped,
        r.min_x() + ds.px(8.0),
        ds::optical_baseline(r.min_y(), h, &font),
        color,
    );
    r
}

impl RelationsView {
    /// Under the diagram, on its left: which relations are shown, and the list of those
    /// found from names (how many wait for a decision).
    fn paint_controls(
        &mut self,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        area: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(area));
        let kinds = [RelationKinds::Real, RelationKinds::Inferred, RelationKinds::Both];
        let labels =
            [t("relations.kind_real"), t("relations.kind_inferred"), t("relations.kind_both")];
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let width = ds::segmented_width(text, ds, &labels);
        let waiting = self.undecided().len();
        let found = tr!("relations.found_button", count = waiting);
        let font = ds.ui_font(-1.5).with_weight(ds::BUTTON_WEIGHT);
        let found_w = text.shape_line(&found, &font).width + px(16.0);
        let mid = area.max_y() - px(36.0);
        let left = area.min_x() + px(16.0);
        let back = Rect::new(left, mid - px(24.0), width + found_w + px(28.0), px(48.0));
        ds::surface_with(layer, ds, back, ds::Elevation::Low, radius::LG, p.editor_background);
        self.hits.push((back, Hit::Bar));
        self.bar = back;
        let x = left + px(8.0);
        let hovered = match self.hover {
            Some(Hit::Kinds(k)) => kinds.iter().position(|&x| x == k),
            _ => None,
        };
        let selected = kinds.iter().position(|&k| k == self.notes.show).unwrap_or(2);
        let spec = ds::SegmentedSpec { right: x + width, mid, labels: &labels, selected, hovered };
        let kind_tips =
            ["relations.tip_kind_real", "relations.tip_kind_inferred", "relations.tip_kind_both"];
        for ((seg, kind), tip) in
            ds::segmented(layer, text, ds, spec).into_iter().zip(kinds).zip(kind_tips)
        {
            self.hits.push((seg, Hit::Kinds(kind)));
            self.tip(Hit::Kinds(kind), seg, tip, None);
        }
        let color = if waiting > 0 { p.terminal_ansi[3] } else { p.text_muted };
        let r = text_button(
            Pen { layer, text, ds },
            back.max_x() - px(8.0),
            mid,
            &found,
            if self.list_open { p.accent } else { color },
            self.hover == Some(Hit::Found),
        );
        self.hits.push((r, Hit::Found));
        self.tip(Hit::Found, r, "relations.tip_found", None);
    }

    /// The list of the relations found from names, over the diagram's right side: those
    /// waiting (accept, set aside), those accepted and those set aside (each undone).
    fn paint_list(
        &mut self,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        area: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let w = super::list_width(area.size.width, ds.scale);
        let panel = Rect::new(
            area.max_x() - w - px(16.0),
            area.min_y() + px(16.0),
            w,
            area.size.height - px(32.0),
        );
        let layer = scene.push_layer(Some(area));
        ds::surface(layer, ds, panel, ds::Elevation::Medium, radius::LG);
        self.hits.push((panel, Hit::List));
        let pad = px(16.0);
        let title_font = ds.ui_font(0.5).with_weight(Font::SEMIBOLD);
        let head = Rect::new(panel.min_x(), panel.min_y(), panel.size.width, px(48.0));
        let title = text.shape_line(&t("relations.found_title"), &title_font);
        draw_text(
            layer,
            &title,
            head.min_x() + pad,
            ds::optical_baseline(head.min_y(), head.size.height, &title_font),
            p.text,
        );
        let close = text_button(
            Pen { layer, text, ds },
            head.max_x() - pad,
            head.min_y() + head.size.height / 2.0,
            "×",
            p.text_muted,
            self.hover == Some(Hit::Found),
        );
        self.hits.push((close, Hit::Found));
        self.tip(Hit::Found, close, "relations.tip_close_list", None);
        ds::divider(layer, ds, panel.min_x(), panel.max_x(), head.max_y());

        // The rows, scrolled, clipped to the panel.
        let body = Rect::new(
            panel.min_x(),
            head.max_y() + ds.line(),
            panel.size.width,
            panel.max_y() - head.max_y() - px(6.0),
        );
        let rows = scene.push_layer(Some(body));
        let row_h = px(52.0);
        let section_h = px(30.0);
        let label_font = ds.code_font(-1.5);
        let small = ds.ui_font(-2.0);
        let mut y = body.min_y() + px(4.0) - (self.list_scroll * ds.scale).round();
        let graph = self.graph.clone();
        let undecided = self.undecided().to_vec();
        let accepted = self.notes.virtual_keys.clone();
        let ignored = self.notes.ignored.clone();
        if undecided.is_empty() && accepted.is_empty() && ignored.is_empty() {
            ds::empty_state(
                rows,
                text,
                ds,
                body,
                &t("relations.found_none"),
                Some(&t("relations.found_none_detail")),
            );
            return;
        }
        let section =
            |rows: &mut render_api::Layer, text: &mut TextSystem, y: &mut f32, title: String| {
                let shaped = text.shape_line(&title, &small);
                draw_text(
                    rows,
                    &shaped,
                    body.min_x() + pad,
                    ds::optical_baseline(*y, section_h, &small),
                    p.text_muted,
                );
                *y += section_h;
            };
        enum Kind {
            Waiting(usize),
            Accepted(usize),
            Ignored(usize),
        }
        let mut items: Vec<(Option<String>, Kind)> = Vec::new();
        for (n, &i) in undecided.iter().enumerate() {
            let title = (n == 0).then(|| tr!("relations.found_waiting", count = undecided.len()));
            items.push((title, Kind::Waiting(i)));
        }
        for i in 0..accepted.len() {
            let title = (i == 0).then(|| tr!("relations.found_accepted", count = accepted.len()));
            items.push((title, Kind::Accepted(i)));
        }
        for i in 0..ignored.len() {
            let title = (i == 0).then(|| tr!("relations.found_ignored", count = ignored.len()));
            items.push((title, Kind::Ignored(i)));
        }
        for (title, kind) in items {
            if let Some(title) = title {
                section(rows, text, &mut y, title);
            }
            let r = Rect::new(body.min_x() + px(6.0), y, body.size.width - px(12.0), row_h);
            y += row_h;
            if r.max_y() < body.min_y() || r.min_y() > body.max_y() {
                continue;
            }
            let (key, confidence) = match &kind {
                Kind::Waiting(i) => (self.found[*i].key.clone(), Some(self.found[*i].confidence)),
                Kind::Accepted(i) => (accepted[*i].clone(), None),
                Kind::Ignored(i) => (ignored[*i].clone(), None),
            };
            let table = graph.as_ref().and_then(|g| g.find(&key.table));
            let row_hit = table.map(Hit::ListRow);
            if row_hit.is_some() && self.hover == row_hit {
                rows.quad(Quad::filled(r, p.text.with_alpha(0.05)).rounded(px(radius::SM)));
            }
            // Its buttons first (the label gives way to them).
            let mid = r.min_y() + r.size.height / 2.0;
            let mut right = r.max_x() - px(8.0);
            let buttons: Vec<(String, Rgba, Hit)> = match kind {
                Kind::Waiting(i) => vec![
                    (t("relations.ignore"), p.text_muted, Hit::Ignore(i)),
                    (t("relations.accept"), p.accent, Hit::Accept(i)),
                ],
                Kind::Accepted(i) => {
                    vec![(t("relations.undo_accept"), p.text_muted, Hit::Unaccept(i))]
                }
                Kind::Ignored(i) => {
                    vec![(t("relations.undo_ignore"), p.text_muted, Hit::Unignore(i))]
                }
            };
            let mut button_hits = Vec::new();
            for (label, color, hit) in buttons {
                let b = text_button(
                    Pen { layer: rows, text, ds },
                    right,
                    mid,
                    &label,
                    color,
                    self.hover == Some(hit),
                );
                right = b.min_x() - px(6.0);
                button_hits.push((b, hit));
            }
            let lx = r.min_x() + px(10.0);
            let max = (right - lx - px(8.0)).max(0.0);
            let label = ds::ellipsize(text, &key_label(&key), &label_font, max);
            let shaped = text.shape_line(&label, &label_font);
            draw_text(rows, &shaped, lx, (r.min_y() + r.size.height * 0.45).round(), p.text);
            let note = match (&kind, confidence) {
                (Kind::Waiting(_), Some(c)) => {
                    format!("{} · {}", t("relations.inferred"), confidence_word(c))
                }
                (Kind::Accepted(_), _) => t("relations.virtual_key"),
                _ => t("relations.ignored"),
            };
            let note_color = match (&kind, confidence) {
                (Kind::Waiting(_), Some(Confidence::High)) => p.terminal_ansi[3],
                (Kind::Accepted(_), _) => p.terminal_ansi[2],
                _ => p.text_muted,
            };
            let note = ds::ellipsize(text, &note, &small, max);
            let shaped = text.shape_line(&note, &small);
            draw_text(rows, &shaped, lx, (r.min_y() + r.size.height * 0.80).round(), note_color);
            if let Some(hit) = row_hit {
                self.hits.push((r, hit));
            }
            self.hits.extend(button_hits);
        }
    }
}

/// A table card's header: `T` (a view: `V`), its name, its schema when the source has
/// several (else its rows, as the engine estimates them; a table only named: why).
fn card_header(
    pen: Pen<'_, '_>,
    fonts: &CardFonts,
    several_schemas: bool,
    table: &GraphTable,
    r: Rect,
    k: f32,
) -> Rect {
    let p: &Palette = pen.ds.palette;
    let (glyph, color) = match (table.external, table.kind) {
        (true, _) => ("T", p.text_muted),
        (false, TableKind::View) => ("V", p.terminal_ansi[5]),
        (false, TableKind::Table) => ("T", p.accent),
    };
    let subtitle = if table.external {
        t("relations.external")
    } else if several_schemas {
        table.table().schema.clone().unwrap_or_default()
    } else {
        table.rows.map(|n| tr!("relations.rows", count = n)).unwrap_or_default()
    };
    let header = Header {
        glyph,
        color,
        title: &table.table().name,
        subtitle: &subtitle,
        muted: table.external,
    };
    diagram::card_header(pen, fonts, header, r, k)
}

/// How many rows meet at each end of a line through screen `points`: `1` where it ends
/// (the table pointed to), `n` (or `1`) where it starts.
fn cardinality(
    pen: Pen<'_, '_>,
    points: &[Point],
    dir: Dir,
    k: f32,
    cardinality: Cardinality,
    color: Rgba,
) {
    let Pen { layer, text, ds } = pen;
    let (Some(&start), Some(&next), Some(&tip)) = (points.first(), points.get(1), points.last())
    else {
        return;
    };
    let font = Font::new(ds.palette.ui_font.clone(), (10.0 * k).round().max(7.0))
        .with_weight(Font::SEMIBOLD);
    let many = match cardinality {
        Cardinality::OneToMany => "n",
        Cardinality::OneToOne => "1",
    };
    let shaped = text.shape_line(many, &font);
    let x = if next.x < start.x { start.x - 5.0 * k - shaped.width } else { start.x + 5.0 * k };
    draw_text(layer, &shaped, x.round(), (start.y + 13.0 * k).round(), color);
    let one = text.shape_line("1", &font);
    let head = (10.0 * k).max(5.0);
    let x = match dir {
        Dir::Left => tip.x + head + 3.0 * k,
        _ => tip.x - head - 3.0 * k - one.width,
    };
    draw_text(layer, &one, x.round(), (tip.y + 13.0 * k).round(), color);
}

/// What joins two tables, in a pill at `r` beside its line (the line itself left in view);
/// the line asked for (`strong`) outlined in its color.
fn label(pen: Pen<'_, '_>, r: Rect, label: &str, font: &Font, color: Rgba, strong: bool) {
    let Pen { layer, text, ds } = pen;
    let p = ds.palette;
    let shaped = text.shape_line(label, font);
    let h = r.size.height;
    layer.quad(
        Quad::filled(r, p.popup_background)
            .rounded(h / 2.0)
            .bordered(ds.line(), color.with_alpha(if strong { 0.9 } else { 0.5 })),
    );
    let at = (r.min_x() + (r.size.width - shaped.width) / 2.0).round();
    draw_text(layer, &shaped, at, ds::optical_baseline(r.min_y(), h, font), p.text);
}

/// A line's label in a word: the columns and what they point to (`user_id → users.id`);
/// the table it starts from is where it starts.
fn short_label(graph: &SchemaGraph, link: &Link) -> String {
    let to = graph.tables.get(link.to).map_or("?", |t| t.table().name.as_str());
    let side = |columns: &[String]| match columns {
        [one] => one.clone(),
        many => format!("({})", many.join(", ")),
    };
    format!("{} → {to}.{}", side(&link.columns), side(&link.references))
}

/// A line's whole label: both ends and its kind (`orders.user_id → users.id · virtual
/// key`).
fn whole_label(graph: &SchemaGraph, link: &Link) -> String {
    match link.origin {
        Origin::Real => graph.label(link),
        Origin::Virtual => format!("{} · {}", graph.label(link), t("relations.virtual")),
        Origin::Inferred(c) => {
            format!("{} · {}, {}", graph.label(link), t("relations.inferred"), confidence_word(c))
        }
    }
}

/// A line's kind in words, and its color: the engine's key, one accepted, one found from
/// names (how sure).
fn kind_note(origin: Origin, p: &Palette) -> (String, Rgba) {
    match origin {
        Origin::Real => (t("relations.fk"), p.text_muted),
        Origin::Virtual => (t("relations.virtual"), p.terminal_ansi[2]),
        Origin::Inferred(c) => (
            format!("{} · {}", t("relations.inferred"), confidence_word(c)),
            if c == Confidence::High { p.terminal_ansi[3] } else { p.text_muted },
        ),
    }
}

/// A [`text_button`]'s width for `label`.
fn text_button_width(text: &mut TextSystem, ds: &Ctx<'_>, label: &str) -> f32 {
    let font = ds.ui_font(-1.5).with_weight(ds::BUTTON_WEIGHT);
    (text.shape_line(label, &font).width + ds.px(16.0)).round()
}

impl RelationsView {
    /// A line's popover: what it joins, its kind, what can be decided about it (accept,
    /// set aside, undo), and its two tables to go to. Under where it was clicked (over it
    /// when there's no room), in what the bars leave of the area, `POPOVER_GAP` from the
    /// bar of controls.
    fn paint_popover(
        &mut self,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        area: Rect,
    ) {
        let Some((key, at)) = self.popover.clone() else { return };
        let Some(graph) = self.graph.clone() else { return };
        let Some(link) = key.find(&graph).and_then(|l| graph.links.get(l)) else { return };
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        // Where: the click, else the middle of the line drawn.
        let anchor = at.or_else(|| {
            let l = key.find(&self.drawn)?;
            let edge = self.layout.layout.edges.iter().find(|e| e.kind == l)?;
            let (a, b) = match edge.points.len() {
                0 => return None,
                1 => (edge.points[0], edge.points[0]),
                n => (edge.points[(n - 1) / 2], edge.points[n / 2]),
            };
            Some(Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0))
        });
        let Some(anchor) = anchor.map(|a| self.view.to_screen(a)) else { return };

        let title = graph.label(link);
        let (note, note_color) = kind_note(link.origin, p);
        let title_font = ds.code_font(-1.0);
        let note_font = ds.ui_font(-2.0);
        let decisions: Vec<(String, Rgba, Hit)> = match link.origin {
            Origin::Inferred(_) => vec![
                (t("relations.accept"), p.accent, Hit::PopAccept),
                (t("relations.ignore"), p.text_muted, Hit::PopIgnore),
            ],
            Origin::Virtual => vec![(t("relations.undo_decision"), p.text_muted, Hit::PopUndo)],
            Origin::Real => Vec::new(),
        };
        let name = |t: usize| graph.tables.get(t).map(|t| t.table().name.clone());
        let mut targets = vec![(link.from, name(link.from))];
        if link.to != link.from {
            targets.push((link.to, name(link.to)));
        }
        let goes: Vec<(String, Hit)> = targets
            .into_iter()
            .filter_map(|(t, n)| Some((tr!("relations.go_to", table = n?), Hit::GoTo(t))))
            .collect();
        let pad = px(14.0);
        let gap = px(6.0);
        let row_w = |text: &mut TextSystem, labels: &[&str]| {
            labels.iter().map(|l| text_button_width(text, ds, l)).sum::<f32>()
                + gap * labels.len().saturating_sub(1) as f32
        };
        let decision_labels: Vec<&str> = decisions.iter().map(|d| d.0.as_str()).collect();
        let go_labels: Vec<&str> = goes.iter().map(|g| g.0.as_str()).collect();
        let content = text
            .shape_line(&title, &title_font)
            .width
            .max(text.shape_line(&note, &note_font).width)
            .max(row_w(text, &decision_labels))
            .max(row_w(text, &go_labels));
        let w = (content + 2.0 * pad).max(px(240.0)).min(area.size.width - px(16.0)).round();
        let title_h = px(22.0);
        let note_h = px(18.0);
        let button_h = px(24.0);
        let mut h = pad + title_h + note_h + px(10.0) + button_h + pad;
        if !decisions.is_empty() {
            h += button_h + px(8.0);
        }
        // What it may cover: the area the bars and the list leave, `POPOVER_GAP` from the
        // bar of controls.
        let visible = self.view.visible();
        let floor = visible.max_y().min(self.bar.min_y() - px(ds::POPOVER_GAP));
        let room = Rect::new(
            visible.min_x() + px(8.0),
            visible.min_y() + px(8.0),
            (visible.size.width - px(16.0)).max(w),
            (floor - visible.min_y() - px(8.0)).max(h),
        );
        let below = anchor.y + px(ds::POPOVER_GAP);
        let top =
            if below + h <= room.max_y() { below } else { anchor.y - px(ds::POPOVER_GAP) - h };
        let r =
            clamp_into(Rect::new((anchor.x - w / 2.0).round(), top.round(), w, h.round()), room);
        let r = Rect::new(r.min_x().round(), r.min_y().round(), w, h.round());

        let layer = scene.push_layer(Some(area));
        ds::surface(layer, ds, r, ds::Elevation::Medium, radius::LG);
        self.hits.push((r, Hit::Popover));
        let x = r.min_x() + pad;
        let max = r.size.width - 2.0 * pad;
        let mut y = r.min_y() + pad;
        let shown = ds::ellipsize(text, &title, &title_font, max);
        let shaped = text.shape_line(&shown, &title_font);
        draw_text(layer, &shaped, x, ds::optical_baseline(y, title_h, &title_font), p.text);
        y += title_h;
        let shown = ds::ellipsize(text, &note, &note_font, max);
        let shaped = text.shape_line(&shown, &note_font);
        draw_text(layer, &shaped, x, ds::optical_baseline(y, note_h, &note_font), note_color);
        y += note_h + px(10.0);
        let mut row = |text: &mut TextSystem, items: Vec<(String, Rgba, Hit)>, y: f32| {
            let mut left = x;
            for (label, color, hit) in items {
                let w = text_button_width(text, ds, &label);
                let pen = Pen { layer: &mut *layer, text, ds };
                let b = text_button(
                    pen,
                    left + w,
                    y + button_h / 2.0,
                    &label,
                    color,
                    self.hover == Some(hit),
                );
                self.hits.push((b, hit));
                left = b.max_x() + gap;
            }
        };
        if !decisions.is_empty() {
            row(text, decisions, y);
            y += button_h + px(8.0);
        }
        let goes = goes.into_iter().map(|(l, h)| (l, p.accent, h)).collect();
        row(text, goes, y);
    }
}
