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
        let Some(input) = self.input.clone() else { return };
        let Some(draft) = self.draft.clone() else { return };
        self.paint_frame(ctx, &input, &draft);
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
        // The settings' scrollbar (when shown).
        if !self.gallery_open
            && !self.picker.is_open()
            && let Some(offset) = self.bar.press(p)
        {
            *self.scroll_mut() = offset;
            return None;
        }
        if self.picker.is_open() {
            if let Some(action) = self.picker.mouse_down(p, &Hex) {
                self.picker_action(action);
            } else {
                self.sync_picker_color();
            }
            // Pressed in the square / on a strip: the preview request waits for the drag
            // (returning it now would count as the click's action and no drag would start).
            if self.picker.dragging() {
                return None;
            }
            return self.pending.take();
        }
        // A slider (where the panel shows it): held (dragged), its stop chosen at once; the
        // keys step it afterwards.
        let slider = self.sliders.iter().find(|(a, _)| a.rect.contains(p) && self.list.contains(p));
        if let Some((area, row)) = slider.cloned() {
            self.slider_held = Some(row);
            self.slider_focused = Some(row);
            self.editing_name = false;
            self.editing_query = false;
            self.slide(&area, row, p.x);
            // The preview is taken with the drag (returning it now would end it).
            return None;
        }
        let hit = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        // A menu open: a click anywhere else closes it.
        if self.menu_for.is_some() && !matches!(hit, Some(Hit::MenuItem(..)) | Some(Hit::Menu(_))) {
            self.menu_for = None;
            return None;
        }
        // The gallery open: only its own parts take clicks (it closes by ×, Esc or a choice).
        if self.gallery_open
            && !matches!(hit, Some(Hit::Gallery(_)) | Some(Hit::Choice(Row::Add | Row::Preset, _)))
        {
            return None;
        }
        self.editing_name = hit == Some(Hit::Name);
        if self.editing_name {
            self.name.click(p.x);
        }
        self.editing_query = matches!(hit, Some(Hit::Search));
        if self.editing_query {
            self.query.click(p.x);
        }
        self.slider_focused = None;
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
            Hit::Handle(i) => {
                self.drag = Some(effects::Drag { layer: i, y: p.y });
                return None;
            }
            Hit::Toggle(i) => self.toggle_effect(i),
            Hit::Menu(i) => self.menu_for = if self.menu_for == Some(i) { None } else { Some(i) },
            Hit::MenuItem(i, item) => self.menu_item(i, item),
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
                self.picker.open(swatch, color, ColorFormat::Hex, &Hex);
                self.picking = Some((key, color));
            }
            Hit::ResetToken(key) => self.reset_token(&key),
            Hit::ResetRow(row) => self.reset_row(row),
            Hit::AddColor(layer, key) => self.change_colors(layer, key, true),
            Hit::DropColor(layer, key) => self.change_colors(layer, key, false),
            Hit::Overlay => self.use_everywhere(),
            Hit::Name | Hit::Search => {}
            Hit::ClearSearch => {
                self.query.set_text("");
                self.editing_query = true;
            }
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
        if let Some(offset) = self.bar.drag(p) {
            *self.scroll_mut() = offset;
            return true;
        }
        if let Some(drag) = &mut self.drag {
            drag.y = p.y;
            return true;
        }
        if let Some(row) = self.slider_held {
            let area = self.sliders.iter().find(|(_, r)| *r == row).map(|(a, _)| a.clone());
            return area.is_some_and(|area| self.slide(&area, row, p.x));
        }
        if self.picker.mouse_drag(p, &Hex) {
            self.sync_picker_color();
            return true;
        }
        false
    }

    fn release(&mut self) -> bool {
        self.finish_drag()
            | self.slider_held.take().is_some()
            | self.picker.mouse_up()
            | self.bar.release()
    }

    /// Its scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.bar.next_frame()
    }

    fn hover(&mut self, p: Point) -> bool {
        self.last_input = Some(std::time::Instant::now());
        if self.picker.is_open() {
            self.picker.hover(p);
            return self.picker.over_control(p);
        }
        self.hover = self.hits.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, h)| h.clone());
        let bar = !self.gallery_open && self.bar.hover(p);
        bar || self.hover.as_ref().is_some_and(|h| !matches!(h, Hit::Name | Hit::Search))
    }

    fn wants_text(&self) -> bool {
        self.editing_name || self.editing_query || self.picker.is_open()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        if self.picker.is_open() {
            self.picker.type_text(typed, &Hex);
            self.sync_picker_color();
            return true;
        }
        if self.editing_name {
            self.name.insert(typed);
            return true;
        }
        if self.editing_query {
            self.query.insert(typed);
            *self.scroll_mut() = 0.0;
            return true;
        }
        false
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if self.picker.is_open() {
            match key {
                ViewerKey::Escape => self.picker_action(PickerAction::Cancel),
                ViewerKey::Enter => {
                    if let Some(action) = self.picker.confirm() {
                        self.picker_action(action);
                    }
                }
                ViewerKey::Backspace => {
                    self.picker.backspace(&Hex);
                    self.sync_picker_color();
                }
                ViewerKey::Left => self.picker.move_cursor(false, false),
                ViewerKey::Right => self.picker.move_cursor(true, false),
                _ => return false,
            }
            return true;
        }
        if self.editing_name {
            match key {
                ViewerKey::Backspace => self.name.backspace(false),
                ViewerKey::Left => self.name.move_left(false, false),
                ViewerKey::Right => self.name.move_right(false, false),
                ViewerKey::Enter | ViewerKey::Escape => self.editing_name = false,
                _ => return false,
            }
            return true;
        }
        if self.editing_query {
            match key {
                ViewerKey::Backspace => self.query.backspace(false),
                ViewerKey::Left => self.query.move_left(false, false),
                ViewerKey::Right => self.query.move_right(false, false),
                // Enter: the first found shown.
                ViewerKey::Enter => self.jump_to(0),
                ViewerKey::Escape => {
                    self.query.set_text("");
                    self.editing_query = false;
                }
                _ => return false,
            }
            return true;
        }
        if key == ViewerKey::Escape && self.menu_for.take().is_some() {
            return true;
        }
        if key == ViewerKey::Escape && self.gallery_open {
            self.close_gallery();
            return true;
        }
        // The slider clicked last: the arrows step it (Shift: a bigger step).
        if let Some(row) = self.slider_focused {
            let delta = match key {
                ViewerKey::Left | ViewerKey::Down => -1,
                ViewerKey::Right | ViewerKey::Up => 1,
                ViewerKey::Escape => {
                    self.slider_focused = None;
                    return true;
                }
                _ => return false,
            };
            let Some(draft) = &self.draft else { return false };
            let next = ui_components::form::stepped(
                row.current(draft),
                row.stops(draft),
                delta,
                reach.extend,
            );
            self.set_stop(row, next);
            return true;
        }
        false
    }

    fn command(&mut self, id: &str) -> bool {
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
            self.editing_query = true;
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
            self.editing_query = true;
            self.editing_name = false;
            self.query.select_all();
            return true;
        }
        if id == "select_all" && (self.editing_name || self.editing_query) {
            if self.editing_name {
                self.name.select_all();
            } else {
                self.query.select_all();
            }
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
