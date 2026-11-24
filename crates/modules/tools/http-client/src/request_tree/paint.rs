//! The tree painted: the search, the rows (the one being named a field of its own), the
//! buttons and the footer telling which collection.

use super::*;

/// The naming field's corners (logical pixels): a row's.
const NAMING_RADIUS: f32 = 5.0;

impl RequestTree {
    pub fn paint(&mut self, ctx: ViewerPaint<'_>) {
        let ViewerPaint {
            bounds: b, scale, text, scene, palette: p, icons, waker, focused, ..
        } = ctx;
        self.scale = scale;
        self.reading.set_waker(&waker);
        self.take_read();
        // Seen: read again if the project's files changed (cheap when they didn't).
        self.read_again();
        let ds = Ctx::new(p, scale);
        let px = |v: f32| ds.px(v);
        let base = scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.sidebar_background));
        let buttons =
            if self.buttons { Button::ALL.len() as f32 * (px(BUTTON) + px(2.0)) } else { 0.0 };
        self.field = Rect::new(
            b.min_x() + px(PADDING),
            b.min_y() + px(FIELD_TOP),
            (b.size.width - 2.0 * px(PADDING) - buttons).max(0.0),
            px(FIELD_HEIGHT),
        );
        let typing = focused && self.typing && self.naming.is_none();
        let placeholder = t("http.search_requests");
        self.filter.paint(base, text, icons, &ds, (self.field, typing), &placeholder);
        self.buttons_at.clear();
        if self.buttons {
            let size = px(BUTTON);
            let mut x = self.field.max_x() + px(4.0);
            for button in Button::ALL {
                let r = Rect::new(x, self.field.min_y(), size, size);
                let spec = ButtonSpec::new(ButtonKind::Ghost)
                    .icon(button.icon())
                    .rest_tone(ds::Tone::Neutral)
                    .hovered(self.hover_button == Some(button));
                ds::button(base, text, icons, &ds, r, spec);
                self.buttons_at.push((r, button));
                x = r.max_x() + px(2.0);
            }
        }
        let footer = Rect::new(b.min_x(), b.max_y() - px(FOOTER), b.size.width, px(FOOTER));
        self.paint_footer(base, text, &ds, footer);
        let top = b.min_y() + px(LIST_TOP);
        self.list = Rect::new(b.min_x(), top, b.size.width, (footer.min_y() - top).max(0.0));
        self.naming_field = Rect::default();
        if self.slots.is_empty() {
            let (title, detail) = if !self.filter.text().trim().is_empty() {
                (t("http.no_match"), None)
            } else if self.within.is_empty() {
                (t("http.no_files"), Some(t("http.no_files_detail")))
            } else {
                (t("http.empty_collection"), Some(t("http.empty_collection_detail")))
            };
            ds::empty_state(base, text, &ds, self.list, &title, detail.as_deref());
            return;
        }
        let h = px(ROW_HEIGHT);
        let content = self.slots.len() as f32 * h + px(12.0);
        self.scroll = self.scroll.clamp(0.0, (content - self.list.size.height).max(0.0));
        let range = TreeList::new(h, self.slots.len()).visible(self.scroll, self.list.size.height);
        let origin = self.list.min_y() + px(6.0) - self.scroll;
        let layer = scene.push_layer(Some(self.list));
        let font = Font::new(p.ui_font.clone(), px(p.ui_font_size - 0.5));
        let small = ds.code_font(-1.5).with_weight(Font::SEMIBOLD);
        let metrics = text.metrics(&font);
        let inset = px(8.0);
        let chevron = px(12.0);
        let guide = p.text.with_alpha(0.08);
        // A request being dragged: the folder it would go into (not its own: nothing moves),
        // lit with what's in it, so where it lands is plain.
        let dragged = self.dragged_to.and_then(|_| self.pressed.map(|(i, ..)| i));
        let target = self.drop_folder();
        if let Some(target) = &target {
            let slots = range.clone().filter(
                |&slot| matches!(self.slots[slot], Slot::Row(i) if self.in_folder(i, target)),
            );
            let (first, last) =
                slots.fold((usize::MAX, 0), |(a, b), slot| (a.min(slot), b.max(slot)));
            let zone = if first == usize::MAX {
                // The root, or a folder scrolled away: the whole list.
                Rect::new(
                    self.list.min_x() + px(2.0),
                    self.list.min_y() + px(2.0),
                    (self.list.size.width - px(4.0)).max(0.0),
                    (self.list.size.height - px(4.0)).max(0.0),
                )
            } else {
                Rect::new(
                    self.list.min_x() + inset - px(2.0),
                    origin + first as f32 * h - px(1.0),
                    self.list.size.width - 2.0 * inset + px(4.0),
                    (last + 1 - first) as f32 * h + px(2.0),
                )
            };
            layer.quad(
                Quad::filled(zone, p.accent.with_alpha(0.10))
                    .rounded(px(8.0))
                    .bordered(ds.line().max(px(1.5)), p.accent.with_alpha(0.85)),
            );
        }
        for slot in range {
            let row_top = origin + slot as f32 * h;
            let center = |size: f32| (row_top + (h - size) / 2.0).round();
            let depth = match self.slots[slot] {
                Slot::Row(i) => self.rows[i].depth(),
                Slot::Naming(depth) => depth,
            };
            // The guides of what holds it, as the project's tree draws them.
            let guide_x = |level: usize| {
                self.list.min_x() + px(PADDING) + level as f32 * px(INDENT) + chevron / 2.0
            };
            tree_row::guides(layer, 0..usize::from(depth), guide_x, (row_top, h), ds.line(), guide);
            let x = self.list.min_x() + px(PADDING) + f32::from(depth) * px(INDENT);
            let i = match self.slots[slot] {
                Slot::Row(i) => i,
                Slot::Naming(_) => {
                    let Some((_, input)) = &self.naming else { continue };
                    let size = px(15.0);
                    let icon_rect =
                        Rect::new((x + chevron + px(4.0)).round(), center(size), size, size);
                    if let Some(id) = icons.id("folder") {
                        draw_icon(layer, icons, id, icon_rect, p.text_muted);
                    }
                    let field = Rect::new(
                        icon_rect.max_x() + px(5.0),
                        row_top + px(2.0),
                        (self.list.max_x() - inset - icon_rect.max_x() - px(5.0)).max(px(40.0)),
                        h - px(4.0),
                    );
                    // The shared field, its border lit while it's being named (in the
                    // danger color when the name can't be used).
                    let mut style = ds::field_style(&ds, font.clone(), self.naming_error);
                    style.border = style.border_focused;
                    style.radius = px(NAMING_RADIUS);
                    style.padding = px(6.0);
                    input.paint(layer, text, field, focused, &t("http.collection_name"), &style);
                    self.naming_field = field;
                    continue;
                }
            };
            let row = &self.rows[i];
            let pill = Rect::new(
                self.list.min_x() + inset,
                row_top + ds.line(),
                self.list.size.width - 2.0 * inset,
                h - 2.0 * ds.line(),
            );
            let current = match row {
                Row::Request { file, request, .. } => {
                    self.current.as_ref().is_some_and(|(path, index)| {
                        index == request
                            && self
                                .listing
                                .files
                                .get(*file)
                                .is_some_and(|f| path.ends_with(&f.path))
                    })
                }
                Row::Folder { .. } => false,
            };
            let moving = dragged == Some(i);
            if target.is_some() || moving {
                // Dragging: only the zone it would land in is lit (the hover would mislead).
            } else if (self.selected == Some(i) && focused) || current {
                tree_row::highlight(layer, pill, p.tree_selection, px(6.0));
            } else if self.hover == Some(i) || self.selected == Some(i) {
                tree_row::highlight(layer, pill, p.tree_hover, px(6.0));
            }
            // The row being dragged, faded where it was.
            let ink = |c: Rgba| if moving { c.with_alpha(c.a * 0.4) } else { c };
            let baseline = centered_baseline(row_top, h, &metrics);
            let end = self.list.max_x() - px(14.0);
            match row {
                Row::Folder { path, expanded, .. } => {
                    let at = Rect::new(x.round(), center(chevron), chevron, chevron);
                    tree_row::chevron(layer, icons, at, *expanded, p.text_muted);
                    let size = px(15.0);
                    let icon_rect =
                        Rect::new((x + chevron + px(4.0)).round(), center(size), size, size);
                    if let Some(id) = icons.id("folder") {
                        draw_icon(layer, icons, id, icon_rect, p.text_muted);
                    }
                    let lx = (icon_rect.max_x() + px(7.0)).round();
                    let label = collections::base_name(path);
                    let shown = ds::ellipsize(text, label, &font, (end - lx).max(0.0));
                    let shaped = text.shape_line(&shown, &font);
                    draw_text(layer, &shaped, lx, baseline, ink(p.text));
                }
                Row::Request { file, request, .. } => {
                    let request = &self.listing.files[*file].requests[*request];
                    let shaped = text.shape_line(&request.method, &small);
                    let mx = x + chevron + px(4.0);
                    draw_text(
                        layer,
                        &shaped,
                        mx.round(),
                        baseline,
                        ink(crate::format::method_color(&request.method, p)),
                    );
                    let lx = (mx + px(46.0).max(shaped.width + px(8.0))).round();
                    let shown = ds::ellipsize(text, &request.label, &font, (end - lx).max(0.0));
                    let shaped = text.shape_line(&shown, &font);
                    draw_text(layer, &shaped, lx, baseline, ink(p.text));
                }
            }
        }
        self.bar.paint(scene, p, scale, self.list, content, self.scroll);
        // What's dragged, under the pointer: its method and name on a card.
        if let (Some(i), Some(at)) = (dragged, self.dragged_to)
            && let Some(Row::Request { file, request, .. }) = self.rows.get(i)
        {
            let request = &self.listing.files[*file].requests[*request];
            let lead = ui_components::drag_card::Lead::Tag(
                &request.method,
                crate::format::method_color(&request.method, p),
            );
            let top = scene.push_layer(Some(b));
            ui_components::drag_card::paint(top, &ds, text, icons, lead, &request.label, at);
        }
    }

    /// The keys' footer: how the rows are walked (or, a name being typed, how it's kept).
    fn paint_footer(&self, layer: &mut Layer, text: &mut TextSystem, ds: &Ctx<'_>, footer: Rect) {
        let p = ds.palette;
        layer.quad(Quad::filled(footer, ds.fade(p.text.with_alpha(0.022))));
        ds::divider(layer, ds, footer.min_x(), footer.max_x(), footer.min_y());
        let naming = self.naming.is_some();
        let labels: Vec<String> = if naming {
            vec![t("http.keys_keep"), t("http.keys_cancel")]
        } else {
            vec![t("http.keys_navigate"), t("http.keys_fold"), t("http.keys_open")]
        };
        let hints: Vec<ui_kit::KeyHint<'_>> = if naming {
            vec![(&["↵"], &labels[0]), (&["esc"], &labels[1])]
        } else {
            vec![(&["↑", "↓"], &labels[0]), (&["←", "→"], &labels[1]), (&["↵"], &labels[2])]
        };
        let row = Rect::new(
            footer.min_x() + ds.px(14.0),
            footer.min_y(),
            (footer.size.width - ds.px(28.0)).max(0.0),
            footer.size.height,
        );
        ui_kit::paint_key_hints(layer, text, &hints, row, &p.key_hints(ds.scale));
    }
}
