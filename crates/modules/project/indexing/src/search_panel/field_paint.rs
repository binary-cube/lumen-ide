//! The search's field (its buttons: the replacement, the options, the folder) and the rows under it: the replacement's, the folder's and its suggestions.

use super::*;

impl ProjectSearch {
    /// The field: the magnifier, the text (larger), a separator and the `esc` key.
    pub(super) fn paint_field(&mut self, text: &mut TextSystem, scene: &mut Scene, f: &Frame<'_>) {
        let (ctx, p, palette, icons) = (&f.ctx, f.panel, f.ctx.palette, f.icons);
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(p));
        let mid = p.min_y() + f.field_h / 2.0;
        let icon = px(19.0);
        if let Some(id) = icons.id("search") {
            draw_icon(
                layer,
                icons,
                id,
                Rect::new(p.min_x() + px(22.0), (mid - icon / 2.0).round(), icon, icon),
                palette.text_muted,
            );
        }
        let esc = ds::keycap(layer, text, ctx, "esc", p.max_x() - px(18.0), mid, self.esc_hover);
        self.esc = esc;
        let line_x = esc.min_x() - px(space::LG);
        ds::divider_vertical(layer, ctx, line_x, mid - px(12.0), mid + px(12.0));
        // The folder's button: lit while a folder is set.
        let side = px(30.0);
        let button =
            Rect::new(line_x - px(space::SM) - side, (mid - side / 2.0).round(), side, side);
        let set = self.narrowed();
        let fill = if set {
            palette.accent.with_alpha(0.14)
        } else if self.scope_hover || self.scope_open {
            palette.text.with_alpha(0.06)
        } else {
            Rgba::TRANSPARENT
        };
        layer.quad(Quad::filled(button, ctx.fade(fill)).rounded(px(radius::SM)));
        if let Some(id) = icons.id("folder") {
            let s = px(16.0);
            let at = Rect::new(
                button.min_x() + (side - s) / 2.0,
                button.min_y() + (side - s) / 2.0,
                s,
                s,
            );
            let color = if set { palette.accent } else { palette.text_muted };
            draw_icon(layer, icons, id, at, ctx.fade(color));
        }
        self.scope_button = button;
        // Before it, the options: lit while on; then the replacement's button.
        let mut x = button.min_x() - px(space::XS);
        let code = ctx.code_font(-1.0).with_weight(Font::SEMIBOLD);
        for (option, label) in SearchOption::ALL.iter().rev() {
            let on = match option {
                SearchOption::Case => self.options.case,
                SearchOption::Word => self.options.word,
                SearchOption::Regex => self.options.regex,
            };
            let r = Rect::new(x - side, button.min_y(), side, side);
            let fill = if on {
                palette.accent.with_alpha(0.14)
            } else if self.option_hover == Some(*option) {
                palette.text.with_alpha(0.06)
            } else {
                Rgba::TRANSPARENT
            };
            layer.quad(Quad::filled(r, ctx.fade(fill)).rounded(px(radius::SM)));
            let shaped = text.shape_line(label, &code);
            let color = if on { palette.accent } else { palette.text_muted };
            draw_text(
                layer,
                &shaped,
                (r.min_x() + (side - shaped.width) / 2.0).round(),
                ds::optical_baseline(r.min_y(), side, &code),
                ctx.fade(color),
            );
            self.option_buttons.push((r, *option));
            x = r.min_x() - px(space::XS);
        }
        let replace = Rect::new(x - side, button.min_y(), side, side);
        let fill = if self.replace_open {
            palette.accent.with_alpha(0.14)
        } else if self.option_hover.is_none() && self.hover_replace {
            palette.text.with_alpha(0.06)
        } else {
            Rgba::TRANSPARENT
        };
        layer.quad(Quad::filled(replace, ctx.fade(fill)).rounded(px(radius::SM)));
        if let Some(id) = icons.id("replace") {
            let s = px(16.0);
            let at = Rect::new(
                replace.min_x() + (side - s) / 2.0,
                replace.min_y() + (side - s) / 2.0,
                s,
                s,
            );
            let color = if self.replace_open { palette.accent } else { palette.text_muted };
            draw_icon(layer, icons, id, at, ctx.fade(color));
        }
        self.replace_button = replace;
        x = replace.min_x() - px(space::XS);
        let field = Rect::new(
            p.min_x() + px(54.0),
            p.min_y(),
            x - px(space::SM) - p.min_x() - px(54.0),
            f.field_h,
        );
        self.query_field = field;
        let style = TextInputStyle {
            font: ctx.ui_font(3.0),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: ctx.fade(palette.text),
            placeholder: ctx.fade(palette.text_muted),
            selection: ctx.fade(palette.selection),
            caret: ctx.fade(palette.caret),
            radius: 0.0,
            border_width: 0.0,
            padding: px(2.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
        };
        let input_layer = scene.push_layer(Some(field));
        let focused = !self.in_scope && !self.in_replace && !self.in_mask;
        self.input.paint(input_layer, text, field, focused, &t("search.placeholder"), &style);
    }

    /// The replacement's row under the field: "With", its field, Replace All (pressable once
    /// every occurrence is found).
    pub(super) fn paint_replace(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        f: &Frame<'_>,
    ) {
        let (ctx, p, palette, icons) = (&f.ctx, f.panel, f.ctx.palette, f.icons);
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(p));
        let top = p.min_y() + f.field_h;
        ds::divider(layer, ctx, p.min_x(), p.max_x(), top);
        let mid = top + f.replace_h / 2.0;
        let small = ctx.ui_font(-1.0);
        let label = text.shape_line(&t("search.replace_with"), &small);
        draw_text(
            layer,
            &label,
            p.min_x() + px(22.0),
            ds::optical_baseline(top, f.replace_h, &small),
            ctx.fade(palette.text_muted),
        );
        let all = t("search.replace_all");
        let button_w = ds::primary_width(text, ctx, &[&all]);
        let button_h = px(ds::BUTTON_H);
        let button = Rect::new(
            p.max_x() - px(18.0) - button_w,
            (mid - button_h / 2.0).round(),
            button_w,
            button_h,
        );
        let hovered = self.hover_replace_all;
        ds::primary_button(layer, text, ctx, button, &all, self.can_replace, hovered);
        self.replace_all = button;
        let x = p.min_x() + px(54.0);
        let field =
            Rect::new(x, (mid - px(16.0)).round(), button.min_x() - px(space::SM) - x, px(32.0));
        self.replace_field = field;
        let mut style = ds::search_input_style(ctx, field.size.height);
        style.lead = px(30.0);
        style.radius = px(radius::MD);
        let input_layer = scene.push_layer(Some(field));
        let placeholder = t("search.replace_placeholder");
        self.replace.paint(input_layer, text, field, self.in_replace, &placeholder, &style);
        if let Some(id) = icons.id("replace") {
            let s = px(15.0);
            let at = Rect::new(field.min_x() + px(10.0), (mid - s / 2.0).round(), s, s);
            draw_icon(input_layer, icons, id, at, ctx.fade(palette.text_muted));
        }
    }

    /// The folder's row under the field: "In", the folder's field (the whole project while
    /// it's empty), then the folders it may go on to while it's typed in.
    pub(super) fn paint_scope(&mut self, text: &mut TextSystem, scene: &mut Scene, f: &Frame<'_>) {
        let (ctx, p, palette, icons) = (&f.ctx, f.panel, f.ctx.palette, f.icons);
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(p));
        let top = p.min_y() + f.field_h + f.replace_h;
        ds::divider(layer, ctx, p.min_x(), p.max_x(), top);
        let mid = top + f.scope_h / 2.0;
        let small = ctx.ui_font(-1.0);
        let label = text.shape_line(&t("search.in_folder"), &small);
        draw_text(
            layer,
            &label,
            p.min_x() + px(22.0),
            ds::optical_baseline(top, f.scope_h, &small),
            ctx.fade(palette.text_muted),
        );
        // The folder's field, then "Files" and the mask's.
        let x = p.min_x() + px(54.0);
        let right = p.max_x() - px(18.0);
        let mask_w = ((right - x) * 0.36).round();
        let files = text.shape_line(&t("search.files_mask"), &small);
        let mask_x = right - mask_w;
        let label_x = mask_x - px(space::SM) - files.width;
        let field_w = (label_x - px(space::MD) - x).max(0.0);
        let field = Rect::new(x, (mid - px(16.0)).round(), field_w, px(32.0));
        self.scope_field = field;
        draw_text(
            layer,
            &files,
            label_x,
            ds::optical_baseline(top, f.scope_h, &small),
            ctx.fade(palette.text_muted),
        );
        let mask_field = Rect::new(mask_x, field.min_y(), mask_w, field.size.height);
        self.mask_field = mask_field;
        let mut mask_style = ds::search_input_style(ctx, mask_field.size.height);
        mask_style.lead = px(4.0);
        mask_style.radius = px(radius::MD);
        let mask_layer = scene.push_layer(Some(mask_field));
        let all = t("search.all_files");
        self.mask.paint(mask_layer, text, mask_field, self.in_mask, &all, &mask_style);
        let mut style = ds::search_input_style(ctx, field.size.height);
        style.lead = px(30.0);
        style.radius = px(radius::MD);
        let input_layer = scene.push_layer(Some(field));
        let placeholder = t("search.whole_project");
        self.scope.paint(input_layer, text, field, self.in_scope, &placeholder, &style);
        if let Some(id) = icons.id("folder") {
            let s = px(15.0);
            let at = Rect::new(field.min_x() + px(10.0), (mid - s / 2.0).round(), s, s);
            draw_icon(input_layer, icons, id, at, ctx.fade(palette.text_muted));
        }
        if !self.has_suggestions() {
            return;
        }
        let layer = scene.push_layer(Some(p));
        let mut y = top + f.scope_h;
        let font = ctx.ui_font(-0.5);
        let metrics = text.metrics(&font);
        for (i, folder) in self.suggestions.iter().enumerate() {
            let row = Rect::new(field.min_x(), y, field.size.width, px(SUGGESTION_H));
            ds::row_highlight(layer, ctx, row, i == self.suggestion, false);
            if let Some(id) = icons.id("folder") {
                let s = px(15.0);
                let at = Rect::new(
                    row.min_x() + px(10.0),
                    row.min_y() + (row.size.height - s) / 2.0,
                    s,
                    s,
                );
                draw_icon(layer, icons, id, at, ctx.fade(palette.accent));
            }
            let shown = ds::ellipsize_path(text, folder, &font, row.size.width - px(48.0));
            let shaped = text.shape_line(&shown, &font);
            draw_text(
                layer,
                &shaped,
                row.min_x() + px(34.0),
                centered_baseline(row.min_y(), row.size.height, &metrics),
                ctx.fade(palette.text),
            );
            self.suggestion_rows.push((row, i));
            y += px(SUGGESTION_H);
        }
    }
}
