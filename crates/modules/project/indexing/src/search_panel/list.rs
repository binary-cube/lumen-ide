//! The list (the sections, the rows, their scrollbar) and the footer.

use super::*;

impl ProjectSearch {
    /// "Clear all" at the right of the recent searches' header (`top`): a quiet link,
    /// lit under the mouse.
    fn paint_clear_all(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        f: &Frame<'_>,
        fonts: &Fonts,
        top: f32,
    ) {
        let (ctx, palette) = (&f.ctx, f.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let shaped = text.shape_line(&t("search.clear_recent"), &fonts.small);
        let right = f.panel.max_x() - px(22.0);
        let pad = px(8.0);
        let area = Rect::new(
            right - shaped.width - pad,
            top + px(2.0),
            shaped.width + pad * 2.0,
            f.header_h - px(4.0),
        );
        if self.clear_hover {
            layer.quad(
                Quad::filled(area, ctx.fade(palette.danger.with_alpha(0.10))).rounded(px(6.0)),
            );
        }
        let color = if self.clear_hover { palette.danger } else { palette.text_muted };
        draw_text(
            layer,
            &shaped,
            right - shaped.width,
            centered_baseline(top, f.header_h, &fonts.small_metrics),
            ctx.fade(color),
        );
        self.clear_all = area;
    }

    /// The × at the right of recent search `index`'s row (`row`): forgets it.
    fn paint_forget(&mut self, layer: &mut Layer, f: &Frame<'_>, row: Rect, index: usize) {
        let (ctx, palette) = (&f.ctx, f.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let side = px(26.0);
        let button = Rect::new(
            row.max_x() - side - px(8.0),
            row.min_y() + (row.size.height - side) / 2.0,
            side,
            side,
        );
        let over = self.forget_hover == Some(index);
        if over {
            layer.quad(
                Quad::filled(button, ctx.fade(palette.text.with_alpha(0.10))).rounded(side / 2.0),
            );
        }
        if let Some(id) = f.icons.id("x") {
            let s = px(12.0);
            let at = Rect::new(
                button.min_x() + (side - s) / 2.0,
                button.min_y() + (side - s) / 2.0,
                s,
                s,
            );
            let color = if over { palette.text } else { palette.text_muted };
            draw_icon(layer, f.icons, id, at, ctx.fade(color));
        }
        self.forget_buttons.push((button, index));
    }

    /// The list's rows (the section headers, the folders, the scrolled rows) and their
    /// scrollbar, each row recorded for the clicks.
    pub(super) fn paint_list(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        f: &Frame<'_>,
        fonts: &Fonts,
        list: Rect,
        file_icon: &dyn Fn(&str) -> (IconId, Rgba),
    ) {
        let (ctx, p) = (&f.ctx, f.panel);
        let px = |v: f32| ctx.px(v);
        let (recent, folders) = (f.recent, f.folders);
        let named = if self.listing { 0 } else { self.named.len() };
        let mut top = list.min_y() + px(space::SM);
        // The rows to draw: (list index, top, height).
        let mut slots: Vec<(usize, f32, f32)> = Vec::with_capacity(folders + f.rows);
        if recent {
            self.paint_clear_all(layer, text, f, fonts, top);
            paint_header(layer, text, f, fonts, &mut top, &t("search.recent"));
        } else if self.listing {
            paint_header(layer, text, f, fonts, &mut top, &t("search.files"));
        } else if folders > 0 {
            // The files by name, then the folders: a section each, when it has rows.
            for (from, to, title) in
                [(0, named, "search.files"), (named, folders, "search.folders")]
            {
                if to > from {
                    paint_header(layer, text, f, fonts, &mut top, &t(title));
                }
                for i in from..to {
                    slots.push((i, top, f.folder_h));
                    top += f.folder_h;
                }
            }
            if f.rows > 0 {
                paint_header(layer, text, f, fonts, &mut top, &t("search.content"));
            }
        }
        let scroll_top = top;
        for slot in 0..f.rows {
            slots.push((folders + self.first + slot, top, f.row_h));
            top += f.row_h;
        }
        // With a scrollbar, the rows stop before it (the selection never runs under it).
        let gutter = if f.count > f.visible { px(10.0) } else { 0.0 };
        for (index, top, height) in slots {
            let rect = Rect::new(
                p.min_x() + px(space::SM),
                top,
                p.size.width - px(space::SM * 2.0) - gutter,
                height,
            );
            // The lit row's background a little inside its slot: the one chosen and the one
            // under the mouse never touch.
            let lit = Rect::new(
                rect.min_x(),
                rect.min_y() + px(2.0),
                rect.size.width,
                rect.size.height - px(4.0),
            );
            ds::row_highlight(layer, ctx, lit, index == self.selected, self.hover == Some(index));
            if recent {
                paint_recent_row(layer, text, f, fonts, rect, &self.recent[index]);
                if index == self.selected || self.hover == Some(index) {
                    self.paint_forget(layer, f, rect, index);
                }
            } else if index < folders {
                let (item, icon) = if index < named {
                    let file = &self.named[index];
                    let (id, color) = file_icon(file.name());
                    (file, (Some(id), color))
                } else {
                    let folder = &self.folders[index - named];
                    (folder, (f.icons.id("folder"), f.ctx.palette.accent))
                };
                if let Some(cut) = paint_name_row(layer, text, f, fonts, rect, item, icon) {
                    self.tips.push((cut, item.path.clone()));
                }
            } else if self.listing {
                let file = &self.named[index];
                let (id, color) = file_icon(file.name());
                if let Some(cut) =
                    paint_name_row(layer, text, f, fonts, rect, file, (Some(id), color))
                {
                    self.tips.push((cut, file.path.clone()));
                }
            } else {
                let hit = &self.results[index - folders];
                let row = FileRow { hit, query: self.input.text() };
                if let Some(cut) = paint_file_row(layer, text, f, fonts, rect, row, file_icon) {
                    self.tips.push((cut, hit.path.clone()));
                }
            }
            self.rows.push((rect, index));
        }
        // The scrollbar of the scrolled rows (drawn over the list: the app's own).
        if f.count > f.visible {
            let area = Rect::new(p.min_x(), scroll_top, p.size.width - px(2.0), top - scroll_top);
            self.bar_area = Some((area, f.row_h, f.count));
        }
    }

    /// The footer: the keys on the left; on the right the search mode or its action.
    pub(super) fn paint_footer(
        &mut self,
        mode: &SearchMode,
        text: &mut TextSystem,
        scene: &mut Scene,
        f: &Frame<'_>,
        fonts: &Fonts,
    ) {
        let (ctx, p, palette) = (&f.ctx, f.panel, f.ctx.palette);
        let px = |v: f32| ctx.px(v);
        let (footer_h, small) = (f.footer_h, &fonts.small);
        let foot = Rect::new(p.min_x(), p.max_y() - footer_h, p.size.width, footer_h);
        let foot_layer = scene.push_layer(Some(p));
        let (choose, open) = (t("keys.choose"), t("keys.open"));
        // While the folders suggested show, esc closes their list first.
        let close = if self.has_suggestions() { t("search.close_list") } else { t("keys.close") };
        // Tab goes between the search's field and the folder's: named for where it goes.
        let (take, tab) = if self.in_scope {
            (t("search.take_folder"), t("search.back_to_search"))
        } else if self.in_mask || self.in_replace {
            (open, t("search.back_to_search"))
        } else if self.replace_open {
            (open, t("search.replace_key"))
        } else {
            (open, t("search.folder_key"))
        };
        let hints: &[KeyHint<'_>] =
            &[(&["↑", "↓"], &choose), (&["↵"], &take), (&["⇥"], &tab), (&["esc"], &close)];
        let right = ds::footer(foot_layer, text, ctx, foot, hints);
        let mid = foot.min_y() + footer_h / 2.0;
        // The number of results, after the keys.
        let exact = self.exact.filter(|_| !f.recent);
        if exact.is_some() || !f.recent && f.count + f.folders > 0 {
            let count = f.count + f.folders;
            let label = match exact {
                Some(ExactState { done: false, .. }) => {
                    tr!("search.searching_count", count = count)
                }
                Some(ExactState { capped: true, .. }) => {
                    tr!("search.results_capped", count = count)
                }
                _ => tr!("search.results", count = count),
            };
            let shaped = text.shape_line(&label, small);
            let x = foot.min_x() + foot.size.width * 0.5 - shaped.width / 2.0;
            draw_text(
                foot_layer,
                &shaped,
                x.round(),
                ds::optical_baseline(foot.min_y(), footer_h, small),
                ctx.fade(palette.text_muted),
            );
        }
        // The mode as a badge; lexical (or failed), the action that turns semantic on.
        let (label, tone) = match mode {
            SearchMode::Semantic => (t("search.semantic"), Tone::Accent),
            SearchMode::Downloading(got, total) => (
                tr!("search.downloading", got = got >> 20, total = (*total).max(1) >> 20),
                Tone::Neutral,
            ),
            SearchMode::Loading | SearchMode::Standby => {
                (t("settings.semantic_loading"), Tone::Neutral)
            }
            SearchMode::Lexical | SearchMode::Failed(_) => {
                let key = if matches!(mode, SearchMode::Failed(_)) {
                    "search.failed"
                } else {
                    "search.enable_semantic"
                };
                let shaped = text.shape_line(&t(key), small);
                let x = right - shaped.width;
                draw_text(
                    foot_layer,
                    &shaped,
                    x,
                    centered_baseline(foot.min_y(), footer_h, &fonts.small_metrics),
                    palette.accent,
                );
                self.enable =
                    Rect::new(x - px(6.0), foot.min_y(), shaped.width + px(12.0), footer_h);
                return;
            }
        };
        let spec = ds::BadgeSpec { label: &label, x: right, mid, align_right: true, tone };
        ds::badge(foot_layer, text, ctx, spec);
    }
}
