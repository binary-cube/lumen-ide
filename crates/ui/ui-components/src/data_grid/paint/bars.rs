//! Painting the bars around the table: toolbar, filters, find, bottom.

use std::sync::Arc;

use super::*;

impl<S: GridSource> DataGrid<S> {
    pub(super) fn paint_toolbar(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        bar: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mid = bar.min_y() + bar.size.height / 2.0;

        // The texts first (shaped once, the text system keeps them), then where things go
        // for this width (worked out again only when something here changes).
        let total = self.source.total();
        let known = total.known();
        let rows = self.page_rows();
        let paged = self.paging.size.is_some();
        let pending = self.source.pending();
        // A slow page can be stopped (after a moment: a quick one doesn't flash a button).
        let slow = self.loading_since.is_some_and(|t| t.elapsed().as_millis() > 400);
        // `1–500 of 12,345`, the total clickable when it's an estimate (counted then).
        let font = ds.ui_font(-0.5);
        // The bar's texts, written again only when what they say changes (the page, the
        // total, the page's size, the changes waiting, the language).
        let shown = self.paging.span(known.or(total.approximate()), rows).filter(|_| rows > 0);
        let key = (i18n::generation(), shown, total, self.paging.size, pending);
        let texts = self.bar_texts.get_or_update(key, || Arc::new(BarTexts::new(key))).clone();
        let (span, of, size_label) = (&texts.span, &texts.of, &texts.size);
        let span_font = font.clone().with_weight(Font::SEMIBOLD);
        let span_shaped = text.shape_line(span, &span_font);
        let of_shaped = (!of.is_empty()).then(|| text.shape_line(of, &font));
        let small = ds.ui_font(-1.0);
        let size_shaped = text.shape_line(size_label, &small);
        let chevron = px(12.0);
        let submit = texts.submit.as_deref().map(|label| {
            let w = (text.shape_line(label, &ds::button_font(ds)).width + px(24.0)).round();
            (label, w)
        });
        // The data extractor's pill: its name and a chevron (rounded up: rounded down, its
        // name wouldn't fit the room left for it and would end in "…").
        let extractor_label = self.extractor_label();
        let extractor_shaped = text.shape_line(&extractor_label, &small);
        let spec = ToolbarSpec {
            extractor_w: (extractor_shaped.width.min(px(140.0)) + chevron + px(24.0)).ceil(),
            min_x: bar.min_x(),
            max_x: bar.max_x(),
            metrics: Metrics::new(px),
            paged,
            editable: self.can_edit(),
            pending: pending > 0,
            cancel: self.source.capabilities().cancel && slow,
            loading: self.is_loading(),
            range_w: span_shaped.width,
            of_w: of_shaped.as_ref().map(|s| s.width),
            size_w: size_shaped.width + chevron + px(26.0),
            submit_w: submit.as_ref().map_or(0.0, |(_, w)| *w),
        };
        let laid = self.toolbar.get_or_update(spec, || Arc::new(toolbar::layout(&spec))).clone();
        self.collapsed.clone_from(&laid.collapsed);

        let base = ds::optical_baseline(bar.min_y(), bar.size.height, &font);
        if let Some(x) = laid.divider {
            ds::divider_vertical(layer, ds, x, mid - px(10.0), mid + px(10.0));
        }
        for &(tool, x, w) in &laid.placed {
            let icon = match tool {
                Tool::First => "arrow-left-to-line",
                Tool::Back => "chevron-left",
                Tool::Forward => "chevron-right",
                Tool::Last => "arrow-right-to-line",
                Tool::Cancel => "x",
                Tool::Add => "plus",
                Tool::Delete => "trash",
                Tool::Revert => "undo",
                Tool::Refresh => "refresh",
                Tool::Find => "search",
                Tool::Transpose => "transpose",
                Tool::More => "more-horizontal",
                Tool::Range => {
                    if laid.range_room + 0.5 >= span_shaped.width {
                        draw_text(layer, &span_shaped, x.round(), base, p.text);
                    } else {
                        draw_fitted(
                            layer,
                            text,
                            span,
                            &span_font,
                            (x.round(), laid.range_room, base),
                            p.text,
                            Align::Left,
                        );
                    }
                    continue;
                }
                Tool::Of => {
                    let Some(of_shaped) = &of_shaped else { continue };
                    let estimate = matches!(total, Total::Estimate(_));
                    if !estimate {
                        draw_text(layer, of_shaped, x.round(), base, p.text_muted);
                        continue;
                    }
                    // An estimate is a link: pressed, the rows are counted.
                    let r = Rect::new(x - px(4.0), mid - px(12.0), w + px(8.0), px(24.0));
                    let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                        .label(of)
                        .strong()
                        .tone(ds::Tone::Accent)
                        .hover_tint(ds::Tone::Accent)
                        .hovered(self.hover == Some(Hit::Count));
                    ds::button(layer, text, icons, ds, r, spec);
                    self.hits.push((r, Hit::Count));
                    continue;
                }
                Tool::PageSize => {
                    // The page's size: a field opening the choices.
                    let r = Rect::new(x, (mid - px(13.0)).round(), w, px(26.0));
                    // Its choices are the app's menu (open there, not here).
                    let open = false;
                    let hovered = self.hover == Some(Hit::PageSize);
                    toolbar_dropdown(layer, text, icons, ds, r, (size_label, open, hovered));
                    self.hits.push((r, Hit::PageSize));
                    continue;
                }
                Tool::Extractor => {
                    let r = Rect::new(x, (mid - px(13.0)).round(), w, px(26.0));
                    let open = self.extract_open;
                    let hovered = self.hover == Some(Hit::Extractor);
                    toolbar_dropdown(layer, text, icons, ds, r, (&extractor_label, open, hovered));
                    self.hits.push((r, Hit::Extractor));
                    continue;
                }
                Tool::Submit => {
                    let Some((label, w)) = &submit else { continue };
                    let r = Rect::new(x, (mid - px(13.0)).round(), *w, px(26.0));
                    let spec = ds::ButtonSpec::new(ds::ButtonKind::Primary)
                        .label(label)
                        .hovered(self.hover == Some(Hit::Submit));
                    ds::button(layer, text, icons, ds, r, spec);
                    self.hits.push((r, Hit::Submit));
                    continue;
                }
                Tool::Loader => {
                    ds::loader(
                        layer,
                        text,
                        ds,
                        x,
                        mid,
                        &ds::Loader { size: 14.0, ..Default::default() },
                    );
                    continue;
                }
            };
            let Some(hit) = tool_hit(tool) else { continue };
            let on = match tool {
                Tool::Transpose => self.transpose,
                Tool::Find => self.find.open,
                _ => false,
            };
            let enabled = self.tool_enabled(tool);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon(icon)
                .enabled(enabled)
                .hovered(self.hover == Some(hit))
                .toggle(on);
            let r = grid_button(layer, text, icons, ds, (x, mid), spec);
            if enabled {
                self.hits.push((r, hit));
            } else {
                self.marks.push((r, Mark::Disabled(tool)));
            }
        }
    }

    /// Whether `WHERE` and `ORDER BY` go one over the other in a grid `width` wide (both
    /// there, too narrow for them side by side).
    pub(in crate::data_grid) fn filters_stacked(&self, ds: &Ctx<'_>, width: f32) -> bool {
        let caps = self.source.capabilities();
        caps.filter && caps.sort && width - ds.px(20.0) < ds.px(STACK_FILTERS_BELOW)
    }

    pub(super) fn paint_filters(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        row: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let caps = self.source.capabilities();
        let mid = row.min_y() + row.size.height / 2.0;
        if !caps.filter && !caps.sort {
            // Why not: one muted line.
            if let Some(limit) = &caps.limit {
                let font = ds.ui_font(-1.0);
                let line = ds::ellipsize(text, limit, &font, row.size.width - px(48.0));
                let shaped = text.shape_line(&line, &font);
                if let Some(id) = icons.id("filter") {
                    let s = px(13.0);
                    ui_kit::draw_icon(
                        layer,
                        icons,
                        id,
                        Rect::new(row.min_x() + px(14.0), (mid - s / 2.0).round(), s, s),
                        p.text_muted,
                    );
                }
                draw_text(
                    layer,
                    &shaped,
                    row.min_x() + px(36.0),
                    ds::optical_baseline(row.min_y(), row.size.height, &font),
                    p.text_muted,
                );
            }
            return;
        }
        let field_h = px(30.0);
        let label_font = ds.ui_font(-1.5).with_weight(Font::SEMIBOLD);
        let pad = px(10.0);
        let total_w = row.size.width - 2.0 * pad;
        let stacked = self.filters_stacked(ds, row.size.width);
        let (where_w, order_w) = match (caps.filter, caps.sort) {
            (true, true) if stacked => (total_w.round(), total_w.round()),
            (true, true) => ((total_w * 0.6).round(), (total_w * 0.4 - px(10.0)).round()),
            (true, false) => (total_w, 0.0),
            _ => (0.0, total_w),
        };
        let mut x = row.min_x() + pad;
        // Stacked, each field is centered in its own row's height.
        let mut mid = if stacked { row.min_y() + px(FILTER_H) / 2.0 } else { mid };
        let mut tip = None;
        let fields = [
            (caps.filter, Field::Where, "WHERE", where_w, Hit::Where, Hit::ClearWhere),
            (caps.sort, Field::Order, "ORDER BY", order_w, Hit::Order, Hit::ClearOrder),
        ];
        for (shown, field, keyword, w, hit, clear) in fields {
            if !shown || w <= 0.0 {
                continue;
            }
            let keyword_shaped = text.shape_line(keyword, &label_font);
            let lead = keyword_shaped.width + px(14.0);
            let r = Rect::new(x, (mid - field_h / 2.0).round(), w, field_h);
            let error = match field {
                Field::Where => self.where_error.clone(),
                _ => self.order_error.clone(),
            };
            let input = if field == Field::Where { &self.where_input } else { &self.order_input };
            // The source's own hint, else the usual one (written once per language).
            let usual = self.hints.get_or_update(i18n::generation(), Hints::new).clone();
            let placeholder = match field {
                Field::Where => self.source.filter_hint().map_or(usual.filter.clone(), Arc::from),
                _ => usual.order.clone(),
            };
            let focused = self.field == Some(field);
            // The field's frame drawn here; the text inside, clear of the × button.
            let style = field_style(ds, error.is_some());
            let border = if focused { style.border_focused } else { style.border };
            layer.quad(
                Quad::filled(r, style.background)
                    .rounded(style.radius)
                    .bordered(style.border_width, border),
            );
            let clear_room = if input.text().is_empty() { 0.0 } else { px(26.0) };
            let inner = Rect::new(r.min_x(), r.min_y(), r.size.width - clear_room, r.size.height);
            let mut style = style;
            style.lead = lead;
            style.background = Rgba::TRANSPARENT;
            style.border = Rgba::TRANSPARENT;
            style.border_focused = Rgba::TRANSPARENT;
            // The hint cut to the field (the field scrolls typed text itself).
            let room = inner.size.width - lead - 2.0 * style.padding;
            let room = room.max(0.0);
            let cut;
            let placeholder = if text.shape_line(&placeholder, &style.font).width <= room + 0.5 {
                &*placeholder
            } else {
                cut = ds::ellipsize(text, &placeholder, &style.font, room);
                cut.as_str()
            };
            input.paint(layer, text, inner, focused, placeholder, &style);
            draw_text(
                layer,
                &keyword_shaped,
                r.min_x() + px(10.0),
                ds::optical_baseline(r.min_y(), r.size.height, &label_font),
                p.accent.with_alpha(0.85),
            );
            self.hits.push((r, hit));
            if !input.text().is_empty() {
                let c = ds::clear_button(
                    layer,
                    icons,
                    ds,
                    r.max_x() - px(15.0),
                    mid,
                    self.hover == Some(clear),
                );
                self.hits.push((c, clear));
            }
            if let Some(error) = error.filter(|_| focused) {
                tip = Some((error, r));
            }
            if stacked {
                mid += px(FILTER_H) - px(FILTER_OVERLAP);
            } else {
                x += w + px(10.0);
            }
        }
        // Drawn over everything at the end (a popover under the field).
        self.field_tip = tip;
    }

    /// Why the field having the keys isn't understood: a popover just under it, over the
    /// headers and the rows (drawn last, so no line of theirs crosses it).
    pub(super) fn paint_field_tip(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
    ) {
        let Some((error, field)) = &self.field_tip else { return };
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(-1.0);
        let icon = px(14.0);
        let (pad, gap) = (px(10.0), px(8.0));
        let room = (field.size.width - 2.0 * pad - icon - gap).max(px(80.0));
        let line = ds::ellipsize(text, error, &font, room);
        let shaped = text.shape_line(&line, &font);
        let h = px(30.0);
        let w = (pad + icon + gap + shaped.width + pad).min(field.size.width);
        let r = Rect::new(field.min_x(), field.max_y() + px(6.0), w, h);
        ds::surface(layer, ds, r, ds::Elevation::Medium, radius::MD);
        layer.quad(
            Quad::filled(r, Rgba::TRANSPARENT)
                .rounded(px(radius::MD))
                .bordered(ds.line(), p.danger.with_alpha(0.55)),
        );
        if let Some(id) = icons.id("alert") {
            let ir = Rect::new(r.min_x() + pad, (r.min_y() + (h - icon) / 2.0).round(), icon, icon);
            ui_kit::draw_icon(layer, icons, id, ir, p.danger);
        }
        draw_text(
            layer,
            &shaped,
            r.min_x() + pad + icon + gap,
            ds::optical_baseline(r.min_y(), h, &font),
            p.text,
        );
    }

    pub(super) fn paint_find(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        row: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let mid = row.min_y() + row.size.height / 2.0;
        // Right to left: ↑ ↓ × at the end, the count before them (a room of its own, so the
        // field doesn't jump while it changes), the field taking all that remains.
        let font = ds.ui_font(-1.0);
        let count = if self.find.search.input.text().trim().is_empty() {
            String::new()
        } else if self.find.matches.is_empty() {
            i18n::t("grid.find_none")
        } else {
            i18n::tr!(
                "grid.find_count",
                current = self.find.current + 1,
                count = self.find.matches.len()
            )
        };
        let shaped = (!count.is_empty()).then(|| text.shape_line(&count, &font));
        let count_w = shaped.as_ref().map_or(0.0, |s| s.width).max(px(64.0));
        let buttons = 3.0 * px(28.0) + 2.0 * px(2.0);
        let buttons_x = row.max_x() - px(10.0) - buttons;
        let w =
            (buttons_x - px(8.0) - count_w - px(10.0) - (row.min_x() + px(10.0))).max(px(120.0));
        let r = Rect::new(row.min_x() + px(10.0), (mid - px(14.0)).round(), w, px(28.0));
        let focused = self.field == Some(Field::Find);
        let placeholder = i18n::t("grid.find_hint");
        self.find.search.paint(layer, text, icons, ds, (r, focused), &placeholder);
        self.hits.push((r, Hit::Find));
        if let Some(clear) = self.find.search.clear_area() {
            self.hits.push((clear, Hit::ClearFind));
        }
        if let Some(shaped) = shaped {
            let color = if self.find.matches.is_empty() { p.danger } else { p.text_muted };
            draw_text(
                layer,
                &shaped,
                r.max_x() + px(10.0),
                ds::optical_baseline(row.min_y(), row.size.height, &font),
                color,
            );
        }
        let mut x = buttons_x.max(r.max_x() + px(10.0) + count_w + px(8.0));
        let any = !self.find.matches.is_empty();
        for (icon, hit, enabled) in [
            ("chevron-up", Hit::FindPrev, any),
            ("chevron-down", Hit::FindNext, any),
            ("x", Hit::FindClose, true),
        ] {
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon(icon)
                .enabled(enabled)
                .hovered(self.hover == Some(hit));
            let r = grid_button(layer, text, icons, ds, (x, mid), spec);
            if enabled {
                self.hits.push((r, hit));
            }
            x = r.max_x() + px(2.0);
        }
    }

    pub(super) fn paint_bottom(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        bar: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        layer.quad(Quad::filled(bar, p.text.with_alpha(ds::BAND_TINT)));
        ds::divider(layer, ds, bar.min_x(), bar.max_x(), bar.min_y());
        let font = ds.ui_font(-1.0);
        let base = ds::optical_baseline(bar.min_y(), bar.size.height, &font);
        let mut x = bar.min_x() + px(14.0);
        let status = self.source.status();
        // How many rows, written once per total and language; a failure's reason as it is.
        let total = self.source.total();
        let count =
            self.rows_line.get_or_update((i18n::generation(), total), || rows_line(total)).clone();
        let left: Option<(&str, Rgba)> = match &status {
            SourceStatus::Failed(why) => Some((why.as_str(), p.danger)),
            _ => count.as_deref().map(|line| (line, p.text_muted)),
        };
        let mut right = bar.max_x() - px(14.0);
        // The selection's figures on the right.
        if let Some(a) = self.aggregates() {
            let mut parts = vec![i18n::tr!("grid.agg_count", count = a.count)];
            if a.numbers > 0 {
                parts.push(i18n::tr!("grid.agg_sum", value = a.sum_text()));
                if let Some(avg) = a.average() {
                    parts.push(i18n::tr!(
                        "grid.agg_avg",
                        value = super::super::aggregate::number(avg)
                    ));
                }
                parts.push(i18n::tr!("grid.agg_min", value = a.min_text()));
                parts.push(i18n::tr!("grid.agg_max", value = a.max_text()));
            }
            let line = parts.join("  ·  ");
            let line = ds::ellipsize(text, &line, &font, bar.size.width * 0.6);
            let shaped = text.shape_line(&line, &font);
            right -= shaped.width;
            draw_text(layer, &shaped, right.round(), base, p.text);
            right -= px(16.0);
        }
        if let Some((line, color)) = left {
            let room = (right - x).max(0.0);
            let (width, _) = ds::draw_ellipsized(layer, text, line, &font, (x, base), room, color);
            x += width;
        }
        if self.query.filter.is_some() && !matches!(status, SourceStatus::Failed(_)) {
            let tag = i18n::t("grid.filtered");
            let shaped = text.shape_line(&tag, &font);
            if x + px(12.0) + shaped.width < right {
                draw_text(layer, &shaped, x + px(10.0), base, p.accent);
                x += px(10.0) + shaped.width;
            }
        }
        // What waits to be submitted; why the last edit wasn't made; why it can't be.
        let pending = self.source.pending();
        let (line, color) = if let Some(notice) = &self.notice {
            (notice.clone(), p.warning)
        } else if pending > 0 {
            (i18n::tr!("grid.pending", count = pending), p.vcs_modified)
        } else {
            (String::new(), p.text_muted)
        };
        if !line.is_empty() && x + px(40.0) < right {
            let dot = Rect::new(
                x + px(14.0),
                (bar.min_y() + bar.size.height / 2.0 - px(3.0)).round(),
                px(6.0),
                px(6.0),
            );
            layer.quad(Quad::filled(dot, color).rounded(px(3.0)));
            let line = ds::ellipsize(text, &line, &font, (right - x - px(28.0)).max(0.0));
            let shaped = text.shape_line(&line, &font);
            draw_text(layer, &shaped, x + px(26.0), base, color);
        }
    }
}

/// A grid toolbar's field opening a list (the page's size, the data extractor): the shared
/// dropdown, compact, in the accent while its list is open.
fn toolbar_dropdown(
    layer: &mut Layer,
    text: &mut TextSystem,
    icons: &icons::Icons,
    ds: &Ctx<'_>,
    r: Rect,
    (value, open, hovered): (&str, bool, bool),
) {
    let spec =
        ds::DropdownSpec { value, open, hovered, enabled: true, note: None, placeholder: None };
    let look = ds::DropdownLook { compact: true, ..ds::DropdownLook::default() };
    ds::dropdown_styled(layer, text, icons, ds, r, (spec, look));
}

/// The fields' usual hints, written once per language.
#[derive(Debug)]
pub(in crate::data_grid) struct Hints {
    filter: Arc<str>,
    order: Arc<str>,
    /// What a cell left to its column's default shows.
    pub(in crate::data_grid) default: Arc<str>,
}

impl Hints {
    pub(in crate::data_grid) fn new() -> Arc<Self> {
        let (filter, order) = (i18n::t("grid.where_hint"), i18n::t("grid.order_hint"));
        let default = i18n::t("grid.default");
        Arc::new(Self { filter: filter.into(), order: order.into(), default: default.into() })
    }
}

/// How many rows the source has, as the bottom bar says it (`None`: not known at all).
fn rows_line(total: Total) -> Option<Arc<str>> {
    let line = match total {
        Total::Known(n) => i18n::tr!("grid.rows", count = n, shown = grouped(n)),
        Total::Estimate(n) => i18n::tr!("grid.rows_estimate", count = n, shown = grouped(n)),
        Total::Pending => i18n::t("grid.counting"),
        Total::AtLeast(n) => i18n::tr!("grid.rows_more", count = n, shown = grouped(n)),
        Total::Unknown => return None,
    };
    Some(line.into())
}

/// What the toolbar says about the page, made once for what it depends on.
#[derive(Debug)]
pub(in crate::data_grid) struct BarTexts {
    /// `1–500`.
    span: String,
    /// `of 12,345` (empty: the total isn't known at all).
    of: String,
    /// The page's size (`500 rows`, `All`).
    size: String,
    /// "Submit (3)" while changes wait.
    submit: Option<String>,
}

/// What [`BarTexts`] are made from: the messages' generation, the rows shown, the total,
/// the page's size, the changes waiting.
pub(in crate::data_grid) type BarKey = (u64, Option<(u64, u64)>, Total, Option<u32>, usize);

impl BarTexts {
    fn new((_, shown, total, size, pending): BarKey) -> Self {
        let span = match shown {
            Some((a, b)) => format!("{}–{}", grouped(a), grouped(b)),
            None => "0".to_owned(),
        };
        let of = match total {
            Total::Known(n) => i18n::tr!("grid.of", total = grouped(n)),
            Total::Estimate(n) => i18n::tr!("grid.of_estimate", total = grouped(n)),
            Total::Pending => i18n::t("grid.of_pending"),
            Total::AtLeast(n) => i18n::tr!("grid.of_more", total = grouped(n)),
            Total::Unknown => String::new(),
        };
        let size = match size {
            Some(n) => i18n::tr!("grid.page_size", count = n),
            None => i18n::t("grid.page_all"),
        };
        let submit = (pending > 0).then(|| i18n::tr!("grid.submit", count = pending));
        Self { span, of, size, submit }
    }
}
