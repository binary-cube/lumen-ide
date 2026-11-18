//! Painting what opens over the grid.

use super::*;

impl<S: GridSource> DataGrid<S> {
    /// A long value edited on several lines, over the grid: its column and row, the text,
    /// Apply and Cancel (nothing behind it gets a click until one of them).
    pub(super) fn paint_big(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(big) = &self.big else { return };
        let column = self
            .source
            .columns()
            .get(big.col)
            .map(|c| (c.name.clone(), c.type_name.clone()))
            .unwrap_or_default();
        let place = big.place;
        let layer = scene.push_layer(Some(area));
        layer.quad(Quad::filled(area, Rgba::BLACK.with_alpha(0.18)));
        let w = (area.size.width - px(48.0)).min(px(760.0)).max(px(200.0));
        let h = (area.size.height - px(40.0)).min(px(440.0)).max(px(160.0));
        let r = Rect::new(
            (area.min_x() + (area.size.width - w) / 2.0).round(),
            (area.min_y() + (area.size.height - h) / 2.0).round(),
            w,
            h,
        );
        ds::surface(layer, ds, r, ds::Elevation::High, radius::LG);
        let title_font = ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = text.shape_line(&column.0, &title_font);
        draw_text(layer, &shaped, r.min_x() + px(18.0), r.min_y() + px(30.0), p.text);
        let small = ds.ui_font(-1.5);
        let row = if place < self.page_rows() {
            i18n::tr!("grid.record", row = grouped(self.row_at(place) + 1))
        } else {
            i18n::t("grid.record_new")
        };
        let sub = format!("{} · {row}", column.1);
        let sub_shaped = text.shape_line(&sub, &small);
        draw_text(
            layer,
            &sub_shaped,
            r.min_x() + px(18.0) + shaped.width + px(10.0),
            r.min_y() + px(30.0),
            p.text_muted,
        );
        // The text.
        let field = Rect::new(
            r.min_x() + px(14.0),
            r.min_y() + px(46.0),
            r.size.width - px(28.0),
            r.size.height - px(46.0 + 58.0),
        );
        layer.quad(
            Quad::filled(field, p.input_background)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.accent),
        );
        self.hits.push((field, Hit::BigArea));
        let font = ds.code_font(-1.0);
        let line_h = px(20.0);
        let inner = Rect::new(
            field.min_x() + px(10.0),
            field.min_y() + px(8.0),
            field.size.width - px(20.0),
            field.size.height - px(16.0),
        );
        let click = self.big_click.take();
        let text_layer = scene.push_layer(Some(inner));
        if let Some(big) = &mut self.big {
            if let Some(at) = click {
                big.area.click(text, at, inner, &font, line_h);
            }
            big.area.paint(
                text_layer,
                text,
                inner,
                (&font, line_h),
                (p.text, p.selection, p.caret),
                px(1.5).max(1.0),
            );
        }
        // The buttons, the keys, an error.
        let layer = scene.push_layer(Some(area));
        let bh = px(30.0);
        let y = r.max_y() - px(14.0) - bh;
        let apply = i18n::t("grid.apply");
        let cancel = i18n::t("grid.cancel");
        let label_font = ds::button_font(ds);
        let aw = text.shape_line(&apply, &label_font).width + px(28.0);
        let a = Rect::new(r.max_x() - px(14.0) - aw, y, aw, bh);
        ds::primary_button(layer, text, ds, a, &apply, true, self.hover == Some(Hit::BigApply));
        self.hits.push((a, Hit::BigApply));
        let cw = text.shape_line(&cancel, &label_font).width + px(28.0);
        let c = Rect::new(a.min_x() - px(8.0) - cw, y, cw, bh);
        let hovered = self.hover == Some(Hit::BigCancel);
        ds::secondary_button(layer, text, ds, c, &cancel, true, hovered);
        self.hits.push((c, Hit::BigCancel));
        let hint = match self.big.as_ref().and_then(|b| b.error.clone()) {
            Some(error) => (error, p.danger),
            None => (i18n::t("grid.big_hint"), p.text_muted),
        };
        let hint_text =
            ds::ellipsize(text, &hint.0, &small, (c.min_x() - r.min_x() - px(30.0)).max(0.0));
        let hs = text.shape_line(&hint_text, &small);
        draw_text(layer, &hs, r.min_x() + px(18.0), ds::optical_baseline(y, bh, &small), hint.1);
    }

    pub(super) fn paint_sizes(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        // Under its pill, or under the "…" holding it (right-aligned with it).
        let anchor = |hit: Hit| self.hits.iter().find(|(_, h)| *h == hit).map(|(r, _)| *r);
        let (anchor, in_more) = match anchor(Hit::PageSize) {
            Some(r) => (r, false),
            None => match anchor(Hit::More) {
                Some(r) => (r, true),
                None => return,
            },
        };
        let font = ds.ui_font(-0.5);
        let total = self.source.total().approximate();
        let large = total.is_some_and(|t| t > LARGE);
        let mut choices: Vec<(Option<u32>, String)> =
            PAGE_SIZES.iter().map(|&n| (Some(n), i18n::tr!("grid.page_size", count = n))).collect();
        choices.push((None, i18n::t("grid.page_all")));
        let row_h = px(30.0);
        let warning =
            large.then(|| i18n::tr!("grid.page_all_warning", total = grouped(total.unwrap_or(0))));
        let small = ds.ui_font(-2.0);
        let width =
            choices.iter().map(|(_, l)| text.shape_line(l, &font).width).fold(px(150.0), f32::max)
                + px(48.0);
        let width = warning.as_ref().map_or(width, |w| {
            width.max(text.shape_line(w, &small).width + px(28.0)).min(px(360.0))
        });
        let height = choices.len() as f32 * row_h
            + px(12.0)
            + if warning.is_some() { px(26.0) } else { 0.0 };
        let x = if in_more {
            (anchor.max_x() - width).max(self.geometry.bounds.min_x() + px(6.0))
        } else {
            anchor.min_x()
        };
        let r = Rect::new(x, anchor.max_y() + px(6.0), width, height);
        ds::surface(layer, ds, r, ds::Elevation::Medium, radius::LG);
        let mut y = r.min_y() + px(6.0);
        for (size, label) in choices {
            let row = Rect::new(r.min_x() + px(6.0), y, r.size.width - px(12.0), row_h);
            let hit = Hit::PageChoice(size);
            if self.hover == Some(hit) {
                layer.quad(Quad::filled(row, p.hover_background).rounded(px(radius::SM)));
            }
            let chosen = self.paging.size == size;
            if chosen && let Some(id) = icons.id("check") {
                let s = px(13.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        row.min_x() + px(8.0),
                        (row.min_y() + (row_h - s) / 2.0).round(),
                        s,
                        s,
                    ),
                    p.accent,
                );
            }
            let shaped = text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                row.min_x() + px(30.0),
                ds::optical_baseline(row.min_y(), row_h, &font),
                if chosen { p.accent } else { p.text },
            );
            self.hits.push((row, hit));
            y += row_h;
        }
        if let Some(warning) = warning {
            let line = ds::ellipsize(text, &warning, &small, r.size.width - px(24.0));
            let shaped = text.shape_line(&line, &small);
            draw_text(layer, &shaped, r.min_x() + px(14.0), (y + px(17.0)).round(), p.warning);
        }
    }

    /// The tools that didn't fit, as a menu under the "…" (right-aligned with it, kept in
    /// the grid): each with its icon, its label and its keys; those that can't be used now
    /// muted.
    pub(super) fn paint_more(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let Some(anchor) = self.hits.iter().find(|(_, h)| *h == Hit::More).map(|(r, _)| *r) else {
            return;
        };
        let font = ds.ui_font(-0.5);
        let keys_font = ds.ui_font(-1.0);
        let row_h = px(30.0);
        let sep_h = px(9.0);
        let rows: Vec<(Tool, String, Option<String>, bool)> = self
            .collapsed
            .clone()
            .into_iter()
            .map(|t| (t, tool_label(self, t), self.tool_keys(t), self.tool_enabled(t)))
            .collect();
        let group = |t: Tool| match t {
            Tool::First | Tool::Back | Tool::Forward | Tool::Last | Tool::PageSize => 0,
            Tool::Add | Tool::Delete => 1,
            _ => 2,
        };
        let separators = rows.windows(2).filter(|w| group(w[0].0) != group(w[1].0)).count();
        let mut width = px(180.0);
        for (_, label, keys, _) in &rows {
            let keys_w =
                keys.as_ref().map_or(0.0, |k| text.shape_line(k, &keys_font).width + px(24.0));
            width = width.max(text.shape_line(label, &font).width + keys_w + px(56.0));
        }
        let height = rows.len() as f32 * row_h + separators as f32 * sep_h + px(12.0);
        let x = (anchor.max_x() - width).max(self.geometry.bounds.min_x() + px(6.0));
        let r = Rect::new(x, anchor.max_y() + px(6.0), width, height);
        ds::surface(layer, ds, r, ds::Elevation::Medium, radius::LG);
        let mut y = r.min_y() + px(6.0);
        let mut last = None;
        for (tool, label, keys, enabled) in rows {
            if last.is_some_and(|g| g != group(tool)) {
                ds::divider(
                    layer,
                    ds,
                    r.min_x() + px(10.0),
                    r.max_x() - px(10.0),
                    (y + sep_h / 2.0).round(),
                );
                y += sep_h;
            }
            last = Some(group(tool));
            let row = Rect::new(r.min_x() + px(6.0), y, r.size.width - px(12.0), row_h);
            let hit = Hit::MoreChoice(tool);
            if enabled && self.hover == Some(hit) {
                layer.quad(Quad::filled(row, p.hover_background).rounded(px(radius::SM)));
            }
            let color = if enabled { p.text } else { p.text_muted.with_alpha(0.6) };
            if let Some(id) = tool_icon(tool).and_then(|i| icons.id(i)) {
                let s = px(14.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(
                        row.min_x() + px(8.0),
                        (row.min_y() + (row_h - s) / 2.0).round(),
                        s,
                        s,
                    ),
                    if enabled { p.text.with_alpha(0.86) } else { color },
                );
            }
            let shaped = text.shape_line(&label, &font);
            draw_text(
                layer,
                &shaped,
                row.min_x() + px(32.0),
                ds::optical_baseline(row.min_y(), row_h, &font),
                color,
            );
            if let Some(keys) = keys {
                let k = text.shape_line(&keys, &keys_font);
                draw_text(
                    layer,
                    &k,
                    (row.max_x() - px(10.0) - k.width).round(),
                    ds::optical_baseline(row.min_y(), row_h, &keys_font),
                    p.text_muted,
                );
            }
            if enabled {
                self.hits.push((row, hit));
            }
            y += row_h;
        }
    }

    /// Under the extractor's pill (or the "…" holding it), right-aligned with it: the groups'
    /// headings, the extractors (the chosen one checked, one that can't be used muted with
    /// its warning), the skips' switches, the module's commands, the note; scrolled when it's
    /// taller than the room under it.
    pub(super) fn paint_extractors(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let anchor = |hit: Hit| self.hits.iter().find(|(_, h)| *h == hit).map(|(r, _)| *r);
        let Some(anchor) = anchor(Hit::Extractor).or_else(|| anchor(Hit::More)) else {
            self.extract_area = None;
            return;
        };
        let font = ds.ui_font(-0.5);
        let heading_font = ds.ui_font(-2.0).with_weight(Font::SEMIBOLD);
        let small = ds.ui_font(-2.0);
        let options = extractors::catalog().options();
        let chosen = self.extractor().id().to_owned();
        let height_of = |row: &PopRow| match row {
            PopRow::Heading(_) => px(26.0),
            PopRow::Note(_) => px(30.0),
            _ => px(28.0),
        };
        let mut width = px(240.0);
        for row in &self.extract_rows {
            let w = match row {
                PopRow::Choice { name, problem, .. } => {
                    text.shape_line(name, &font).width
                        + if problem.is_some() { px(24.0) } else { 0.0 }
                }
                PopRow::Toggle { label, .. } | PopRow::Action { label, .. } => {
                    text.shape_line(label, &font).width
                }
                PopRow::Heading(_) | PopRow::Note(_) => 0.0,
            };
            width = width.max(w + px(56.0));
        }
        let width = width.min(px(380.0));
        let content: f32 = self.extract_rows.iter().map(height_of).sum::<f32>() + px(12.0);
        let top = anchor.max_y() + px(6.0);
        let room = (self.geometry.bounds.max_y() - top - px(6.0)).max(px(160.0));
        let height = content.min(room);
        let max_scroll = (content - height).max(0.0);
        self.extract_scroll = self.extract_scroll.clamp(0.0, max_scroll);
        let x = (anchor.max_x() - width).max(self.geometry.bounds.min_x() + px(6.0));
        let r = Rect::new(x, top, width, height);
        self.extract_area = Some(r);
        let layer = scene.push_layer(None);
        ds::surface(layer, ds, r, ds::Elevation::Medium, radius::LG);
        let layer = scene.push_layer(Some(r));
        let mut y = r.min_y() + px(6.0) - self.extract_scroll;
        let rows = self.extract_rows.clone();
        for (at, row) in rows.iter().enumerate() {
            let h = height_of(row);
            let line = Rect::new(r.min_x() + px(6.0), y, r.size.width - px(12.0), h);
            y += h;
            if line.max_y() < r.min_y() || line.min_y() > r.max_y() {
                continue;
            }
            let base = ds::optical_baseline(line.min_y(), h, &font);
            let hit = Hit::ExtractorRow(at);
            let hovered = self.hover == Some(hit);
            let mark = |layer: &mut Layer, icon: &str, color: Rgba| {
                if let Some(id) = icons.id(icon) {
                    let s = px(13.0);
                    ui_kit::draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(
                            line.min_x() + px(8.0),
                            (line.min_y() + (h - s) / 2.0).round(),
                            s,
                            s,
                        ),
                        color,
                    );
                }
            };
            match row {
                PopRow::Heading(label) => {
                    if at > 0 {
                        ds::divider(
                            layer,
                            ds,
                            r.min_x() + px(10.0),
                            r.max_x() - px(10.0),
                            line.min_y().round(),
                        );
                    }
                    let shaped = text.shape_line(&label.to_uppercase(), &heading_font);
                    draw_text(
                        layer,
                        &shaped,
                        line.min_x() + px(10.0),
                        ds::optical_baseline(line.min_y() + px(4.0), h - px(4.0), &heading_font),
                        p.text_muted,
                    );
                }
                PopRow::Choice { id, name, problem } => {
                    let usable = problem.is_none();
                    if usable && hovered {
                        layer.quad(Quad::filled(line, p.hover_background).rounded(px(radius::SM)));
                    }
                    let on = *id == chosen;
                    if on {
                        mark(layer, "check", p.accent);
                    }
                    let color = if !usable {
                        p.text_muted.with_alpha(0.7)
                    } else if on {
                        p.accent
                    } else {
                        p.text
                    };
                    let shaped = text.shape_line(name, &font);
                    draw_text(layer, &shaped, line.min_x() + px(30.0), base, color);
                    if !usable && let Some(icon) = icons.id("alert") {
                        let s = px(13.0);
                        ui_kit::draw_icon(
                            layer,
                            icons,
                            icon,
                            Rect::new(
                                line.max_x() - px(10.0) - s,
                                (line.min_y() + (h - s) / 2.0).round(),
                                s,
                                s,
                            ),
                            p.warning,
                        );
                    }
                    self.hits.push((line, hit));
                }
                PopRow::Toggle { computed, label } => {
                    if hovered {
                        layer.quad(Quad::filled(line, p.hover_background).rounded(px(radius::SM)));
                    }
                    let on = if *computed { options.skip_computed } else { options.skip_generated };
                    let check = if on { ds::Check::On } else { ds::Check::Off };
                    ds::checkbox(
                        layer,
                        text,
                        ds,
                        line.min_x() + px(7.0),
                        line.min_y() + h / 2.0,
                        check,
                        true,
                    );
                    let shaped = text.shape_line(label, &font);
                    draw_text(layer, &shaped, line.min_x() + px(30.0), base, p.text);
                    self.hits.push((line, hit));
                }
                PopRow::Action { command, label } => {
                    if hovered {
                        layer.quad(Quad::filled(line, p.hover_background).rounded(px(radius::SM)));
                    }
                    let icon = if *command == extractors::OPEN_DIRECTORY_COMMAND {
                        "folder-open"
                    } else {
                        "settings"
                    };
                    mark(layer, icon, p.text_muted);
                    let shaped = text.shape_line(label, &font);
                    draw_text(layer, &shaped, line.min_x() + px(30.0), base, p.text);
                    self.hits.push((line, hit));
                }
                PopRow::Note(note) => {
                    let room = line.size.width - px(20.0);
                    let cut = ds::ellipsize(text, note, &small, room);
                    let shaped = text.shape_line(&cut, &small);
                    draw_text(
                        layer,
                        &shaped,
                        line.min_x() + px(10.0),
                        ds::optical_baseline(line.min_y(), h, &small),
                        p.text_muted,
                    );
                }
            }
        }
        // Taller than the room below the pill: faded edges say there's more to scroll to.
        if max_scroll > 0.0 {
            let hints = scene.push_layer(Some(r));
            ds::scroll_hints(
                hints,
                icons,
                ds,
                ds::ScrollHintsSpec {
                    area: r,
                    axis: ds::Axis::Vertical,
                    before: self.extract_scroll > 0.5,
                    after: self.extract_scroll < max_scroll - 0.5,
                    background: p.popup_background,
                },
            );
        }
    }
}
