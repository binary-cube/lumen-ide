//! The settings painted: the window, the page's content revealed as it scrolls, the bar,
//! the footer's keys, and the scroll kept within what was painted.

use super::*;

impl SettingsView {
    pub fn paint(
        &mut self,
        model: &SettingsModel,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        self.forget_painted();
        // The entries are only needed while searching.
        let entries = if self.has_query() { self.entries_of(model) } else { Default::default() };
        self.search(&entries);
        let pages = pages(&model.blocks);
        let groups = group_labels();
        let sections = modal_sections(&pages, &groups);
        self.section_ids = sections.iter().map(|s| s.id).collect();
        let title = i18n::t("settings.title");
        let subtitle = i18n::t("settings.subtitle");
        let hint_texts = self.key_hints();
        let hints: Vec<KeyHint<'_>> =
            hint_texts.iter().map(|(keys, what)| (*keys, what.as_str())).collect();
        let spec = ModalSpec {
            title: &title,
            subtitle: Some(&subtitle),
            icon: Some("gear"),
            size: ModalSize::XLarge,
            sections: &sections,
            active: if self.has_query() { None } else { Some(self.active()) },
            // The keys that work now, as the list modals show theirs (the footer stays at the
            // bottom; the navigation and the content scroll above it).
            hints: &hints,
            buttons: &[],
            status: None,
            nav_header: 0.0,
            header_field: SEARCH_FIELD_W,
            fit: None,
            fit_width: None,
            title_widens: true,
            closable: true,
            zoom: None,
        };
        let _dialog = self.modal.told(&spec);
        let Some((content, ctx)) = self.modal.paint(&spec, icons, text, scene, palette) else {
            return;
        };
        self.paint_search_field(icons, text, scene, &ctx);
        let layer = scene.push_layer(Some(content));
        let pad = ctx.px(28.0);
        let x0 = content.min_x() + pad;
        let width = content.size.width - 2.0 * pad;
        self.content = content;
        // Whole pixels: a fractional scroll (trackpad momentum) would move text and boxes
        // out of step by a pixel (jitter).
        let y = content.min_y() + ctx.px(24.0) - self.scroll.round();
        if self.has_query() {
            self.paint_results(
                &entries,
                &sections,
                icons,
                SectionArea { layer, text, ctx: &ctx, x0, top: y, width },
            );
            self.clamp_scroll(ctx.px(24.0));
            // No cards among the results: their previews let go.
            self.paint_card_pictures(scene, text, palette, ctx.scale);
            self.paint_bar(scene, palette, ctx.px(1.0));
            return;
        }
        let active = self.active();
        let area = SectionArea { layer, text, ctx: &ctx, x0, top: y, width };
        // The app's pages draw their own content first; a page's blocks follow it.
        let fixed = match active {
            MODULES => {
                self.paint_modules(model, icons, area);
                true
            }
            ICONS => {
                self.paint_icon_sets(model, icons, area);
                true
            }
            KEYMAP => {
                let label = i18n::t("settings.keymap");
                self.paint_keymap(model, icons, area, &label);
                true
            }
            LANGUAGE => {
                let label = i18n::t("settings.language");
                self.paint_languages(model, area, &label);
                true
            }
            APPEARANCE => {
                self.paint_appearance(model, area);
                true
            }
            _ => false,
        };
        if !fixed || model.blocks.iter().any(|b| b.page == active) {
            let top = if fixed { self.painted_bottom() + ctx.px(24.0) } else { y };
            self.paint_blocks(
                model,
                icons,
                active,
                SectionArea { layer, text, ctx: &ctx, x0, top, width },
            );
        }
        self.clamp_scroll(ctx.px(24.0));
        self.paint_revealed(layer, &ctx);
        // The cards' pictures, over the content's layer (the modules push their own).
        self.paint_card_pictures(scene, text, palette, ctx.scale);
        self.paint_bar(scene, palette, ctx.px(1.0));
        // A dropdown's list, over everything, from its field (where it is now).
        if let Some(open) = &self.picker_for
            && self.picker.is_open()
        {
            let field = self
                .dropdowns
                .iter()
                .find(|d| d.block == open.block && d.key == open.key)
                .map_or(open.field, |d| d.field);
            self.picker.paint(scene, text, icons, &ctx, (field, self.viewport), "");
        }
    }

    /// The content's scrollbar along its right edge, in its margin (nothing when it fits).
    fn paint_bar(&mut self, scene: &mut Scene, palette: &Palette, scale: f32) {
        let area = self.content;
        self.bar.paint(scene, palette, scale, area, self.content_height, self.scroll.round());
    }

    /// The keys that work in the settings now, for its footer: while a field is written,
    /// ↵ saves and Esc gives it up; while searching, ↑ ↓ walk the results, ↵ opens one and
    /// Esc clears the search; else ↑ ↓ change the page, typing searches and Esc closes.
    pub fn key_hints(&self) -> Vec<(&'static [&'static str], String)> {
        let t = i18n::t;
        if self.editing_text() || self.shortcuts_have_keys() {
            return vec![(&["↵"], t("settings.hint_save")), (&["esc"], t("settings.hint_cancel"))];
        }
        if self.has_query() {
            return vec![
                (&["↑", "↓"], t("settings.hint_results")),
                (&["↵"], t("settings.hint_open")),
                (&["esc"], t("settings.hint_clear")),
            ];
        }
        vec![
            (&["↑", "↓"], t("settings.hint_pages")),
            (&["A–Z"], t("settings.hint_type")),
            (&["esc"], t("keys.close")),
        ]
    }

    /// Forgets what the last paint recorded (the clickable areas, the anchors), before
    /// painting again.
    fn forget_painted(&mut self) {
        self.shortcuts.forget();
        self.cards.clear();
        self.filter_rects.clear();
        self.languages.clear();
        self.keymap_chips.clear();
        self.binding_rows.clear();
        self.toggle = Rect::default();
        self.icon_cards.clear();
        self.file_icon_cards.clear();
        self.framework_rows.clear();
        self.module_rows.clear();
        self.module_hits.clear();
        self.block_hits.clear();
        self.dropdowns.clear();
        self.sliders.clear();
        self.text_fields.clear();
        self.secret_fields.clear();
        // Nothing painted yet (not 0: a page scrolled past its top paints above 0, and a 0
        // counted as its bottom put the blocks at a fixed place and made it grow each frame).
        self.blocks_bottom = f32::NEG_INFINITY;
        self.row_anchors.clear();
        self.studio = Rect::default();
        self.result_rects.clear();
        self.anchors.clear();
        self.pictures.pending.clear();
    }

    /// A search result was opened: its element is brought into view (next frame) and
    /// highlighted.
    fn paint_revealed(&mut self, layer: &mut Layer, ctx: &Ctx<'_>) {
        let flashed = self.flash_rect();
        if std::mem::take(&mut self.reveal)
            && let Some(r) = flashed
        {
            let view = self.content;
            let margin = ctx.px(24.0);
            if r.min_y() < view.min_y() + margin || r.max_y() > view.max_y() - margin {
                self.scroll += r.min_y() - (view.min_y() + view.size.height / 3.0);
                self.limit_scroll();
            }
        }
        let value = self.flash.as_ref().map_or(0.0, |(_, _, t)| t.value());
        Self::paint_flash(layer, ctx, flashed, value);
    }

    /// Measures the content just painted (its height doesn't depend on the scroll) and
    /// keeps the scroll within bounds. Only rects from this paint count: stale ones from
    /// another section, or positions from a previous scroll, made the content look taller
    /// (scrolling past the end) and the limit jump between frames (jitter with momentum).
    pub(super) fn clamp_scroll(&mut self, margin: f32) {
        let top = self.content.min_y() - self.scroll.round();
        self.content_height = self.painted_bottom().max(top) - top + margin;
        if std::mem::take(&mut self.scroll_to_end) {
            self.scroll = f32::MAX;
        }
        self.limit_scroll();
    }

    /// Where what was painted last ends (its lowest element).
    fn painted_bottom(&self) -> f32 {
        let top = self.content.min_y() - self.scroll.round();
        self.cards
            .iter()
            .map(|(r, _)| r.max_y())
            .chain(self.languages.iter().map(|(r, _)| r.max_y()))
            .chain(self.keymap_chips.iter().map(|(r, _)| r.max_y()))
            .chain(self.binding_rows.iter().map(Rect::max_y))
            .chain(self.framework_rows.iter().map(|(r, _)| r.max_y()))
            .chain(self.module_rows.iter().map(Rect::max_y))
            .chain(self.module_hits.iter().map(|(r, _)| r.max_y()))
            .chain(self.result_rects.iter().map(Rect::max_y))
            .chain(
                [self.toggle, self.studio]
                    .iter()
                    .filter(|r| **r != Rect::default())
                    .map(Rect::max_y),
            )
            .chain([self.blocks_bottom])
            .chain(self.icon_cards.iter().map(|(r, _)| r.max_y()))
            .chain(self.file_icon_cards.iter().map(|(r, _)| r.max_y()))
            .fold(top, f32::max)
    }

    pub(super) fn limit_scroll(&mut self) {
        let max = (self.content_height - self.content.size.height).max(0.0);
        self.scroll = self.scroll.clamp(0.0, max);
    }

    /// The mouse wheel: over the navigation it scrolls the navigation (when it's taller than
    /// its room; at its end nothing moves), anywhere else the content. `true` if it scrolled.
    pub fn scroll_by(&mut self, dy: f32) -> bool {
        // A dropdown's list open: the wheel scrolls it (nothing behind it moves).
        if self.picker.is_open() {
            return self.picker.scroll(dy);
        }
        if let Some(p) = self.mouse
            && self.modal.over_nav(p)
        {
            return self.modal.scroll_nav(p, dy);
        }
        let before = self.scroll;
        self.scroll += dy;
        self.limit_scroll();
        self.scroll != before
    }
}
