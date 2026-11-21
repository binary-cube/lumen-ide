//! The profile's parts painted: its bar, summary, panel, tables and menus.

use super::*;
use ui_components::ds::{ButtonKind, ButtonSpec, button_size};

/// The height of the bar's controls: the switcher, the buttons, the filter (logical pixels).
const BAR_CONTROL_H: f32 = button_size::MEDIUM;

/// How lit a severity's pill is under the mouse (one filtering by it: all of it).
const SEVERITY_HOVER: f32 = 0.5;

/// A panel's count beside its title: its fill's alpha of the text and the room on each side.
const COUNT_FILL: f32 = 0.08;
const COUNT_PAD: f32 = 6.0;

/// A severity's tone: its pill tinted in it.
fn severity_tone(level: Level) -> Tone {
    match level {
        Level::Info => Tone::Accent,
        Level::Warning => Tone::Warning,
        Level::Critical => Tone::Danger,
    }
}

/// A figure's chip (the summary's, the comparison's): its height, the room on each side,
/// its fill's alpha of its tint (logical pixels).
pub(super) const FIGURE_H: f32 = 22.0;
pub(super) const FIGURE_PAD: f32 = 6.0;
pub(super) const FIGURE_FILL: f32 = 0.05;

impl ProfileView {
    pub(super) fn paint_bar(&mut self, ctx: &mut ViewerPaint<'_>, bar: Rect) {
        let p = ctx.palette;
        let ds = Ctx::new(p, ctx.scale);
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(bar));
        layer.quad(Quad::filled(bar, p.editor_background));
        ds::divider(layer, &ds, bar.min_x(), bar.max_x(), bar.max_y() - ds.line());
        // Pressed only where it's drawn this time.
        self.filter.forget();
        let Some(set) = self.set.clone() else { return };
        let report = set.report.clone();
        let mid = (bar.min_y() + bar.size.height / 2.0).round();
        let h = px(BAR_CONTROL_H);
        let mut x = bar.min_x() + px(8.0);
        let mut right = bar.max_x() - px(8.0);
        // The right: the engine, ANALYZE / EXPLAIN, rolled back.
        let mut badges: Vec<(String, Tone, Hit)> = Vec::new();
        if set.rolled_back {
            badges.push((i18n::t("console.plan_rolled_back"), Tone::Warning, Hit::RolledBack));
        }
        let depth =
            i18n::t(if set.analyze { "console.plan_analyzed" } else { "console.plan_estimated" });
        badges.push((depth, if set.analyze { Tone::Accent } else { Tone::Neutral }, Hit::Depth));
        if let Some(r) = &report {
            let e = &r.profile.engine;
            let mut engine = if e.name.is_empty() { e.driver.clone() } else { e.name.clone() };
            if let Some(v) = &e.version {
                engine = format!("{engine} {v}");
            }
            if !engine.trim().is_empty() {
                badges.push((engine, Tone::Neutral, Hit::Engine));
            }
        }
        // What the left needs; what's dropped when the room is short.
        let small = ds.ui_font(-1.5);
        let badge_w = |text: &mut text_layout::TextSystem, l: &str| {
            text.shape_line(l, &small).width + px(14.0) + px(6.0)
        };
        let mode_labels: Vec<String> = ProfileMode::ALL.iter().map(|m| m.label()).collect();
        let mode_refs: Vec<&str> = mode_labels.iter().map(String::as_str).collect();
        let mode_label = self.mode.label();
        let mode_spec = ButtonSpec::new(ButtonKind::Soft)
            .icon(self.mode.icon())
            .label(&mode_label)
            .menu(true)
            .hovered(self.hover == Some(Hit::ModeMenu));
        let switcher_w = if self.laid.switcher_menu {
            ds::button_width(ctx.text, &ds, &mode_spec, h)
        } else {
            ds::segmented_width(ctx.text, &ds, &mode_refs)
        };
        let export_label = i18n::t("profile.view.export");
        let export_spec = ButtonSpec::new(ButtonKind::Soft)
            .icon("download")
            .label(&export_label)
            .menu(true)
            .hovered(self.hover == Some(Hit::Export));
        let export_w = ds::button_width(ctx.text, &ds, &export_spec, h);
        let icon_w = px(button_size::LARGE);
        // Compare ▾ (an icon alone when the switcher is a menu), on while compared.
        let compare_label = i18n::t("profile.compare.button");
        let mut compare_spec = ButtonSpec::new(ButtonKind::Soft)
            .icon("compare")
            .menu(true)
            .on(self.compare.is_some())
            .hovered(self.hover == Some(Hit::Compare));
        if !self.laid.switcher_menu {
            compare_spec = compare_spec.label(&compare_label);
        }
        let compare_w = ds::button_width(ctx.text, &ds, &compare_spec, h);
        let services = matches!(self.source, Source::Services(_));
        let drawer = !self.laid.mode_wide;
        let fixed = switcher_w
            + compare_w
            + px(6.0)
            + export_w
            + px(16.0)
            + if drawer { icon_w + px(24.0) } else { 0.0 };
        let mut tools = (2 + usize::from(services)) as f32 * icon_w;
        let mut filter_w = px(190.0);
        let badges_w: f32 = badges.iter().map(|(l, ..)| badge_w(ctx.text, l)).sum();
        let room = bar.size.width - px(16.0);
        let mut show_badges = badges.len();
        let mut more = false;
        if fixed + tools + filter_w + badges_w > room {
            filter_w = px(130.0);
        }
        while show_badges > 1
            && fixed
                + tools
                + filter_w
                + badges.iter().take(show_badges).map(|(l, ..)| badge_w(ctx.text, l)).sum::<f32>()
                > room
        {
            show_badges -= 1;
        }
        if fixed
            + tools
            + filter_w
            + badges.iter().take(show_badges).map(|(l, ..)| badge_w(ctx.text, l)).sum::<f32>()
            > room
        {
            more = true;
            tools = icon_w;
        }
        if fixed + tools + filter_w > room {
            filter_w = icon_w;
        }
        // The badges, from the right, never over what's on the left.
        let left_end = bar.min_x()
            + px(8.0)
            + switcher_w
            + px(8.0)
            + compare_w
            + px(6.0)
            + filter_w
            + px(6.0)
            + tools
            + px(4.0)
            + export_w
            + px(6.0)
            + if drawer { icon_w + px(28.0) } else { 0.0 };
        for (label, tone, hit) in badges.iter().take(show_badges) {
            if right - badge_w(ctx.text, label) < left_end + px(4.0) {
                break;
            }
            let r = ds::badge(
                layer,
                ctx.text,
                &ds,
                ds::BadgeSpec { label, x: right, mid, align_right: true, tone: *tone },
            );
            let tip = match hit {
                Hit::Depth if set.analyze => i18n::t("console.plan_analyzed_tip"),
                Hit::Depth => i18n::t("console.plan_estimated_tip"),
                Hit::RolledBack => i18n::t("console.plan_rolled_back_tip"),
                _ => i18n::t("profile.view.engine_tip"),
            };
            self.hits.push((r, hit.clone(), Some(tip)));
            right = r.min_x() - px(6.0);
        }
        // The switcher.
        if self.laid.switcher_menu {
            let r = Rect::new(x, mid - h / 2.0, switcher_w, h);
            ds::button(layer, ctx.text, ctx.icons, &ds, r, mode_spec);
            self.hits.push((r, Hit::ModeMenu, Some(i18n::t("profile.view.mode_tip"))));
            x = r.max_x() + px(8.0);
        } else {
            let selected = ProfileMode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
            let hovered = match self.hover {
                Some(Hit::Mode(i)) => Some(i),
                _ => None,
            };
            let disabled: Vec<usize> = ProfileMode::ALL
                .iter()
                .enumerate()
                .filter(|(_, m)| !m.available())
                .map(|(i, _)| i)
                .collect();
            let spec = ds::SegmentedSpec { right: 0.0, mid, labels: &mode_refs, selected, hovered };
            let look = ds::SegmentedLook {
                height: BAR_CONTROL_H,
                place: ds::SegmentPlace::Left(x),
                disabled: &disabled,
                icons: &[],
            };
            let hits = &mut self.hits;
            ds::segmented_styled(layer, ctx.text, ctx.icons, &ds, (spec, look), |i, seg| {
                let tip = ProfileMode::ALL.get(i).map(|m| m.label());
                hits.push((seg, Hit::Mode(i), tip));
            });
            x += switcher_w + px(8.0);
        }
        // Compare ▾.
        {
            let r = Rect::new(x, mid - h / 2.0, compare_w, h);
            ds::button(layer, ctx.text, ctx.icons, &ds, r, compare_spec);
            self.hits.push((r, Hit::Compare, Some(i18n::t("profile.compare.button_tip"))));
            x = r.max_x() + px(6.0);
        }
        // The filter.
        let fr = Rect::new(x, mid - h / 2.0, filter_w, h);
        if filter_w > icon_w {
            let placeholder = i18n::t("profile.view.filter");
            let field = (fr, self.typing);
            self.filter.paint(layer, ctx.text, ctx.icons, &ds, field, &placeholder);
            self.hits.push((fr, Hit::Filter, Some(i18n::t("profile.view.filter_tip"))));
        } else {
            // An icon alone: the field isn't there to be pressed.
            self.filter.forget();
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("search")
                .hovered(self.hover == Some(Hit::Filter));
            ds::button(layer, ctx.text, ctx.icons, &ds, fr, spec);
            self.hits.push((fr, Hit::Filter, Some(i18n::t("profile.view.filter_tip"))));
        }
        x = fr.max_x() + px(6.0);
        // The tools.
        let tool_list: Vec<(Hit, &'static str, String)> = if more {
            vec![(Hit::More, "more-horizontal", i18n::t("console.tip_more"))]
        } else {
            let mut v = vec![
                (Hit::ExpandAll, "chevrons-expand", i18n::t("console.plan_expand_all")),
                (Hit::CollapseAll, "chevrons-collapse", i18n::t("console.plan_collapse_all")),
            ];
            if services {
                v.push((Hit::OpenInTab, "arrow-up-right", i18n::t("profile.view.open_in_tab")));
            }
            v
        };
        for (hit, icon, tip) in tool_list {
            let r = Rect::new(x, mid - h / 2.0, icon_w - px(2.0), h);
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon(icon)
                .hovered(self.hover.as_ref() == Some(&hit));
            ds::button(layer, ctx.text, ctx.icons, &ds, r, spec);
            self.hits.push((r, hit, Some(tip)));
            x = r.max_x() + px(2.0);
        }
        // Export ▾.
        let er = Rect::new(x + px(4.0), mid - h / 2.0, export_w, h);
        ds::button(layer, ctx.text, ctx.icons, &ds, er, export_spec);
        self.hits.push((er, Hit::Export, Some(i18n::t("profile.view.export_tip"))));
        x = er.max_x() + px(6.0);
        // The panel's drawer.
        if drawer {
            let r = Rect::new(x, mid - h / 2.0, icon_w + px(22.0), h);
            let count =
                report.as_ref().map_or(0, |r| r.findings.iter().filter(|f| f.is_problem()).count());
            let count = count.to_string();
            let spec = ButtonSpec::new(ButtonKind::Ghost)
                .icon("lightbulb")
                .label(&count)
                .on(self.drawer_open)
                .hovered(self.hover == Some(Hit::Drawer));
            ds::button(layer, ctx.text, ctx.icons, &ds, r, spec);
            self.hits.push((r, Hit::Drawer, Some(i18n::t("profile.view.drawer_tip"))));
            x = r.max_x() + px(6.0);
        }
        // The statement, between them.
        let room = right - px(8.0) - (x + px(6.0));
        if room > px(120.0) {
            let code = ds.code_font(-1.5);
            let one = kernel::text::one_line(&set.statement).into_owned();
            let shown = ds::ellipsize(ctx.text, &one, &code, room);
            let metrics = ctx.text.metrics(&code);
            let sx = x + px(6.0);
            ui_components::code_tokens::draw_code(
                layer,
                ctx.text,
                ui_components::CodeLine {
                    source: &shown,
                    extension: "sql",
                    font: &code,
                    x: sx,
                    baseline: ui_kit::centered_baseline(bar.min_y(), bar.size.height, &metrics),
                    palette: p,
                    max_x: sx + room,
                },
            );
            let tip = format!("{}\n{}", i18n::t("console.plan_statement_tip"), set.statement);
            self.hits.push((
                Rect::new(sx, bar.min_y(), room, bar.size.height),
                Hit::Statement,
                Some(tip),
            ));
        }
    }

    pub(super) fn paint_summary(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        area: Rect,
        report: &ProfileReport,
    ) {
        let p = ctx.palette;
        let ds = Ctx::new(p, ctx.scale);
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.editor_background));
        layer.quad(Quad::filled(area, p.text.with_alpha(0.018)));
        ds::divider(layer, &ds, area.min_x(), area.max_x(), area.max_y() - ds.line());
        let mid = (area.min_y() + area.size.height / 2.0).round();
        let label_font = ds.ui_font(-1.5);
        let value_font = ds.ui_font(-1.0).with_weight(Font::SEMIBOLD);
        // The severities on the right, first (they always show).
        let findings = shown_of(report, &self.shown);
        let counts = summary::counts(&findings);
        let mut right = area.max_x() - px(10.0);
        for (l, n) in counts.iter().rev() {
            let text = summary::count_text(*l, *n);
            let on = self.severity == Some(*l);
            let hovered = self.hover == Some(Hit::Severity(*l));
            // On: as lit as it gets; under the mouse, part way.
            let lit = if on {
                1.0
            } else if hovered {
                SEVERITY_HOVER
            } else {
                0.0
            };
            let spec = ButtonSpec::new(ButtonKind::Soft)
                .icon(l.icon())
                .label(&text)
                .round()
                .tinted()
                .tone(severity_tone(*l))
                .tone_color(l.color(p))
                .hover_amount(lit);
            let h = px(button_size::SMALL);
            let w = ds::button_width(ctx.text, &ds, &spec, h);
            let r = Rect::new((right - w).round(), (mid - h / 2.0).round(), w, h);
            ds::button(layer, ctx.text, ctx.icons, &ds, r, spec);
            self.hits.push((r, Hit::Severity(*l), Some(i18n::t("profile.view.severity_tip"))));
            right = r.min_x() - px(6.0);
        }
        if counts.is_empty() && report.profile.depth.runs() {
            let text = i18n::t("profile.view.no_problems");
            let shaped = ctx.text.shape_line(&text, &label_font);
            let x = right - shaped.width;
            draw_text(
                layer,
                &shaped,
                x.round(),
                ds::optical_baseline(area.min_y(), area.size.height, &label_font),
                p.vcs_added,
            );
            right = x - px(10.0);
        }
        // The figures, from the left, as many as fit.
        let mut x = area.min_x() + px(10.0);
        for (i, pill) in summary::pills(report).into_iter().enumerate() {
            let tint = if pill.warn { p.warning } else { p.text };
            let badge = ds::Badge::new(&pill.value)
                .font(&value_font)
                .lead(&pill.label)
                .lead_font(&label_font)
                .color(tint)
                .fill(tint.with_alpha(FIGURE_FILL))
                .icon(ctx.icons, pill.icon)
                .icon_color(if pill.warn { p.warning } else { p.text_muted })
                .size(ds::BadgeSize::Custom(FIGURE_H))
                .pad(FIGURE_PAD)
                .radius(radius::SM);
            if x + badge.width(ctx.text, &ds) > right - px(8.0) {
                break;
            }
            let r = badge.paint(layer, ctx.text, &ds, x.round(), mid);
            self.hits.push((r, Hit::Pill(i), Some(format!("{}: {}", pill.label, pill.tip))));
            x = r.max_x() + px(6.0);
        }
    }

    pub(super) fn paint_compact(&mut self, ctx: &mut ViewerPaint<'_>, area: Rect) {
        let p = ctx.palette;
        let ds = Ctx::new(p, ctx.scale);
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, p.editor_background));
        ds::divider(layer, &ds, area.min_x(), area.max_x(), area.max_y() - ds.line());
        let font = ds.ui_font(-1.0);
        let Some(report) = self.report().cloned() else { return };
        let findings = shown_of(&report, &self.shown);
        let listed = self.listed();
        let mut y = area.min_y() + px(4.0);
        for &i in listed.iter().take(4) {
            let Some(f) = findings.get(i) else { continue };
            let row = Rect::new(area.min_x(), y, area.size.width, px(layout::COMPACT_ROW));
            ds::RowHighlight::new(
                self.selected_finding == Some(i),
                self.hover == Some(Hit::Compact(i)),
            )
            .selected_fill(p.tree_selection)
            .hover(ds::HoverFill::Tree)
            .radius(0.0)
            .paint(layer, &ds, row);
            let l = figures::level(f.severity);
            let color = if f.is_problem() { l.color(p) } else { p.heat(f.impact as f32) };
            let icon = if f.is_problem() { l.icon() } else { "flame" };
            if let Some(id) = ctx.icons.id(icon) {
                let s = px(13.0);
                draw_icon(
                    layer,
                    ctx.icons,
                    id,
                    Rect::new(
                        (row.min_x() + px(10.0)).round(),
                        (row.min_y() + (row.size.height - s) / 2.0).round(),
                        s,
                        s,
                    ),
                    color,
                );
            }
            let share = figures::share(f.impact);
            let share_shaped = ctx.text.shape_line(&share, &font);
            let title = analysis::texts(f).title;
            let room = row.size.width - px(40.0) - share_shaped.width - px(16.0);
            let title = ds::ellipsize(ctx.text, &title, &font, room);
            let shaped = ctx.text.shape_line(&title, &font);
            let base = ds::optical_baseline(row.min_y(), row.size.height, &font);
            draw_text(layer, &shaped, (row.min_x() + px(32.0)).round(), base, p.text);
            draw_text(
                layer,
                &share_shaped,
                (row.max_x() - px(10.0) - share_shaped.width).round(),
                base,
                p.text_muted,
            );
            self.hits.push((row, Hit::Compact(i), Some(analysis::texts(f).why)));
            y += px(layout::COMPACT_ROW);
        }
    }

    pub(super) fn paint_panel(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        area: Rect,
        report: &Arc<ProfileReport>,
    ) {
        let p = ctx.palette;
        let ds = Ctx::new(p, ctx.scale);
        let px = |v: f32| ds.px(v);
        {
            let layer = ctx.scene.push_layer(Some(area));
            if self.laid.drawer {
                // Over the tree: a shadow along its left.
                let shadow =
                    Rect::new(area.min_x() - px(10.0), area.min_y(), px(10.0), area.size.height);
                layer.quad(Quad::filled(shadow, p.shadow.with_alpha(0.12)));
            }
            layer.quad(Quad::filled(area, p.editor_background));
            if self.laid.drawer {
                ds::divider_vertical(layer, &ds, area.min_x(), area.min_y(), area.max_y());
            }
        }
        let findings = shown_of(report, &self.shown);
        let listed: Vec<usize> = self.listed();
        let list: Vec<&Finding> = listed.iter().filter_map(|&i| findings.get(i).copied()).collect();
        let selected = self.selected_finding.and_then(|s| listed.iter().position(|&l| l == s));
        // The two parts' heights: the findings as tall as they need, at most 58 %.
        let head = px(PANEL_HEAD);
        let width = (area.size.width - px(24.0)).max(px(80.0));
        let cards_h = self
            .findings
            .cards(
                ctx.text,
                &ds,
                width,
                &list,
                report,
                self.shown_generation + self.severity.map_or(0, |l| l as u64 * 1000),
            )
            .height;
        let wanted = if list.is_empty() { px(110.0) } else { cards_h + px(24.0) };
        let usable = (area.size.height - head * 2.0).max(0.0);
        let findings_h = wanted.min((usable * 0.58).round()).max(px(60.0).min(usable));
        let f_head = Rect::new(area.min_x(), area.min_y(), area.size.width, head);
        let f_area = Rect::new(area.min_x(), f_head.max_y(), area.size.width, findings_h);
        let d_head = Rect::new(area.min_x(), f_area.max_y(), area.size.width, head);
        let d_area = Rect::new(
            area.min_x(),
            d_head.max_y(),
            area.size.width,
            (area.max_y() - d_head.max_y()).max(0.0),
        );
        self.findings_area = f_area;
        self.details_area = d_area;
        self.panel_heads = [f_head, d_head];
        // The heads.
        let node = self.selected_node();
        let head_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let problems = findings.iter().filter(|f| f.is_problem()).count();
        let details_title = match node.and_then(|n| report.profile.get(n)) {
            Some(n) => format!(
                "{} · {}",
                i18n::t("profile.view.details_title"),
                database::profile::export::head(n)
            ),
            None => format!(
                "{} · {}",
                i18n::t("profile.view.details_title"),
                i18n::t("profile.view.details_statement")
            ),
        };
        for (r, title, icon, count, close) in [
            (
                f_head,
                i18n::t("profile.view.findings"),
                "lightbulb",
                Some(problems),
                self.laid.drawer,
            ),
            (d_head, details_title, "info", None, false),
        ] {
            let layer = ctx.scene.push_layer(Some(r));
            layer.quad(Quad::filled(r, p.text.with_alpha(0.025)));
            ds::divider(layer, &ds, r.min_x(), r.max_x(), r.max_y() - ds.line());
            if r.min_y() > area.min_y() {
                ds::divider(layer, &ds, r.min_x(), r.max_x(), r.min_y());
            }
            let mid = r.min_y() + r.size.height / 2.0;
            if let Some(id) = ctx.icons.id(icon) {
                let s = px(13.0);
                draw_icon(
                    layer,
                    ctx.icons,
                    id,
                    Rect::new((r.min_x() + px(12.0)).round(), (mid - s / 2.0).round(), s, s),
                    p.text_muted,
                );
            }
            let mut room = r.size.width - px(44.0);
            if close {
                room -= px(26.0);
            }
            let shown = ds::ellipsize(ctx.text, &title, &head_font, room);
            let shaped = ctx.text.shape_line(&shown, &head_font);
            let base = ds::optical_baseline(r.min_y(), r.size.height, &head_font);
            draw_text(layer, &shaped, (r.min_x() + px(32.0)).round(), base, p.text);
            if let Some(n) = count.filter(|n| *n > 0) {
                let text = n.to_string();
                ds::Badge::new(&text)
                    .fill_alpha(COUNT_FILL)
                    .size(ds::BadgeSize::Small)
                    .pad(COUNT_PAD)
                    .paint(layer, ctx.text, &ds, r.min_x() + px(40.0) + shaped.width, mid);
            }
            if close {
                let cr = ds::square_button(&ds, (r.max_x() - px(30.0), mid), button_size::COMPACT);
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon("x")
                    .hovered(self.hover == Some(Hit::DrawerClose));
                ds::button(layer, ctx.text, ctx.icons, &ds, cr, spec);
                self.hits.push((cr, Hit::DrawerClose, Some(i18n::t("profile.view.drawer_close"))));
            }
        }
        let can_insert = self.runner.as_ref().and_then(Runner::file).is_some();
        let ignored = report.findings.len().saturating_sub(self.shown.len());
        self.findings.paint(ListPaint {
            area: f_area,
            scale: ctx.scale,
            text: ctx.text,
            scene: ctx.scene,
            palette: p,
            icons: ctx.icons,
            report,
            findings: &list,
            generation: self.shown_generation + self.severity.map_or(0, |l| l as u64 * 1000),
            selected,
            ignored,
            can_insert,
        });
        let key = (node, self.shown_generation);
        if self.details_for != Some(key) {
            self.sections = details::sections(report, node, &findings);
            self.details_for = Some(key);
            self.details.scroll = 0.0;
        }
        let sections = std::mem::take(&mut self.sections);
        let painter = ui_components::Painter {
            text: ctx.text,
            scene: ctx.scene,
            palette: p,
            icons: ctx.icons,
        };
        self.details.paint(painter, ctx.scale, d_area, &sections);
        self.sections = sections;
    }

    /// The tables' view in `area`: its table, and under it what's said of the statistics
    /// (being read, or not to be had).
    pub(super) fn paint_tables(
        &mut self,
        ctx: &mut ViewerPaint<'_>,
        area: Rect,
        report: &Arc<ProfileReport>,
    ) {
        self.ask_stats();
        self.rebuild_tables(report, area.size.width / ctx.scale.max(0.01) - 12.0);
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let px = |v: f32| ds.px(v);
        let note = self.tables_note();
        let note_h = if note.is_some() { px(30.0).round() } else { 0.0 };
        let table_area = Rect::new(
            area.min_x(),
            area.min_y(),
            area.size.width,
            (area.size.height - note_h).max(0.0),
        );
        self.tables.paint(TablePaint {
            bounds: table_area,
            scale: ctx.scale,
            text: ctx.text,
            scene: ctx.scene,
            palette: ctx.palette,
            icons: ctx.icons,
        });
        if let Some(note) = note {
            let p = ctx.palette;
            let r = Rect::new(area.min_x(), table_area.max_y(), area.size.width, note_h);
            let layer = ctx.scene.push_layer(Some(r));
            layer.quad(Quad::filled(r, p.editor_background));
            layer.quad(Quad::filled(r, p.text.with_alpha(0.025)));
            ds::divider(layer, &ds, r.min_x(), r.max_x(), r.min_y());
            let font = ds.ui_font(-1.5);
            if let Some(id) = ctx.icons.id("info") {
                let s = px(12.0);
                draw_icon(
                    layer,
                    ctx.icons,
                    id,
                    Rect::new(
                        (r.min_x() + px(12.0)).round(),
                        (r.min_y() + (r.size.height - s) / 2.0).round(),
                        s,
                        s,
                    ),
                    p.text_muted,
                );
            }
            let shown = ds::ellipsize(ctx.text, &note, &font, r.size.width - px(44.0));
            let shaped = ctx.text.shape_line(&shown, &font);
            draw_text(
                layer,
                &shaped,
                (r.min_x() + px(32.0)).round(),
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                p.text_muted,
            );
            self.hits.push((r, Hit::TablesNote, Some(note)));
        }
    }
}

/// Of `report`'s findings, those at `shown`.
pub(super) fn shown_of<'a>(report: &'a ProfileReport, shown: &[usize]) -> Vec<&'a Finding> {
    shown.iter().filter_map(|&i| report.findings.get(i)).collect()
}
