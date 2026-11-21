//! The blocks of a page, as Theme Studio's form draws its settings (`ui_components::form`):
//! each block its title and description over a card, its rows in the card one under the
//! other (a line between them) — a row's label and description on the left, its control on
//! the right (under them when narrow), "↺" while it differs from its default —, the notes
//! that end a block under its card, small.

use ui_components::form;

use super::*;

impl SettingsView {
    /// The blocks of `page`, one under the other from `area.top`, in their places (those
    /// under "Advanced" last, under its foldable header).
    pub(super) fn paint_blocks(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        page: &'static str,
        area: SectionArea<'_, '_>,
    ) {
        let SectionArea { layer, text, ctx, x0, mut top, width } = area;
        // A language's own block only while it's the one chosen.
        let shown = |b: &ContributedBlock| {
            b.language.as_ref().is_none_or(|l| model.code_language.as_ref() == Some(l))
        };
        let mut blocks: Vec<&ContributedBlock> =
            model.blocks.iter().filter(|b| b.page == page && shown(b)).collect();
        blocks.sort_by_key(|b| (b.advanced, b.order, b.id));
        if page == workbench_model::SettingsPage::LANGUAGES.id && !model.code_languages.is_empty() {
            top = self.paint_language_chooser(
                model,
                icons,
                SectionArea { layer, text, ctx, x0, top, width },
            );
        }
        let mut folded = false;
        let mut advanced = false;
        for block in blocks {
            if block.advanced && !advanced {
                advanced = true;
                let open = self.advanced_open.contains(&page);
                let action = SettingsAction::ToggleAdvanced(page);
                let hovered = self.hover.as_ref() == Some(&action);
                let header = form::fold_section(
                    &mut ui_components::LayerPainter::new(layer, text, icons, *ctx),
                    (x0, top, width),
                    &i18n::t("settings.advanced"),
                    open,
                    hovered,
                );
                self.block_hits.push((header, action));
                self.blocks_bottom = self.blocks_bottom.max(header.max_y());
                top = header.max_y() + ctx.px(14.0);
                folded = !open;
            }
            if folded {
                continue;
            }
            let end =
                self.paint_block(block, icons, SectionArea { layer, text, ctx, x0, top, width });
            top = end + ctx.px(28.0);
        }
    }

    /// The Languages page's chooser: one card with every language as a pill, the one shown
    /// chosen. Returns where the blocks start.
    fn paint_language_chooser(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) -> f32 {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let px = |v: f32| ctx.px(v);
        let mut rows =
            BlockRows { layer, text, ctx, icons, x0, width, y: top, block: LANGUAGE_CHOOSER };
        let at = rows.layer.quads.len();
        let label = i18n::t("settings.languages_choose");
        let detail = i18n::t("settings.languages_choose_detail");
        let gap = px(8.0);
        let widths: Vec<f32> =
            model.code_languages.iter().map(|(_, n)| ds::choice_width(rows.text, ctx, n)).collect();
        // The pills go the card's whole width, under the label.
        let lines = pill_lines(&widths, gap, width - 2.0 * px(form::ROW_PAD));
        let row =
            Labelled { key: "language", label: &label, detail: &detail, reset: None, icon: None };
        let control_h = 32.0 * lines as f32 + 8.0 * lines.saturating_sub(1) as f32;
        // Under the label, the whole width: the list is long.
        let area = self.setting_row(&mut rows, &row, width, control_h);
        let start = area.control.min_x();
        let end = x0 + width - px(form::ROW_PAD);
        let (mut x, mut mid) = (start, area.control.min_y() + px(16.0));
        for ((id, name), w) in model.code_languages.iter().zip(&widths) {
            if x > start && x + w > end + 0.5 {
                x = start;
                mid += px(40.0);
            }
            let action = SettingsAction::SelectCodeLanguage(id.clone());
            let r = ds::choice(
                rows.layer,
                rows.text,
                ctx,
                ds::ChoiceSpec {
                    x,
                    mid,
                    label: name,
                    selected: model.code_language.as_ref() == Some(id),
                    hovered: self.hover == Some(action.clone()),
                },
            );
            self.block_hits.push((r, action));
            x = r.max_x() + gap;
        }
        let card = Rect::new(x0, top, width, rows.y - top);
        rows.layer.quads.insert(at, form::group_quad(ctx, card));
        self.anchors.push((LANGUAGE_CHOOSER, card));
        self.blocks_bottom = self.blocks_bottom.max(rows.y);
        rows.y + px(28.0)
    }

    /// A block: its title (and description), its rows in a card, the notes ending it under
    /// the card. Returns where it ends.
    pub(super) fn paint_block(
        &mut self,
        contributed: &ContributedBlock,
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) -> f32 {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let px = |v: f32| ctx.px(v);
        let block = &contributed.block;
        let y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            top,
            &block.title,
            Some(block.detail.as_str()).filter(|d| !d.is_empty()),
        ) + px(10.0);
        // The notes that end the block go under its card.
        let trailing = block.rows.iter().rev().take_while(|r| matches!(r, SettingsRow::Notes(_)));
        let (body, notes) = block.rows.split_at(block.rows.len() - trailing.count());
        let mut rows = BlockRows { layer, text, ctx, icons, x0, width, y, block: contributed.id };
        // The block's anchor: the whole block, unless a row makes itself what a search
        // highlights (a switch card).
        let anchor = self.anchors.len();
        self.anchors.push((contributed.id, Rect::default()));
        if !body.is_empty() {
            let at = rows.layer.quads.len();
            let card_top = rows.y;
            for (i, row) in body.iter().enumerate() {
                if i > 0 {
                    form::row_divider(rows.layer, ctx, x0, x0 + width, rows.y);
                }
                self.paint_row(&mut rows, row);
            }
            let card = Rect::new(x0, card_top, width, rows.y - card_top);
            rows.layer.quads.insert(at, form::group_quad(ctx, card));
        }
        for row in notes {
            if let SettingsRow::Notes(lines) = row {
                rows.footnotes(lines);
            }
        }
        if self.anchors[anchor].1 == Rect::default() {
            self.anchors[anchor].1 = Rect::new(x0, top, width, rows.y - top);
        }
        self.blocks_bottom = self.blocks_bottom.max(rows.y);
        rows.y
    }

    /// A row of a block's card, as its kind draws it.
    fn paint_row(&mut self, rows: &mut BlockRows<'_, '_>, row: &SettingsRow) {
        match row {
            SettingsRow::Choices { key, label, detail, choices, current, default } => {
                let reset = default.filter(|d| d != current);
                self.paint_choices_row(
                    rows,
                    Labelled { key, label, detail, reset, icon: None },
                    choices,
                    *current,
                );
            }
            SettingsRow::Dropdown { key, label, detail, icon, choices, current, default } => {
                let reset = default.filter(|d| d != current);
                let row = Labelled { key, label, detail, reset, icon: *icon };
                self.paint_dropdown_row(rows, row, choices, *current);
            }
            SettingsRow::Segmented { key, label, detail, choices, current, default } => {
                let reset = default.filter(|d| d != current);
                let row = Labelled { key, label, detail, reset, icon: None };
                self.paint_segmented_row(rows, row, choices, *current);
            }
            SettingsRow::Slider { key, label, detail, stops, current, default } => {
                let reset = default.filter(|d| d != current);
                self.paint_slider_row(
                    rows,
                    Labelled { key, label, detail, reset, icon: None },
                    stops,
                    *current,
                );
            }
            SettingsRow::Toggle { key, label, detail, on, default } => {
                let reset = default.filter(|d| d != on).map(i64::from);
                self.paint_toggle_row(
                    rows,
                    Labelled { key, label, detail, reset, icon: None },
                    *on,
                );
            }
            SettingsRow::Text { key, label, detail, value, placeholder } => {
                let row = Labelled { key, label, detail, reset: None, icon: None };
                self.paint_text_row(rows, row, value, placeholder);
            }
            SettingsRow::Secret { key, label, detail, set, placeholder } => {
                let row = Labelled { key, label, detail, reset: None, icon: None };
                self.paint_secret_row(rows, row, *set, placeholder);
            }
            SettingsRow::SwitchCard { key, label, detail, on, status, tone } => {
                let row = Labelled { key, label, detail, reset: None, icon: None };
                self.paint_switch_card(rows, row, *on, (status, *tone));
            }
            SettingsRow::Status { text: status, button } => {
                let buttons: Vec<(&'static str, &str, bool)> =
                    button.iter().map(|(a, l)| (*a, l.as_str(), true)).collect();
                self.paint_status_row(rows, status, &buttons);
            }
            SettingsRow::Actions { text: status, buttons } => {
                let buttons: Vec<(&'static str, &str, bool)> =
                    buttons.iter().map(|(a, l)| (*a, l.as_str(), false)).collect();
                self.paint_status_row(rows, status, &buttons);
            }
            SettingsRow::Notes(lines) => rows.notes(lines),
            SettingsRow::Cards { key, label, detail, cards, current, default, preview } => {
                let reset = default.filter(|d| d != current);
                let row = Labelled { key, label, detail, reset, icon: None };
                self.paint_cards_row(rows, row, cards, *current, preview);
            }
        }
    }

    /// A setting's row (its label, description, "↺"), its control `control_w` wide and
    /// `control_h` tall (logical) left to the caller: where its parts went.
    pub(super) fn setting_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: &Labelled<'_>,
        control_w: f32,
        control_h: f32,
    ) -> form::RowArea {
        let reset =
            row.reset.map(|value| SettingsAction::Reset { block: rows.block, key: row.key, value });
        let spec = form::RowSpec {
            x: rows.x0,
            y: rows.y,
            width: rows.width,
            label: row.label,
            detail: Some(row.detail).filter(|d| !d.is_empty()),
            control_w,
            control_h,
            modified: reset.is_some(),
            reset_hovered: reset.is_some() && self.hover == reset,
            highlight: 0.0,
            icon: row.icon,
        };
        let area = form::setting_row(rows.layer, rows.text, rows.ctx, rows.icons, spec);
        if let (Some(r), Some(action)) = (area.reset, reset) {
            self.block_hits.push((r, action));
        }
        self.row_anchors.push((rows.block, row.key, area.rect));
        rows.y = area.rect.max_y();
        area
    }

    /// A row with a list to choose from: the field showing the choice on the right; a click
    /// opens its list ([`SettingsView::hit`]).
    pub(super) fn paint_dropdown_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        choices: &[(i64, String)],
        current: i64,
    ) {
        let ctx = rows.ctx;
        let width = ctx.px(DROPDOWN_W).min(rows.room());
        let area = self.setting_row(rows, &row, width, 32.0);
        let field = area.control;
        let value = choices.iter().find(|(v, _)| *v == current).map_or("", |(_, n)| n.as_str());
        let open =
            self.picker_for.as_ref().is_some_and(|d| d.block == rows.block && d.key == row.key);
        let hovered = self.mouse.is_some_and(|p| field.contains(p));
        ds::dropdown(
            rows.layer,
            rows.text,
            rows.icons,
            ctx,
            field,
            ds::DropdownSpec { value, open, hovered, enabled: true, note: None, placeholder: None },
        );
        self.dropdowns.push(Dropdown {
            field,
            block: rows.block,
            key: row.key,
            choices: choices.to_vec(),
            current,
        });
    }

    /// A row of choices: the pills side by side on the right, or under the label on as many
    /// lines as they need when they don't fit beside it.
    pub(super) fn paint_choices_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        choices: &[(i64, String)],
        current: i64,
    ) {
        let ctx = rows.ctx;
        let px = |v: f32| ctx.px(v);
        let gap = px(8.0);
        let widths: Vec<f32> =
            choices.iter().map(|(_, name)| ds::choice_width(rows.text, ctx, name)).collect();
        let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
        let room = rows.room();
        let (control_w, lines) = if form::stacks(ctx, rows.width, total) {
            (room, pill_lines(&widths, gap, room))
        } else {
            (total, 1)
        };
        let control_h = 32.0 * lines as f32 + 8.0 * lines.saturating_sub(1) as f32;
        let area = self.setting_row(rows, &row, control_w, control_h);
        let start = area.control.min_x();
        let (mut x, mut mid) = (start, area.control.min_y() + px(16.0));
        for ((value, name), w) in choices.iter().zip(&widths) {
            if x > start && x + w > start + control_w + 0.5 {
                x = start;
                mid += px(40.0);
            }
            let action = SettingsAction::Choose { block: rows.block, key: row.key, value: *value };
            let r = ds::choice(
                rows.layer,
                rows.text,
                ctx,
                ds::ChoiceSpec {
                    x,
                    mid,
                    label: name,
                    selected: *value == current,
                    hovered: self.hover == Some(action.clone()),
                },
            );
            self.block_hits.push((r, action));
            x = r.max_x() + gap;
        }
    }

    /// A segmented row: the choices in one track on the right (as pills when the track
    /// wouldn't fit even under the label).
    pub(super) fn paint_segmented_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        choices: &[(i64, String)],
        current: i64,
    ) {
        let ctx = rows.ctx;
        let labels: Vec<&str> = choices.iter().map(|(_, l)| l.as_str()).collect();
        let width = ds::segmented_width(rows.text, ctx, &labels);
        if width > rows.room() {
            self.paint_choices_row(rows, row, choices, current);
            return;
        }
        let area = self.setting_row(rows, &row, width, 32.0);
        let block = rows.block;
        let action = |value: i64| SettingsAction::Choose { block, key: row.key, value };
        let hovered = choices.iter().position(|(v, _)| self.hover == Some(action(*v)));
        let selected = choices.iter().position(|(v, _)| *v == current).unwrap_or(0);
        let segments = ds::segmented(
            rows.layer,
            rows.text,
            ctx,
            ds::SegmentedSpec {
                right: area.control.max_x(),
                mid: area.control.min_y() + area.control.size.height / 2.0,
                labels: &labels,
                selected,
                hovered,
            },
        );
        for (r, (value, _)) in segments.into_iter().zip(choices) {
            self.block_hits.push((r, action(*value)));
        }
    }

    /// A slider row: the slider over the stops on one line, the current stop said on its
    /// right.
    pub(super) fn paint_slider_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        stops: &[(i64, String)],
        current: i64,
    ) {
        let ctx = rows.ctx;
        let at = stops.iter().position(|(v, _)| *v == current).unwrap_or(0);
        let control_w = ctx.px(300.0).min(rows.room());
        let area = self.setting_row(rows, &row, control_w, form::VALUE_SLIDER_H);
        let index = self.sliders.len();
        let lit = self.slider_held.is_some_and(|(i, _)| i == index)
            || matches!(&self.hover,
                Some(SettingsAction::Choose { block, key: k, .. })
                    if *block == rows.block && *k == row.key);
        let slider = form::value_slider(
            rows.layer,
            rows.text,
            ctx,
            form::ValueSliderSpec {
                x: area.control.min_x(),
                mid: area.control.min_y() + area.control.size.height / 2.0,
                width: control_w,
                stops: stops.len(),
                current: at,
                value: stops.get(at).map_or("", |(_, l)| l.as_str()),
                hovered: lit,
            },
        );
        let values = stops.iter().map(|(v, _)| *v).collect();
        self.sliders.push((slider, rows.block, row.key, values));
    }

    /// A switch row: the switch on the right; a click anywhere on the row (but "↺") turns
    /// it.
    pub(super) fn paint_toggle_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        on: bool,
    ) {
        let ctx = rows.ctx;
        let action =
            SettingsAction::Choose { block: rows.block, key: row.key, value: i64::from(!on) };
        let area = self.setting_row(rows, &row, ctx.px(36.0), 20.0);
        let hovered = self.hover == Some(action.clone());
        let mid = area.control.min_y() + area.control.size.height / 2.0;
        ds::switch(rows.layer, ctx, area.control.min_x(), mid, on, hovered);
        let hit = Rect::new(
            area.rect.min_x(),
            area.rect.min_y(),
            area.control.max_x() + ctx.px(4.0) - area.rect.min_x(),
            area.rect.size.height,
        );
        self.block_hits.push((hit, action));
    }

    /// A switch with, before it, a badge saying what it's doing now. A search leading to its
    /// block highlights the row.
    pub(super) fn paint_switch_card(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        on: bool,
        (status, tone): (&str, workbench_model::StatusTone),
    ) {
        let ctx = rows.ctx;
        let px = |v: f32| ctx.px(v);
        let badge_w = (rows.text.shape_line(status, &ctx.ui_font(-1.5)).width + px(14.0)).round();
        let control_w = badge_w + px(12.0) + px(36.0);
        let action =
            SettingsAction::Choose { block: rows.block, key: row.key, value: i64::from(!on) };
        let area = self.setting_row(rows, &row, control_w, 20.0);
        let mid = area.control.min_y() + area.control.size.height / 2.0;
        let tone = match tone {
            workbench_model::StatusTone::Neutral => ds::Tone::Neutral,
            workbench_model::StatusTone::Accent => ds::Tone::Accent,
            workbench_model::StatusTone::Danger => ds::Tone::Danger,
        };
        ds::badge(
            rows.layer,
            rows.text,
            ctx,
            ds::BadgeSpec { label: status, x: area.control.min_x(), mid, align_right: false, tone },
        );
        let hovered = self.hover == Some(action.clone());
        ds::switch(rows.layer, ctx, area.control.max_x() - px(36.0), mid, on, hovered);
        if let Some(anchor) = self.anchors.iter_mut().rev().find(|(a, _)| *a == rows.block) {
            anchor.1 = area.rect;
        }
        let hit = Rect::new(
            area.rect.min_x(),
            area.rect.min_y(),
            area.control.max_x() + px(4.0) - area.rect.min_x(),
            area.rect.size.height,
        );
        self.block_hits.push((hit, action));
    }

    /// A status line (on up to three lines) and its buttons on the right: destructive ones
    /// outlined in the danger color, the others in the accent's.
    pub(super) fn paint_status_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        status: &str,
        buttons: &[(&'static str, &str, bool)],
    ) {
        let ctx = rows.ctx;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let label_font = ds::button_label_font(ctx);
        let widths: Vec<f32> = buttons
            .iter()
            .map(|(_, label, _)| {
                (rows.text.shape_line(label, &label_font).width + px(24.0)).round()
            })
            .collect();
        let buttons_w: f32 = widths.iter().map(|w| w + px(8.0)).sum();
        let pad = px(form::ROW_PAD);
        let font = ctx.ui_font(-0.5);
        let room = (rows.width - 2.0 * pad - buttons_w - px(8.0)).max(px(80.0));
        let lines = ds::wrap(rows.text, status, &font, room, 3);
        let line_h = px(19.0);
        let h = (line_h * lines.len() as f32 + px(22.0)).max(px(form::ROW_H)).round();
        let top = rows.y;
        let mid = top + h / 2.0;
        let mut y = mid - line_h * lines.len() as f32 / 2.0;
        for line in &lines {
            let s = rows.text.shape_line(line, &font);
            draw_text(
                rows.layer,
                &s,
                rows.x0 + pad,
                ds::optical_baseline(y.round(), line_h, &font),
                ctx.fade(palette.text),
            );
            y += line_h;
        }
        let mut right = rows.x0 + rows.width - pad;
        for ((action, label, danger), w) in buttons.iter().zip(&widths).rev() {
            let button = Rect::new((right - w).round(), (mid - px(15.0)).round(), *w, px(30.0));
            right = button.min_x() - px(8.0);
            let action = SettingsAction::Press { block: rows.block, action };
            let tone = if *danger { ds::Tone::Danger } else { ds::Tone::Accent };
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .label(label)
                .tone(tone)
                .hover_tint(tone)
                .outlined()
                .corner(8.0)
                .hovered(self.hover == Some(action.clone()));
            ds::button(rows.layer, rows.text, rows.icons, ctx, button, spec);
            self.block_hits.push((button, action));
        }
        rows.y = top + h;
    }

    /// A text field row: the field on the right (under the label when narrow), being
    /// written or showing its saved value.
    pub(super) fn paint_text_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        value: &str,
        placeholder: &str,
    ) {
        let ctx = rows.ctx;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let control_w = px(340.0).min(rows.room());
        let area = self.setting_row(rows, &row, control_w, 34.0);
        let field = area.control;
        let (block, key) = (rows.block, row.key);
        let editing = self.editing.as_mut().filter(|e| e.block == block && e.key == key);
        let font = ctx.ui_font(-0.5);
        let style = ui_kit::TextInputStyle {
            font,
            background: ctx.fade(palette.input_background),
            border: ctx.fade(palette.input_border),
            border_focused: ctx.fade(palette.accent),
            text: ctx.fade(palette.text),
            placeholder: ctx.fade(palette.text_muted),
            selection: ctx.fade(palette.selection),
            caret: ctx.fade(palette.caret),
            radius: px(8.0),
            border_width: ctx.line(),
            padding: px(12.0),
            caret_width: px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        };
        let (layer, text) = (&mut *rows.layer, &mut *rows.text);
        // A long hint cut to the field: never drawn past its edge.
        let placeholder =
            ds::ellipsize(text, placeholder, &style.font, field.size.width - px(24.0));
        let placeholder = placeholder.as_str();
        match editing {
            Some(editing) => editing.input.paint(layer, text, field, true, placeholder, &style),
            // Not written: its saved value, as the field shows it.
            None => {
                let shown = ds::ellipsize(text, value, &style.font, field.size.width - px(24.0));
                ui_kit::TextInput::new(&shown).paint(
                    layer,
                    text,
                    field,
                    false,
                    placeholder,
                    &style,
                );
            }
        }
        self.text_fields.push((field, block, key, value.to_owned()));
    }

    /// A secret's field: being written (as dots), or empty with its placeholder (whether one
    /// is set: what's stored is never shown).
    pub(super) fn paint_secret_row(
        &mut self,
        rows: &mut BlockRows<'_, '_>,
        row: Labelled<'_>,
        set: bool,
        placeholder: &str,
    ) {
        self.secret_fields.push((rows.block, row.key));
        let shown = if set { "••••••••••••••••" } else { "" };
        self.paint_text_row(rows, row, shown, placeholder);
        // What the field holds is never the secret: written over from empty.
        if let Some(field) = self.text_fields.last_mut() {
            field.3.clear();
        }
    }
}

/// The Languages page's chooser, as an anchor (where a search leads).
const LANGUAGE_CHOOSER: &str = "languages-chooser";

/// A row with a label: its key, its label and description, and the default it goes back
/// to with "↺" while it differs (`None`: none, or it's the default).
pub(super) struct Labelled<'a> {
    pub(super) key: &'static str,
    pub(super) label: &'a str,
    pub(super) detail: &'a str,
    pub(super) reset: Option<i64>,
    /// An icon before its label.
    pub(super) icon: Option<&'static str>,
}

/// How many lines pills `widths` wide take, `gap` apart, in `room`.
fn pill_lines(widths: &[f32], gap: f32, room: f32) -> usize {
    let mut lines = 1;
    let mut x = 0.0;
    for w in widths {
        if x > 0.0 && x + w > room + 0.5 {
            lines += 1;
            x = 0.0;
        }
        x += w + gap;
    }
    lines
}

/// A dropdown row's field (logical pixels wide).
const DROPDOWN_W: f32 = 200.0;

/// Where a block's rows are painted: the layer, the text, the drawing context, the icons,
/// the card's column (its left edge and width), the next row's top (each row moves it
/// down) and the block's id.
pub(super) struct BlockRows<'a, 'c> {
    pub(super) layer: &'a mut Layer,
    pub(super) text: &'a mut TextSystem,
    pub(super) ctx: &'a Ctx<'c>,
    icons: &'a Icons,
    x0: f32,
    pub(super) width: f32,
    y: f32,
    pub(super) block: &'static str,
}

impl BlockRows<'_, '_> {
    /// The room a control has under its label (a row too narrow beside it).
    fn room(&self) -> f32 {
        let ctx = self.ctx;
        (self.width - ctx.px(2.0 * form::ROW_PAD + form::RESET_W + 8.0)).max(ctx.px(60.0))
    }

    /// Lines of explanation in the card, muted, wrapped to its width.
    fn notes(&mut self, lines: &[String]) {
        let ctx = self.ctx;
        let pad = ctx.px(form::ROW_PAD);
        let font = ctx.ui_font(-1.5);
        let line_h = (font.size * 1.55).round();
        let mut y = self.y + ctx.px(11.0);
        for line in lines {
            for part in ds::wrap(self.text, line, &font, self.width - 2.0 * pad, 3) {
                let shaped = self.text.shape_line(&part, &font);
                draw_text(
                    self.layer,
                    &shaped,
                    self.x0 + pad,
                    ds::optical_baseline(y, line_h, &font),
                    ctx.fade(ctx.palette.text_muted),
                );
                y += line_h;
            }
        }
        self.y = y + ctx.px(11.0);
    }

    /// The notes ending a block, under its card: small, muted, wrapped.
    fn footnotes(&mut self, lines: &[String]) {
        let ctx = self.ctx;
        let font = ctx.ui_font(-1.5);
        let line_h = (font.size * 1.55).round();
        let x = self.x0 + ctx.px(4.0);
        let mut y = self.y + ctx.px(8.0);
        for line in lines {
            for part in ds::wrap(self.text, line, &font, self.width - ctx.px(8.0), 3) {
                let shaped = self.text.shape_line(&part, &font);
                draw_text(
                    self.layer,
                    &shaped,
                    x,
                    ds::optical_baseline(y, line_h, &font),
                    ctx.fade(ctx.palette.text_muted),
                );
                y += line_h;
            }
        }
        self.y = y;
    }
}
