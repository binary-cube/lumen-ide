//! The small parts the right side is drawn from: labels, fields, dropdowns, buttons,
//! titles, notes, the inherited chip, the color swatches, a row with a check.

use super::*;

impl SourcesView {
    pub(super) fn label(
        &self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        y: f32,
        label: &str,
        error: Option<&str>,
        width: f32,
    ) {
        let ds = pen.ds;
        let font = ds.ui_font(-1.0);
        let shaped = pen.text.shape_line(label, &font);
        let base = ds::optical_baseline(y, ds.px(18.0), &font);
        draw_text(pen.layer, &shaped, x, base, ds.palette.text_muted);
        if let Some(error) = error {
            let room = (width - shaped.width - ds.px(16.0)).max(0.0);
            let shown = ds::ellipsize(pen.text, error, &font, room);
            let e = pen.text.shape_line(&shown, &font);
            draw_text(pen.layer, &e, x + width - e.width, base, ds.palette.danger);
        }
    }

    /// A text field in `r`, `placeholder` when empty.
    pub(super) fn input(
        &mut self,
        pen: &mut Pen<'_, '_>,
        field: Field,
        r: Rect,
        placeholder: &str,
    ) {
        let style = Self::style(pen.ds, true);
        let focused = self.focus.as_ref() == Some(&field);
        if let Some(input) = self.editor.as_ref().and_then(|e| e.input(&field)) {
            input.paint(pen.layer, pen.text, r, focused, placeholder, &style);
        }
        self.hits.push((r, Hit::Field(field)));
    }

    /// A field drawn disabled (what isn't there yet: the SSH tunnel).
    pub(super) fn disabled(&self, pen: &mut Pen<'_, '_>, r: Rect, placeholder: &str) {
        let style = Self::style(pen.ds, false);
        ui_kit::TextInput::new("").paint(pen.layer, pen.text, r, false, placeholder, &style);
    }

    /// A list to choose from, shown as a field with its value and a chevron.
    pub(super) fn dropdown(&mut self, pen: &mut Pen<'_, '_>, choice: Choice, r: Rect, value: &str) {
        let open = self.popup.as_ref() == Some(&choice);
        let hovered = self.hover == Some(Hit::Choice(choice.clone()));
        let spec =
            ds::DropdownSpec { value, open, hovered, enabled: true, note: None, placeholder: None };
        ds::dropdown(pen.layer, pen.text, pen.icons, pen.ds, r, spec);
        self.hits.push((r, Hit::Choice(choice.clone())));
        self.anchors.insert(choice, r);
    }

    /// A bordered button.
    pub(super) fn secondary(
        &mut self,
        pen: &mut Pen<'_, '_>,
        r: Rect,
        label: &str,
        hit: Hit,
        enabled: bool,
    ) {
        let hovered = self.hover.as_ref() == Some(&hit);
        ds::secondary_button(pen.layer, pen.text, pen.ds, r, label, enabled, hovered);
        if enabled {
            self.hits.push((r, hit));
        }
    }

    pub(super) fn title(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, title: &str) {
        let font = pen.ds.ui_font(0.0).with_weight(Font::SEMIBOLD);
        let shaped = pen.text.shape_line(title, &font);
        draw_text(
            pen.layer,
            &shaped,
            x,
            ds::optical_baseline(y, pen.ds.px(22.0), &font),
            pen.ds.palette.text,
        );
    }

    /// A note in muted words, wrapped at `w`; its height.
    pub(super) fn note(&self, pen: &mut Pen<'_, '_>, x: f32, y: f32, w: f32, text: &str) -> f32 {
        let ds = pen.ds;
        let font = ds.ui_font(-1.0);
        let line_h = ds.px(19.0);
        let mut lines: Vec<String> = Vec::new();
        let mut line = String::new();
        for word in text.split(' ') {
            let candidate =
                if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
            if !line.is_empty() && pen.text.shape_line(&candidate, &font).width > w {
                lines.push(std::mem::replace(&mut line, word.to_owned()));
            } else {
                line = candidate;
            }
        }
        if !line.is_empty() {
            lines.push(line);
        }
        for (i, l) in lines.iter().enumerate() {
            let shaped = pen.text.shape_line(l, &font);
            draw_text(
                pen.layer,
                &shaped,
                x,
                ds::optical_baseline(y + i as f32 * line_h, line_h, &font),
                ds.palette.text_muted,
            );
        }
        lines.len() as f32 * line_h
    }

    /// The "none" choice of a color row, as a chip: its label, and the color it means
    /// (what's inherited) as a dot, when there's one. Returns its area.
    pub(super) fn inherit_chip(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        mid: f32,
        label: &str,
        means: Option<SourceColor>,
        chosen: bool,
    ) -> Rect {
        let ds = pen.ds;
        let label = t(label);
        let hovered = self.hover == Some(Hit::SourceColor(None))
            || self.hover == Some(Hit::FolderColor(None));
        let spec = ds::ChoiceSpec { x, mid, label: &label, selected: chosen, hovered };
        match means.and_then(|c| ui_components::tag_colors::named(&c.name(), ds.palette)) {
            Some(mark) => ds::choice_with_dot(pen.layer, pen.text, ds, spec, mark),
            None => ds::choice(pen.layer, pen.text, ds, spec),
        }
    }

    /// The named colors as round swatches from `x`, centered on `mid`, `now` ringed; a
    /// folder's (`folder`) or the selected source's.
    pub(super) fn swatches(
        &mut self,
        pen: &mut Pen<'_, '_>,
        x: f32,
        mid: f32,
        now: Option<SourceColor>,
        folder: bool,
    ) {
        let ds = pen.ds;
        let p = ds.palette;
        let px = |v: f32| ds.px(v);
        let size = px(SWATCH);
        let mut sx = x;
        for color in SourceColor::NAMED {
            let Some(fill) = ui_components::tag_colors::named(color.key(), p) else { continue };
            let r = Rect::new(sx, (mid - size / 2.0).round(), size, size);
            let hit =
                if folder { Hit::FolderColor(Some(color)) } else { Hit::SourceColor(Some(color)) };
            ds::ColorSwatch::new(fill)
                .radius(SWATCH / 2.0)
                .ringed_around(now == Some(color))
                .hovered(self.hover.as_ref() == Some(&hit))
                .paint(pen.layer, ds, r);
            self.tips.push((r, t(&format!("datasources.color_{}", color.key()))));
            self.hits.push((r, hit));
            sx += size + px(12.0);
        }
    }

    /// A checkbox and its label at `y` (its row clickable as `hit` when `enabled`); where
    /// the label starts (a note goes under it).
    pub(super) fn check_row(
        &mut self,
        pen: &mut Pen<'_, '_>,
        (x, y): (f32, f32),
        label: &str,
        on: bool,
        enabled: bool,
        hit: Hit,
    ) -> f32 {
        let ds = pen.ds;
        let px = |v: f32| ds.px(v);
        let mid = y + px(14.0);
        let state = if on { Check::On } else { Check::Off };
        let hovered = enabled && self.hover.as_ref() == Some(&hit);
        let check = ds::Checkbox::new(state)
            .enabled(enabled)
            .hovered(hovered)
            .paint(pen.layer, pen.text, ds, x, mid);
        let font = ds.ui_font(-0.5);
        let shaped = pen.text.shape_line(label, &font);
        let lx = check.max_x() + px(8.0);
        let color = if enabled { ds.palette.text } else { ds.palette.text_muted };
        draw_text(
            pen.layer,
            &shaped,
            lx,
            ds::optical_baseline(mid - px(10.0), px(20.0), &font),
            color,
        );
        if enabled {
            self.hits.push((
                Rect::new(x - px(4.0), mid - px(12.0), lx + shaped.width - x + px(8.0), px(24.0)),
                hit,
            ));
        }
        lx
    }
}
