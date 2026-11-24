//! The mouse over the settings: what a click gives, the hover, a slider or a bar dragged,
//! the dropdown's list, the window's controls.

use super::*;

impl SettingsView {
    /// The action at point `p` (a click outside the panel does nothing). Clicking
    /// a section activates it (no action for the app).
    pub fn hit(&mut self, p: Point) -> Option<SettingsAction> {
        // Its controls first: an open dropdown list takes the click (a choice, or closed by
        // one outside), a scrollbar's thumb is held or jumped to (nothing under it reached).
        if let Some((part, _)) = self.with_controls(|c, parts| c.press(parts, p, 1)) {
            self.limit_scroll();
            // A search field's × emptied it: the results with it.
            if part == QUERY && self.query.take_cleared() {
                self.clear_query();
            }
            self.shortcuts.filter.take_cleared();
            let event = self.picker.take_event();
            return self.picked(event);
        }
        // A dropdown's field (in the content): its list opens.
        if self.modal.hit(p) == Some(ModalHit::Content)
            && let Some(dropdown) = self.dropdowns.iter().find(|d| d.field.contains(p)).cloned()
        {
            let items = dropdown
                .choices
                .iter()
                .map(|(_, name)| ui_components::picker::PickerItem {
                    label: name.clone(),
                    ..Default::default()
                })
                .collect();
            let chosen = dropdown.choices.iter().position(|(v, _)| *v == dropdown.current);
            self.picker.open(items, chosen);
            self.picker_for = Some(dropdown);
            return None;
        }
        // The frame first: the content's rows scrolled under the header keep their rects
        // but are hidden there, so a click on esc closes instead of reaching a row.
        let at = self.modal.hit(p);
        if at == Some(ModalHit::Close) {
            return Some(SettingsAction::Close);
        }
        let in_content = at == Some(ModalHit::Content);
        // The shortcuts' editor: its search, its commands, their buttons.
        if in_content && self.shortcuts.over(p) {
            return self.shortcuts.click(p).map(SettingsAction::Shortcuts);
        }
        // A block's text field: a click on it starts writing it; while one is written, a
        // click elsewhere saves it (and does nothing else).
        let field = self.text_fields.iter().find(|(r, ..)| in_content && r.contains(p)).cloned();
        if let Some(editing) = &self.editing {
            if field.as_ref().is_some_and(|(_, b, k, _)| *b == editing.block && *k == editing.key) {
                return None;
            }
            return self.commit_text();
        }
        if let Some((_, block, key, value)) = field {
            // A secret is written from empty, as dots (what's stored is never shown).
            let secret = self.secret_fields.contains(&(block, key));
            let mut input = ui_kit::TextInput::new(if secret { "" } else { &value });
            input.set_masked(secret);
            input.select_all();
            self.editing = Some(Editing { block, key, input });
            return None;
        }
        if let Some(i) = self.filter_rects.iter().position(|r| in_content && r.contains(p)) {
            self.theme_filter = ThemeFilter::ALL[i];
            return None;
        }
        if let Some(i) = self.result_rects.iter().position(|r| in_content && r.contains(p)) {
            self.selected = i;
            return Some(SettingsAction::OpenResult);
        }
        match at? {
            ModalHit::Close => Some(SettingsAction::Close),
            // Its footer has no buttons, nor a zoom.
            ModalHit::Button(_) | ModalHit::Zoom(_) => None,
            ModalHit::Section(id) => {
                self.section = Some(id);
                self.scroll = 0.0;
                None
            }
            ModalHit::Content => {
                let action = self.content_hit(p);
                if let Some(SettingsAction::ToggleAdvanced(page)) = action {
                    self.toggle_advanced(page);
                    return None;
                }
                // On a slider: the following moves drag it.
                self.slider_held = match &action {
                    Some(SettingsAction::Choose { value, .. }) => self
                        .sliders
                        .iter()
                        .position(|(a, ..)| a.rect.contains(p))
                        .map(|i| (i, *value)),
                    _ => None,
                };
                action
            }
        }
    }

    fn content_hit(&self, p: Point) -> Option<SettingsAction> {
        if self.studio.contains(p) {
            return Some(SettingsAction::OpenThemeStudio);
        }
        if self.toggle.contains(p) {
            return Some(SettingsAction::ToggleFollowSystem);
        }
        if let Some((_, set)) = self.file_icon_cards.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectFileIcons(*set));
        }
        if let Some((_, style)) = self.icon_cards.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectIcons(*style));
        }
        if let Some((_, id)) = self.framework_rows.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::ToggleFramework(id.clone()));
        }
        if let Some((area, block, key, values)) =
            self.sliders.iter().find(|(a, ..)| a.rect.contains(p))
        {
            let value = *values.get(area.nearest(p.x)?)?;
            return Some(SettingsAction::Choose { block, key, value });
        }
        if let Some((_, action)) = self.block_hits.iter().find(|(r, _)| r.contains(p)) {
            return Some(action.clone());
        }

        if let Some((_, block, key, value)) = self.text_fields.iter().find(|(r, ..)| r.contains(p))
        {
            return Some(SettingsAction::SetText { block, key, value: value.clone() });
        }
        if let Some((_, action)) = self.module_hits.iter().find(|(r, _)| r.contains(p)) {
            return Some(action.clone());
        }
        if let Some((_, code)) = self.languages.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectLanguage(code.clone()));
        }
        if let Some((_, id)) = self.keymap_chips.iter().find(|(r, _)| r.contains(p)) {
            return Some(SettingsAction::SelectKeymap(id.clone()));
        }
        self.cards
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|(_, id)| SettingsAction::SelectTheme(id.clone()))
    }

    pub fn hover(&mut self, p: Point) -> bool {
        self.mouse = Some(p);
        // Its controls: an open list takes the mouse over everything.
        let bar = self.with_controls(|c, parts| c.hover(parts, Some(p)));
        if self.picker.is_open() {
            return bar;
        }
        // Rows scrolled under the header aren't lit there.
        let in_content = self.modal.hit(p) == Some(ModalHit::Content);
        let shortcut = if in_content { p } else { Point::new(f32::MIN, f32::MIN) };
        let frame = self.modal.hover(p) | self.shortcuts.hover(shortcut);
        let hover = match self.modal.hit(p) {
            Some(ModalHit::Content) => self.content_hit(p),
            _ => None,
        };
        let filter = self.filter_rects.iter().position(|r| r.contains(p));
        let changed = hover != self.hover || filter != self.filter_hover;
        self.filter_hover = filter;
        self.hover = hover;
        frame || changed || bar
    }

    /// A slider is held (pressed): the mouse's moves drag it.
    pub fn slider_held(&self) -> bool {
        self.slider_held.is_some()
    }

    /// The held slider dragged to `p`: the value of the stop nearest to it, when it changes.
    pub fn drag(&mut self, p: Point) -> Option<SettingsAction> {
        let (i, last) = self.slider_held?;
        let (area, block, key, values) = self.sliders.get(i)?;
        let value = *values.get(area.nearest(p.x)?)?;
        if value == last {
            return None;
        }
        self.slider_held = Some((i, value));
        Some(SettingsAction::Choose { block, key, value })
    }

    /// The mouse was released (a held slider lets go). `true` if one was held.
    pub fn release(&mut self) -> bool {
        let bars = self.with_controls(|c, parts| c.release(parts)).is_some();
        self.slider_held.take().is_some() | bars
    }

    /// Something clickable is under the mouse (the cursor becomes a hand).
    pub fn over_clickable(&self, p: Point) -> bool {
        // A dropdown's list open: a hand on its rows only (never what's under it).
        if self.picker.is_open() {
            return Control::cursor(&self.picker, p) == Some(ui_components::ViewerCursor::Pointer);
        }
        let in_content = self.modal.hit(p) == Some(ModalHit::Content);
        matches!(self.modal.hit(p), Some(ModalHit::Section(_)))
            || (in_content
                && (self.content_hit(p).is_some()
                    || self.dropdowns.iter().any(|d| d.field.contains(p))
                    || self.shortcuts.over(p)
                    || self.result_rects.iter().any(|r| r.contains(p))
                    || self.filter_rects.iter().any(|r| r.contains(p))))
            || Control::cursor(&self.query, p) == Some(ui_components::ViewerCursor::Pointer)
    }

    /// A dropdown's list is open (it has the keys: ↑ ↓ ↵, Esc closes it).
    pub fn picker_open(&self) -> bool {
        self.picker.is_open()
    }

    /// A key for an open dropdown's list: what it chose, when it did.
    pub fn picker_key(&mut self, key: ui_components::ViewerKey) -> Option<SettingsAction> {
        let event = self.picker.key(key, ui_components::KeyReach::default());
        self.picked(event)
    }

    /// What the list's `event` means: a choice of its row's (saved like a pill's).
    fn picked(
        &mut self,
        event: Option<ui_components::picker::PickerEvent>,
    ) -> Option<SettingsAction> {
        use ui_components::picker::PickerEvent;
        match event {
            Some(PickerEvent::Chosen(i)) => {
                let dropdown = self.picker_for.take()?;
                let (value, _) = dropdown.choices.get(i)?;
                Some(SettingsAction::Choose {
                    block: dropdown.block,
                    key: dropdown.key,
                    value: *value,
                })
            }
            Some(PickerEvent::Closed) => {
                self.picker_for = None;
                None
            }
            _ => None,
        }
    }

    /// Its controls (an open dropdown list, the navigation's bar, the content's), the
    /// topmost first, with their state of use: pressed, held, hovered alike, none named.
    pub(super) fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let keymap = self.on_keymap();
        let mut content = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut nav = self.modal.nav_grab();
        let mut filter = Shown { part: &mut self.shortcuts.filter, shown: keymap };
        let mut parts: [&mut dyn Control; 5] =
            [&mut self.picker, &mut nav, &mut content, &mut self.query, &mut filter];
        f(&mut self.controls, &mut parts)
    }

    /// One of its controls holds the pointer: the mouse's moves drag it.
    pub fn scrollbar_held(&self) -> bool {
        self.controls.held()
    }

    /// The control holding the pointer dragged to `p`: `true` if it moved.
    pub fn drag_scrollbar(&mut self, p: Point) -> bool {
        let moved = self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, m)| m);
        self.limit_scroll();
        moved
    }
}
