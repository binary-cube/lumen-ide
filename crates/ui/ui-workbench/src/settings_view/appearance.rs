//! Appearance, icons and language: the theme cards and their previews, the icon sets, the
//! languages.

use super::*;

impl SettingsView {
    /// The Language section: the interface's languages, as choices.
    pub(super) fn paint_languages(
        &mut self,
        model: &SettingsModel,
        area: SectionArea<'_, '_>,
        title: &str,
    ) {
        let SectionArea { layer, text, ctx, x0, top, .. } = area;
        let y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            top,
            title,
            Some(&i18n::t("settings.language_detail")),
        );
        let mid = y + ctx.px(34.0);
        let mut x = x0;
        for (code, name) in &model.languages {
            let hovered = self.hover == Some(SettingsAction::SelectLanguage(code.clone()));
            let r = ds::choice(
                layer,
                text,
                ctx,
                ds::ChoiceSpec {
                    x,
                    mid,
                    label: name,
                    selected: *code == model.current_language,
                    hovered,
                },
            );
            self.languages.push((r, code.clone()));
            x = r.max_x() + ctx.px(10.0);
        }
        if let (Some(first), Some(last)) = (self.languages.first(), self.languages.last()) {
            let (a, b) = (first.0, last.0);
            self.anchors.push((
                "language",
                Rect::new(a.min_x(), a.min_y(), b.max_x() - a.min_x(), a.size.height),
            ));
        }
    }

    /// The Appearance section: the title with All / Dark / Light beside it, then the
    /// theme cards.
    pub(super) fn paint_appearance(&mut self, model: &SettingsModel, area: SectionArea<'_, '_>) {
        let SectionArea { layer, text, ctx, x0, top: title_top, width } = area;
        let mut y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            title_top,
            &i18n::t("settings.theme"),
            Some(&i18n::t("settings.theme_detail")),
        );
        // All / Dark / Light, beside the title.
        let labels = ThemeFilter::ALL.map(ThemeFilter::label);
        let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
        let selected = ThemeFilter::ALL.iter().position(|f| *f == self.theme_filter);
        self.filter_rects = ds::segmented(
            layer,
            text,
            ctx,
            ds::SegmentedSpec {
                right: x0 + width,
                mid: (title_top + y) / 2.0,
                labels: &labels,
                selected: selected.unwrap_or(0),
                hovered: self.filter_hover,
            },
        );
        y += ctx.px(16.0);
        self.paint_theme_cards(model, SectionArea { layer, text, ctx, x0, top: y, width });
    }

    /// The "Icons" section: the icon sets, each a card drawing a few icons in its style.
    pub(super) fn paint_icon_sets(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            top,
            &i18n::t("settings.icons"),
            Some(&i18n::t("settings.icons_detail")),
        ) + px(16.0);
        const SAMPLE: &[&str] =
            &["folder", "file-code", "search", "git-branch", "terminal", "palette", "pin"];
        let cols = GRID_COLUMNS;
        let gap = px(14.0);
        let card_w = ((width - gap * (cols - 1) as f32) / cols as f32).floor();
        let card_h = px(108.0);
        let font = ctx.ui_font(0.0);
        let small = ctx.ui_font(-1.0);
        for (i, choice) in model.icon_looks.iter().enumerate() {
            let look = choice.look;
            let (c, r) = (i % cols, i / cols);
            let card = Rect::new(
                x0 + c as f32 * (card_w + gap),
                y + r as f32 * (card_h + gap),
                card_w,
                card_h,
            );
            let selected = look == model.icon_look;
            let hovered = self.hover == Some(SettingsAction::SelectIcons(look));
            let (border, line) = if selected {
                (palette.accent, px(2.0))
            } else if hovered {
                (palette.text_muted.with_alpha(0.6), ctx.line())
            } else {
                (palette.border, ctx.line())
            };
            layer.quad(
                Quad::filled(card, ctx.fade(palette.popup_background))
                    .rounded(px(12.0))
                    .bordered(line, ctx.fade(border)),
            );
            // The sample, drawn in the card's style.
            let size = px(22.0);
            let mut x = card.min_x() + px(16.0);
            for name in SAMPLE {
                let Some(id) = icons.id(name) else { continue };
                if x + size > card.max_x() - px(12.0) {
                    break;
                }
                let key = icons.key_in(id, size as u16, look);
                layer.glyphs.push(render_api::GlyphInstance {
                    key,
                    origin: kernel::Point::new(x.round(), (card.min_y() + px(18.0) + size).round()),
                    color: ctx.fade(if selected { palette.accent } else { palette.text }),
                });
                x += size + px(14.0);
            }
            let name = ds::ellipsize(text, &choice.name, &font, card.size.width - px(32.0));
            let name = text.shape_line(&name, &font);
            let baseline = card.max_y() - px(36.0);
            draw_text(layer, &name, card.min_x() + px(16.0), baseline, ctx.fade(palette.text));
            let about = ds::ellipsize(text, &choice.detail, &small, card.size.width - px(32.0));
            let about = text.shape_line(&about, &small);
            draw_text(
                layer,
                &about,
                card.min_x() + px(16.0),
                card.max_y() - px(15.0),
                ctx.fade(palette.text_muted),
            );
            self.icon_cards.push((card, look));
        }
        if let (Some((first, _)), Some((last, _))) =
            (self.icon_cards.first(), self.icon_cards.last())
        {
            let area = Rect::new(first.min_x(), first.min_y(), width, last.max_y() - first.min_y());
            self.anchors.push(("icons", area));
        }
        if model.file_icon_sets.len() > 1 {
            let top = self.icon_cards.last().map_or(y, |(r, _)| r.max_y()) + px(32.0);
            self.paint_file_icon_sets(
                model,
                icons,
                SectionArea { layer, text, ctx, x0, top, width },
            );
        }
    }

    /// The file icons: ours (by language) or a set's (colored), each card with a preview of
    /// a few files and folders.
    pub(super) fn paint_file_icon_sets(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let y = ds::section_title(
            layer,
            text,
            ctx,
            x0,
            top,
            &i18n::t("settings.file_icons"),
            Some(&i18n::t("settings.file_icons_detail")),
        ) + px(16.0);
        // A few folders, then the files the languages show there (name, icon, language).
        const FOLDERS: &[&str] = &["src", "tests"];
        let cols = GRID_COLUMNS;
        let gap = px(14.0);
        let card_w = ((width - gap * (cols - 1) as f32) / cols as f32).floor();
        let card_h = px(108.0);
        let font = ctx.ui_font(0.0);
        let small = ctx.ui_font(-1.0);
        for (i, choice) in model.file_icon_sets.iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            let card = Rect::new(
                x0 + c as f32 * (card_w + gap),
                y + r as f32 * (card_h + gap),
                card_w,
                card_h,
            );
            let selected = choice.set == model.file_icon_set;
            let hovered = self.hover == Some(SettingsAction::SelectFileIcons(choice.set));
            let (border, line) = if selected {
                (palette.accent, px(2.0))
            } else if hovered {
                (palette.text_muted.with_alpha(0.6), ctx.line())
            } else {
                (palette.border, ctx.line())
            };
            layer.quad(
                Quad::filled(card, ctx.fade(palette.popup_background))
                    .rounded(px(12.0))
                    .bordered(line, ctx.fade(border)),
            );
            // The sample: the folders then the files, as the set (or ours) draws them.
            let ours =
                |name: &str, tint: icons::Tint| (icons.id_or_file(name), palette.tint(&tint));
            let sample = FOLDERS
                .iter()
                .map(|f| match choice.set.and_then(|s| icons.folder_icon_in(s, f, false)) {
                    Some(id) => (id, palette.tint(&icons::Tint::Own)),
                    None => ours("folder", icons::Tint::Folder),
                })
                .chain(model.icon_samples.iter().map(|(name, icon, language)| {
                    match choice.set.and_then(|s| icons.file_icon_in(s, name, language)) {
                        Some(id) => (id, palette.tint(&icons::Tint::Own)),
                        None => ours(icon, icons::Tint::Language(language.clone())),
                    }
                }));
            let size = px(20.0);
            let mut x = card.min_x() + px(16.0);
            for (id, color) in sample {
                if x + size > card.max_x() - px(12.0) {
                    break;
                }
                layer.glyphs.push(render_api::GlyphInstance {
                    key: icons.key(id, size as u16),
                    origin: kernel::Point::new(x.round(), (card.min_y() + px(19.0) + size).round()),
                    color: ctx.fade(color),
                });
                x += size + px(12.0);
            }
            let name = ds::ellipsize(text, &choice.name, &font, card.size.width - px(32.0));
            let name = text.shape_line(&name, &font);
            draw_text(
                layer,
                &name,
                card.min_x() + px(16.0),
                card.max_y() - px(36.0),
                ctx.fade(palette.text),
            );
            let about = ds::ellipsize(text, &choice.detail, &small, card.size.width - px(32.0));
            let about = text.shape_line(&about, &small);
            draw_text(
                layer,
                &about,
                card.min_x() + px(16.0),
                card.max_y() - px(15.0),
                ctx.fade(palette.text_muted),
            );
            self.file_icon_cards.push((card, choice.set));
        }
    }

    pub(super) fn paint_theme_cards(&mut self, model: &SettingsModel, area: SectionArea<'_, '_>) {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let font = ctx.ui_font(0.0);
        let cols = GRID_COLUMNS;
        let gap = px(14.0);
        let card_w = ((width - gap * (cols - 1) as f32) / cols as f32).floor();
        let preview_h = (card_w * 0.52).round();
        let card_h = preview_h + px(40.0);
        let shown: Vec<&ThemeOption> =
            model.themes.iter().filter(|t| self.theme_filter.keeps(t)).collect();
        for (i, theme) in shown.iter().enumerate() {
            let (c, r) = (i % cols, i / cols);
            let card = Rect::new(
                x0 + c as f32 * (card_w + gap),
                top + r as f32 * (card_h + gap),
                card_w,
                card_h,
            );
            let selected = theme.id == model.current_theme;
            let hovered = self.hover == Some(SettingsAction::SelectTheme(theme.id.clone()));
            let (border, width) = if selected {
                (palette.accent, px(2.0))
            } else if hovered {
                (palette.text_muted.with_alpha(0.6), ctx.line())
            } else {
                (palette.border, ctx.line())
            };
            layer.quad(
                Quad::filled(card, ctx.fade(palette.popup_background))
                    .rounded(px(12.0))
                    .bordered(width, ctx.fade(border)),
            );
            let preview = Rect::new(
                card.min_x() + px(8.0),
                card.min_y() + px(8.0),
                card.size.width - px(16.0),
                preview_h,
            );
            paint_preview(layer, preview, &theme.preview, ctx.scale);
            let row = Rect::new(
                card.min_x(),
                preview.max_y(),
                card.size.width,
                card.max_y() - preview.max_y(),
            );
            let kind = i18n::t(if theme.dark { "settings.dark" } else { "settings.light" });
            // The name, as much as fits beside its kind.
            let kind_w = text.shape_line(&kind, &ctx.ui_font(-1.5)).width + px(36.0);
            let room = (card.size.width - px(14.0) - kind_w).max(0.0);
            let name = ds::ellipsize(text, &theme.name, &font, room);
            let label = text.shape_line(&name, &font);
            let baseline = ds::optical_baseline(row.min_y(), row.size.height, &font);
            draw_text(layer, &label, card.min_x() + px(14.0), baseline, ctx.fade(palette.text));
            ds::badge(
                layer,
                text,
                ctx,
                ds::BadgeSpec {
                    label: &kind,
                    x: card.max_x() - px(12.0),
                    mid: row.min_y() + row.size.height / 2.0,
                    align_right: true,
                    tone: ds::Tone::Neutral,
                },
            );
            if selected {
                ds::badge(
                    layer,
                    text,
                    ctx,
                    ds::BadgeSpec {
                        label: "✓",
                        x: card.max_x() - px(12.0),
                        mid: card.min_y() + px(22.0),
                        align_right: true,
                        tone: ds::Tone::Accent,
                    },
                );
            }
            self.cards.push((card, theme.id.clone()));
        }
        // Follow the system theme: toggle + text.
        let rows = shown.len().div_ceil(cols) as f32;
        let mid = top + rows * (card_h + gap) + px(18.0);
        let hovered = self.hover == Some(SettingsAction::ToggleFollowSystem);
        let switch = ds::switch(layer, ctx, x0, mid, model.follow_system, hovered);
        let label = text.shape_line(&i18n::t("settings.follow_system"), &font);
        draw_text(
            layer,
            &label,
            switch.max_x() + px(12.0),
            ds::optical_baseline(switch.min_y(), switch.size.height, &font),
            ctx.fade(palette.text),
        );
        if let (Some(first), Some(last)) = (self.cards.first(), self.cards.last()) {
            let (a, b) = (first.0, last.0);
            self.anchors.push((
                "theme",
                Rect::new(a.min_x(), a.min_y(), b.max_x() - a.min_x(), b.max_y() - a.min_y()),
            ));
        }
        self.toggle = Rect::new(
            x0,
            switch.min_y() - px(6.0),
            switch.size.width + px(12.0) + label.width,
            switch.size.height + px(12.0),
        );
        self.anchors.push(("follow", self.toggle));
        // The theme configurator: a custom theme, starting from the current one.
        if !workbench_model::presence::command(workbench_model::THEME_STUDIO) {
            self.studio = Rect::default();
            return;
        }
        let mid = self.toggle.max_y() + px(34.0);
        let hovered = self.hover == Some(SettingsAction::OpenThemeStudio);
        self.studio = ds::choice(
            layer,
            text,
            ctx,
            ds::ChoiceSpec {
                x: x0,
                mid,
                label: &i18n::t("settings.theme_studio"),
                selected: false,
                hovered,
            },
        );
    }
}

/// Mini editor in the theme's colors: the window, the editor card, a few "code rows".
pub(super) fn paint_preview(layer: &mut Layer, r: Rect, t: &ThemePreview, scale: f32) {
    let px = |v: f32| (v * scale).round();
    layer.quad(Quad::filled(r, t.window).rounded(px(6.0)));
    let editor = Rect::new(
        r.min_x() + px(18.0),
        r.min_y() + px(8.0),
        r.size.width - px(26.0),
        r.size.height - px(16.0),
    );
    layer.quad(Quad::filled(editor, t.editor).rounded(px(5.0)));
    layer.quad(
        Quad::filled(
            Rect::new(r.min_x() + px(6.0), r.min_y() + px(8.0), px(6.0), px(6.0)),
            t.accent,
        )
        .rounded(px(2.0)),
    );
    let rows: [&[(f32, Rgba)]; 5] = [
        &[(0.18, t.keyword), (0.30, t.text)],
        &[(0.10, t.text), (0.22, t.keyword), (0.18, t.string)],
        &[(0.10, t.text), (0.40, t.string)],
        &[(0.16, t.keyword), (0.12, t.accent)],
        &[(0.24, t.text)],
    ];
    let line_h = ((editor.size.height - px(12.0)) / rows.len() as f32).floor();
    for (i, row) in rows.iter().enumerate() {
        let mut x = editor.min_x() + px(10.0);
        let y = editor.min_y() + px(8.0) + i as f32 * line_h;
        for &(w, color) in row.iter() {
            let width = (editor.size.width * w).round();
            layer.quad(
                Quad::filled(Rect::new(x, y, width, px(4.0)), color.with_alpha(0.9))
                    .rounded(px(2.0)),
            );
            x += width + px(6.0);
        }
    }
}
