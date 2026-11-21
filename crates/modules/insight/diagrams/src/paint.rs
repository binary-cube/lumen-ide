//! Painting the diagram: the type cards with their members, the relations' lines and arrows, the
//! toolbar and its legend.

use super::*;
use ui_components::diagram::{self, CardLook, EdgeStyle, Head, Header, Row};

impl DiagramView {
    pub(crate) fn paint_toolbar(
        &mut self,
        ds: &Ctx<'_>,
        (text, icons): (&mut text_layout::TextSystem, &icons::Icons),
        scene: &mut render_api::Scene,
        b: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let layer = scene.push_layer(Some(b));
        let bar = diagram::toolbar(layer, ds, b, TOOLBAR_H);
        let mid = bar.min_y() + bar.size.height / 2.0;
        let mut x = bar.min_x() + px(20.0);
        if !self.history.is_empty() {
            let r = ds::square_button(ds, (x - px(6.0), mid), ds::button_size::LARGE);
            diagram::back_button(Pen { layer, text, ds }, icons, r, self.hover == Some(Hit::Back));
            self.hits.push((r, Hit::Back));
            x = r.max_x() + px(10.0);
        }
        x = diagram::toolbar_title(text, layer, ds, bar, x, &t("diagram.title")) + px(12.0);
        if !self.fqn.is_empty() {
            ds::badge(
                layer,
                text,
                ds,
                ds::BadgeSpec {
                    label: code_intelligence::short_name(&self.fqn),
                    x,
                    mid,
                    align_right: false,
                    tone: Tone::Accent,
                },
            );
        }

        // The legend, on the right.
        let small = ds.ui_font(-1.5);
        let mut right = bar.max_x() - px(20.0);
        let items = [
            (t("diagram.extends"), Relation::Extends),
            (t("diagram.implements"), Relation::Implements),
            (t("diagram.uses_trait"), Relation::UsesTrait),
            (t("diagram.depends"), Relation::Depends),
        ];
        for (label, relation) in items.iter().rev() {
            let shaped = text.shape_line(label, &small);
            right -= shaped.width;
            if right < x + px(160.0) {
                break;
            }
            draw_text(
                layer,
                &shaped,
                right,
                ds::optical_baseline(bar.min_y(), bar.size.height, &small),
                p.text_muted,
            );
            let x1 = right - px(8.0);
            let x0 = x1 - px(26.0);
            diagram::legend_sample(layer, ds, x0, x1, mid, edge_style(*relation, p));
            right = x0 - px(18.0);
        }
    }

    /// The diagram in `area`: the edges, then the cards over them.
    pub(crate) fn paint_diagram(
        &mut self,
        diagram: &ClassDiagram,
        ds: &Ctx<'_>,
        text: &mut text_layout::TextSystem,
        scene: &mut render_api::Scene,
        area: Rect,
    ) {
        let p = ds.palette;
        let k = self.k();
        // Edges, under the cards.
        let edges = scene.push_layer(Some(area));
        self.paint_edges(edges, p);

        // Cards.
        let fonts = CardFonts::new(p, k);
        for c in 0..self.layout.cards.len() {
            let card = self.layout.cards[c].clone();
            let r = self.rect_to_screen(card.rect);
            if r.intersect(&area).is_none() {
                continue;
            }
            let Some(node) = diagram.nodes.get(card.node) else { continue };
            let layer = scene.push_layer(Some(area));
            self.paint_card(Pen { layer, text, ds }, &fonts, &card, node);
        }
    }

    /// The edges: lines (dashed for some relations) ending in their arrows.
    pub(crate) fn paint_edges(&self, edges: &mut Layer, p: &Palette) {
        let k = self.k();
        for edge in &self.layout.edges {
            let points: Vec<Point> = edge.points.iter().map(|&q| self.view.to_screen(q)).collect();
            diagram::edge(edges, &points, edge.dir, k, edge_style(edge.kind, p));
        }
    }

    /// A class's card: its frame (raised and outlined in the accent for the focused one),
    /// its header, its members.
    pub(crate) fn paint_card(
        &mut self,
        pen: Pen<'_, '_>,
        fonts: &CardFonts,
        card: &Card,
        node: &DiagramNode,
    ) {
        let Pen { layer, text, ds } = pen;
        let k = self.k();
        let r = self.rect_to_screen(card.rect);
        let focus = node.role == DiagramRole::Focus;
        let hovered = self.hover == Some(Hit::Card(card.node));
        let look = CardLook { focus, hovered, muted: node.path.is_none() };
        diagram::card_frame(layer, ds, r, look, k, self.view.zoom, self.view.scale);
        let header = self.paint_card_header(Pen { layer, text, ds }, fonts, node, r);
        self.hits.push((header, Hit::Card(card.node)));
        self.paint_members(Pen { layer, text, ds }, fonts, card, node, header);
    }

    /// A card's header in `r`: the kind's badge, the name, the namespace. Returns its
    /// rectangle.
    pub(crate) fn paint_card_header(
        &self,
        pen: Pen<'_, '_>,
        fonts: &CardFonts,
        node: &DiagramNode,
        r: Rect,
    ) -> Rect {
        let p = pen.ds.palette;
        let k = self.k();
        let (glyph, color) = kind_badge(node.kind, p);
        // The namespace: the name qualified, without its own part (the language named it).
        let namespace = node
            .fqn
            .strip_suffix(node.name.as_str())
            .map_or("", |ns| ns.trim_end_matches(|c: char| !code_intelligence::is_name_char(c)));
        let header = Header {
            glyph,
            color,
            title: &node.name,
            subtitle: namespace,
            muted: node.path.is_none(),
        };
        diagram::card_header(pen, fonts, header, r, k)
    }

    /// A card's members under its `header` (a line between them), then how many more.
    pub(crate) fn paint_members(
        &mut self,
        pen: Pen<'_, '_>,
        fonts: &CardFonts,
        card: &Card,
        node: &DiagramNode,
        header: Rect,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.k();
        let r = self.rect_to_screen(card.rect);
        let row_font = &fonts.row;
        if card.shown + card.more > 0 {
            diagram::header_divider(layer, ds, r, header);
        }
        let row_h = ROW_H * k;
        let mut y = header.max_y() + 4.0 * k;
        for (m, member) in node.members.iter().take(card.shown).enumerate() {
            let label = match (member.kind, &member.detail) {
                (SymbolKind::Method, Some(sig)) => format!("{}{sig}", member.name),
                (SymbolKind::Method, None) => format!("{}()", member.name),
                (SymbolKind::Property, Some(ty)) => format!("${}: {ty}", member.name),
                (SymbolKind::Property, None) => format!("${}", member.name),
                _ => member.name.clone(),
            };
            let color = match member.kind {
                SymbolKind::Method => p.text,
                _ => p.text.with_alpha(0.8),
            };
            let row = Row {
                mark: visibility_mark(member.visibility),
                mark_color: p.text_muted,
                label: &label,
                color: if member.is_static { p.accent } else { color },
                detail: None,
                hovered: self.hover == Some(Hit::Member(card.node, m)),
            };
            let (rect, _) =
                diagram::card_row(Pen { layer, text, ds }, row_font, r, y, row_h, k, row);
            if node.path.is_some() {
                self.hits.push((rect, Hit::Member(card.node, m)));
            }
            y += row_h;
        }
        if card.more > 0 {
            let more = tr!("diagram.more", count = card.more);
            diagram::more_row(Pen { layer, text, ds }, &fonts.sub, r, y, row_h, k, &more);
        }
    }
}

/// The kind's letter and color (also in the legend).
pub(crate) fn kind_badge(kind: SymbolKind, p: &Palette) -> (&'static str, Rgba) {
    match kind {
        SymbolKind::Interface => ("I", p.terminal_ansi[2]),
        SymbolKind::Trait => ("T", p.terminal_ansi[3]),
        SymbolKind::Enum => ("E", p.terminal_ansi[5]),
        _ => ("C", p.accent),
    }
}

/// An edge's color and whether it's dashed / ends in a hollow triangle (UML: inheritance
/// and realization hollow, the rest an open arrow).
pub(crate) fn edge_style(relation: Relation, p: &Palette) -> EdgeStyle {
    let (color, dashed, hollow) = match relation {
        Relation::Extends => (p.accent, false, true),
        Relation::Implements => (p.terminal_ansi[2], true, true),
        Relation::UsesTrait => (p.terminal_ansi[3], true, false),
        Relation::Depends => (p.text_muted, true, false),
    };
    EdgeStyle {
        color,
        dashed,
        head: Head::Arrow { hollow: hollow.then_some(p.editor_background) },
        width: EdgeStyle::LINE,
    }
}

/// The visibility as UML marks it.
pub(crate) fn visibility_mark(v: Visibility) -> &'static str {
    match v {
        Visibility::Public => "+",
        Visibility::Protected => "#",
        Visibility::Private => "−",
    }
}
