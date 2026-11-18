//! Painting the graph: the file and code cards, their headers and rows, the callers, the toolbar,
//! the links between cards.

use super::*;

impl Canvas {
    /// The search field's caret: a thin bar at `x`, centered in `field`.
    pub(crate) fn search_caret(
        &self,
        x: f32,
        field: Rect,
        height: f32,
        thickness: f32,
        color: Rgba,
    ) -> ui_kit::caret::CaretPaint {
        ui_kit::caret::CaretPaint {
            id: (std::ptr::from_ref(self) as u64) ^ 1,
            cell: Rect::new(
                x,
                (field.min_y() + (field.size.height - height) / 2.0).round(),
                thickness,
                height,
            ),
            color,
            scale: self.scale,
            shape: Some(theming::caret::CaretShape::Bar),
            thickness: Some(thickness),
            steady: false,
        }
    }

    /// One of its actions ([`ACTIONS`]); `None`: not one.
    pub(crate) fn action(&mut self, id: &str) -> Option<bool> {
        let mode = match id {
            "zoom_in" => return Some(self.zoom_at(1.2, self.center())),
            "zoom_out" => return Some(self.zoom_at(1.0 / 1.2, self.center())),
            "fit" => {
                self.fit();
                return Some(true);
            }
            "functions" => Mode::Functions,
            "files" => Mode::Files,
            "code" => Mode::Code,
            _ => return None,
        };
        self.mode = mode;
        self.relayout();
        self.fit_pending = true;
        Some(true)
    }

    pub(crate) fn paint_graph(
        &mut self,
        graph: &DepGraph,
        layout: &Layout,
        ds: &Ctx<'_>,
        text: &mut TextSystem,
        scene: &mut render_api::Scene,
        icons: &icons::Icons,
    ) {
        let p = ds.palette;
        let k = self.zoom * self.scale;
        // Edges, below the cards: their dots are computed once per shape and zoom (in
        // zoomed canvas space), then only translated; edges outside the view are skipped.
        let scale = self.scale;
        let shapes: Vec<_> = layout
            .edges
            .iter()
            .map(|edge| {
                let width = (if edge.strong { 2.2 } else { 1.6 } * scale).max(1.0);
                [
                    (edge.from.x * k).to_bits(),
                    (edge.from.y * k).to_bits(),
                    (edge.to.x * k).to_bits(),
                    (edge.to.y * k).to_bits(),
                    width.to_bits(),
                    u32::from(edge.dashed),
                ]
            })
            .collect();
        crate::update_dots(&mut self.edge_dots, &shapes);
        let dots = self.edge_dots.iter().map(|(_, d)| d);
        let origin =
            Point::new(self.bounds.min_x() - self.pan.x * k, self.bounds.min_y() - self.pan.y * k);
        let edges = scene.push_layer(Some(self.bounds));
        for (edge, (bbox, points)) in layout.edges.iter().zip(dots) {
            let screen = Rect::new(
                bbox.min_x() + origin.x,
                bbox.min_y() + origin.y,
                bbox.size.width,
                bbox.size.height,
            );
            if screen.intersect(&self.bounds).is_none() {
                continue;
            }
            let color = if edge.strong { p.accent } else { p.accent.with_alpha(0.38) };
            let width = (if edge.strong { 2.2 } else { 1.6 } * scale).max(1.0);
            for q in points {
                let (x, y) = (q.x + origin.x, q.y + origin.y);
                if !self.bounds.contains(Point::new(x, y)) {
                    continue;
                }
                edges.quad(
                    Quad::filled(Rect::new(x - width / 2.0, y - width / 2.0, width, width), color)
                        .rounded(width / 2.0),
                );
            }
        }
        // Cards.
        for card in &layout.cards {
            let r = self.rect_to_screen(card.rect);
            if r.intersect(&self.bounds).is_none() {
                continue;
            }
            let layer = scene.push_layer(Some(self.bounds));
            let selected = card.selected;
            let radius = (12.0 * k).round();
            if selected {
                ds::surface(layer, ds, r, ds::Elevation::Medium, 12.0 * self.zoom);
                layer.quad(
                    Quad::filled(r, Rgba::TRANSPARENT)
                        .rounded(radius)
                        .bordered((2.0 * self.scale).round(), p.accent),
                );
            } else {
                layer.quad(
                    Quad::filled(r, p.editor_background)
                        .rounded(radius)
                        .bordered(ds.line(), p.card_border),
                );
            }
            self.hits.push((r, Hit::Card(card.kind)));
            let hovered_hit = self.hover.clone();
            let content = scene.push_layer(Some(r.intersect(&self.bounds).unwrap_or(r)));
            match &card.kind {
                CardKind::File { file, focus } => {
                    let f = &graph.files[*file];
                    let pen = Pen { layer: content, text, ds };
                    self.paint_file_card(pen, card, r, f, *focus, hovered_hit.as_ref());
                    let hit = if *focus { Hit::FocusHeader } else { Hit::File(*file) };
                    self.nav_button(content, icons, ds, r, hit);
                }
                CardKind::Code { symbol } => {
                    let f = &graph.files[graph.focus];
                    let pen = Pen { layer: content, text, ds };
                    self.paint_code_card(pen, card, r, f, *symbol, graph);
                    if card.selected {
                        let line = f.symbols[*symbol].line;
                        self.nav_button(content, icons, ds, r, Hit::Open { line });
                    }
                }
            }
        }
    }

    /// A file's card: its header, then its rows (a preview, a summary or its symbols).
    pub(crate) fn paint_file_card(
        &mut self,
        pen: Pen<'_, '_>,
        card: &Card,
        r: Rect,
        file: &code_intelligence::GraphFile,
        focus: bool,
        hover: Option<&Hit>,
    ) {
        let Pen { layer, text, ds } = pen;
        self.paint_file_header(Pen { layer, text, ds }, r, file);
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let lines = CardLines {
            r,
            x0: r.min_x() + z(18.0),
            row_h: z(layout::ROW),
            code_font: Font::new(p.code_font.clone(), font_px(13.0, k)),
        };
        let mut y = r.min_y() + z(layout::HEADER) + z(10.0);
        let (x0, row_h, code_font) = (lines.x0, lines.row_h, &lines.code_font);
        match &card.rows {
            layout::Rows::Preview(n) => {
                for source in file.preview.iter().take(*n) {
                    ui_components::draw_code(
                        layer,
                        text,
                        ui_components::CodeLine {
                            source,
                            extension: file
                                .path
                                .extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or_default(),
                            font: code_font,
                            x: x0,
                            baseline: ds::optical_baseline(y, row_h, code_font),
                            palette: p,
                            max_x: r.max_x() - z(12.0),
                        },
                    );
                    y += row_h;
                }
            }
            layout::Rows::Summary => {
                let small = Font::new(p.ui_font.clone(), font_px(12.5, k));
                let mut summary = tr!("canvas.lines", count = i64::from(file.lines));
                if file.unconnected {
                    summary.push_str(&format!(" · {}", t("canvas.unconnected")));
                }
                let shaped = text.shape_line(&summary, &small);
                draw_text(layer, &shaped, x0, ds::optical_baseline(y, row_h, &small), p.text_muted);
            }
            layout::Rows::Symbols(rows) => {
                for row in rows {
                    let s = &file.symbols[row.symbol];
                    if row.title {
                        let keyword = match s.kind {
                            code_intelligence::SymbolKind::Interface => "interface",
                            code_intelligence::SymbolKind::Trait => "trait",
                            code_intelligence::SymbolKind::Enum => "enum",
                            _ => "class",
                        };
                        let kw = text.shape_line(keyword, code_font);
                        let base = ds::optical_baseline(y, row_h, code_font);
                        draw_text(layer, &kw, x0, base, p.terminal_ansi[5]);
                        let name = text.shape_line(&s.name, code_font);
                        draw_text(layer, &name, x0 + kw.width + z(8.0), base, p.terminal_ansi[3]);
                        y += row_h;
                        continue;
                    }
                    let selected = self.selected == Some(row.symbol) && focus;
                    let hovered = hover == Some(&Hit::Symbol(row.symbol));
                    let pen = Pen { layer, text, ds };
                    let rr = self.paint_symbol_row(pen, &lines, s, y, selected, hovered);
                    if focus {
                        self.hits.push((rr, Hit::Symbol(row.symbol)));
                    }
                    y += row_h;
                }
            }
        }
    }

    /// A file card's header: its language badge, its name and folder, the line under them.
    pub(crate) fn paint_file_header(
        &self,
        pen: Pen<'_, '_>,
        r: Rect,
        file: &code_intelligence::GraphFile,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let header_h = z(layout::HEADER);
        let badge_font =
            Font::new(p.code_font.clone(), font_px(10.5, k)).with_weight(Font::SEMIBOLD);
        let name_font = Font::new(p.ui_font.clone(), font_px(14.0, k)).with_weight(Font::SEMIBOLD);
        let badge = language_badge(&file.language);
        let shaped = text.shape_line(badge, &badge_font);
        let bx = r.min_x() + z(16.0);
        let bh = z(18.0);
        let by = r.min_y() + (header_h - bh) / 2.0;
        let br = Rect::new(bx, by.round(), shaped.width + z(10.0), bh);
        let badge_color = if badge == "JS" { Rgba::rgb(0xb7791f) } else { p.accent };
        layer.quad(Quad::filled(br, badge_color.with_alpha(0.10)).rounded(z(4.0)));
        draw_text(
            layer,
            &shaped,
            br.min_x() + z(5.0),
            ds::optical_baseline(br.min_y(), bh, &badge_font),
            badge_color,
        );
        let name = file
            .path
            .file_name()
            .map_or_else(|| file.path.display().to_string(), |n| n.to_string_lossy().into_owned());
        let shaped = text.shape_line(&name, &name_font);
        draw_text(
            layer,
            &shaped,
            br.max_x() + z(10.0),
            ds::optical_baseline(r.min_y(), header_h, &name_font),
            p.text,
        );
        if let Some(dir) = file.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            let small = Font::new(p.ui_font.clone(), font_px(11.5, k));
            let x = br.max_x() + z(10.0) + shaped.width + z(8.0);
            let room = r.max_x() - x - z(12.0);
            if room > z(30.0) {
                let dir = ds::ellipsize(text, &dir.display().to_string(), &small, room);
                let d = text.shape_line(&dir, &small);
                draw_text(
                    layer,
                    &d,
                    x,
                    ds::optical_baseline(r.min_y(), header_h, &small),
                    p.text_muted,
                );
            }
        }
        layer.quad(Quad::filled(
            Rect::new(r.min_x(), r.min_y() + header_h, r.size.width, ds.line()),
            p.card_border,
        ));
    }

    /// A function's row in a file card at `y`: `ƒ`, its signature, `private`. Returns its
    /// rectangle (clicked to select it).
    pub(crate) fn paint_symbol_row(
        &self,
        pen: Pen<'_, '_>,
        lines: &CardLines,
        s: &code_intelligence::GraphSymbol,
        y: f32,
        selected: bool,
        hovered: bool,
    ) -> Rect {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let (r, code_font) = (lines.r, &lines.code_font);
        let rr = Rect::new(
            r.min_x() + z(12.0),
            y + z(1.0),
            r.size.width - z(24.0),
            lines.row_h - z(4.0),
        );
        let fill = if selected {
            p.accent.with_alpha(0.12)
        } else if hovered {
            p.text.with_alpha(0.06)
        } else {
            p.text.with_alpha(0.035)
        };
        layer.quad(Quad::filled(rr, fill).rounded(z(6.0)));
        let base = ds::optical_baseline(rr.min_y(), rr.size.height, code_font);
        let glyph = text.shape_line("ƒ", code_font);
        let color = if selected { p.accent } else { p.text_muted };
        draw_text(layer, &glyph, rr.min_x() + z(10.0), base, color);
        let label = ds::ellipsize(text, &s.signature, code_font, rr.size.width - z(110.0));
        let shaped = text.shape_line(&label, code_font);
        let tx = rr.min_x() + z(28.0);
        draw_text(layer, &shaped, tx, base, if selected { p.accent } else { p.text });
        if s.private {
            let tag = text.shape_line("private", code_font);
            draw_text(layer, &tag, tx + shaped.width + z(10.0), base, p.text_muted);
        }
        rr
    }

    /// A function's card: on the selected one a header, line numbers, the on-card editor and
    /// where it's called from.
    pub(crate) fn paint_code_card(
        &mut self,
        pen: Pen<'_, '_>,
        card: &Card,
        r: Rect,
        file: &code_intelligence::GraphFile,
        symbol: usize,
        graph: &DepGraph,
    ) {
        let Pen { layer, text, ds } = pen;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let selected = card.selected;
        let mut y = r.min_y();
        if selected {
            self.paint_code_header(Pen { layer, text, ds }, r, file, symbol);
            y += z(layout::HEADER) + z(10.0);
        } else {
            y += z(10.0);
        }
        let code = self.paint_code_lines(Pen { layer, text, ds }, r, file, symbol, selected, y);
        if selected {
            let header = Rect::new(r.min_x(), r.min_y(), r.size.width, z(layout::HEADER));
            let code_area = Rect::new(
                r.min_x(),
                header.max_y(),
                r.size.width,
                (code.bottom + z(8.0) - header.max_y()).max(0.0),
            );
            self.hits.push((code_area, Hit::Edit));
            self.paint_card_editor(Pen { layer, text, ds }, r, &code);
            self.paint_callers(Pen { layer, text, ds }, r, symbol, graph);
        }
    }

    /// The selected function's header: `Car › stop()` and the location (`class.php:8–13`).
    pub(crate) fn paint_code_header(
        &self,
        pen: Pen<'_, '_>,
        r: Rect,
        file: &code_intelligence::GraphFile,
        symbol: usize,
    ) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let s = &file.symbols[symbol];
        let header_h = z(layout::HEADER);
        let header = Rect::new(r.min_x(), r.min_y(), r.size.width, header_h);
        let top = scale_rect_top(header, z(12.0));
        layer.quad(Quad::filled(top, p.accent.with_alpha(0.06)).rounded(z(12.0)));
        let title_font = Font::new(p.ui_font.clone(), font_px(13.0, k)).with_weight(Font::SEMIBOLD);
        let title = match &s.container {
            Some(c) => format!("{c} › {}()", s.name),
            None => format!("{}()", s.name),
        };
        let shaped = text.shape_line(&title, &title_font);
        draw_text(
            layer,
            &shaped,
            r.min_x() + z(18.0),
            ds::optical_baseline(r.min_y(), header_h, &title_font),
            p.accent,
        );
        let small = Font::new(p.ui_font.clone(), font_px(12.0, k));
        let name =
            file.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let editing = self.editing.as_ref().is_some_and(|e| e.symbol == symbol);
        let mut place = format!("{name}:{}–{}", s.line + 1, s.end_line + 1);
        if editing {
            place = format!("{place} · {}", t("canvas.editing"));
        }
        let shaped = text.shape_line(&place, &small);
        draw_text(
            layer,
            &shaped,
            r.max_x() - z(12.0 + NAV + 10.0) - shaped.width,
            ds::optical_baseline(r.min_y(), header_h, &small),
            p.text_muted,
        );
    }

    /// A function's code from `top` (with line numbers, on the selected one; the text being
    /// edited, if it is). Returns where it was drawn.
    pub(crate) fn paint_code_lines(
        &self,
        pen: Pen<'_, '_>,
        r: Rect,
        file: &code_intelligence::GraphFile,
        symbol: usize,
        selected: bool,
        top: f32,
    ) -> CodeLines {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let s = &file.symbols[symbol];
        let code_font = Font::new(p.code_font.clone(), font_px(13.0, k));
        let row_h = z(layout::ROW);
        let mut y = top;
        let gutter = if selected { z(46.0) } else { 0.0 };
        let number_font = Font::new(p.code_font.clone(), font_px(12.0, k));
        let editing = selected && self.editing.as_ref().is_some_and(|e| e.symbol == symbol);
        let edited: Vec<String> = match (&self.editing, editing) {
            (Some(edit), true) => {
                let buffer = edit.doc.buffer();
                (0..buffer.line_count())
                    .map(|l| buffer.slice(buffer.line_start(l)..buffer.line_end(l)))
                    .collect()
            }
            _ => Vec::new(),
        };
        let lines: Vec<&String> = if editing {
            edited.iter().collect()
        } else if selected {
            s.code.iter().take(layout::MAX_CODE).collect()
        } else {
            s.code.iter().filter(|l| !l.trim_start().starts_with(['*', '/'])).take(3).collect()
        };
        // When editing, the text stays exact (common indentation not stripped).
        let indent = if editing {
            0
        } else {
            lines
                .iter()
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0)
        };
        let code_x = r.min_x() + z(18.0) + gutter;
        let first_line = s.line;
        for (i, source) in lines.iter().enumerate() {
            let base = ds::optical_baseline(y, row_h, &code_font);
            if selected {
                let n = text.shape_line(&(first_line + i as u32 + 1).to_string(), &number_font);
                draw_text(layer, &n, r.min_x() + gutter - z(10.0) - n.width, base, p.gutter_text);
            }
            let source = source.get(indent.min(source.len())..).unwrap_or(source);
            let source = source.replace('\t', "    ");
            ui_components::draw_code(
                layer,
                text,
                ui_components::CodeLine {
                    source: &source,
                    extension: file.path.extension().and_then(|e| e.to_str()).unwrap_or_default(),
                    font: &code_font,
                    x: r.min_x() + z(18.0) + gutter,
                    baseline: base,
                    palette: p,
                    max_x: r.max_x() - z(12.0),
                },
            );
            y += row_h;
        }
        CodeLines { top, x: code_x, bottom: y, row_h, font: code_font, editing }
    }

    /// The on-card editor, while the function is edited: subtle background, the cursor.
    pub(crate) fn paint_card_editor(&mut self, pen: Pen<'_, '_>, r: Rect, code: &CodeLines) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let row_h = code.row_h;
        let cell = text.shape_line("M", &code.font).width.max(1.0);
        if code.editing
            && let Some(edit) = &mut self.editing
        {
            edit.area = Some(edit::EditArea { x0: code.x, top: code.top, row_h, cell });
            let (line, column) = edit.caret();
            let cy = code.top + line as f32 * row_h;
            layer.quad(Quad::filled(
                Rect::new(r.min_x() + ds.line(), cy, r.size.width - 2.0 * ds.line(), row_h),
                p.current_line,
            ));
            let cx = (code.x + column as f32 * cell).round();
            let caret_h = (row_h * 0.75).round();
            ui_kit::caret::paint(
                layer,
                ui_kit::caret::CaretPaint {
                    id: std::ptr::from_ref(self) as u64,
                    cell: Rect::new(cx, (cy + (row_h - caret_h) / 2.0).round(), cell, caret_h),
                    color: p.caret,
                    scale: self.scale,
                    shape: None,
                    thickness: None,
                    steady: false,
                },
            );
        }
    }

    /// The selected function's footer: where it is called from (as many chips as fit).
    pub(crate) fn paint_callers(&self, pen: Pen<'_, '_>, r: Rect, symbol: usize, graph: &DepGraph) {
        let Pen { layer, text, ds } = pen;
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let callers: Vec<String> = graph
            .edges
            .iter()
            .filter(|e| e.to == graph.focus && e.symbol == Some(symbol) && e.from != graph.focus)
            .map(|e| {
                let name = graph.files[e.from]
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                tr!("canvas.called_from", place = format!("{name}:{}", e.line + 1))
            })
            .take(4)
            .collect();
        if callers.is_empty() {
            return;
        }
        let foot_y = r.max_y() - z(layout::FOOTER);
        layer.quad(Quad::filled(
            Rect::new(r.min_x(), foot_y, r.size.width, ds.line()),
            p.card_border,
        ));
        let chip_font = Font::new(p.ui_font.clone(), font_px(12.0, k));
        let mut x = r.min_x() + z(16.0);
        let h = z(24.0);
        let cy = foot_y + (z(layout::FOOTER) - h) / 2.0;
        for caller in callers {
            let shaped = text.shape_line(&caller, &chip_font);
            let w = shaped.width + z(20.0);
            if x + w > r.max_x() - z(12.0) {
                break;
            }
            let chip = Rect::new(x, cy.round(), w, h);
            layer.quad(Quad::filled(chip, p.text.with_alpha(0.06)).rounded(h / 2.0));
            draw_text(
                layer,
                &shaped,
                x + z(10.0),
                ds::optical_baseline(chip.min_y(), h, &chip_font),
                p.text_muted,
            );
            x += w + z(8.0);
        }
    }

    /// The ↗ button on the right of a card header (opens / centers on the card); the rest
    /// of the card is for dragging.
    pub(crate) fn nav_button(
        &mut self,
        layer: &mut Layer,
        icons: &icons::Icons,
        ds: &Ctx<'_>,
        r: Rect,
        hit: Hit,
    ) {
        let p = ds.palette;
        let k = self.zoom * self.scale;
        let z = |v: f32| (v * k).round();
        let size = z(NAV);
        let header_h = z(layout::HEADER);
        let b = Rect::new(
            r.max_x() - z(12.0) - size,
            (r.min_y() + (header_h - size) / 2.0).round(),
            size,
            size,
        );
        let hovered = self.hover.as_ref() == Some(&hit);
        layer.quad(
            Quad::filled(b, if hovered { p.accent.with_alpha(0.14) } else { p.tab_hover })
                .rounded(z(7.0)),
        );
        if let Some(id) = icons.id("arrow-up-right") {
            let i = z(14.0);
            let ir = Rect::new(
                (b.min_x() + (size - i) / 2.0).round(),
                (b.min_y() + (size - i) / 2.0).round(),
                i,
                i,
            );
            let color = if hovered { p.accent } else { p.text_muted };
            ui_kit::draw_icon(layer, icons, id, ir, color);
        }
        self.hits.push((b, hit));
    }

    /// Bottom bar: modes and zoom, as a dark pill.
    pub(crate) fn paint_toolbar(&mut self, ds: &Ctx<'_>, text: &mut TextSystem, layer: &mut Layer) {
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let font = ds.ui_font(0.0);
        let modes = [
            (Mode::Functions, t("canvas.functions")),
            (Mode::Files, t("canvas.files")),
            (Mode::Code, t("canvas.code")),
        ];
        let shaped: Vec<_> = modes.iter().map(|(_, l)| text.shape_line(l, &font)).collect();
        let zoom_label = text.shape_line(&format!("{}%", (self.zoom * 100.0).round()), &font);
        let minus = text.shape_line("−", &font);
        let plus = text.shape_line("+", &font);
        let h = px(44.0);
        let seg_pad = px(16.0);
        let widths: Vec<f32> = shaped.iter().map(|s| s.width + 2.0 * seg_pad).collect();
        let zoom_w = px(30.0) + zoom_label.width + px(16.0) + px(30.0);
        let w = px(6.0) + widths.iter().sum::<f32>() + px(17.0) + zoom_w + px(6.0);
        let b = self.bounds;
        let bar = Rect::new(
            (b.min_x() + (b.size.width - w) / 2.0).round(),
            b.max_y() - h - px(18.0),
            w.round(),
            h,
        );
        let dark = Rgba::rgb(0x1f1f24);
        ds::surface(layer, ds, bar, ds::Elevation::Medium, 12.0);
        layer.quad(
            Quad::filled(bar, dark)
                .rounded(px(12.0))
                .bordered(ds.line(), Rgba::WHITE.with_alpha(0.08)),
        );
        let mut x = bar.min_x() + px(6.0);
        let inner_h = h - px(12.0);
        let top = bar.min_y() + px(6.0);
        for (i, (mode, _)) in modes.iter().enumerate() {
            let r = Rect::new(x, top, widths[i], inner_h);
            let active = *mode == self.mode;
            let hovered = self.hover == Some(Hit::Mode(*mode));
            if active {
                layer.quad(Quad::filled(r, p.accent).rounded(px(8.0)));
            } else if hovered {
                layer.quad(Quad::filled(r, Rgba::WHITE.with_alpha(0.08)).rounded(px(8.0)));
            }
            draw_text(
                layer,
                &shaped[i],
                x + seg_pad,
                ds::optical_baseline(top, inner_h, &font),
                Rgba::WHITE.with_alpha(if active { 1.0 } else { 0.85 }),
            );
            self.hits.push((r, Hit::Mode(*mode)));
            x += widths[i];
        }
        x += px(8.0);
        layer.quad(Quad::filled(
            Rect::new(x, top + px(8.0), ds.line(), inner_h - px(16.0)),
            Rgba::WHITE.with_alpha(0.18),
        ));
        x += px(9.0);
        let base = ds::optical_baseline(top, inner_h, &font);
        for (shaped, hit, width) in [
            (&minus, Hit::ZoomOut, px(30.0)),
            (&zoom_label, Hit::Fit, zoom_label.width + px(16.0)),
            (&plus, Hit::ZoomIn, px(30.0)),
        ] {
            let r = Rect::new(x, top, width, inner_h);
            if self.hover.as_ref() == Some(&hit) {
                layer.quad(Quad::filled(r, Rgba::WHITE.with_alpha(0.08)).rounded(px(8.0)));
            }
            draw_text(
                layer,
                shaped,
                (x + (width - shaped.width) / 2.0).round(),
                base,
                Rgba::WHITE.with_alpha(0.9),
            );
            self.hits.push((r, hit));
            x += width;
        }
    }
}

/// Top part of a rectangle, for the header background rounded only at the top.
pub(crate) fn scale_rect_top(header: Rect, radius: f32) -> Rect {
    Rect::new(header.min_x(), header.min_y(), header.size.width, header.size.height + radius)
}

pub(crate) fn language_badge(language: &str) -> &'static str {
    match language {
        "php" => "PHP",
        "javascript" | "jsx" => "JS",
        "typescript" | "tsx" => "TS",
        "css" | "scss" => "CSS",
        "html" | "twig" => "HTML",
        _ => "TXT",
    }
}

/// A font size at zoom `k`, in quarter-pixel steps: continuous zoom would otherwise
/// create a new font size (and fill the text caches) on every zoom frame.
pub(crate) fn font_px(size: f32, k: f32) -> f32 {
    ((size * k * 4.0).round() / 4.0).max(1.0)
}

/// The dots of a horizontal Bézier curve from `a` (a card's right side) to `b` (another
/// card's left side), evenly spaced along the curve's length (equal steps in `t` would
/// leave gaps on steep parts); `dashed`: in equal-length dashes. Points are in zoomed
/// canvas space (screen scale, no pan), so they survive panning and are computed once
/// per layout and zoom. Also returns their bounding box (for culling).
pub(crate) fn curve_dots(a: Point, b: Point, width: f32, dashed: bool) -> (Rect, Vec<Point>) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0);
    let (c1, c2) = (Point::new(a.x + dx, a.y), Point::new(b.x - dx, b.y));
    let at = |t: f32| {
        let u = 1.0 - t;
        Point::new(
            u * u * u * a.x + 3.0 * u * u * t * c1.x + 3.0 * u * t * t * c2.x + t * t * t * b.x,
            u * u * u * a.y + 3.0 * u * u * t * c1.y + 3.0 * u * t * t * c2.y + t * t * t * b.y,
        )
    };
    // The approximate length (control polygon) gives how many fine samples we take.
    let rough = (c1.x - a.x).hypot(c1.y - a.y)
        + (c2.x - c1.x).hypot(c2.y - c1.y)
        + (b.x - c2.x).hypot(b.y - c2.y);
    let samples = ((rough / 2.0).ceil() as usize).clamp(16, 8000);
    let spacing = (width * 0.4).max(0.5);
    let (dash, gap) = (width * 5.0, width * 3.5);
    let mut dots = Vec::new();
    let (mut lo, mut hi) = (a, a);
    let mut prev = a;
    let mut walked = 0.0f32;
    let mut next = 0.0f32;
    for i in 0..=samples {
        let p = at(i as f32 / samples as f32);
        let step = (p.x - prev.x).hypot(p.y - prev.y);
        // Points on the segment `prev → p`, every `spacing` along the curve.
        while next <= walked + step {
            let k = if step > 0.0 { (next - walked) / step } else { 0.0 };
            let q = Point::new(prev.x + (p.x - prev.x) * k, prev.y + (p.y - prev.y) * k);
            if !dashed || next % (dash + gap) < dash {
                lo = Point::new(lo.x.min(q.x), lo.y.min(q.y));
                hi = Point::new(hi.x.max(q.x), hi.y.max(q.y));
                dots.push(q);
            }
            next += spacing;
        }
        walked += step;
        prev = p;
    }
    let half = width / 2.0;
    let bbox = Rect::new(lo.x - half, lo.y - half, hi.x - lo.x + width, hi.y - lo.y + width);
    (bbox, dots)
}
