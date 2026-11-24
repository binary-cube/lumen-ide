//! The panel painted as a whole: the search, the agents' filter, the list and its
//! scrollbar (each row drawn by `paint`).

use i18n::t;
use kernel::Rect;
use ui_components::ds::Ctx;
use ui_components::{ViewerPaint, ds};

use super::{FIELD_H, FIELD_MARGIN, FILTER_W, STALE, SessionsPanel};

impl SessionsPanel {
    /// The search field: the app's (its pill, what's typed, its ×, its caret while it has
    /// the keys).
    fn paint_search(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        let typing = self.typing();
        let placeholder = t("agents.search");
        self.query.paint(layer, ctx.text, ctx.icons, ds, (self.field, typing), &placeholder);
    }

    /// The panel: its search, its agents' field, its rows (the agents' list over them is
    /// painted after).
    pub(super) fn paint_panel(&mut self, ctx: &mut ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.take_read();
        if self.read_at.is_none_or(|at| at.elapsed() >= STALE) {
            self.read();
        }
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let b = ctx.bounds;
        let margin = ds.px(FIELD_MARGIN);
        self.field = Rect::new(
            b.min_x() + margin,
            b.min_y() + ds.px(4.0),
            (b.size.width - margin * 2.0).max(0.0),
            ds.px(FIELD_H),
        );
        // The agents' filter on the search's right, while there's a choice to make.
        self.filter_field = None;
        if self.filters_agents() {
            let width = ds.px(FILTER_W).min(self.field.size.width * 0.45).round();
            let gap = ds.px(6.0);
            self.field.size.width = (self.field.size.width - width - gap).max(0.0);
            self.filter_field = Some(Rect::new(
                self.field.max_x() + gap,
                self.field.min_y(),
                width,
                self.field.size.height,
            ));
        }
        let top = self.field.max_y() + ds.px(8.0);
        self.list = Rect::new(b.min_x(), top, b.size.width, (b.max_y() - top).max(0.0));
        self.filter();
        if self.root.is_some() {
            self.paint_search(ctx, &ds);
            self.paint_agent_filter(ctx, &ds);
        }
        let b = self.list;
        let layer = ctx.scene.push_layer(Some(b));
        if self.root.is_none() {
            ds::empty_state(layer, ctx.text, &ds, b, &t("agents.no_project"), None);
            return;
        }
        if self.read_at.is_none() {
            let label = t("agents.reading");
            let mid = b.min_y() + ds.px(24.0);
            ds::loader(
                layer,
                ctx.text,
                &ds,
                b.min_x() + ds.px(16.0),
                mid,
                &ds::Loader::with_text(&label),
            );
            return;
        }
        if self.shown.is_empty() && self.missing.is_empty() {
            if self.only_favorites && self.query.text().trim().is_empty() {
                let detail = t("agents.no_favorites_detail");
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.no_favorites"), Some(&detail));
            } else if self.sessions.is_empty() {
                let detail = t("agents.none_detail");
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.none"), Some(&detail));
            } else {
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.not_found"), None);
            }
            return;
        }
        self.paint_rows(ctx, &ds);
    }

    /// The agents' filter: our dropdown, the agent chosen (or every one) in it.
    fn paint_agent_filter(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let Some(r) = self.filter_field else { return };
        let value = match self.agent_filter {
            Some(id) => self.agent_name(id),
            None => t("agents.all_agents"),
        };
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        let spec = ds::DropdownSpec {
            value: &value,
            open: false,
            hovered: self.hover_filter,
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, ctx.text, ctx.icons, ds, r, spec);
    }
}
