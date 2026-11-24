//! The mouse and the keys over the tree: the row or button under the pointer, a press, a
//! drag into a folder, the hover, the text typed in the search or a name, the keys.

use super::*;

impl RequestTree {
    fn slot_at(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) {
            return None;
        }
        let h = self.row_h();
        let y = p.y - self.list.min_y() - self.px(6.0) + self.scroll;
        TreeList::new(h, self.slots.len()).row_at(y)
    }

    pub(super) fn row_at(&self, p: Point) -> Option<usize> {
        match self.slots.get(self.slot_at(p)?)? {
            Slot::Row(i) => Some(*i),
            Slot::Naming(_) => None,
        }
    }

    /// The folder a request being dragged would go into, if it moves (`None`: not dragging,
    /// over nothing, or over its own folder).
    pub(super) fn drop_folder(&self) -> Option<String> {
        let at = self.dragged_to?;
        let (i, ..) = self.pressed?;
        let Some(Row::Request { file, .. }) = self.rows.get(i) else { return None };
        let target = self.folder_of(Some(self.row_at(at)?));
        let own = self.listing.files.get(*file).map(|f| f.folder())?;
        (own != target).then_some(target)
    }

    /// Whether row `i` is in `folder` (it, or under it; everything under the root).
    pub(super) fn in_folder(&self, i: usize, folder: &str) -> bool {
        let path = match self.rows.get(i) {
            Some(Row::Folder { path, .. }) => path.as_str(),
            Some(Row::Request { file, .. }) => {
                self.listing.files.get(*file).map_or("", |f| f.folder())
            }
            None => return false,
        };
        folder.is_empty()
            || path == folder
            || path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
    }

    pub fn press(&mut self, p: Point, clicks: u32) {
        if let Some((_, input)) = &mut self.naming {
            if self.naming_field.contains(p) {
                input.press(p.x, clicks);
                return;
            }
            // Pressed elsewhere: the name kept, as Enter (the tree's way).
            self.finish_naming(true);
        }
        if let Some(button) = self.button_at(p) {
            match button {
                Button::NewRequest => self.new_request(None),
                Button::NewCollection => self.new_collection(None),
                Button::Environments => {
                    let within = self.within.clone();
                    self.edit_environments(&within);
                }
                Button::Import => self.action = Some(TreeAction::Import),
            }
            return;
        }
        self.typing = self.field.contains(p);
        if self.typing {
            self.selecting = Control::press(&mut self.filter, p, clicks) == Press::Held;
            // × emptied it: every row again.
            if self.filter.take_cleared() {
                self.scroll = 0.0;
                self.filtered();
            }
            self.pressed = None;
            return;
        }
        self.dragged_to = None;
        self.pressed = self.row_at(p).map(|i| {
            let depth = f32::from(self.rows.get(i).map_or(0, Row::depth));
            let arrow_end =
                self.list.min_x() + self.px(PADDING) + depth * self.px(INDENT) + self.px(16.0);
            (i, p, p.x < arrow_end, clicks >= 2)
        });
        if let Some((i, ..)) = self.pressed {
            self.selected = Some(i);
        }
    }

    /// A request dragged: where it's over (it moves once it went a few pixels).
    pub fn drag(&mut self, p: Point) -> bool {
        if self.selecting {
            return Control::drag(&mut self.filter, p);
        }
        let Some((i, from, ..)) = self.pressed else { return false };
        if !matches!(self.rows.get(i), Some(Row::Request { .. })) {
            return false;
        }
        if self.dragged_to.is_none() && (p.x - from.x).hypot(p.y - from.y) < self.px(4.0) {
            return false;
        }
        self.dragged_to = Some(p);
        self.hover = self.row_at(p);
        true
    }

    /// Released: a click chooses its row; a request dropped on a folder (or another
    /// request's folder) moves into that folder's `requests.http`.
    pub fn release(&mut self) -> bool {
        if std::mem::take(&mut self.selecting) {
            return false;
        }
        let Some((i, _, on_arrow, double)) = self.pressed.take() else { return false };
        match self.dragged_to.take() {
            None => self.activate(i, on_arrow, double),
            Some(at) => {
                let target = self.row_at(at).map(|t| self.folder_of(Some(t)));
                if let (Some(Row::Request { file, request, .. }), Some(folder)) =
                    (self.rows.get(i).cloned(), target)
                {
                    let same = self.listing.files.get(file).is_some_and(|f| f.folder() == folder);
                    if !same {
                        let into = if folder.is_empty() {
                            "requests.http".to_owned()
                        } else {
                            format!("{folder}/requests.http")
                        };
                        self.move_request(file, request, &into);
                    }
                }
            }
        }
        true
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.row_at(p);
        let button = self.button_at(p);
        // The search's × lit under the mouse.
        let clear = Control::hover(&mut self.filter, Some(p));
        let changed = clear || self.hover != hover || self.hover_button != button;
        self.hover = hover;
        self.hover_button = button;
        changed | self.bar.hover(p)
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons_at.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }

    pub fn clickable(&self, p: Point) -> bool {
        self.row_at(p).is_some() || self.button_at(p).is_some()
    }

    pub fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        // The search's: the text's, a hand on its ×.
        if self.field.contains(p) {
            return Control::cursor(&self.filter, p);
        }
        if self.naming_field.contains(p) {
            return Some(ViewerCursor::Text);
        }
        self.clickable(p).then_some(ViewerCursor::Pointer)
    }

    pub fn tooltip(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        let (r, button) = self.buttons_at.iter().find(|(r, _)| r.contains(p))?;
        Some(ui_components::tooltip::Tip::new(*r, button.tooltip()))
    }

    pub fn wants_text(&self) -> bool {
        self.typing || self.naming.is_some()
    }

    pub fn text_input(&mut self, text: &str) -> bool {
        if let Some((_, input)) = &mut self.naming {
            let name: String = text.chars().filter(|c| !c.is_control()).collect();
            input.insert(&name);
            self.naming_error = false;
            return true;
        }
        self.typing = true;
        // On one line, the search field's own way (its breaks as spaces).
        Control::text(&mut self.filter, text);
        self.scroll = 0.0;
        self.filtered();
        true
    }

    pub fn selected_text(&self) -> Option<String> {
        let selected = match &self.naming {
            Some((_, input)) => input.selected_text(),
            None if self.typing => self.filter.input.selected_text(),
            None => "",
        };
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    /// ⌘A in the name being typed, or in the search.
    pub fn select_all(&mut self) -> bool {
        self.edit_command(SELECT_ALL)
    }

    /// An edit command (⌘A, ⌘X, ⌘Z, ⌘⇧Z) in the name being typed, or in the search (what
    /// it changes filtered again).
    pub fn edit_command(&mut self, id: &str) -> bool {
        if let Some((_, input)) = &mut self.naming {
            self.naming_error = false;
            return Control::command(input, id);
        }
        if !self.typing || !Control::command(&mut self.filter, id) {
            return false;
        }
        if id != SELECT_ALL {
            self.scroll = 0.0;
            self.filtered();
        }
        true
    }

    pub fn edit_key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if let Some((_, input)) = &mut self.naming {
            match key {
                ViewerKey::Escape => self.cancel_naming(),
                ViewerKey::Enter => self.finish_naming(false),
                _ => {
                    self.naming_error = false;
                    return reach.edit(input, key);
                }
            }
            return true;
        }
        if key == ViewerKey::Escape {
            let had = !self.filter.text().is_empty();
            self.filter.set_text("");
            self.typing = false;
            self.filtered();
            return had;
        }
        // The rows walked with the keys (from the search too: ↑ ↓ and ↵ are the list's).
        match key {
            ViewerKey::Up => self.select_by(-1),
            ViewerKey::Down => self.select_by(1),
            ViewerKey::PageUp | ViewerKey::PageDown if !self.typing => {
                let page = (self.list.size.height / self.row_h()).floor().max(1.0) as isize;
                self.select_by(if key == ViewerKey::PageUp { -page } else { page });
            }
            ViewerKey::Home | ViewerKey::End if !self.typing => {
                self.select_by(if key == ViewerKey::Home { isize::MIN } else { isize::MAX });
            }
            ViewerKey::Left | ViewerKey::Right if !self.typing => {
                self.fold(key == ViewerKey::Right);
            }
            ViewerKey::Enter => {
                let first = (self.typing && !self.rows.is_empty()).then_some(0);
                let Some(i) = self.selected.or(first) else { return false };
                self.activate(i, false, false);
            }
            _ => {
                if self.typing && reach.edit(&mut self.filter.input, key) {
                    self.filtered();
                    return true;
                }
                return false;
            }
        }
        true
    }

    /// The keyboard went elsewhere: the search stops, a name being typed is kept.
    pub fn focus_lost(&mut self) {
        self.typing = false;
        self.finish_naming(true);
    }
}
