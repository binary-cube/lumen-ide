//! The commit screen's keyboard lanes as a form's controls: the message (a field of several
//! lines kept as the repository's draft at every edit), the places' filter, the files'
//! lists. Which one has the keys is the screen's [`Controls`]' focus; Tab and Esc hand it
//! on as the screen says ([`super::CommitView`]), so each keeps them.

use std::path::Path;

use kernel::Point;
use ui_components::control::{Control, Press};
use ui_components::{KeyReach, MultilineField, PressMods, ViewerCursor, ViewerKey};
use ui_kit::TextInput;

/// The commit's message: the pointer and the editing keys as any field of several lines
/// (a press never extends: ⇧ chooses files here), ↵ a new line; every edit is kept as the
/// draft of the repository at `draft`. Tab is the screen's (to the lists).
pub(super) struct MessagePart<'a> {
    pub(super) field: &'a mut MultilineField,
    pub(super) draft: Option<&'a Path>,
}

impl MessagePart<'_> {
    fn keep_draft(&self) {
        if let Some(root) = self.draft {
            super::drafts().insert(root.to_path_buf(), MultilineField::text(self.field).to_owned());
        }
    }
}

impl Control for MessagePart<'_> {
    fn hit(&self, p: Point) -> bool {
        Control::hit(&*self.field, p)
    }

    fn press_with(&mut self, p: Point, clicks: u32, _mods: PressMods) -> Press {
        Control::press_with(&mut *self.field, p, clicks, PressMods::default())
    }

    /// Held, it's selecting: a frame at every move.
    fn drag(&mut self, p: Point) -> bool {
        Control::drag(&mut *self.field, p);
        true
    }

    /// Held, it was selecting: what it shows ends that.
    fn release(&mut self) -> bool {
        Control::release(&mut *self.field);
        true
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        Control::hover(&mut *self.field, p)
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Control::cursor(&*self.field, p)
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        Control::scroll(&mut *self.field, dx, dy)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        if key == ViewerKey::Tab {
            return true;
        }
        if !Control::key(&mut *self.field, key, reach) {
            return false;
        }
        self.keep_draft();
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.field.insert(typed);
        self.keep_draft();
        true
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&*self.field)
    }

    /// The edit commands, each kept as the draft (undo and redo too, whether or not they
    /// had anything to do).
    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => self.field.select_all(),
            "cut" => self.field.insert(""),
            "undo" => {
                self.field.undo();
            }
            "redo" => {
                self.field.redo();
            }
            _ => return false,
        }
        self.keep_draft();
        true
    }

    fn frame_deadline(&self) -> Option<std::time::Instant> {
        self.field.next_frame()
    }
}

/// The places' filter with the keys (it was clicked: the screen gives it the focus): the
/// editing keys and the text edit it, Esc empties it; empty, Esc and Tab ask to leave it
/// (`leave`). `edited`: what it holds changed (the places' rows are made again).
pub(super) struct PlacesLane<'a> {
    pub(super) filter: &'a mut TextInput,
    pub(super) edited: &'a mut bool,
    pub(super) leave: &'a mut bool,
}

impl PlacesLane<'_> {
    /// `edit` applied to the filter, noting a change of what it holds.
    fn edit(&mut self, edit: impl FnOnce(&mut TextInput)) {
        let before = TextInput::text(self.filter).to_owned();
        edit(self.filter);
        *self.edited |= TextInput::text(self.filter) != before;
    }
}

impl Control for PlacesLane<'_> {
    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        match key {
            ViewerKey::Escape if !TextInput::text(self.filter).is_empty() => {
                self.edit(|input| input.set_text(""));
            }
            ViewerKey::Escape | ViewerKey::Tab => *self.leave = true,
            _ => {
                let mut used = false;
                self.edit(|input| used = reach.edit(input, key));
                return used;
            }
        }
        true
    }

    fn text(&mut self, typed: &str) -> bool {
        self.edit(|input| input.insert(typed));
        true
    }

    fn selected_text(&self) -> Option<String> {
        let selected = self.filter.selected_text();
        (!selected.is_empty()).then(|| selected.to_owned())
    }

    fn command(&mut self, id: &str) -> bool {
        match id {
            "select_all" => self.filter.select_all(),
            "cut" => self.edit(|input| input.insert("")),
            "undo" => self.edit(|input| {
                input.undo();
            }),
            "redo" => self.edit(|input| {
                input.redo();
            }),
            _ => return false,
        }
        true
    }
}

/// What the files' lists were asked with the keys, for the screen to do (moving through
/// both lists, opening the shown file, staging it, reads the screen's whole state).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ListsAsked {
    Key(ViewerKey, KeyReach),
    Text(String),
}

/// The files' lists with the keys (a file was clicked, or Tab from the message): ↑ ↓ (⇧
/// choosing a run), the pages, the ends, ↵, Tab; whatever is typed. What they're asked is
/// written to `asked`.
pub(super) struct ListsLane<'a> {
    pub(super) asked: &'a mut Option<ListsAsked>,
}

impl Control for ListsLane<'_> {
    fn focusable(&self) -> bool {
        true
    }

    fn keeps_tab(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = matches!(
            key,
            ViewerKey::Up
                | ViewerKey::Down
                | ViewerKey::PageUp
                | ViewerKey::PageDown
                | ViewerKey::Home
                | ViewerKey::End
                | ViewerKey::Enter
                | ViewerKey::Tab
        );
        if used {
            *self.asked = Some(ListsAsked::Key(key, reach));
        }
        used
    }

    fn text(&mut self, typed: &str) -> bool {
        *self.asked = Some(ListsAsked::Text(typed.to_owned()));
        true
    }
}

#[cfg(test)]
#[path = "../../tests/unit/commit_view/parts.rs"]
mod tests;
