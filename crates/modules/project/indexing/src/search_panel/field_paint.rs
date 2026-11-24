//! The search's field (its buttons: the replacement, the options, the folder) and the rows under it: the replacement's, the folder's, and the folders suggested under it.

use super::*;
use ui_components::ds::{ButtonKind, ButtonSpec, Tone, button_size};
use ui_components::search_field::SearchExtras;

/// The field row's margin from the panel's edges (logical pixels).
const EDGE: f32 = 18.0;
/// The search field's height in its row (logical pixels).
const QUERY_H: f32 = 40.0;
/// A button in the search field: the replacement's, an option's, the folder's (logical
/// pixels, square).
const BUTTON: f32 = button_size::ROOMY;
/// The room those five buttons take on the field's right, with the gaps between them
/// (logical pixels).
const TOOLS_W: f32 = BUTTON * 5.0 + space::XS * 4.0;

impl ProjectSearch {
    /// The field row: the app's search field (its magnifier, the text, × once there's
    /// text) with the replacement's button, the options and the folder's in the room it
    /// keeps on its right (× left of them), then a separator and the `esc` key.
    pub(super) fn paint_field(&mut self, text: &mut TextSystem, scene: &mut Scene, f: &Frame<'_>) {
        let (ctx, p, icons) = (&f.ctx, f.panel, f.icons);
        let px = |v: f32| ctx.px(v);
        let layer = scene.push_layer(Some(p));
        let mid = p.min_y() + f.field_h / 2.0;
        let esc = ds::keycap(layer, text, ctx, "esc", p.max_x() - px(EDGE), mid, self.esc_hover);
        self.esc = esc;
        let line_x = esc.min_x() - px(space::LG);
        ds::divider_vertical(layer, ctx, line_x, mid - px(12.0), mid + px(12.0));
        let left = p.min_x() + px(EDGE);
        let h = px(QUERY_H);
        let field = Rect::new(left, (mid - h / 2.0).round(), line_x - px(space::SM) - left, h);
        self.query_field = field;
        let layer = scene.push_layer(Some(field));
        let focused = !self.in_scope && !self.in_replace && !self.in_mask;
        let placeholder = t("search.placeholder");
        let extras = SearchExtras { trailing: TOOLS_W, invalid: false };
        let room = self.input.paint_with(
            (layer, text, icons),
            ctx,
            (field, focused),
            &placeholder,
            extras,
        );
        // In its room, from the right: the folder's button (lit while a folder is set), the
        // options (lit while on), the replacement's.
        let side = px(BUTTON);
        let button = Rect::new(room.max_x() - side, (mid - side / 2.0).round(), side, side);
        let quiet = ButtonSpec::new(ButtonKind::Ghost).rest_tone(Tone::Neutral);
        let folder = quiet
            .icon("folder")
            .toggle(self.narrowed())
            .hovered(self.scope_hover || self.scope_open);
        ds::button(layer, text, icons, ctx, button, folder);
        self.scope_button = button;
        let mut x = button.min_x() - px(space::XS);
        for (option, label) in SearchOption::ALL.iter().rev() {
            let on = match option {
                SearchOption::Case => self.options.case,
                SearchOption::Word => self.options.word,
                SearchOption::Regex => self.options.regex,
            };
            let r = Rect::new(x - side, button.min_y(), side, side);
            let spec = quiet.label(label).toggle(on).hovered(self.option_hover == Some(*option));
            ds::button(layer, text, icons, ctx, r, spec);
            self.option_buttons.push((r, *option));
            x = r.min_x() - px(space::XS);
        }
        let replace = Rect::new(x - side, button.min_y(), side, side);
        let spec = quiet
            .icon("replace")
            .toggle(self.replace_open)
            .hovered(self.option_hover.is_none() && self.hover_replace);
        ds::button(layer, text, icons, ctx, replace, spec);
        self.replace_button = replace;
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
        let spec = ButtonSpec::new(ButtonKind::Primary)
            .label(&all)
            .enabled(self.can_replace)
            .hovered(hovered);
        ds::button(layer, text, icons, ctx, button, spec);
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
    /// it's empty), "Files" and the mask's.
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
    }

    /// The folders suggested for the one typed: the shared list under the folder's field (or
    /// over it, when there's more room there), as tall as the window lets it, its rows
    /// scrolled within, in layers of its own over the panel.
    pub(super) fn paint_suggestions(
        &mut self,
        text: &mut TextSystem,
        scene: &mut Scene,
        f: &Frame<'_>,
    ) {
        if !self.has_suggestions() {
            return;
        }
        let ctx = &f.ctx;
        let margin = ctx.px(ds::POPOVER_GAP);
        let v = self.viewport;
        let bounds = Rect::new(
            v.min_x() + margin,
            v.min_y() + margin,
            (v.size.width - margin * 2.0).max(0.0),
            (v.size.height - margin * 2.0).max(0.0),
        );
        let placeholder = t("search.whole_project");
        let anchor = (self.scope_field, bounds);
        self.suggestion_list.paint(scene, text, f.icons, ctx, anchor, &placeholder);
    }
}
