//! The "Database" window drawn: the filter field, then the
//! tree's rows in view only (as the project tree draws its own: the same row height,
//! indentation, chevrons, guides and highlights).

use icons::Icons;
use kernel::{Rect, Rgba};
use render_api::{Layer, Quad};
use text_layout::{Font, TextSystem};
use ui_components::ds::{self, Ctx};
use ui_components::tree::{self as tree_row, TreeList};
use ui_components::{Palette, ViewerPaint};
use ui_kit::{TextInputStyle, centered_baseline, draw_icon, draw_text};

use super::{
    DatabaseWindow, FIELD_HEIGHT, FIELD_TOP, Hover, INDENT, LIST_TOP, PADDING, ROW_HEIGHT,
};
use crate::catalog::Listed;
use crate::tree::{Group, NodeKey, Row, RowKind};

/// A row's icon and its color.
fn icon_of(row: &Row, source_icon: &'static str, p: &Palette) -> (&'static str, Rgba) {
    match &row.kind {
        RowKind::Folder => ("folder", p.folder_icon),
        RowKind::Source => (source_icon, p.accent),
        RowKind::Database => ("database", p.text_muted),
        RowKind::Schema => ("layers", p.text_muted),
        RowKind::Group(Group::Tables) | RowKind::Group(Group::Views) | RowKind::Part(_) => {
            ("folder", p.folder_icon)
        }
        RowKind::Table(database::TableKind::Table) => ("table", p.accent),
        RowKind::Table(database::TableKind::View) => ("eye", p.vcs_modified),
        RowKind::Column { key: true, .. } => ("key", p.warning),
        RowKind::Column { foreign: true, .. } => ("link", p.accent),
        RowKind::Column { .. } => ("column", p.text_muted),
        RowKind::Index { .. } => ("index", p.text_muted),
        RowKind::Key => ("link", p.text_muted),
        RowKind::Loading => ("clock", p.text_muted),
        RowKind::Error => ("alert", p.danger),
        RowKind::Hint => ("alert", p.warning),
    }
}

impl DatabaseWindow {
    /// Row `i`'s icon's area (physical pixels), as [`Self::paint_rows`] draws it.
    pub(super) fn icon_rect(&self, i: usize) -> Rect {
        let px = |v: f32| (v * self.scale).round();
        let Some(row) = self.rows.get(i) else { return Rect::default() };
        let h = px(ROW_HEIGHT);
        let top = self.list.min_y() + px(6.0) - self.scroll + i as f32 * h;
        let x = self.list.min_x() + px(PADDING) + f32::from(row.depth) * px(INDENT);
        let size = px(16.0);
        Rect::new((x + px(12.0) + px(4.0)).round(), (top + (h - size) / 2.0).round(), size, size)
    }

    /// What row `row`'s icon stands for (a key, a foreign key and where it points, an
    /// index, a table or a view, a source's driver); `None`: nothing more than its name.
    pub(super) fn icon_tip(&self, row: &Row) -> Option<String> {
        let t = i18n::t;
        Some(match (&row.kind, &row.key) {
            (RowKind::Source, NodeKey::Source(id)) => {
                let source = self.tree.source(*id)?;
                i18n::tr!("datasources.tip_source", driver = source.driver_name.as_str())
            }
            (RowKind::Schema, _) => t("datasources.tip_schema"),
            (RowKind::Table(database::TableKind::Table), _) => t("datasources.tip_table"),
            (RowKind::Table(database::TableKind::View), _) => t("datasources.tip_view"),
            (RowKind::Column { key: true, .. }, _) => t("datasources.tip_primary"),
            (RowKind::Column { foreign: true, .. }, NodeKey::Column(id, table, column)) => {
                let target = self.tree.structure(*id, table).and_then(|s| {
                    let key = s.foreign_keys.iter().find(|k| k.columns.contains(column))?;
                    let at = key.columns.iter().position(|c| c == column)?;
                    let to = key.references.get(at).map_or("", String::as_str);
                    Some(format!("{}.{to}", key.table))
                });
                match target {
                    Some(target) => {
                        i18n::tr!("datasources.tip_foreign_to", target = target.as_str())
                    }
                    None => t("datasources.tip_foreign"),
                }
            }
            (RowKind::Column { .. }, _) => t("datasources.tip_column"),
            (RowKind::Index { unique: true }, _) => t("datasources.tip_unique_index"),
            (RowKind::Index { unique: false }, _) => t("datasources.tip_index"),
            (RowKind::Key, _) => t("datasources.tip_foreign"),
            _ => return None,
        })
    }

    pub(super) fn paint_window(&mut self, ctx: ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        self.bounds = b;
        self.scale = ctx.scale;
        let base = ctx.scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.sidebar_background));
        let inset = px(PADDING);
        self.field = Rect::new(
            b.min_x() + inset,
            b.min_y() + px(FIELD_TOP),
            (b.size.width - 2.0 * inset).max(0.0),
            px(FIELD_HEIGHT),
        );
        self.paint_field(base, ctx.text, ctx.icons, &ds, ctx.focused);
        let top = b.min_y() + px(LIST_TOP);
        self.list = Rect::new(b.min_x(), top, b.size.width, (b.max_y() - top).max(0.0));
        if self.rows.is_empty() {
            self.paint_empty(base, ctx.text, &ds);
            return;
        }
        let h = px(ROW_HEIGHT);
        let content = self.rows.len() as f32 * h + px(12.0);
        self.scroll = self.scroll.clamp(0.0, (content - self.list.size.height).max(0.0));
        self.paint_rows(ctx.scene, ctx.text, ctx.icons, &ds);
        self.bar.paint(ctx.scene, p, ctx.scale, self.list, content, self.scroll);
    }

    /// The filter: a pill with the magnifier, its text or placeholder, × with text.
    fn paint_field(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &Icons,
        ds: &Ctx<'_>,
        focused: bool,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let f = self.field;
        if f.size.width <= 0.0 {
            return;
        }
        let typing = self.typing && focused;
        let border = if typing { p.search_field_border_focused } else { p.search_field_border };
        layer.quad(
            Quad::filled(f, p.search_field_background)
                .rounded(f.size.height / 2.0)
                .bordered(ds.line(), border),
        );
        let icon = px(15.0);
        let icon_x = f.min_x() + px(13.0);
        if let Some(id) = icons.id("search") {
            let r =
                Rect::new(icon_x, (f.min_y() + (f.size.height - icon) / 2.0).round(), icon, icon);
            draw_icon(layer, icons, id, r, p.text_muted);
        }
        let mut right = f.max_x() - px(14.0);
        self.clear = None;
        if !self.filter.text().is_empty() {
            let s = px(18.0);
            let r = Rect::new(
                f.max_x() - px(8.0) - s,
                (f.min_y() + (f.size.height - s) / 2.0).round(),
                s,
                s,
            );
            let hovered = self.hover == Some(Hover::Clear);
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
            self.clear = Some(r);
            right = r.min_x() - px(6.0);
        }
        let text_x = icon_x + icon + px(9.0);
        let style = TextInputStyle {
            font: Font::new(p.ui_font.clone(), px(p.ui_font_size)),
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
        let field = Rect::new(text_x, f.min_y(), (right - text_x).max(0.0), f.size.height);
        let placeholder = i18n::t("datasources.filter");
        self.filter.paint(layer, text, field, typing, &placeholder, &style);
    }

    /// No rows: the lists being read, or no source yet (how to add one).
    fn paint_empty(&self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>) {
        let loading = self
            .sessions
            .as_ref()
            .is_some_and(|s| s.catalog().list(database::Scope::Project) == Listed::Loading);
        let area = Rect::new(
            self.list.min_x(),
            self.list.min_y(),
            self.list.size.width,
            (self.list.size.height * 0.6).max(0.0),
        );
        if !self.filter.text().trim().is_empty() {
            ds::empty_state(layer, text, ds, area, &i18n::t("datasources.no_match"), None);
        } else if loading {
            ds::empty_state(layer, text, ds, area, &i18n::t("datasources.loading"), None);
        } else {
            ds::empty_state(
                layer,
                text,
                ds,
                area,
                &i18n::t("datasources.empty"),
                Some(&i18n::t("datasources.empty_hint")),
            );
        }
    }

    /// The rows in view: highlights across the width, then each row's guides, chevron,
    /// icon, name, detail and count.
    fn paint_rows(
        &self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let b = self.list;
        let h = px(ROW_HEIGHT);
        let line = ds.line();
        let inset = px(8.0);
        let origin = b.min_y() + px(6.0) - self.scroll;
        let range = TreeList::new(h, self.rows.len()).visible(self.scroll, b.size.height);
        let pills = scene.push_layer(Some(b));
        for i in range.clone() {
            let top = origin + i as f32 * h;
            let pill = Rect::new(
                b.min_x() + inset,
                top + line,
                b.size.width - 2.0 * inset,
                h - 2.0 * line,
            );
            if self.selected.as_ref() == Some(&self.rows[i].key) {
                tree_row::highlight(pills, pill, p.tree_selection, px(6.0));
            } else if self.hover == Some(Hover::Row(i)) {
                tree_row::highlight(pills, pill, p.tree_hover, px(6.0));
            }
        }
        let clip =
            Rect::new(b.min_x(), b.min_y(), (b.size.width - px(10.0)).max(0.0), b.size.height);
        let layer = scene.push_layer(Some(clip));
        let font = Font::new(p.ui_font.clone(), px(p.ui_font_size));
        let small = ds.ui_font(-1.5);
        let metrics = text.metrics(&font);
        let indent = px(INDENT);
        let padding = px(PADDING);
        // The counts' column: as wide as the widest in view, each right-aligned in it, so the
        // badges before it line up from row to row (a `43` no longer pushes its row's left).
        let counts = range
            .clone()
            .filter_map(|i| self.rows[i].count)
            .map(|c| text.shape_line(&c.to_string(), &small).width)
            .fold(0.0_f32, f32::max);
        // The details (a column's type, a server's version) in a column of their own among
        // the rows next to each other at one level with one kind (a table's columns): after
        // the widest of their names, so they line up as in a table.
        let label_font_of = |row: &Row| {
            if matches!(row.kind, RowKind::Source | RowKind::Folder) {
                font.clone().with_weight(Font::SEMIBOLD)
            } else {
                font.clone()
            }
        };
        let mut detail_at = vec![0.0_f32; self.rows.len()];
        let mut start = range.start;
        while start < range.end {
            let depth = self.rows[start].depth;
            let kind = std::mem::discriminant(&self.rows[start].kind);
            let mut stop = start;
            let mut widest = 0.0_f32;
            while stop < range.end
                && self.rows[stop].depth == depth
                && std::mem::discriminant(&self.rows[stop].kind) == kind
            {
                if !self.rows[stop].detail.is_empty() {
                    let f = label_font_of(&self.rows[stop]);
                    widest = widest.max(text.shape_line(&self.rows[stop].label, &f).width);
                }
                stop += 1;
            }
            detail_at[start..stop].fill(widest);
            start = stop;
        }
        for i in range {
            let row = &self.rows[i];
            let top = origin + i as f32 * h;
            let guide_x = |level: usize| b.min_x() + padding + level as f32 * indent + indent * 0.3;
            tree_row::guides(
                layer,
                1..usize::from(row.depth),
                guide_x,
                (top, h),
                line,
                p.indent_guide,
            );
            let x = b.min_x() + padding + f32::from(row.depth) * indent;
            let center = |size: f32| (top + (h - size) / 2.0).round();
            let chevron = px(12.0);
            if row.expandable {
                let at = Rect::new(x.round(), center(chevron), chevron, chevron);
                tree_row::chevron(layer, icons, at, row.expanded, p.text_muted);
            }
            let source_icon =
                row.key.source().and_then(|id| self.tree.source(id)).map_or("database", |s| s.icon);
            let (icon, mut color) = icon_of(row, source_icon, p);
            // A folder's or a source's color: its icon in it, and a mark at the row's start.
            let mark = row.color.and_then(|c| ui_components::tag_colors::named(&c.name(), p));
            if let Some(mark) = mark {
                color = mark;
                let bar =
                    Rect::new(b.min_x() + inset + px(2.0), top + px(7.0), px(3.0), h - px(14.0));
                layer.quad(Quad::filled(bar, mark).rounded(px(1.5)));
            }
            let icon_rect = self.icon_rect(i);
            if let Some(id) = icons.id(icon).or_else(|| icons.id("file")) {
                draw_icon(layer, icons, id, icon_rect, color);
            }
            let baseline = centered_baseline(top, h, &metrics);
            let label_x = (icon_rect.max_x() + px(7.0)).round();
            let right = clip.max_x() - px(10.0);
            // The count at the end (in the counts' column), then the name, then as much of the
            // detail as fits.
            let mut end = if counts > 0.0 { (right - counts).round() - px(10.0) } else { right };
            if let Some(count) = row.count {
                let shaped = text.shape_line(&count.to_string(), &small);
                let x = (right - shaped.width).round();
                draw_text(layer, &shaped, x, baseline, p.text_muted);
            }
            let label_color = match row.kind {
                RowKind::Error => p.danger,
                RowKind::Loading | RowKind::Hint => p.text_muted,
                RowKind::Folder | RowKind::Source => p.text,
                _ => p.text.with_alpha(0.86),
            };
            let label_font = if matches!(row.kind, RowKind::Source | RowKind::Folder) {
                font.clone().with_weight(Font::SEMIBOLD)
            } else {
                font.clone()
            };
            // The badges (a console's transaction, production) at the end, before the count.
            let mid = top + h / 2.0;
            let mut badged = false;
            for (on, key, tone) in [
                (row.marks.transaction, "datasources.badge_tx", ds::Tone::Warning),
                (row.marks.production, "datasources.badge_production", ds::Tone::Danger),
            ] {
                if !on || end - label_x < px(120.0) {
                    continue;
                }
                let label = i18n::t(key);
                let spec = ds::BadgeSpec { label: &label, x: end, mid, align_right: true, tone };
                let r = ds::badge(layer, text, ds, spec);
                end = r.min_x() - px(6.0);
                badged = true;
            }
            // Nothing at its end (no count, no badge): its text may reach the end.
            if row.count.is_none() && !badged {
                end = right;
            }
            let room = (end - label_x).max(0.0);
            let fitted = ds::ellipsize(text, &row.label, &label_font, room);
            let shaped = text.shape_line(&fitted, &label_font);
            draw_text(layer, &shaped, label_x, baseline, label_color);
            if !row.detail.is_empty() && fitted == row.label {
                // Its siblings' column, unless that leaves no room (then right after it).
                let column = label_x + detail_at[i] + px(8.0);
                let own = label_x + shaped.width + px(8.0);
                let x = if end - column > px(48.0) { column } else { own };
                let room = end - x;
                if room > px(24.0) {
                    let detail = ds::ellipsize(text, &row.detail, &small, room);
                    let shaped = text.shape_line(&detail, &small);
                    draw_text(layer, &shaped, x.round(), baseline, p.text_muted);
                }
            }
        }
    }
}
