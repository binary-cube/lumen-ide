//! The modules and frameworks: when the modules' data loads, the list with each one's state, the
//! frameworks of the project.

use super::*;

impl SettingsView {
    /// The "Module" section: the project's frameworks (switches, with why each is on) and
    /// the modules that are always on.
    pub(super) fn paint_modules(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let bottom = paint_module_summary(model, SectionArea { layer, text, ctx, x0, top, width });
        let top = bottom + ctx.px(28.0);
        let small = ctx.ui_font(-1.0);
        let small_m = text.metrics(&small);
        let mut rows = ModuleRows {
            layer,
            text,
            icons,
            ctx,
            x0,
            width,
            row_h: ctx.px(56.0),
            strong: ctx.ui_font(0.0).with_weight(text_layout::Font::SEMIBOLD),
            small,
            small_m,
        };
        let y = self.paint_module_loading(model, &mut rows, top);
        let y = self.paint_module_list(model, &mut rows, y);
        self.paint_framework_list(model, &mut rows, y + ctx.px(28.0));
    }

    /// How the modules' data loads (on demand / at startup), from `top`, with a note on
    /// what the choice means; returns the y below it.
    pub(super) fn paint_module_loading(
        &mut self,
        model: &SettingsModel,
        rows: &mut ModuleRows<'_, '_>,
        top: f32,
    ) -> f32 {
        let ctx = rows.ctx;
        let px = |v: f32| ctx.px(v);
        let (layer, text, x0) = (&mut *rows.layer, &mut *rows.text, rows.x0);
        let mut y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            top,
            &i18n::t("settings.module_loading"),
            Some(&i18n::t("settings.module_loading_detail")),
        );
        y += px(10.0);
        let mid = y + px(16.0);
        let mut x = x0;
        for (startup, key) in
            [(false, "settings.loading_on_demand"), (true, "settings.loading_startup")]
        {
            let action = SettingsAction::SetModulesAtStartup(startup);
            let hovered = self.hover.as_ref() == Some(&action);
            let selected = model.modules_at_startup == startup;
            let r = ds::choice(
                layer,
                text,
                ctx,
                ds::ChoiceSpec { x, mid, label: &i18n::t(key), selected, hovered },
            );
            self.module_hits.push((r, action));
            x = r.max_x() + px(8.0);
        }
        y += px(40.0);
        let note = i18n::t(if model.modules_at_startup {
            "settings.loading_startup_note"
        } else {
            "settings.loading_on_demand_note"
        });
        let note = ds::ellipsize(text, &note, &rows.small, rows.width);
        let shaped = text.shape_line(&note, &rows.small);
        draw_text(
            layer,
            &shaped,
            x0,
            (y + rows.small_m.ascent).round(),
            ctx.fade(ctx.palette.text_muted),
        );
        y + px(44.0)
    }

    /// Every module: on / off (from the next start), its data at startup, its state; from
    /// `y`, returns the y below them.
    pub(super) fn paint_module_list(
        &mut self,
        model: &SettingsModel,
        rows: &mut ModuleRows<'_, '_>,
        y: f32,
    ) -> f32 {
        let ctx = rows.ctx;
        let px = |v: f32| ctx.px(v);
        let mut y = ds::section_title(
            rows.layer,
            rows.text,
            ctx,
            rows.x0,
            y,
            &i18n::t("settings.core_modules"),
            Some(&i18n::t("settings.core_modules_detail")),
        );
        y += px(12.0);
        let labels = (i18n::t("settings.module_at_startup"), i18n::t("settings.module_hidden"));
        for row in &model.modules {
            let r = Rect::new(rows.x0, y, rows.width, rows.row_h);
            self.paint_module_row(rows, row, r, &labels, model.modules_at_startup);
            y += rows.row_h + px(4.0);
        }
        y
    }

    /// A module's row `r`: its switch, name and description, "hidden" and "at startup" with
    /// their checkboxes (`labels`), and why it's in its state.
    pub(super) fn paint_module_row(
        &mut self,
        rows: &mut ModuleRows<'_, '_>,
        row: &ModuleRow,
        r: Rect,
        labels: &(String, String),
        modules_at_startup: bool,
    ) {
        let (at_startup, hidden_label) = (labels.0.as_str(), labels.1.as_str());
        let ctx = rows.ctx;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let (layer, text, small, strong) =
            (&mut *rows.layer, &mut *rows.text, &rows.small, &rows.strong);
        let locked = row.enabled && !row.required_by.is_empty();
        let toggle = SettingsAction::ToggleModule(row.id.clone());
        let row_hovered = self.hover.as_ref() == Some(&toggle);
        row_hover(layer, ctx, r, row_hovered);
        let mid = r.min_y() + r.size.height / 2.0;
        let switch = ds::switch(layer, ctx, rows.x0 + px(12.0), mid, row.enabled, row_hovered);
        // Its icon, then its name.
        let icon_size = px(18.0);
        let icon_x = switch.max_x() + px(14.0);
        if let Some(id) = rows.icons.id(row.icon).or_else(|| rows.icons.id("modules")) {
            let at = Rect::new(icon_x, (mid - icon_size / 2.0).round(), icon_size, icon_size);
            let color = if row.enabled { palette.text } else { palette.text_muted };
            ui_kit::draw_icon(layer, rows.icons, id, at, ctx.fade(color));
        }
        let tx = icon_x + icon_size + px(12.0);
        // On the right: "at startup", then the state.
        let preload = SettingsAction::TogglePreload(row.id.clone());
        let can_preload = row.enabled && !modules_at_startup;
        let label = text.shape_line(at_startup, small);
        let lx = r.max_x() - px(12.0) - label.width;
        let state = if modules_at_startup || row.preload { ds::Check::On } else { ds::Check::Off };
        let check = ds::checkbox(layer, text, ctx, lx - px(24.0), mid, state, can_preload);
        let label_color = if can_preload { palette.text } else { palette.text_muted };
        draw_text(
            layer,
            &label,
            lx,
            ds::optical_baseline(r.min_y(), r.size.height, small),
            ctx.fade(label_color),
        );
        let check_area = Rect::new(
            check.min_x() - px(6.0),
            r.min_y() + px(8.0),
            r.max_x() - check.min_x() + px(6.0),
            r.size.height - px(16.0),
        );
        if can_preload {
            if self.hover.as_ref() == Some(&preload) {
                layer.quad(
                    Quad::filled(check_area, ctx.fade(palette.text.with_alpha(0.05)))
                        .rounded(px(8.0)),
                );
            }
            self.module_hits.push((check_area, preload));
        }
        // Left of it: "hidden" (runs, shows nothing of its own); kept while it's off, so
        // turning it on again brings it back as it was.
        let hide = SettingsAction::ToggleModuleHidden(row.id.clone());
        let label = text.shape_line(hidden_label, small);
        let lx = check_area.min_x() - px(16.0) - label.width;
        let state = if row.hidden { ds::Check::On } else { ds::Check::Off };
        let check = ds::checkbox(layer, text, ctx, lx - px(24.0), mid, state, row.enabled);
        let label_color = if row.enabled { palette.text } else { palette.text_muted };
        draw_text(
            layer,
            &label,
            lx,
            ds::optical_baseline(r.min_y(), r.size.height, small),
            ctx.fade(label_color),
        );
        let hide_area = Rect::new(
            check.min_x() - px(6.0),
            r.min_y() + px(8.0),
            (check_area.min_x() - px(4.0) - (check.min_x() - px(6.0))).max(0.0),
            r.size.height - px(16.0),
        );
        if row.enabled {
            if self.hover.as_ref() == Some(&hide) {
                layer.quad(
                    Quad::filled(hide_area, ctx.fade(palette.text.with_alpha(0.05)))
                        .rounded(px(8.0)),
                );
            }
            self.module_hits.push((hide_area, hide));
        }
        // Why it's in its state: needed by others, changed for the next start, in memory.
        let badge_font = ctx.ui_font(-1.5);
        let (status, tone) = if locked {
            (
                i18n::tr!(
                    "settings.module_needed_by",
                    modules = row.required_by.join(", ").as_str()
                ),
                ds::Tone::Neutral,
            )
        } else if row.enabled != row.active || (row.active && row.hidden != row.hidden_now) {
            (i18n::t("settings.module_at_restart"), ds::Tone::Accent)
        } else if row.loaded {
            (i18n::t("settings.module_in_memory"), ds::Tone::Accent)
        } else if !row.active {
            (i18n::t("settings.module_off"), ds::Tone::Neutral)
        } else {
            (String::new(), ds::Tone::Neutral)
        };
        let mut right = hide_area.min_x() - px(10.0);
        if !status.is_empty() {
            let status = ds::ellipsize_middle(text, &status, &badge_font, rows.width * 0.3);
            let badge = ds::badge(
                layer,
                text,
                ctx,
                ds::BadgeSpec { label: &status, x: right, mid, align_right: true, tone },
            );
            right = badge.min_x() - px(10.0);
        }
        let name = ds::ellipsize(text, &row.name, strong, (right - tx).max(0.0));
        let name = text.shape_line(&name, strong);
        let name_color = if row.enabled { palette.text } else { palette.text_muted };
        draw_text(layer, &name, tx, (mid - px(2.0)).round(), ctx.fade(name_color));
        // Beside its name, muted: who made it, its version, when it last changed.
        let about = module_about(row);
        let ax = tx + name.width + px(10.0);
        if !about.is_empty() && ax < right {
            let about = ds::ellipsize(text, &about, small, (right - ax).max(0.0));
            let about = text.shape_line(&about, small);
            draw_text(layer, &about, ax, (mid - px(2.0)).round(), ctx.fade(palette.text_muted));
        }
        let info = ds::ellipsize(text, &row.description, small, (right - tx).max(0.0));
        let info = text.shape_line(&info, small);
        draw_text(
            layer,
            &info,
            tx,
            (mid + px(4.0) + rows.small_m.ascent).round(),
            ctx.fade(palette.text_muted),
        );
        // The switch (the row, but its checkbox); a module others need stays on.
        if locked {
            self.module_rows.push(r);
        } else {
            let area = Rect::new(
                r.min_x(),
                r.min_y(),
                (hide_area.min_x() - r.min_x()).max(0.0),
                r.size.height,
            );
            self.module_hits.push((area, toggle));
        }
    }

    /// The project's frameworks, from `y` (switches, with why each is on).
    pub(super) fn paint_framework_list(
        &mut self,
        model: &SettingsModel,
        rows: &mut ModuleRows<'_, '_>,
        y: f32,
    ) {
        let ctx = rows.ctx;
        let px = |v: f32| ctx.px(v);
        let detail = match &model.project {
            Some(project) => {
                i18n::tr!("settings.frameworks_project", project = project.as_str())
            }
            None => i18n::t("settings.frameworks_no_project"),
        };
        let mut y = ds::section_title(
            rows.layer,
            rows.text,
            ctx,
            rows.x0,
            y,
            &i18n::t("settings.frameworks"),
            Some(&detail),
        );
        y += px(16.0);
        let first = y;
        let enabled = model.project.is_some();
        for row in &model.frameworks {
            let r = Rect::new(rows.x0, y, rows.width, rows.row_h);
            self.paint_framework_row(rows, row, r, enabled);
            y += rows.row_h + px(4.0);
        }
        if y > first {
            self.anchors.push(("frameworks", Rect::new(rows.x0, first, rows.width, y - first)));
        }
    }

    /// A framework's row `r`: its switch (`enabled` with a project open), name, why it is
    /// on / off and its description.
    pub(super) fn paint_framework_row(
        &mut self,
        rows: &mut ModuleRows<'_, '_>,
        row: &FrameworkRow,
        r: Rect,
        enabled: bool,
    ) {
        let ctx = rows.ctx;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let (layer, text, small) = (&mut *rows.layer, &mut *rows.text, &rows.small);
        let hovered = self.hover == Some(SettingsAction::ToggleFramework(row.id.clone()));
        row_hover(layer, ctx, r, hovered);
        let mid = r.min_y() + r.size.height / 2.0;
        let switch =
            ds::switch_enabled(layer, ctx, (rows.x0 + px(12.0), mid), row.on, hovered, enabled);
        let tx = switch.max_x() + px(14.0);
        let name = text.shape_line(&row.name, &rows.strong);
        draw_text(layer, &name, tx, (mid - px(2.0)).round(), ctx.fade(palette.text));
        // Why it is on / off: detected (from where) or chosen for this project.
        let (status, tone) = match (row.on, row.chosen, &row.detected) {
            (true, true, _) => (i18n::t("settings.framework_chosen"), ds::Tone::Accent),
            (true, false, Some(from)) => {
                (i18n::tr!("settings.framework_detected", source = from.as_str()), ds::Tone::Accent)
            }
            (false, true, _) => (i18n::t("settings.framework_off"), ds::Tone::Neutral),
            (false, false, Some(_)) => (i18n::t("settings.framework_excluded"), ds::Tone::Neutral),
            _ => (i18n::t("settings.framework_not_detected"), ds::Tone::Neutral),
        };
        // The badge first (at most half the row), then the description up to it: they
        // never overlap.
        let badge_font = ctx.ui_font(-1.5);
        let status = ds::ellipsize_middle(text, &status, &badge_font, rows.width * 0.45);
        let badge = ds::badge(
            layer,
            text,
            ctx,
            ds::BadgeSpec { label: &status, x: r.max_x() - px(12.0), mid, align_right: true, tone },
        );
        let room = (badge.min_x() - px(16.0) - tx).max(0.0);
        let info = ds::ellipsize(text, &row.description, small, room);
        let info = text.shape_line(&info, small);
        draw_text(
            layer,
            &info,
            tx,
            (mid + px(4.0) + rows.small_m.ascent).round(),
            ctx.fade(palette.text_muted),
        );
        if enabled {
            self.framework_rows.push((r, row.id.clone()));
        } else {
            self.module_rows.push(r);
        }
    }
}

/// Where the "Module" section's rows are painted: the layer, the text, the drawing context,
/// the column (its left edge and width), the rows' height and their fonts (the names', the
/// details' and its metrics).
pub(super) struct ModuleRows<'a, 'c> {
    layer: &'a mut Layer,
    text: &'a mut TextSystem,
    icons: &'a Icons,
    ctx: &'a Ctx<'c>,
    x0: f32,
    width: f32,
    row_h: f32,
    strong: text_layout::Font,
    small: text_layout::Font,
    small_m: text_layout::FontMetrics,
}

/// What a module's row says of it beside its name: its author, version and date, those
/// known (`Binary Cube · 1.0.0 · 2026-11-20`).
pub(super) fn module_about(row: &ModuleRow) -> String {
    [row.author.as_str(), row.version.as_str(), row.updated.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// The "Module" section's summary at `area.top`: how many modules there are, how many work
/// now, which frameworks. Returns where it ends.
pub(super) fn paint_module_summary(model: &SettingsModel, area: SectionArea<'_, '_>) -> f32 {
    let SectionArea { layer, text, ctx, x0, top, width } = area;
    let palette = ctx.palette;
    let px = |v: f32| ctx.px(v);
    let frameworks_on = model.frameworks.iter().filter(|f| f.on).count();
    let total = model.modules.len() + model.frameworks.len();
    let active = model.modules.iter().filter(|m| m.active).count() + frameworks_on;
    let stats = [
        (total, i18n::t("settings.modules_total")),
        (active, i18n::t("settings.modules_active")),
        (frameworks_on, i18n::t("settings.modules_frameworks_on")),
    ];
    let band = Rect::new(x0, top, width, px(62.0));
    layer.quad(
        Quad::filled(band, ctx.fade(palette.text.with_alpha(0.025)))
            .rounded(px(12.0))
            .bordered(ctx.line(), ctx.fade(palette.border.with_alpha(0.6))),
    );
    let number_font = ctx.ui_font(5.0).with_weight(text_layout::Font::SEMIBOLD);
    let label_font = ctx.ui_font(-1.5);
    let column = band.size.width / stats.len() as f32;
    for (i, (count, label)) in stats.iter().enumerate() {
        let cx = band.min_x() + column * i as f32 + px(20.0);
        if i > 0 {
            ds::divider_vertical(
                layer,
                ctx,
                band.min_x() + column * i as f32,
                band.min_y() + px(14.0),
                band.max_y() - px(14.0),
            );
        }
        let number = text.shape_line(&count.to_string(), &number_font);
        let color = if i == 1 { palette.accent } else { palette.text };
        draw_text(layer, &number, cx, band.min_y() + px(30.0), ctx.fade(color));
        let label = text.shape_line(label, &label_font);
        draw_text(layer, &label, cx, band.min_y() + px(49.0), ctx.fade(palette.text_muted));
    }
    band.max_y()
}

/// A module's or a framework's row under the mouse: the text faint, the card's corners.
fn row_hover(layer: &mut render_api::Layer, ctx: &ds::Ctx<'_>, r: Rect, hovered: bool) {
    ds::RowHighlight::new(false, hovered)
        .radius(ROW_RADIUS)
        .hover(ds::HoverFill::Faint(ROW_HOVER))
        .paint(layer, ctx, r);
}

/// A row's corners (logical pixels) and its hover's alpha of the text.
const ROW_RADIUS: f32 = 10.0;
const ROW_HOVER: f32 = 0.04;
