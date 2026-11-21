//! The configurator as a view: drawn, clicked, dragged (the sliders), typed into (the name,
//! the search), stepped with the keys (a slider clicked last), what it asks the app (the
//! eyedropper, the decorations for every theme), the theme saved.

use super::*;
use ui_components::KeyReach;

impl Viewer for Studio {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        // Opened again: it keeps what it shows (the user's draft).
        if self.input.is_some() {
            return;
        }
        let Some(input) = data.downcast_ref::<StudioInput>() else { return };
        let base = input.themes.iter().position(|theme| theme.id == input.current).unwrap_or(0);
        let name = input
            .themes
            .get(base)
            .map_or_else(String::new, |theme| format!("{} {}", theme.name, t("studio.mine")));
        // Every theme read once (the gallery's thumbnails, its filter).
        self.thumbs = input.themes.iter().map(|th| th.draft()).collect();
        self.input = Some(input.clone());
        self.name.set_text(&name);
        self.load_base(base);
        // Nothing changes on open: the preview starts on the first edit.
        self.pending = None;
    }

    /// Split into another group: the other one starts from the same themes (a draft of its
    /// own).
    fn mirror_data(&self) -> Option<Box<dyn std::any::Any>> {
        self.input.clone().map(|i| Box::new(i) as Box<dyn std::any::Any>)
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.bounds = ctx.bounds;
        self.scale = ctx.scale;
        self.hits.clear();
        self.sliders.clear();
        let b = ctx.bounds;
        ctx.scene.push_layer(Some(b)).quad(Quad::filled(b, ctx.palette.editor_background));
        if let (Some(input), Some(draft)) = (self.input.clone(), self.draft.clone()) {
            self.paint_frame(ctx, &input, &draft);
        }
        // What the parts want of the frames, for the questions before the next one.
        self.with_controls(|c, parts| c.settle(parts));
    }

    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.last_input = Some(std::time::Instant::now());
        if self.gallery_open {
            let before = self.gallery_scroll;
            let max = (self.gallery_h - self.list.size.height * 0.6).max(0.0);
            self.gallery_scroll = (self.gallery_scroll + dy).clamp(0.0, max);
            return self.gallery_scroll != before;
        }
        let before = self.scrolled();
        *self.scroll_mut() += dy;
        self.clamp_scroll();
        self.scrolled() != before
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.last_input = Some(std::time::Instant::now());
        // The parts first: the picker (over everything while open), the settings' scrollbar,
        // a slider, what an open menu or the gallery lets through, the fields, a handle.
        match self.with_controls(|c, parts| c.press(parts, p, 1)) {
            Some((PICKER, _)) => {
                match self.picker_asked.take() {
                    Some(action) => self.picker_action(action),
                    None => self.sync_picker_color(),
                }
                // Pressed in the square / on a strip: the preview request waits for the drag
                // (returning it now would count as the click's action and no drag would start).
                if self.picker.dragging() {
                    return None;
                }
                return self.pending.take();
            }
            // A slider: held (dragged), its stop chosen at once; the keys step it afterwards.
            // The preview is taken with the drag (returning it now would end it).
            Some((SLIDERS, _)) => {
                self.follow_slider();
                return None;
            }
            Some((NAME | QUERY, _)) => {
                self.highlight = None;
                return self.pending.take();
            }
            Some((HANDLE, _)) => {
                self.focus_part(None);
                self.highlight = None;
                return None;
            }
            // The scrollbar; a press an open menu closed, or one the gallery didn't let through.
            Some(_) => return None,
            None => {}
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        if hit.is_some() && !matches!(hit, Some(Hit::Found(_))) {
            self.highlight = None;
        }
        match hit? {
            Hit::Category(c) => {
                self.show(c);
                self.preview_open = false;
                self.backdrop_shown = false;
            }
            Hit::Base(i) => self.load_base(i),
            Hit::BaseFilter(f) => self.base_filter = f,
            Hit::Choice(row, i) => {
                if let Some(draft) = &mut self.draft {
                    row.choose(draft, i);
                }
                self.backdrop_shown = matches!(row, Row::Backdrop(_) | Row::Floating(_));
                // From the gallery: it closes, the new effect (or the preset's) shown open.
                if matches!(row, Row::Add | Row::Preset) && self.gallery_open {
                    self.close_gallery();
                    let n = self.draft.as_ref().map_or(0, |d| d.decorations.layers.len());
                    if row == Row::Add {
                        self.open_layers.resize(n, false);
                        if let Some(last) = self.open_layers.last_mut() {
                            *last = true;
                        }
                    } else {
                        self.open_layers = vec![false; n];
                        self.sections.clear();
                    }
                }
                self.preview();
            }
            Hit::Fold(i) => self.toggle_layer(i),
            Hit::Toggle(i) => self.toggle_effect(i),
            Hit::Menu(i) => {
                let anchor = self.hits.iter().find(|(_, h)| *h == Hit::Menu(i)).map(|(r, _)| *r);
                self.asked_menu = anchor.map(|r| (r, effects::MenuItem::entries(i)));
            }
            Hit::Section(i, sec) => self.toggle_section(i, sec),
            Hit::Gallery(open) => {
                if open {
                    self.gallery_open = true;
                    self.gallery_scroll = 0.0;
                } else {
                    self.close_gallery();
                }
            }
            Hit::Global(g, value) => self.choose_global(g, value),
            Hit::Token(key) => {
                let color = self.draft.as_ref().and_then(|d| d.get(&key))?;
                let anchor = self
                    .hits
                    .iter()
                    .find(|(_, h)| *h == Hit::Token(key.clone()))
                    .map(|(r, _)| *r)?;
                let swatch = Rect::new(
                    anchor.min_x(),
                    anchor.min_y(),
                    anchor.size.width.min(40.0 * self.scale),
                    anchor.size.height,
                );
                self.open_picker(swatch, color);
                self.picking = Some((key, color));
            }
            Hit::ResetToken(key) => self.reset_token(&key),
            Hit::ResetRow(row) => self.reset_row(row),
            Hit::AddColor(layer, key) => self.change_colors(layer, key, true),
            Hit::DropColor(layer, key) => self.change_colors(layer, key, false),
            Hit::Overlay => self.use_everywhere(),
            // The fields' and the handles' presses are their parts'.
            Hit::Name | Hit::Search | Hit::Handle(_) => {}
            Hit::Found(n) => self.jump_to(n),
            Hit::PreviewMoving(moving) => self.preview_still = !moving,
            Hit::PreviewLight(light) => {
                let dark = self.draft.as_ref().is_some_and(rows::dark);
                self.preview_light = light == dark;
            }
            Hit::PreviewOpen => self.preview_open = !self.preview_open,
            Hit::Scope(global) => self.global = global,
            Hit::Save => self.save(),
            // Cancel (with changes) or Close: the changes dropped, the theme the window had
            // brought back and the studio closed — one press, whatever changed (a draft that
            // differs from its theme from the start, as fonts a system lacks can make it,
            // never needs a second one).
            Hit::Discard => {
                self.discard();
                self.pending = Some(ViewerAction::Request(AppRequest::CancelTheme));
            }
        }
        self.pending.take()
    }

    fn drag(&mut self, p: Point) -> bool {
        let Some((part, moved)) = self.with_controls(|c, parts| c.drag(parts, p)) else {
            return false;
        };
        match part {
            PICKER => {
                if moved {
                    self.sync_picker_color();
                }
                moved
            }
            SLIDERS => self.follow_slider(),
            // The scrollbar's thumb, an effect's card.
            _ => true,
        }
    }

    fn release(&mut self) -> bool {
        match self.with_controls(|c, parts| c.release(parts)) {
            // An effect's card dropped where it is.
            Some((HANDLE, _)) => self.finish_drag(),
            Some((_, changed)) => changed,
            None => false,
        }
    }

    /// A part fading (the scrollbar): a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.controls.next_frame()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_input = Some(std::time::Instant::now());
        let changed = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        // Under the open picker: a redraw over its controls.
        if self.controls.hovered() == Some(PICKER) {
            return self.controls.cursor_at(p).is_some();
        }
        self.hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        changed || self.hover.as_ref().is_some_and(|h| !matches!(h, Hit::Name | Hit::Search))
    }

    /// ⌘C: what's selected in the search while it has the keys.
    fn selected_text(&self) -> Option<String> {
        let query: &dyn Control = &self.query;
        self.controls.selected_text_with(|i| (i == QUERY).then_some(query))
    }

    fn wants_text(&self) -> bool {
        self.editing_name() || self.editing_query() || self.picker.is_open()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let took = self.with_controls(|c, parts| c.text(parts, typed));
        self.follow_keys();
        took.is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // Tab isn't the studio's (no part moves on with it).
        if key == ViewerKey::Tab {
            return false;
        }
        // The part with the keys: the picker, a field, the slider pressed last.
        if let Some(part) = self.with_controls(|c, parts| c.key(parts, key, reach)) {
            if part == SLIDERS {
                return self.follow_slider();
            }
            self.follow_keys();
            return true;
        }
        if key != ViewerKey::Escape {
            return false;
        }
        if self.gallery_open {
            self.close_gallery();
            return true;
        }
        // The slider pressed last lets go of the keys.
        if self.focused_part() == Some(SLIDERS) {
            self.focus_part(None);
            return true;
        }
        false
    }

    fn take_menu(&mut self) -> Option<(Rect, Vec<ViewMenuEntry>)> {
        self.asked_menu.take()
    }

    /// An effect's card (its header, its handle, its button): its menu.
    fn context_menu(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        if self.gallery_open || self.picker.is_open() {
            return None;
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        match hit? {
            Hit::Fold(i) | Hit::Handle(i) | Hit::Menu(i) => Some(effects::MenuItem::entries(i)),
            _ => None,
        }
    }

    fn command(&mut self, id: &str) -> bool {
        // A row of an effect's menu (`effect:<row>:<effect>`).
        if let Some((i, item)) = effects::MenuItem::parse(id) {
            self.menu_item(i, item);
            return true;
        }
        // The decorations from a preset (`preset:<id>`, `preset:none`); a category shown
        // (`category:<id>`, or `scroll-to:<id>`: a category or a section); a setting found
        // (`search:<text>`, the first result shown with `search:<text>!`); the backdrop's
        // style (`backdrop:glass`).
        if let Some(preset) = id.strip_prefix("preset:") {
            return self.use_preset(preset);
        }
        if let Some(section) =
            id.strip_prefix("scroll-to:").or_else(|| id.strip_prefix("category:"))
        {
            return self.scroll_to(section);
        }
        if let Some(query) = id.strip_prefix("search:") {
            let (query, go) = match query.strip_suffix('!') {
                Some(q) => (q, true),
                None => (query, false),
            };
            self.query.set_text(query);
            self.focus_part(Some(QUERY));
            if go {
                self.jump_to(0);
            }
            return true;
        }
        if id == "open-all" {
            self.open_everything();
            return true;
        }
        // The backdrop in a style (`backdrop:glass`), its dialog shown in the preview.
        if let Some(style) = id.strip_prefix("backdrop:")
            && let Some(kind) = theming::backdrop::BackdropKind::parse(style)
            && let Some(draft) = &mut self.draft
        {
            draft.backdrop.style = Some(kind);
            self.backdrop_shown = true;
            self.preview();
            return true;
        }
        if id == "gallery" {
            self.gallery_open = true;
            return true;
        }
        if id == "gallery:end" {
            self.gallery_open = true;
            self.gallery_to_end = true;
            return true;
        }
        if id == "still" {
            self.preview_still = true;
            return true;
        }
        if id == "search" || id == "find" {
            self.focus_part(Some(QUERY));
            self.query.input.select_all();
            return true;
        }
        let picking = self.picker.is_open();
        // ⌘A in the field with the keys; under the open picker (it takes no edit command),
        // the field's that has the focus there.
        let taken = self.with_controls(|c, parts| {
            c.command(parts, id).or_else(|| {
                let under = c.focused().filter(|_| picking)?;
                parts.get_mut(under)?.command(id).then_some(under)
            })
        });
        if taken.is_some() {
            return true;
        }
        // The color picked with the screen eyedropper (`#rrggbb`).
        if let Some(hex) = id.strip_prefix("screen-color:")
            && self.picker.is_open()
            && let Some(color) = Hex.parse(hex).map(|literal| literal.color)
        {
            self.picker.set_color(color, &Hex);
            self.sync_picker_color();
            return true;
        }
        false
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    /// Its effects move (the preview's, a gallery thumbnail under the mouse): frames come at
    /// a loader's pace (20 a second) while they do, none once the user is idle or chose
    /// still.
    fn is_loading(&self) -> bool {
        self.animating()
    }

    /// What it holds besides its draft: the effects' painters (their art leaves for the GPU).
    fn purge(&mut self) {
        self.live = None;
        self.gallery_painters.clear();
    }
}

impl Studio {
    /// What a key or a text in a part asked: the picker's color applied, given up or
    /// changed; a field let go, the search's first result shown, its results from the top.
    fn follow_keys(&mut self) {
        if let Some(action) = self.picker_asked.take() {
            self.picker_action(action);
        }
        if std::mem::take(&mut self.picker_recolored) {
            self.sync_picker_color();
        }
        if self.name_asked.take() == Some(parts::FieldAsk::Leave) {
            self.leave(NAME);
        }
        match self.query_asked.take() {
            Some(parts::FieldAsk::Leave) => self.leave(QUERY),
            Some(parts::FieldAsk::Jump) => self.jump_to(0),
            Some(parts::FieldAsk::Searched) => *self.scroll_mut() = 0.0,
            None => {}
        }
    }

    /// What a slider asked: its stop at the pointer (`true` if it changed), or a step from
    /// the keys (`false` with nothing to step).
    fn follow_slider(&mut self) -> bool {
        match self.slider_asked.take() {
            Some(parts::SliderAsk::Slide(area, row, x)) => self.slide(&area, row, x),
            Some(parts::SliderAsk::Step(row, delta, big)) => {
                let Some(draft) = &self.draft else { return false };
                let next =
                    ui_components::form::stepped(row.current(draft), row.stops(draft), delta, big);
                self.set_stop(row, next);
                true
            }
            None => false,
        }
    }
}
