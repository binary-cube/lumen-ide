//! The shortcuts: the presets, the commands with their keys, the one flashed after a change.

use super::*;

impl SettingsView {
    /// Highlighting of the found setting (an accent-colored outline that fades out).
    /// The "Keymap" section: the keymap presets and the map of
    /// the active one's shortcuts.
    pub(super) fn paint_keymap(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        area: SectionArea<'_, '_>,
        title: &str,
    ) {
        let SectionArea { layer, text, ctx, x0, top: y, width } = area;
        let px = |v: f32| ctx.px(v);
        let y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            y,
            title,
            Some(&i18n::t("settings.keymap_detail")),
        );
        // The presets, wrapping onto more rows if needed.
        let mut x = x0;
        let mut mid = y + px(34.0);
        for (id, name) in &model.keymaps {
            let hovered = self.hover == Some(SettingsAction::SelectKeymap(id.clone()));
            let mut r = ds::choice(
                layer,
                text,
                ctx,
                ds::ChoiceSpec {
                    x,
                    mid,
                    label: name,
                    selected: *id == model.current_keymap,
                    hovered,
                },
            );
            if r.max_x() > x0 + width && x > x0 {
                // Doesn't fit: the next row (drawn again there).
                x = x0;
                mid += px(48.0);
                r = ds::choice(
                    layer,
                    text,
                    ctx,
                    ds::ChoiceSpec {
                        x,
                        mid,
                        label: name,
                        selected: *id == model.current_keymap,
                        hovered,
                    },
                );
            }
            self.keymap_chips.push((r, id.clone()));
            x = r.max_x() + px(10.0);
        }
        let chips_bottom = self.keymap_chips.iter().map(|(r, _)| r.max_y()).fold(mid, f32::max);
        if let (Some(first), Some(last)) = (self.keymap_chips.first(), self.keymap_chips.last()) {
            let (a, b) = (first.0, last.0);
            self.anchors.push((
                "keymap",
                Rect::new(a.min_x(), a.min_y(), (x0 + width) - a.min_x(), b.max_y() - a.min_y()),
            ));
        }
        // Every command and its keys: searched, changed, its conflicts answered.
        let area = (x0, chips_bottom + px(24.0), width);
        let focused = self.controls.focused() == Some(super::FILTER);
        let (x, top, width) = area;
        let bottom = self.shortcuts.paint(
            layer,
            text,
            (ctx, icons),
            (x, top, width, focused),
            &model.bindings,
        );
        self.binding_rows.push(Rect::new(x0, chips_bottom, width, bottom - chips_bottom));
    }

    /// Where the opened result is: its exact element (a theme's card) or its anchor.
    pub(super) fn flash_rect(&self) -> Option<Rect> {
        let (anchor, target, _) = self.flash.as_ref()?;
        if let Some(id) = target
            && let Some((r, _)) = self.cards.iter().find(|(_, card)| card == id)
        {
            return Some(*r);
        }
        if let Some(key) = target
            && let Some((.., r)) = self.row_anchors.iter().find(|(b, k, _)| b == anchor && k == key)
        {
            return Some(*r);
        }
        self.anchors.iter().find(|(a, _)| a == anchor).map(|(_, r)| *r)
    }

    pub(super) fn paint_flash(layer: &mut Layer, ctx: &Ctx<'_>, rect: Option<Rect>, value: f32) {
        let Some(r) = rect else { return };
        if value <= 0.01 {
            return;
        }
        let pad = ctx.px(8.0);
        let rect = Rect::new(
            r.min_x() - pad,
            r.min_y() - pad,
            r.size.width + 2.0 * pad,
            r.size.height + 2.0 * pad,
        );
        layer.quad(
            Quad::filled(rect, ctx.fade(ctx.palette.accent.with_alpha(0.07 * value)))
                .rounded(ctx.px(14.0))
                .bordered(ctx.px(2.0), ctx.fade(ctx.palette.accent.with_alpha(0.8 * value))),
        );
    }
}
