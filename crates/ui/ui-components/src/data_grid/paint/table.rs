//! Painting the table and the record view.

use super::*;

/// An icon before a cell's text (`GridSource::cell_icon`), and the room after it (logical
/// pixels).
const CELL_ICON: f32 = 14.0;
const CELL_ICON_GAP: f32 = 6.0;
/// What a measured cell with an icon is given beyond it (the icon's pixel rounding, so its
/// widest text isn't cut by a hair).
const CELL_ICON_SLACK: f32 = 6.0;

impl<S: GridSource> DataGrid<S> {
    /// The row numbers' width for the page's last row.
    pub(super) fn gutter_width(&self, text: &mut TextSystem, ds: &Ctx<'_>, rows: u64) -> f32 {
        let last = self.paging.start + rows.max(1);
        let digits = grouped(last).len().max(2);
        let font = ds.code_font(-2.0);
        let w = text.shape_line(&"0".repeat(digits), &font).width;
        (w + ds.px(22.0)).max(ds.px(44.0)).round()
    }

    /// Columns not measured yet are fitted to their header and the rows in view; one
    /// asked for (a double click on its edge) to every row of the page that came.
    pub(super) fn measure_columns(
        &mut self,
        text: &mut TextSystem,
        ds: &Ctx<'_>,
        first: u64,
        last: u64,
    ) {
        let fit = self.fit_pending.take();
        if !self.measure && fit.is_none() {
            return;
        }
        if fit.is_some() {
            self.layout_touched();
        }
        let rows_ready =
            (first..last).any(|i| self.source.row_state(self.row_at(i)) == RowState::Ready);
        let name_font = crate::table::name_font(ds);
        let type_font = crate::table::subtitle_font(ds);
        let cell_font = ds.ui_font(-0.5);
        let scale = ds.scale.max(0.01);
        let mut buffer = String::new();
        let mut measure = |grid: &mut Self, col: usize, from: u64, to: u64| -> f32 {
            let c = &grid.source.columns()[col];
            let key = if c.key { ds.px(18.0) } else { 0.0 };
            // Room for the order's arrow and place too.
            let header = text
                .shape_line(&c.name, &name_font)
                .width
                .max(text.shape_line(&c.type_name, &type_font).width)
                + key
                + ds.px(54.0);
            let mut widest = header;
            for i in from..to {
                let row = grid.row_at(i);
                if grid.source.row_state(row) != RowState::Ready {
                    continue;
                }
                cell_line(&mut buffer, &grid.source.cell(row, col));
                // An icon before the text (a level's): its room too.
                let icon = if grid.source.cell_icon(row, col).is_some() {
                    ds.px(CELL_ICON + CELL_ICON_GAP + CELL_ICON_SLACK)
                } else {
                    0.0
                };
                let w = text.shape_line(&buffer, &cell_font).width + 2.0 * ds.px(CELL_PAD) + icon;
                widest = widest.max(w);
            }
            widest / scale
        };
        if let Some(col) = fit {
            let rows = self.page_rows();
            let w = measure(self, col, 0, rows.min(2000));
            self.layout.fit(col, w.max(MIN_WIDTH));
        }
        if self.measure && rows_ready {
            for col in 0..self.layout.len() {
                if !self.layout.is_measured(col) {
                    let w = measure(self, col, first, last.min(first + 100));
                    self.layout.fit(col, w.clamp(MIN_WIDTH + 20.0, 320.0));
                }
            }
            self.measure = false;
        }
    }

    pub(super) fn paint_table(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let row_h = self.geometry.row_h;
        let header = Rect::new(area.min_x(), area.min_y(), area.size.width, px(HEADER_H));
        let body = Rect::new(
            area.min_x(),
            header.max_y(),
            area.size.width,
            (area.max_y() - header.max_y()).max(0.0),
        );

        // Where the page ends, when the source says (rows past it aren't drawn).
        let first_guess = (self.scroll_y / row_h.max(1.0)).floor().max(0.0) as u64;
        let span = (body.size.height / row_h.max(1.0)).ceil() as u64 + 2;
        for i in first_guess..first_guess + span {
            let row = self.row_at(i);
            if self.source.row_state(row) == RowState::Missing {
                self.note_missing(row);
                break;
            }
        }
        let rows = self.shown_rows();
        let gutter = self.gutter_width(text, ds, self.page_rows());
        self.geometry.header = header;
        self.geometry.body = body;
        self.geometry.gutter = gutter;
        self.geometry.rows = rows;

        // The scrolling's limits (the columns' scrollbar, when there is one, under the rows).
        let bar_room = if self.hbar.is_some() {
            crate::scrollbar::Scrollbar::track_thickness(ds.scale)
        } else {
            0.0
        };
        let content_h = rows as f32 * row_h + bar_room;
        let view_h = body.size.height - px(10.0);
        self.scroll_y = self.scroll_y.clamp(0.0, (content_h - view_h).max(0.0));
        let first = (self.scroll_y / row_h.max(1.0)).floor().max(0.0) as u64;
        let last = (first + span).min(rows);
        self.request_visible(body.size.height);
        self.measure_columns(text, ds, first, last);
        let edges: Vec<f32> = self.layout.edges().iter().map(|e| e * ds.scale).collect();
        let content_w = edges.last().copied().unwrap_or(0.0);
        let room = body.size.width - gutter;
        self.scroll_x = self.scroll_x.clamp(0.0, (content_w - room + px(12.0)).max(0.0));
        let x0 = body.min_x() + gutter - self.scroll_x;
        let visible = self.layout.visible();
        let columns = self.source.columns().to_vec();

        // The rows.
        let cells = Rect::new(body.min_x() + gutter, body.min_y(), room, body.size.height);
        let layer = scene.push_layer(Some(cells));
        let cell_font = ds.ui_font(-0.5);
        let null_font = ds.ui_font(-2.0).with_italic(true);
        let metrics = text.metrics(&cell_font);
        let mut buffer = String::new();
        let selected_fill = p.accent.with_alpha(0.16);
        let find_fill = p.search_match;
        let current_find = self.find.matches.get(self.find.current).copied();
        let default_label = i18n::t("grid.default");
        for i in first..last {
            let row = self.row_at(i);
            let top = body.min_y() + i as f32 * row_h - self.scroll_y;
            let line = Rect::new(cells.min_x(), top, cells.size.width, row_h);
            crate::table::stripe(layer, ds, line, i as usize);
            let state = self.source.row_state(row);
            let baseline = ui_kit::centered_baseline(top, row_h, &metrics);
            for (pos, &col) in visible.iter().enumerate() {
                let (left, right) = (x0 + edges[pos], x0 + edges[pos + 1]);
                if right < cells.min_x() || left > cells.max_x() {
                    continue;
                }
                let cell = Rect::new(left, top, right - left, row_h);
                let at = CellPos::new(i, pos);
                let mark = self.source.mark(row, col);
                match mark {
                    CellMark::Modified => {
                        layer.quad(Quad::filled(cell, p.vcs_modified.with_alpha(0.22)))
                    }
                    CellMark::Inserted => {
                        layer.quad(Quad::filled(cell, p.vcs_added.with_alpha(0.16)))
                    }
                    CellMark::Deleted => layer.quad(Quad::filled(cell, p.danger.with_alpha(0.12))),
                    CellMark::None => layer,
                };
                if self.selection.contains(at) {
                    layer.quad(Quad::filled(cell, selected_fill));
                }
                if self.find.open && self.find.matches.binary_search(&at).is_ok() {
                    let fill =
                        if current_find == Some(at) { p.search_match_current } else { find_fill };
                    layer.quad(Quad::filled(cell, fill));
                }
                let pad = px(CELL_PAD);
                let room = (cell.size.width - 2.0 * pad).max(0.0);
                match state {
                    RowState::Ready => {
                        let value = self.source.cell(row, col);
                        if value.is_null() {
                            let nb =
                                ui_kit::centered_baseline(top, row_h, &text.metrics(&null_font));
                            let align = columns[col].align();
                            let label = if self.source.is_default(row, col) {
                                default_label.as_str()
                            } else {
                                "NULL"
                            };
                            draw_fitted(
                                layer,
                                text,
                                label,
                                &null_font,
                                (left + pad, room, nb),
                                p.text_muted.with_alpha(0.6),
                                align,
                            );
                        } else {
                            cell_line(&mut buffer, &value);
                            let color = match value {
                                CellRef::Bytes(_) => p.text_muted,
                                _ => p.text,
                            };
                            // An icon of the source's before the text (a level's), the text
                            // after it.
                            let mut start = left + pad;
                            let mut room = room;
                            if let Some((name, tone)) = self.source.cell_icon(row, col)
                                && let Some(id) = icons.id(name)
                            {
                                let size = px(CELL_ICON);
                                let at = Rect::new(
                                    start,
                                    (top + (row_h - size) / 2.0).round(),
                                    size,
                                    size,
                                );
                                ui_kit::draw_icon(layer, icons, id, at, tone.color(p));
                                start += size + px(CELL_ICON_GAP);
                                room = (room - size - px(CELL_ICON_GAP)).max(0.0);
                            }
                            draw_fitted(
                                layer,
                                text,
                                &buffer,
                                &cell_font,
                                (start, room, baseline),
                                color,
                                columns[col].align(),
                            );
                        }
                    }
                    RowState::Pending => {
                        // A placeholder bar while the page comes.
                        let w = (room * (0.35 + ((row as usize * 7 + pos * 13) % 5) as f32 * 0.1))
                            .min(room);
                        let bar = Rect::new(left + pad, top + row_h / 2.0 - px(4.0), w, px(8.0));
                        layer.quad(Quad::filled(bar, p.text.with_alpha(0.06)).rounded(px(4.0)));
                    }
                    RowState::Failed | RowState::Missing => {}
                }
            }
        }
        // The columns' separators.
        crate::table::column_lines(
            layer,
            ds,
            (0..visible.len()).map(|pos| x0 + edges[pos + 1]),
            body.min_y(),
            (content_h - self.scroll_y).min(body.size.height).max(0.0),
            cells,
        );
        // Rows to be deleted: struck through.
        for i in first..last {
            let row = self.row_at(i);
            if visible.first().is_some_and(|&c| self.source.mark(row, c) == CellMark::Deleted) {
                let y = (body.min_y() + i as f32 * row_h - self.scroll_y + row_h / 2.0).round();
                let line = Rect::new(
                    cells.min_x(),
                    y,
                    (x0 + content_w - cells.min_x()).min(cells.size.width),
                    ds.line(),
                );
                layer.quad(Quad::filled(line, p.danger.with_alpha(0.8)));
            }
        }
        // The cursor's cell outlined.
        if let Some(cursor) = self.selection.cursor()
            && cursor.row >= first
            && cursor.row < last
            && cursor.col < visible.len()
        {
            let i = cursor.row;
            let top = body.min_y() + i as f32 * row_h - self.scroll_y;
            let r = Rect::new(
                x0 + edges[cursor.col],
                top,
                edges[cursor.col + 1] - edges[cursor.col],
                row_h,
            );
            layer.quad(Quad::filled(r, Rgba::TRANSPARENT).bordered(px(1.5).max(1.0), p.accent));
        }
        // The cell edited in place, over its cell (on top of the rows).
        if let Some(editor) = &self.editor
            && editor.place >= first
            && editor.place < last
            && let Some(pos) = visible.iter().position(|&c| c == editor.col)
        {
            let top = body.min_y() + editor.place as f32 * row_h - self.scroll_y;
            let r = Rect::new(
                x0 + edges[pos],
                top,
                (edges[pos + 1] - edges[pos]).max(px(120.0)),
                row_h,
            );
            let layer = scene.push_layer(Some(cells));
            let mut style = field_style(ds, editor.error.is_some());
            style.font = cell_font.clone();
            style.background = p.popup_background;
            style.radius = px(3.0);
            style.padding = px(CELL_PAD) - ds.line();
            editor.input.paint(layer, text, r, true, "", &style);
            if let Some(error) = &editor.error {
                let font = ds.ui_font(-2.0);
                let line = ds::ellipsize(text, error, &font, cells.size.width - px(20.0));
                let shaped = text.shape_line(&line, &font);
                let tip = Rect::new(r.min_x(), r.max_y(), shaped.width + px(12.0), px(20.0));
                layer.quad(Quad::filled(tip, p.danger).rounded(px(radius::SM)));
                draw_text(
                    layer,
                    &shaped,
                    tip.min_x() + px(6.0),
                    ds::optical_baseline(tip.min_y(), tip.size.height, &font),
                    Rgba::WHITE,
                );
            }
        }

        // The row numbers.
        let numbers = Rect::new(body.min_x(), body.min_y(), gutter, body.size.height);
        let layer = scene.push_layer(Some(numbers));
        layer.quad(Quad::filled(numbers, p.text.with_alpha(0.02)));
        let number_font = ds.code_font(-2.0);
        let nm = text.metrics(&number_font);
        for i in first..last {
            let row = self.row_at(i);
            let top = body.min_y() + i as f32 * row_h - self.scroll_y;
            let selected = self.selection.has_row(i);
            if selected {
                layer.quad(Quad::filled(
                    Rect::new(numbers.min_x(), top, gutter, row_h),
                    p.accent.with_alpha(0.10),
                ));
            }
            let label = if i < self.page_rows() {
                grouped(row + 1)
            } else {
                format!("+{}", i - self.page_rows() + 1)
            };
            let shaped = text.shape_line(&label, &number_font);
            let color = if i >= self.page_rows() {
                p.vcs_added
            } else if selected {
                p.text
            } else {
                p.gutter_text
            };
            // A pending change on the row: a bar of its color by the number.
            let mark =
                visible.iter().map(|&c| self.source.mark(row, c)).find(|m| *m != CellMark::None);
            if let Some(mark) = mark {
                let color = match mark {
                    CellMark::Inserted => p.vcs_added,
                    CellMark::Deleted => p.danger,
                    _ => p.vcs_modified,
                };
                layer.quad(
                    Quad::filled(
                        Rect::new(
                            numbers.min_x() + px(3.0),
                            top + px(5.0),
                            px(3.0),
                            row_h - px(10.0),
                        ),
                        color,
                    )
                    .rounded(px(1.5)),
                );
            }
            draw_text(
                layer,
                &shaped,
                (numbers.max_x() - px(10.0) - shaped.width).round(),
                ui_kit::centered_baseline(top, row_h, &nm),
                color,
            );
        }
        ds::divider_vertical(layer, ds, numbers.max_x() - ds.line(), body.min_y(), body.max_y());

        // The headers.
        let layer = scene.push_layer(Some(header));
        crate::table::header_band(layer, ds, header);
        let corner = Rect::new(header.min_x(), header.min_y(), gutter, header.size.height);
        self.hits.push((corner, Hit::Corner));
        let layer = scene.push_layer(Some(Rect::new(
            header.min_x() + gutter,
            header.min_y(),
            room,
            header.size.height,
        )));
        let name_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let type_font = ds.ui_font(-2.0);
        let sort = self.query.sort.clone();
        for (pos, &col) in visible.iter().enumerate() {
            let (left, right) = (x0 + edges[pos], x0 + edges[pos + 1]);
            if right < cells.min_x() || left > cells.max_x() {
                continue;
            }
            let c = &columns[col];
            let r = Rect::new(left, header.min_y(), right - left, header.size.height);
            if self.hover_header == Some(pos) {
                layer.quad(Quad::filled(r, p.text.with_alpha(0.04)));
            }
            let mut x = left + px(CELL_PAD);
            let mut room = r.size.width - 2.0 * px(CELL_PAD);
            if c.key
                && let Some(id) = icons.id("key")
            {
                let s = px(12.0);
                let mark = Rect::new(x, (r.min_y() + px(9.0)).round(), s, s);
                ui_kit::draw_icon(layer, icons, id, mark, p.warning);
                self.marks.push((crate::diagram::inflate(mark, px(3.0)), Mark::Key));
                x += s + px(5.0);
                room -= s + px(5.0);
            }
            // The order: an arrow (and its place, with more than one column).
            let sorted = sort.iter().position(|k| k.column == c.name);
            if let Some(at) = sorted {
                let s = px(12.0);
                let icon = if sort[at].descending { "arrow-down" } else { "arrow-up" };
                let mut ax = right - px(CELL_PAD) - s;
                if sort.len() > 1 {
                    let n = text.shape_line(&(at + 1).to_string(), &type_font);
                    draw_text(
                        layer,
                        &n,
                        (right - px(CELL_PAD) - n.width).round(),
                        (r.min_y() + px(19.0)).round(),
                        p.accent,
                    );
                    ax -= n.width + px(2.0);
                }
                let mark = Rect::new(ax.round(), (r.min_y() + px(9.0)).round(), s, s);
                if let Some(id) = icons.id(icon) {
                    ui_kit::draw_icon(layer, icons, id, mark, p.accent);
                }
                let place =
                    Mark::Sort { descending: sort[at].descending, place: at + 1, of: sort.len() };
                let end = right - px(CELL_PAD);
                let area = Rect::new(mark.min_x(), mark.min_y(), end - mark.min_x(), s);
                self.marks.push((crate::diagram::inflate(area, px(3.0)), place));
                room -= right - ax;
            }
            draw_fitted(
                layer,
                text,
                &c.name,
                &name_font,
                (x, room.max(0.0), (r.min_y() + px(20.0)).round()),
                p.text,
                Align::Left,
            );
            let type_room = r.size.width - 2.0 * px(CELL_PAD);
            draw_fitted(
                layer,
                text,
                &c.type_name,
                &type_font,
                (left + px(CELL_PAD), type_room.max(0.0), (r.min_y() + px(36.0)).round()),
                p.text_muted,
                Align::Left,
            );
            crate::table::header_edge(layer, ds, right + ds.line() / 2.0, header, false);
        }
        // Where a header being moved would go.
        if let Some(to) = self.drop_at
            && let Some(&x) = edges.get(to)
        {
            let r = Rect::new(
                (x0 + x - px(1.0)).round(),
                header.min_y(),
                px(2.0),
                header.size.height + body.size.height,
            );
            scene.push_layer(Some(area)).quad(Quad::filled(r, p.accent));
        }
        let layer = scene.push_layer(Some(area));
        crate::table::header_rule(
            layer,
            ds,
            Rect::new(area.min_x(), header.min_y(), area.size.width, header.size.height),
        );

        // Nothing to show: why.
        if rows == 0 {
            let layer = scene.push_layer(Some(body));
            match self.source.status() {
                SourceStatus::Failed(why) => {
                    ds::empty_state(layer, text, ds, body, &i18n::t("grid.failed"), Some(&why))
                }
                SourceStatus::Loading => {}
                SourceStatus::Ready if self.query.filter.is_some() => ds::empty_state(
                    layer,
                    text,
                    ds,
                    body,
                    &i18n::t("grid.no_match"),
                    Some(&i18n::t("grid.no_match_detail")),
                ),
                SourceStatus::Ready => {
                    ds::empty_state(layer, text, ds, body, &i18n::t("grid.empty"), None)
                }
            }
        } else if let SourceStatus::Failed(why) = self.source.status()
            && (first..last).any(|i| self.source.row_state(self.row_at(i)) == RowState::Failed)
        {
            let layer = scene.push_layer(Some(body));
            let font = ds.ui_font(-1.0);
            let line = ds::ellipsize(text, &why, &font, body.size.width - px(60.0));
            let shaped = text.shape_line(&line, &font);
            let r = Rect::new(
                body.min_x() + (body.size.width - shaped.width - px(24.0)) / 2.0,
                body.min_y() + px(16.0),
                shaped.width + px(24.0),
                px(28.0),
            );
            ds::surface_with(layer, ds, r, ds::Elevation::Low, radius::MD, p.popup_background);
            draw_text(
                layer,
                &shaped,
                r.min_x() + px(12.0),
                ds::optical_baseline(r.min_y(), r.size.height, &font),
                p.danger,
            );
        }

        // The scrollbars.
        self.vbar.paint(scene, p, ds.scale, body, content_h + px(10.0), self.scroll_y);
        if content_w > room {
            let thickness = crate::scrollbar::Scrollbar::track_thickness(ds.scale);
            let track =
                Rect::new(cells.min_x(), body.max_y() - thickness, room - thickness, thickness);
            let bar = self.hbar.get_or_insert_with(|| horizontal_bar(p));
            bar.set_style(crate::scrollbar::ScrollbarStyle::from_palette(p));
            bar.set_track(track, ds.scale);
            bar.set_expansion(if bar.is_hovered() || bar.is_dragging() { 1.0 } else { 0.0 });
            self.hbar_metrics = ScrollMetrics {
                content: content_w + px(12.0),
                viewport: room,
                offset: self.scroll_x,
            };
            bar.paint(scene, self.hbar_metrics);
        } else {
            self.hbar = None;
        }
    }

    /// One row transposed: each column's name and value on a line of its own.
    pub(super) fn paint_record(
        &mut self,
        scene: &mut render_api::Scene,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        area: Rect,
    ) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let row_h = self.geometry.row_h;
        let header = Rect::new(area.min_x(), area.min_y(), area.size.width, px(HEADER_H));
        let body = Rect::new(
            area.min_x(),
            header.max_y(),
            area.size.width,
            (area.max_y() - header.max_y()).max(0.0),
        );
        let rows = self.shown_rows();
        self.geometry.header = Rect::default();
        self.geometry.body = body;
        self.geometry.gutter = 0.0;
        self.geometry.rows = rows;
        self.request_visible(body.size.height);
        let layer = scene.push_layer(Some(area));
        layer.quad(Quad::filled(header, p.text.with_alpha(0.03)));
        ds::divider(layer, ds, area.min_x(), area.max_x(), header.max_y() - ds.line());
        let Some(place) = self.record_row() else {
            ds::empty_state(layer, text, ds, body, &i18n::t("grid.empty"), None);
            return;
        };
        let row = self.row_at(place);
        let mid = header.min_y() + header.size.height / 2.0;
        let mut x = header.min_x() + px(10.0);
        let back = place > 0;
        let r = grid_button(
            layer,
            text,
            icons,
            ds,
            (x, mid),
            ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon("chevron-left")
                .enabled(back)
                .hovered(self.hover == Some(Hit::RecordPrev)),
        );
        if back {
            self.hits.push((r, Hit::RecordPrev));
        }
        x = r.max_x() + px(4.0);
        let font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let label = if place < self.page_rows() {
            i18n::tr!("grid.record", row = grouped(row + 1))
        } else {
            i18n::t("grid.record_new")
        };
        let shaped = text.shape_line(&label, &font);
        draw_text(
            layer,
            &shaped,
            x + px(4.0),
            ds::optical_baseline(header.min_y(), header.size.height, &font),
            p.text,
        );
        x += shaped.width + px(12.0);
        let forward = place + 1 < rows;
        let r = grid_button(
            layer,
            text,
            icons,
            ds,
            (x, mid),
            ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon("chevron-right")
                .enabled(forward)
                .hovered(self.hover == Some(Hit::RecordNext)),
        );
        if forward {
            self.hits.push((r, Hit::RecordNext));
        }

        let visible = self.layout.visible();
        let columns = self.source.columns().to_vec();
        let content_h = visible.len() as f32 * row_h;
        self.scroll_y =
            self.scroll_y.clamp(0.0, (content_h - body.size.height + px(10.0)).max(0.0));
        let name_w = (body.size.width * 0.3).clamp(px(140.0), px(300.0));
        let layer = scene.push_layer(Some(body));
        let name_font = ds.ui_font(-0.5).with_weight(Font::SEMIBOLD);
        let cell_font = ds.ui_font(-0.5);
        let null_font = ds.ui_font(-2.0).with_italic(true);
        let metrics = text.metrics(&cell_font);
        let state = self.source.row_state(row);
        let cursor = self.selection.cursor();
        let mut buffer = String::new();
        for (pos, &col) in visible.iter().enumerate() {
            let top = body.min_y() + pos as f32 * row_h - self.scroll_y;
            if top + row_h < body.min_y() || top > body.max_y() {
                continue;
            }
            let line = Rect::new(body.min_x(), top, body.size.width, row_h);
            if cursor == Some(CellPos::new(place, pos)) {
                layer.quad(Quad::filled(line, p.accent.with_alpha(0.14)));
            } else if pos % 2 == 1 {
                layer.quad(Quad::filled(line, p.text.with_alpha(0.018)));
            }
            let c = &columns[col];
            let baseline = ui_kit::centered_baseline(top, row_h, &metrics);
            let mut nx = body.min_x() + px(16.0);
            if c.key
                && let Some(id) = icons.id("key")
            {
                let s = px(12.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(nx, (top + (row_h - s) / 2.0).round(), s, s),
                    p.warning,
                );
                nx += s + px(6.0);
            }
            draw_fitted(
                layer,
                text,
                &c.name,
                &name_font,
                (nx, name_w - (nx - body.min_x()) - px(8.0), baseline),
                p.text,
                Align::Left,
            );
            let vx = body.min_x() + name_w + px(CELL_PAD);
            let room = body.size.width - name_w - px(2.0 * CELL_PAD + 12.0);
            if state == RowState::Ready {
                let value = self.source.cell(row, col);
                if value.is_null() {
                    let nb = ui_kit::centered_baseline(top, row_h, &text.metrics(&null_font));
                    draw_fitted(
                        layer,
                        text,
                        "NULL",
                        &null_font,
                        (vx, room, nb),
                        p.text_muted.with_alpha(0.6),
                        Align::Left,
                    );
                } else {
                    cell_line(&mut buffer, &value);
                    draw_fitted(
                        layer,
                        text,
                        &buffer,
                        &cell_font,
                        (vx, room, baseline),
                        p.text,
                        Align::Left,
                    );
                }
            }
        }
        layer.quad(Quad::filled(
            Rect::new(
                body.min_x() + name_w,
                body.min_y(),
                ds.line(),
                content_h.min(body.size.height),
            ),
            p.border.with_alpha(0.55),
        ));
        self.vbar.paint(scene, p, ds.scale, body, content_h + px(10.0), self.scroll_y);
    }
}
