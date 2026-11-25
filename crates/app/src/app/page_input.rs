//! The app's input routed to the pages it draws: a key to the page with the keyboard (but
//! for the app's shortcuts and its editing commands), its release where its press went;
//! the wheel and files dragged to the page under the mouse unless something of the app's
//! floats there; a page's own menu shown by the app; the keyboard given back when the app
//! moves its focus.

use super::*;

/// The most keys remembered held whose press went to the app or a page's editing (keys
/// held at once are a few; past it, the oldest is forgotten).
const HELD_AWAY: usize = 16;

/// Where a key's press went that didn't go to the page with the keyboard: its release goes
/// there too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Away {
    /// The page's editing action (its release is nothing).
    Edit,
    /// The app's shortcut.
    App,
}

/// A key as its press and its release both name it: its place, or the key itself.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeyId {
    Place(char),
    Key(platform_api::Key),
}

impl KeyId {
    fn of(key: &KeyEvent) -> Self {
        match key.physical {
            Some(place) => Self::Place(place),
            None => Self::Key(key.key.clone()),
        }
    }
}

/// What routing a key to the pages keeps between events: the keys held whose press went
/// elsewhere, the app's focus moves as last seen.
#[derive(Debug, Default)]
pub(super) struct PageRouting {
    away: Vec<(KeyId, Away)>,
    /// The workbench's focus moves as last seen ([`Workbench::focus_moves`]).
    focus_moves: u64,
}

impl PageRouting {
    /// `key` pressed went `away`: its release goes the same way.
    fn pressed(&mut self, key: &KeyEvent, away: Away) {
        let id = KeyId::of(key);
        self.away.retain(|(held, _)| *held != id);
        if self.away.len() >= HELD_AWAY {
            self.away.remove(0);
        }
        self.away.push((id, away));
    }

    /// `key` let go: where its press went, if it went away from the page (forgotten now).
    fn released(&mut self, key: &KeyEvent) -> Option<Away> {
        let id = KeyId::of(key);
        let at = self.away.iter().position(|(held, _)| *held == id)?;
        Some(self.away.remove(at).1)
    }
}

/// Who a key goes to while a page the app draws has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageKey {
    /// One of its editing actions.
    Edit(platform_api::PageEdit),
    /// The app's shortcut.
    App,
    /// The page's own.
    Page,
}

impl EditorApp {
    /// A key handed to the page the app draws with the keyboard: `true` if it took it (or
    /// it's nothing: the release of its editing action), `false` if it's the app's. An
    /// editing command's keys are the page's editing (copy, paste…), a shortcut of the
    /// app's — with ⌘ / Ctrl or a function key — the app's, anything else the page's
    /// (letters, arrows, Esc). A release goes where its press went.
    pub(super) fn key_to_page(&mut self, key: &KeyEvent) -> bool {
        if !key.pressed
            && let Some(away) = self.page_routing.released(key)
        {
            return away == Away::Edit;
        }
        let open = !self.workbench.overlay_open();
        if !self.web_views.has_keyboard(open) {
            return false;
        }
        match self.page_key_owner(key) {
            PageKey::Edit(action) if key.pressed => {
                self.web_views.edit(action, open);
                self.page_routing.pressed(key, Away::Edit);
                true
            }
            // Its press went as an editing action, never as a key: no key-up of it.
            PageKey::Edit(_) => true,
            PageKey::App => {
                if key.pressed {
                    self.page_routing.pressed(key, Away::App);
                }
                false
            }
            PageKey::Page => {
                self.web_views.key(key, open);
                true
            }
        }
    }

    /// Who `key` goes to while a page the app draws has the keyboard ([`page_key`]).
    fn page_key_owner(&self, key: &KeyEvent) -> PageKey {
        // Nothing of the app's to look up (typing, a plain arrow, a key the page edits with):
        // nothing gathered at each key.
        if !may_be_the_apps(key) {
            return PageKey::Page;
        }
        let Some(chord) = super::chords::chord(key) else { return PageKey::Page };
        let ids = self.keymap.lookup_all_in(&chord, &self.workbench.key_places());
        page_key(&ids)
    }

    /// A page took the keyboard: the app's focus as it is now is the one it took it from.
    pub(super) fn page_took_keyboard(&mut self) {
        self.page_routing.focus_moves = self.workbench.focus_moves();
    }

    /// The app moved its focus since last seen (a command, a key of the app's, a tab
    /// opened): the pages give the keyboard back. A count compared, nothing else.
    pub(super) fn pages_follow_focus(&mut self) {
        let moves = self.workbench.focus_moves();
        if std::mem::replace(&mut self.page_routing.focus_moves, moves) != moves {
            self.web_views.release_focus();
        }
    }

    /// Whether the pages the app draws get the mouse at `p`: nothing of the app's is open
    /// over them (a window over the editor, something floating there).
    pub(super) fn pages_open_at(&self, p: Point) -> bool {
        // No page the app draws: nothing to look at.
        if !self.web_views.any_drawn() {
            return false;
        }
        !self.workbench.overlay_open() && !self.workbench.floats_over_pages_at(p)
    }

    /// The app's menu for a page that asked one, where it asked: Cut, Copy, Paste as what's
    /// under the click allows, then Select All; each row the page's editing action.
    pub(super) fn page_menu(&mut self, ask: platform_api::PageMenuAsk) {
        let items: Vec<platform_api::MenuItem> = page_menu_rows(&ask)
            .into_iter()
            .map(|id| platform_api::MenuItem::Command {
                id: id.0.to_owned(),
                label: cmd::label(id),
                shortcut: crate::keymaps::shortcuts(id).first().cloned(),
                icon: None,
            })
            .collect();
        self.show_entries(cmd::menu_entries(items), Vec::new(), ask.at);
    }

    /// Files dragged at `p`: a page's that takes them (the editor's drop let go), else the
    /// editor's (its drop shown, following the pointer).
    pub(super) fn files_moved(&mut self, p: Point) {
        // Over something floating over the pages (a notification): the window's, as the mouse.
        let open = self.pages_open_at(p);
        let changed = match self.web_views.files_at(p, open) {
            web_views::FilesOver::Page => self.workbench.end_file_drag(),
            web_views::FilesOver::Window if self.workbench.file_drag_active() => {
                self.workbench.move_file_drag(p)
            }
            web_views::FilesOver::Window => {
                self.workbench.drag_files_over(&self.web_views.dragged_files(), p);
                true
            }
        };
        if changed {
            self.redraw();
        }
    }
}

/// The rows of a page's menu (`ask`): Cut where it's editable with a selection, Copy with a
/// selection, Paste where it's editable, Select All always.
fn page_menu_rows(ask: &platform_api::PageMenuAsk) -> Vec<command_model::CommandId> {
    let rows = [
        (cmd::CUT, ask.editable && ask.selection),
        (cmd::COPY, ask.selection),
        (cmd::PASTE, ask.editable),
        (cmd::SELECT_ALL, true),
    ];
    rows.into_iter().filter(|(_, shown)| *shown).map(|(id, _)| id).collect()
}

/// Whether `key` may be a shortcut of the app's while a page has the keyboard: with ⌘ /
/// Ctrl, or a function key — but never a key a field edits with (Backspace, Delete, the
/// arrows, Home, End), whatever is held: those are the page's (a word deleted, a line's end
/// reached).
fn may_be_the_apps(key: &KeyEvent) -> bool {
    use platform_api::{Key, NamedKey};
    let m = key.modifiers;
    match &key.key {
        Key::Named(
            NamedKey::Backspace
            | NamedKey::Delete
            | NamedKey::ArrowUp
            | NamedKey::ArrowDown
            | NamedKey::ArrowLeft
            | NamedKey::ArrowRight
            | NamedKey::Home
            | NamedKey::End,
        ) => false,
        Key::Named(NamedKey::F(_)) => true,
        _ => m.ctrl || m.meta,
    }
}

/// Who a key that may be the app's ([`may_be_the_apps`]) goes to while a page has the
/// keyboard, `ids` the app's commands it's bound to: an editing command's are the page's
/// editing; a shortcut of a command that means something outside the text editor (Save, a
/// tool window, Find), the app's; the rest — an editor's own command (delete a line,
/// duplicate, go to a definition: they need a text file) or nothing — the page's.
fn page_key(ids: &[command_model::CommandId]) -> PageKey {
    if let Some(action) = ids.iter().find_map(|id| page_edit(id.0)) {
        return PageKey::Edit(action);
    }
    let editors_own = |id: &command_model::CommandId| {
        cmd::requires(*id).contains(&command_model::Requires::TextFile)
    };
    if ids.iter().any(|id| !editors_own(id)) { PageKey::App } else { PageKey::Page }
}

/// The wheel's `delta` for a page (physical pixels; `line`: the editor's line, physical): a
/// notch as far as the editor scrolls.
pub(super) fn page_wheel(delta: ScrollDelta, line: f32) -> (f32, f32) {
    match delta {
        ScrollDelta::Lines { x, y } => {
            let notch = super::events::WHEEL_LINES * line;
            (x * notch, y * notch)
        }
        ScrollDelta::Pixels { x, y } => (x, y),
    }
}

/// The page's editing action an Edit command is (`None`: not one).
pub(super) fn page_edit(command: &str) -> Option<platform_api::PageEdit> {
    use platform_api::PageEdit;
    Some(match command {
        c if c == cmd::UNDO.0 => PageEdit::Undo,
        c if c == cmd::REDO.0 => PageEdit::Redo,
        c if c == cmd::CUT.0 => PageEdit::Cut,
        c if c == cmd::COPY.0 => PageEdit::Copy,
        c if c == cmd::PASTE.0 => PageEdit::Paste,
        c if c == cmd::SELECT_ALL.0 => PageEdit::SelectAll,
        _ => return None,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/app/page_input.rs"]
mod tests;
