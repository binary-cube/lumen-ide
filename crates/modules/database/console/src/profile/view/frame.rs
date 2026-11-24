//! The view's frame: everything it shows with its parts placed, a press on the findings or
//! a step's details, its parts handed as controls.

use super::parts::{FilterPart, Panel};
use super::*;

/// The splitter's and the filter's places among the view's parts.
pub(super) const SPLITTER: usize = 0;
pub(super) const FILTER: usize = 3;

impl ProfileView {
    /// Everything it shows, its parts placed.
    pub(super) fn paint_view(&mut self, mut ctx: ViewerPaint<'_>) {
        self.refresh();
        let b = ctx.bounds;
        self.bounds = b;
        self.scale = ctx.scale;
        self.hits.clear();
        self.splitter.set_style(SplitterStyle::from_palette(ctx.palette));
        let compact_rows = if self.report().is_some() { self.listed().len() } else { 0 };
        let spec = LayoutSpec {
            bounds: b,
            scale: ctx.scale,
            panel_w: self.panel_w,
            drawer_open: self.drawer_open,
            compact_rows,
        };
        let key = (
            b.min_x().to_bits(),
            b.min_y().to_bits(),
            b.size.width.to_bits(),
            b.size.height.to_bits(),
            ctx.scale.to_bits(),
            self.panel_w.to_bits(),
            self.drawer_open,
            compact_rows,
        );
        self.laid = *self.layout.get_or_update(key, || layout::layout(&spec));
        let laid = self.laid;
        self.paint_bar(&mut ctx, laid.bar);
        let Some(report) = self.report().cloned() else {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let body = Rect::new(
                b.min_x(),
                laid.bar.max_y(),
                b.size.width,
                (b.max_y() - laid.bar.max_y()).max(0.0),
            );
            let layer = ctx.scene.push_layer(Some(body));
            layer.quad(Quad::filled(body, ctx.palette.editor_background));
            ds::empty_state(layer, ctx.text, &ds, body, &i18n::t("console.result_gone"), None);
            return;
        };
        self.paint_summary(&mut ctx, laid.summary, &report);
        if laid.compact.size.height > 0.0 {
            self.paint_compact(&mut ctx, laid.compact);
        }
        // The columns that fit the tree's room, its words first.
        let tree_logical = laid.tree.size.width / ctx.scale.max(0.01);
        let words = self.table.tree_width(ctx.text, ctx.palette, ctx.scale);
        let gutter = if self.table.rows().iter().any(|r| r.sign.is_some()) {
            ui_components::tree_table::GUTTER
        } else {
            0.0
        };
        let fit = tree::fitting(&self.columns_all, tree_logical - gutter - 12.0, words);
        self.rebuild(&report, Some(fit));
        let selected = self.selected_node();
        // The comparison and the tables take the panel's room too when it's beside (their
        // tables are wide); in a drawer the panel still opens over them.
        let whole = if laid.mode_wide {
            Rect::new(
                laid.tree.min_x(),
                laid.tree.min_y(),
                laid.panel.max_x() - laid.tree.min_x(),
                laid.tree.size.height,
            )
        } else {
            laid.tree
        };
        self.body = laid.tree;
        let own_room = self.compare.is_some() || self.mode == ProfileMode::Tables;
        if own_room {
            self.body = whole;
            if let Some(compare) = self.compare.as_mut() {
                compare.paint(whole, ctx.scale, ctx.text, ctx.scene, ctx.palette, ctx.icons);
            } else {
                self.paint_tables(&mut ctx, whole, &report);
            }
            if laid.drawer && laid.panel.size.width > 0.0 && self.compare.is_none() {
                self.paint_panel(&mut ctx, laid.panel, &report);
            } else {
                self.findings_area = Rect::default();
                self.details_area = Rect::default();
            }
            // No splitter: nothing beside.
            self.splitter.set_bounds(Rect::default(), ctx.scale);
            return;
        }
        match self.mode {
            ProfileMode::Tables => {}
            ProfileMode::Graph => {
                let findings = shown_of(&report, &self.shown);
                self.graph.paint(crate::profile::graph::GraphPaint {
                    area: laid.tree,
                    scale: ctx.scale,
                    text: ctx.text,
                    scene: ctx.scene,
                    palette: ctx.palette,
                    icons: ctx.icons,
                    report: &report,
                    findings: &findings,
                    table: &mut self.table,
                    row_of: &self.built.row_of,
                    selected,
                });
            }
            ProfileMode::Flame => {
                let findings = shown_of(&report, &self.shown);
                self.flame.sync(&report, &findings, self.shown_generation);
                let painter = ui_components::Painter {
                    text: ctx.text,
                    scene: ctx.scene,
                    palette: ctx.palette,
                    icons: ctx.icons,
                };
                self.flame.paint(laid.tree, ctx.scale, painter, selected);
            }
            ProfileMode::Tree => self.table.paint(TablePaint {
                bounds: laid.tree,
                scale: ctx.scale,
                text: ctx.text,
                scene: ctx.scene,
                palette: ctx.palette,
                icons: ctx.icons,
            }),
        }
        if laid.panel.size.width > 0.0 {
            self.paint_panel(&mut ctx, laid.panel, &report);
        } else {
            self.findings_area = Rect::default();
            self.details_area = Rect::default();
        }
        // The splitter after both panes.
        self.splitter.set_bounds(laid.split, ctx.scale);
        self.splitter.paint(ctx.scene);
    }

    /// What a press on the findings' list hit.
    pub(super) fn press_finding(&mut self, hit: FindingHit) {
        let listed = self.listed();
        match hit {
            FindingHit::Card(i) | FindingHit::Step(i) => {
                if let Some(&f) = listed.get(i) {
                    self.choose_finding(f);
                }
            }
            FindingHit::Copy(i) => {
                let sql = listed.get(i).and_then(|&f| {
                    self.shown_findings().get(f)?.suggestion.as_ref()?.sql().map(str::to_owned)
                });
                if let Some(sql) = sql {
                    self.copy(sql, "profile.view.copied_sql");
                }
            }
            FindingHit::Insert(i) => {
                if let Some(&f) = listed.get(i) {
                    self.insert(f);
                }
            }
            FindingHit::Ignore(i) => {
                let rule =
                    listed.get(i).and_then(|&f| self.shown_findings().get(f).map(|f| f.rule));
                if let (Some(rule), Some(runner)) = (rule, &self.runner) {
                    runner.ignore_rule(rule, true);
                    self.selected_finding = None;
                    self.notifications.push(Notification::info(i18n::t("profile.view.ignored")));
                }
            }
            FindingHit::ShowIgnored => {
                if let Some(runner) = &self.runner {
                    runner.unignore_all();
                }
            }
        }
    }

    /// What a press on a step's details hit.
    pub(super) fn press_detail(&mut self, hit: DetailHit) {
        match hit {
            DetailHit::Value(v) if !v.is_empty() => self.copy(v, "profile.view.copied_value"),
            DetailHit::Problem(i) => self.choose_finding(i),
            _ => {}
        }
    }

    /// Its parts (the splitter, the findings' and the details' panels while they're drawn),
    /// the topmost first, with their state of use. The body's views (the tree, the graph,
    /// the flame graph, the tables, the comparison: one at a time, by its mode) and the
    /// filter stay its own.
    pub(super) fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let (mut findings_scrolled, mut details_scrolled, mut edited) = (false, false, false);
        let mut splitter = Divider { splitter: &mut self.splitter, at: &mut self.split_at };
        let mut findings = Panel {
            rows: &mut self.findings,
            area: self.findings_area,
            pressed: &mut self.finding_pressed,
            scrolled: &mut findings_scrolled,
        };
        let mut details = Panel {
            rows: &mut self.details,
            area: self.details_area,
            pressed: &mut self.detail_pressed,
            scrolled: &mut details_scrolled,
        };
        let mut filter =
            FilterPart { field: &mut self.filter, typing: &mut self.typing, edited: &mut edited };
        let mut parts: [&mut dyn Control; 4] =
            [&mut splitter, &mut findings, &mut details, &mut filter];
        // The filter has the keys without a press (nothing else takes them).
        self.controls.focus(&mut parts, Some(FILTER));
        let answer = f(&mut self.controls, &mut parts);
        self.panel_scrolled = findings_scrolled || details_scrolled;
        if edited {
            self.filtered();
        }
        answer
    }
}
