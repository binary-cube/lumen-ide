//! The branches pane: its head (the filter field), its rows (a local branch with its
//! commits to push and to pull), painted and chosen.

use super::filters::Field;
use super::*;
use ui_components::TreeList;
use ui_components::tree;

/// The room above the pane's first row (logical pixels).
const TOP_PAD: f32 = 6.0;

impl LogView {
    pub(super) fn branch_row_h(&self) -> f32 {
        (30.0 * self.scale.max(0.1)).round()
    }

    /// Where the panel's rows are.
    pub(super) fn branch_list(&self) -> Rect {
        let s = self.scale.max(0.1);
        let top = (TOP_PAD * s).round();
        let a = self.branches;
        Rect::new(a.min_x(), a.min_y() + top, a.size.width, (a.size.height - top).max(0.0))
    }

    /// The pane's head in `head` (over its rows, as the filter bar is over the commits, as
    /// tall): the field filtering the branches on its first line, the line under it.
    pub(super) fn paint_branches_head(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        head: Rect,
    ) {
        let px = |v: f32| ds.px(v);
        let h = px(28.0);
        let field = Rect::new(
            head.min_x() + px(8.0),
            (head.min_y() + (px(super::filter_bar::BAR_H) - h) / 2.0).round(),
            (head.size.width - px(16.0)).max(0.0),
            h,
        );
        let mut pen = ui_components::LayerPainter::new(layer, text, icons, *ds);
        self.paint_search_field(&mut pen, field, Field::Branches);
    }

    /// The branches panel: All, HEAD, the sections with their folders; the
    /// branches the log shows lit, the one clicked marked; a local branch's arrows (`↗1`:
    /// to push, `↙2`: to pull) on its right.
    pub(super) fn paint_branches(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        // Its tint is the view's (under the splitter, up to the seam's middle).
        let area = self.branch_list();
        let layer = scene.push_layer(Some(area));
        let row_h = self.branch_row_h();
        let max = (self.branches_content() - area.size.height).max(0.0);
        self.branches_scroll = self.branches_scroll.clamp(0.0, max);
        let font = ds.ui_font(-0.5);
        let strong = font.clone().with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-1.5);
        let current = self.sync.as_ref().map(|s| s.branch.clone());
        let shown = &self.filter.references;
        let icon = px(14.0);
        let list = TreeList::new(row_h, self.branch_rows.len());
        for i in list.visible(self.branches_scroll, area.size.height) {
            let BranchRow { depth, item } = &self.branch_rows[i];
            let top = (area.min_y() + i as f32 * row_h - self.branches_scroll).round();
            let row = Rect::new(area.min_x() + px(6.0), top, area.size.width - px(12.0), row_h);
            let chosen = match item {
                BranchItem::All => shown.is_empty(),
                BranchItem::Head => shown.iter().any(|r| r == "HEAD"),
                BranchItem::Ref { name, .. } => shown.iter().any(|r| r == name),
                BranchItem::Group { .. } => false,
            };
            // Inset from its neighbours: a chosen row and a hovered one next to it keep a gap.
            let gap = px(2.0).round();
            let lit = Rect::new(row.min_x(), top + gap, row.size.width, row_h - 2.0 * gap);
            if chosen {
                tree::highlight(layer, lit, p.accent.with_alpha(0.16), px(radius::MD));
            } else if self.branch_selected == Some(i) {
                tree::highlight(layer, lit, p.text.with_alpha(0.08), px(radius::MD));
            } else if self.hover == Some(Hit::Branch(i)) {
                tree::highlight(layer, lit, p.text.with_alpha(0.05), px(radius::MD));
            }
            if let Some(hit) = row.intersect(&area) {
                self.hits.push((hit, Hit::Branch(i)));
            }
            let mut x = row.min_x() + px(8.0) + *depth as f32 * px(14.0);
            let (glyph, color, label, bold) = match item {
                BranchItem::All => ("graph", p.text_muted, t("git.branches_all"), false),
                BranchItem::Head => {
                    let name = current.clone().unwrap_or_else(|| "HEAD".into());
                    ("crosshair", p.accent, tr!("git.branches_head", name = name.as_str()), false)
                }
                BranchItem::Group { label, expanded, .. } => {
                    let chevron = tree::chevron_icon(*expanded);
                    (chevron, p.text_muted, label.clone(), *depth == 0)
                }
                BranchItem::Ref { label, kind, name } => {
                    let is_current = *kind == RefKind::Branch && current.as_deref() == Some(name);
                    let glyph = if *kind == RefKind::Tag { "tag" } else { "git-branch" };
                    let color = if is_current { p.accent } else { p.text_muted };
                    (glyph, color, label.clone(), is_current)
                }
            };
            let mid = top + row_h / 2.0;
            if let Some(id) = icons.id(glyph) {
                let r = Rect::new(x, (mid - icon / 2.0).round(), icon, icon);
                ui_kit::draw_icon(layer, icons, id, r, color);
            }
            x += icon + px(8.0);
            // A local branch against its upstream: to push ↗, to pull ↙.
            let mut right = row.max_x() - px(6.0);
            if let BranchItem::Ref { name, kind: RefKind::Branch, .. } = item
                && let Some(state) = self.branch_states.iter().find(|s| s.branch == *name)
                && (state.ahead > 0 || state.behind > 0)
            {
                let mut parts = Vec::new();
                if state.ahead > 0 {
                    parts.push((format!("↗{}", state.ahead), p.accent));
                }
                if state.behind > 0 {
                    parts.push((format!("↙{}", state.behind), p.vcs_modified));
                }
                let start = right;
                for (label, color) in parts.into_iter().rev() {
                    let shaped = text.shape_line(&label, &small);
                    right -= shaped.width;
                    draw_text(
                        layer,
                        &shaped,
                        right,
                        ds::optical_baseline(top, row_h, &small),
                        color,
                    );
                    right -= px(6.0);
                }
                let arrows = Rect::new(right, top, start - right, row_h);
                if let Some(hit) = arrows.intersect(&area) {
                    self.hits.push((hit, Hit::BranchSync(i)));
                }
            }
            let f = if bold { &strong } else { &font };
            let label = ds::ellipsize(text, &label, f, (right - x - px(6.0)).max(0.0));
            let shaped = text.shape_line(&label, f);
            draw_text(layer, &shaped, x, ds::optical_baseline(top, row_h, f), p.text);
        }
    }
}
