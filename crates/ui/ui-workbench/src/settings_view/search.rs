//! The settings' search: every setting as a searchable entry, the matches (by words and meaning),
//! the results drawn, an entry opened.

use super::*;

impl SettingsView {
    /// [`Self::entries`], kept while the model keeps its shape.
    pub(super) fn entries_of(&mut self, model: &SettingsModel) -> std::sync::Arc<Vec<Entry>> {
        let shape = entries_shape(model);
        match &self.entries {
            Some((made, entries)) if *made == shape => std::sync::Arc::clone(entries),
            _ => {
                let entries = std::sync::Arc::new(Self::entries(model));
                self.entries = Some((shape, std::sync::Arc::clone(&entries)));
                entries
            }
        }
    }

    pub(super) fn entries(model: &SettingsModel) -> Vec<Entry> {
        let t = i18n::t;
        let keywords = |block: &str| i18n::in_every_language(&format!("settings.keywords_{block}"));
        let mut out = vec![
            Entry {
                section: APPEARANCE,
                anchor: "theme",
                target: None,
                title: t("settings.theme"),
                detail: t("settings.theme_detail"),
                keywords: keywords("theme"),
                ..Entry::default()
            },
            Entry {
                section: APPEARANCE,
                anchor: "follow",
                target: None,
                title: t("settings.follow_system"),
                detail: String::new(),
                keywords: keywords("follow"),
                ..Entry::default()
            },
            Entry {
                section: ICONS,
                anchor: "icons",
                target: None,
                title: t("settings.icons"),
                detail: t("settings.icons_detail"),
                keywords: keywords("icons"),
                ..Entry::default()
            },
            Entry {
                section: MODULES,
                anchor: "frameworks",
                target: None,
                title: t("settings.frameworks"),
                detail: t("settings.frameworks_detail"),
                keywords: keywords("frameworks"),
                ..Entry::default()
            },
            Entry {
                section: LANGUAGE,
                anchor: "language",
                target: None,
                title: t("settings.language"),
                detail: t("settings.language_detail"),
                keywords: keywords("language"),
                ..Entry::default()
            },
        ];
        // The blocks, after the app's pages' own.
        let at = out.iter().position(|e| e.anchor == "frameworks").map_or(out.len(), |i| i + 1);
        // Each block, and each of its rows (a search finds a row by its label and
        // description).
        let contributed = model.blocks.iter().flat_map(|b| {
            let block = Entry {
                section: b.page,
                anchor: b.id,
                target: None,
                title: b.block.title.clone(),
                detail: b.block.detail.clone(),
                keywords: b.keywords.clone(),
                language: b.language.clone(),
                advanced: b.advanced,
            };
            let rows = b.block.rows.iter().filter_map(SettingsRow::searchable).map(
                |(key, label, detail)| Entry {
                    section: b.page,
                    anchor: b.id,
                    target: Some(key.to_owned()),
                    title: label.to_owned(),
                    detail: detail.to_owned(),
                    keywords: String::new(),
                    language: b.language.clone(),
                    advanced: b.advanced,
                },
            );
            std::iter::once(block).chain(rows)
        });
        out.splice(at..at, contributed);
        out.push(Entry {
            section: KEYMAP,
            anchor: "keymap",
            target: None,
            title: t("settings.keymap"),
            detail: t("settings.keymap_detail"),
            keywords: keywords("keymap"),
            ..Entry::default()
        });
        for theme in &model.themes {
            out.push(Entry {
                section: APPEARANCE,
                anchor: "theme",
                target: Some(theme.id.clone()),
                title: theme.name.clone(),
                detail: t(if theme.dark { "settings.dark" } else { "settings.light" }),
                keywords: keywords("theme_card"),
                ..Entry::default()
            });
        }
        out
    }

    /// The results for the searched text: word matching (title, description, keywords,
    /// without diacritics) and, if the semantic model is active, by meaning.
    pub(super) fn search(&mut self, entries: &[Entry]) {
        let query = ui_kit::fold(self.query.text().trim());
        if query.is_empty() {
            self.results.clear();
            self.searched = None;
            return;
        }
        let fingerprint = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for e in entries {
                (e.section, e.anchor, &e.target, &e.title, &e.detail, &e.keywords).hash(&mut h);
            }
            h.finish()
        };
        // Made once per entries (another language, a theme added: made again), in the
        // background: by words until they're here, then searched again.
        if let Some((made_for, rx)) = &self.making
            && let Ok(vectors) = rx.try_recv()
        {
            self.vectors = vectors;
            self.vectors_for = Some(*made_for);
            self.making = None;
            // Searched again, by meaning too.
            self.searched = None;
        }
        if let Some(embedder) = &self.embedder
            && self.vectors_for != Some(fingerprint)
            && self.making.as_ref().is_none_or(|(f, _)| *f != fingerprint)
        {
            let texts: Vec<String> = entries
                .iter()
                .map(|e| format!("{} {} {}", e.title, e.detail, e.keywords))
                .collect();
            let (embedder, (tx, rx)) =
                (std::sync::Arc::clone(&embedder.0), std::sync::mpsc::channel());
            kernel::work::run("settings-search", move || {
                let _ = tx.send(texts.iter().map(|t| embedder.embed(t)).collect());
            });
            self.making = Some((fingerprint, rx));
        }
        let ready = self.vectors_for == Some(fingerprint);
        let searched = (query.clone(), fingerprint);
        if self.searched.as_ref() == Some(&searched) {
            return;
        }
        self.searched = Some(searched);
        self.results.clear();
        // By meaning only once the entries' vectors are theirs.
        let query_vector = self.embedder.as_ref().filter(|_| ready).map(|e| e.0.embed(&query));
        let mut scored: Vec<(usize, f32)> = entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let hay = ui_kit::fold(&format!("{} {} {}", e.title, e.detail, e.keywords));
                let words: Vec<&str> = query.split_whitespace().collect();
                let lexical =
                    words.iter().filter(|w| hay.contains(*w)).count() as f32 / words.len() as f32;
                let title = ui_kit::fold(&e.title);
                let title_hit = if title == query {
                    3.0
                } else if title.starts_with(&query) {
                    1.5
                } else if title.contains(&query) {
                    1.0
                } else {
                    0.0
                };
                let semantic = match (&query_vector, self.vectors.get(i)) {
                    (Some(q), Some(v)) if q.len() == v.len() => {
                        q.iter().zip(v).map(|(a, b)| a * b).sum::<f32>()
                    }
                    _ => 0.0,
                };
                let score = lexical * 2.0 + title_hit + semantic.max(0.0);
                (lexical >= 1.0 || semantic > 0.45).then_some((i, score))
            })
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        // A single entry per anchor (e.g. several themes → "Theme").
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (i, _) in scored {
            let key = (entries[i].section, entries[i].anchor);
            if entries[i].anchor != "theme" || !seen.contains(&key) {
                seen.push(key);
                self.results.push(i);
            }
        }
        self.selected = self.selected.min(self.results.len().saturating_sub(1));
    }

    pub(super) fn open_entry(&mut self, model: &SettingsModel, index: usize) {
        let entries = self.entries_of(model);
        let Some(entry) = entries.get(index) else { return };
        self.section = Some(entry.section);
        // Under "Advanced": opened; another language's: the app chooses it.
        if entry.advanced && !self.advanced_open.contains(&entry.section) {
            self.advanced_open.push(entry.section);
        }
        self.wanted_language = entry.language.clone();
        self.scroll = 0.0;
        let mut flash = crate::anim::Tween::new(1.0, std::time::Duration::from_millis(1400));
        flash.retarget(0.0);
        self.flash = Some((entry.anchor, entry.target.clone(), flash));
        self.reveal = true;
        self.clear_query();
    }

    /// The search field, in the header left of `esc` (the app's: × when it has text, its
    /// caret while it has the focus).
    pub(super) fn paint_search_field(
        &mut self,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        ctx: &Ctx<'_>,
    ) {
        let field = self.modal.header_field();
        let layer = scene.push_layer(Some(field));
        let focused = self.controls.focused() == Some(super::QUERY);
        let placeholder = i18n::t("settings.search_placeholder");
        self.query.paint(layer, text, icons, ctx, (field, focused), &placeholder);
    }

    /// The settings search results: the section (icon), the title and the description.
    pub(super) fn paint_results(
        &mut self,
        entries: &[Entry],
        sections: &[ModalSection<'_>],
        icons: &Icons,
        area: SectionArea<'_, '_>,
    ) {
        let SectionArea { layer, text, ctx, x0, top, width } = area;
        let palette = ctx.palette;
        let px = |v: f32| ctx.px(v);
        let title = i18n::tr!("settings.results_for", query = self.query.text().trim().to_owned());
        let mut y = ds::section_title(layer, text, ctx, x0, top, &title, None) + px(14.0);
        if self.results.is_empty() {
            let area = Rect::new(x0, y, width, px(120.0));
            ds::empty_state(layer, text, ctx, area, &i18n::t("settings.no_results"), None);
            return;
        }
        let font = ctx.ui_font(0.0);
        let small = ctx.ui_font(-1.0);
        let row_h = px(54.0);
        for (slot, &index) in self.results.iter().enumerate() {
            let entry = &entries[index];
            let r = Rect::new(x0 - px(10.0), y, width + px(20.0), row_h);
            if slot == self.selected {
                layer.quad(
                    Quad::filled(r, ctx.fade(palette.accent.with_alpha(0.10))).rounded(px(10.0)),
                );
            }
            let section = sections.iter().find(|s| s.id == entry.section);
            if let Some(id) = section.and_then(|s| icons.id(s.icon)) {
                let size = px(16.0);
                ui_kit::draw_icon(
                    layer,
                    icons,
                    id,
                    Rect::new(r.min_x() + px(14.0), r.min_y() + (row_h - size) / 2.0, size, size),
                    ctx.fade(palette.accent),
                );
            }
            let tx = r.min_x() + px(42.0);
            let shaped = text.shape_line(&entry.title, &font);
            draw_text(
                layer,
                &shaped,
                tx,
                ds::optical_baseline(r.min_y() + px(7.0), px(20.0), &font),
                ctx.fade(palette.text),
            );
            if let Some(section) = section {
                ds::badge(
                    layer,
                    text,
                    ctx,
                    ds::BadgeSpec {
                        label: section.label,
                        x: tx + shaped.width + px(10.0),
                        mid: r.min_y() + px(17.0),
                        align_right: false,
                        tone: ds::Tone::Neutral,
                    },
                );
            }
            if !entry.detail.is_empty() {
                let detail = ds::ellipsize(text, &entry.detail, &small, r.size.width - px(60.0));
                let shaped = text.shape_line(&detail, &small);
                draw_text(
                    layer,
                    &shaped,
                    tx,
                    ds::optical_baseline(r.min_y() + px(28.0), px(20.0), &small),
                    ctx.fade(palette.text_muted),
                );
            }
            self.result_rects.push(r);
            y += row_h + px(4.0);
        }
    }
}

/// What the entries are made from, in short: the language (their words), the blocks and
/// their rows, the themes, the modules, the frameworks, the project (a change of any makes
/// them again).
fn entries_shape(model: &SettingsModel) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    i18n::locale().hash(&mut h);
    model.current_language.hash(&mut h);
    for block in &model.blocks {
        (block.id, block.page, &block.language, &block.keywords).hash(&mut h);
        (&block.block.title, &block.block.detail, block.block.rows.len()).hash(&mut h);
    }
    for theme in &model.themes {
        theme.id.hash(&mut h);
    }
    for module in &model.modules {
        (&module.id, module.enabled).hash(&mut h);
    }
    (model.frameworks.len(), &model.project, &model.code_language).hash(&mut h);
    h.finish()
}
