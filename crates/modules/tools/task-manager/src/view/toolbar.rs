//! The toolbar: the scope (this project / every project), the grouping, the filter (a
//! field — with how many it finds and its × —, or a magnifier opening it over the bar on a
//! narrow page), then on the right "Sort by…" (the lists), pause / resume and the interval
//! ("1 s · 1 min ▾"); what doesn't fit is in its "…" (the layout's `overflow`). Under it, the
//! status line: how many threads, how often, how far back.

use kernel::Rect;
use render_api::{Quad, Scene};
use text_layout::TextSystem;
use ui_components::LayerPainter;
use ui_components::ds::{self, Ctx, SegmentedSpec, radius};
use ui_kit::{TextInputStyle, draw_text};

use super::table::icon_button;
use super::{Focus, Hit, Popover, TaskManager};
use crate::layout::{PageLayout, Tool};
use crate::model::Grouping;

/// The groupings offered, in their order (by project only in every project's view).
pub(super) fn groupings(all: bool) -> &'static [Grouping] {
    if all {
        &[Grouping::Kind, Grouping::Project, Grouping::List]
    } else {
        &[Grouping::Kind, Grouping::List]
    }
}

impl TaskManager {
    pub(super) fn paint_toolbar(
        &mut self,
        scene: &mut Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        page: &PageLayout,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let bar = page.toolbar;
        let mid = (bar.min_y() + bar.size.height / 2.0).round();
        let layer = scene.push_layer(Some(page.bounds));
        for &(tool, x, w) in &page.tools.placed {
            let rect = Rect::new(x, bar.min_y(), w, bar.size.height);
            match tool {
                Tool::Scope => {
                    let labels: [&str; 2] =
                        [&self.texts.labels.scope[0], &self.texts.labels.scope[1]];
                    let hovered = match self.hover {
                        Some(Hit::Scope(all)) => Some(usize::from(all)),
                        _ => None,
                    };
                    let spec = SegmentedSpec {
                        right: x + w,
                        mid,
                        labels: &labels,
                        selected: usize::from(self.all),
                        hovered,
                    };
                    let hits = &mut self.hits;
                    let mut area: Option<Rect> = None;
                    ds::segmented_with(layer, text, ds, spec, |i, r| {
                        area = Some(area.map_or(r, |a| union(a, r)));
                        hits.push((r, Hit::Scope(i == 1)))
                    });
                    self.stops.push((area.unwrap_or(rect), Focus::Tool(tool)));
                }
                Tool::Grouping => {
                    let offered = groupings(self.all);
                    let g = &self.texts.labels.grouping;
                    let all: [&str; 3] = [&g[0], &g[1], &g[2]];
                    let two: [&str; 2] = [&g[0], &g[2]];
                    let labels: &[&str] = if self.all { &all } else { &two };
                    let current = self.state.grouping.on(&self.frame);
                    let selected = offered.iter().position(|g| *g == current).unwrap_or(0);
                    let hovered = match self.hover {
                        Some(Hit::Grouping(g)) => offered.iter().position(|o| *o == g),
                        _ => None,
                    };
                    let spec = SegmentedSpec { right: x + w, mid, labels, selected, hovered };
                    let hits = &mut self.hits;
                    let mut area: Option<Rect> = None;
                    ds::segmented_with(layer, text, ds, spec, |i, r| {
                        area = Some(area.map_or(r, |a| union(a, r)));
                        hits.push((r, Hit::Grouping(offered[i])))
                    });
                    self.stops.push((area.unwrap_or(rect), Focus::Tool(tool)));
                }
                Tool::Filter if page.filter_icon => {
                    let hit = Hit::FilterIcon;
                    let on = !self.filter.text().is_empty();
                    let drawn =
                        icon_button(layer, icons, ds, rect, "search", self.hover == Some(hit), on);
                    self.hits.push((rect, hit));
                    self.stops.push((drawn, Focus::Tool(tool)));
                }
                Tool::Filter => {
                    let field = Rect::new(x, mid - px(16.0), w, px(32.0));
                    self.paint_filter(&mut LayerPainter::new(layer, text, icons, *ds), field);
                    self.stops.push((field, Focus::Tool(tool)));
                }
                Tool::Pause => {
                    let icon = if self.paused { "play" } else { "pause" };
                    let drawn = icon_button(
                        layer,
                        icons,
                        ds,
                        rect,
                        icon,
                        self.hover == Some(Hit::Pause),
                        self.paused,
                    );
                    self.hits.push((rect, Hit::Pause));
                    self.stops.push((drawn, Focus::Tool(tool)));
                }
                Tool::Interval | Tool::Sort => {
                    let popover =
                        if tool == Tool::Sort { Popover::Sort } else { Popover::Interval };
                    let pill = self.paint_pill(
                        &mut LayerPainter::new(layer, text, icons, *ds),
                        rect,
                        mid,
                        popover,
                    );
                    self.stops.push((pill, Focus::Tool(tool)));
                }
            }
        }
        if let Some((x, w)) = page.tools.more {
            let rect = Rect::new(x, bar.min_y(), w, bar.size.height);
            let hit = Hit::Open(Popover::More);
            let open = self.popover.is_some_and(|(p, _)| p == Popover::More);
            let drawn = icon_button(
                layer,
                icons,
                ds,
                rect,
                "more-horizontal",
                self.hover == Some(hit),
                open,
            );
            self.hits.push((rect, hit));
            self.stops.push((drawn, Focus::More));
        }
        // A narrow page's filter, opened over the bar.
        if page.filter_icon && self.filter_open {
            let cover = Rect::new(bar.min_x(), bar.min_y(), bar.size.width, bar.size.height);
            let layer = scene.push_layer(Some(page.bounds));
            layer.quad(Quad::filled(cover, p.editor_background));
            let field = Rect::new(cover.min_x(), mid - px(16.0), cover.size.width, px(32.0));
            self.hits.push((cover, Hit::PopoverArea));
            self.paint_filter(&mut LayerPainter::new(layer, text, icons, *ds), field);
        }
        if let Some(status) = page.status {
            let layer = scene.push_layer(Some(status));
            let font = ds.ui_font(-1.5);
            let base = ds::optical_baseline(status.min_y(), status.size.height, &font);
            let x = status.min_x() + px(2.0);
            ds::draw_ellipsized(
                layer,
                text,
                &self.texts.status,
                &font,
                (x, base),
                status.size.width,
                p.text_muted,
            );
        }
    }

    /// The filter field: the magnifier, what's typed (or its hint), how many it finds and
    /// its ×.
    fn paint_filter(&mut self, pen: &mut LayerPainter<'_>, field: Rect) {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let typed = !self.filter.text().is_empty();
        let clear_w = if typed || self.filter_open { px(26.0) } else { 0.0 };
        let small = ds.ui_font(-1.5);
        let count_w = if typed {
            pen.text.shape_line(&self.texts.matches, &small).width + px(8.0)
        } else {
            0.0
        };
        let style = TextInputStyle {
            font: ds.ui_font(-0.5),
            background: p.search_field_background,
            border: p.search_field_border,
            border_focused: p.search_field_border_focused,
            text: p.text,
            placeholder: p.text_muted,
            selection: p.selection,
            caret: p.caret,
            radius: field.size.height / 2.0,
            border_width: ds.line(),
            padding: px(12.0),
            caret_width: px(1.5).max(1.0),
            lead: px(20.0),
        };
        let focused = self.focused
            && (typed
                || self.filter_open
                || self.hover == Some(Hit::Filter)
                || self.focus == Focus::Tool(Tool::Filter));
        let input = Rect::new(field.min_x(), field.min_y(), field.size.width, field.size.height);
        self.filter.paint(
            pen.layer,
            pen.text,
            input,
            focused || typed,
            &self.texts.labels.filter,
            &style,
        );
        if let Some(id) = pen.icons.id("search") {
            let s = px(14.0);
            let at = Rect::new(
                field.min_x() + px(11.0),
                field.min_y() + (field.size.height - s) / 2.0,
                s,
                s,
            );
            ui_kit::draw_icon(pen.layer, pen.icons, id, at, p.text_muted);
        }
        self.hits.push((field, Hit::Filter));
        let mid = field.min_y() + field.size.height / 2.0;
        if typed {
            let base = ds::optical_baseline(field.min_y(), field.size.height, &small);
            let cx = field.max_x() - clear_w - count_w;
            // Its background under the count (the typed pen.text stops before it).
            pen.layer.quad(Quad::filled(
                Rect::new(
                    cx - px(2.0),
                    field.min_y() + px(4.0),
                    count_w,
                    field.size.height - px(8.0),
                ),
                p.search_field_background,
            ));
            draw_text(
                pen.layer,
                &pen.text.shape_line(&self.texts.matches, &small),
                cx + px(2.0),
                base,
                p.text_muted,
            );
        }
        if clear_w > 0.0 {
            let hovered = self.hover == Some(Hit::FilterClear);
            let r =
                ds::clear_button(pen.layer, pen.icons, ds, field.max_x() - px(15.0), mid, hovered);
            self.hits.push((r, Hit::FilterClear));
        }
    }

    /// A pill opening a popover ("1 s · 1 min ▾", "CPU ▾").
    fn paint_pill(
        &mut self,
        pen: &mut LayerPainter<'_>,
        rect: Rect,
        mid: f32,
        popover: Popover,
    ) -> Rect {
        let ds = pen.ds;
        let ds = &ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let hit = Hit::Open(popover);
        let open = self.popover.is_some_and(|(o, _)| o == popover);
        let pill = Rect::new(rect.min_x(), mid - px(14.0), rect.size.width, px(28.0));
        let fill = if open {
            p.accent.with_alpha(0.12)
        } else if self.hover == Some(hit) {
            p.text.with_alpha(0.07)
        } else {
            p.text.with_alpha(0.03)
        };
        pen.layer
            .quad(Quad::filled(pill, fill).rounded(px(radius::SM)).bordered(ds.line(), p.border));
        let label = match popover {
            Popover::Sort => &self.texts.sort_pill,
            _ => &self.texts.interval_pill,
        };
        let font = ds.ui_font(-1.0);
        let base = ds::optical_baseline(pill.min_y(), pill.size.height, &font);
        ds::draw_ellipsized(
            pen.layer,
            pen.text,
            label,
            &font,
            (pill.min_x() + px(12.0), base),
            pill.size.width - px(20.0),
            if open { p.accent } else { p.text },
        );
        self.hits.push((pill, hit));
        pill
    }
}

/// The rectangle holding both.
fn union(a: Rect, b: Rect) -> Rect {
    let (x0, y0) = (a.min_x().min(b.min_x()), a.min_y().min(b.min_y()));
    let (x1, y1) = (a.max_x().max(b.max_x()), a.max_y().max(b.max_y()));
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
