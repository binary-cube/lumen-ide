//! The overlays as a contract: each window over the editor (settings, project search, the
//! projects list, About, a dialog, a module's modal view, the pickers, the menus) is an
//! [`OverlaySlot`] — what it is (modal or popover), whether it's open, and what it does
//! with the keys, the text, a click, the mouse, the scroll and its paint. The defaults are
//! the common behavior; each overlay overrides only what it does differently.
//!
//! [`Overlay::ALL`] is the stack, topmost first: the order they're drawn in (reversed) and
//! the order they get input in, by construction. It's what's open — nothing to keep in sync
//! or clean up: a closed overlay simply isn't in it. A key goes down the open ones until one
//! keeps it (a modal always does: nothing behind it gets anything, `CLAUDE.md`), then down
//! the focus chain (a side panel, the bottom panel, a view, the editor), where each part
//! uses it or lets it go on. Closing a modal leaves the focus where it was before it.
//!
//! A floating overlay ([`OverlaySlot::floats`]: the notifications) is drawn over all of
//! them but is never the one on top: it never has the keyboard and blocks nothing — only a
//! click, the mouse or the scroll on its own area reaches it.

use super::*;
use render_api::Scene;
use ui_workbench::Painter;

/// What an overlay is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OverlayKind {
    /// Nothing behind it gets keys, text, commands, scroll or hover; only ×, Esc or its
    /// buttons close it.
    Modal,
    /// A small window over its place (a menu, a picker): it has the keyboard, a click
    /// outside closes it, the scroll goes to what's under the mouse.
    Popover,
}

/// The contract of a window over the editor. Every method gets the workbench (the overlays
/// are its fields; what they do reaches the rest of it).
pub(super) trait OverlaySlot: Sync {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Modal
    }

    fn is_open(&self, wb: &Workbench) -> bool;

    /// It floats over everything (the notifications): never the overlay on top — it never
    /// has the keyboard and blocks nothing; only what falls on its own area
    /// ([`Self::holds`]) reaches it, modals open or not.
    fn floats(&self, _wb: &Workbench) -> bool {
        false
    }

    /// Whether `p` is on a floating overlay's own area.
    fn holds(&self, _wb: &Workbench, _p: Point) -> bool {
        false
    }

    /// A floating overlay's own area, while it shows (what's under it that the app can't
    /// draw over — a page of the system's web engine — is hidden while they meet).
    fn area(&self, _wb: &Workbench) -> Option<kernel::Rect> {
        None
    }

    /// It replaces the empty screen while open (the floating ones don't).
    fn covers_home(&self, wb: &Workbench) -> bool {
        self.kind(wb) == OverlayKind::Modal
    }

    /// A key; `true` if it was used. Nothing under it gets the key, unless it
    /// [passes](Self::passes) it on.
    fn key(&self, _wb: &mut Workbench, _text: &mut TextSystem, _key: EditKey) -> bool {
        false
    }

    /// Esc with a search typed in it: the search is cleared (`true`) and the window stays;
    /// the next Esc closes it. Without a search (the default), Esc goes to [`Self::key`].
    fn clear_query(&self, _wb: &mut Workbench) -> bool {
        false
    }

    /// A popover may let a key go on to what's under it (the next overlay, then what has
    /// the keyboard); a modal never does — the stack doesn't ask it.
    fn passes(&self, _wb: &Workbench, _key: &EditKey) -> bool {
        false
    }

    /// Typed text; `true` if it was used.
    fn text(&self, _wb: &mut Workbench, _text: &mut TextSystem, _typed: &str) -> bool {
        false
    }

    /// A click anywhere while it's on top (outside it too).
    fn click(
        &self,
        _wb: &mut Workbench,
        _text: &mut TextSystem,
        _p: Point,
        _clicks: u32,
    ) -> MouseResult {
        MouseResult::None
    }

    /// The mouse moved: whether to redraw, and the cursor.
    fn hover(&self, _wb: &mut Workbench, _p: Point) -> (bool, CursorIcon) {
        (false, CursorIcon::Default)
    }

    /// The tooltip of what it holds under the mouse (the only one shown while it's open:
    /// nothing behind it gives one). None by default.
    fn tooltip_at(&self, _wb: &Workbench, _p: Point) -> Option<ui_workbench::Tip> {
        None
    }

    /// A scroll: `Some(redraw)` if it takes it; `None` lets it reach what's under the mouse
    /// (a popover's default).
    fn scroll(&self, wb: &mut Workbench, _dx: f32, _dy: f32) -> Option<bool> {
        match self.kind(wb) {
            OverlayKind::Modal => Some(false),
            OverlayKind::Popover => None,
        }
    }

    /// Draws it (called every frame, open or not: a closing one may still fade out).
    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene);

    fn animating(&self, wb: &Workbench) -> bool;

    /// When it next needs a frame at its own pace (a scrollbar fading in or out), instead
    /// of frames meanwhile; `None` (the default): nothing of its own counts down.
    fn frame_deadline(&self, _wb: &Workbench) -> Option<std::time::Instant> {
        None
    }

    /// It has a text field with the keyboard: it takes the clipboard commands, and the
    /// cursor keys it doesn't use itself (←→, a word, Home / End) move in the field.
    fn has_input(&self, _wb: &Workbench) -> bool {
        false
    }
}

/// The cursor over something clickable or not.
fn pointer_if(clickable: bool) -> CursorIcon {
    if clickable { CursorIcon::Pointer } else { CursorIcon::Default }
}

impl Workbench {
    /// When a window over the editor next wants a frame at its own pace (a scrollbar fading
    /// in a popover or a modal, in the completion list or the code's hover card): the soonest; `None` at rest.
    pub fn overlays_deadline(&self) -> Option<std::time::Instant> {
        let stack = Overlay::ALL
            .into_iter()
            .filter(|o| self.overlay_is_open(*o))
            .filter_map(|o| o.slot().frame_deadline(self));
        stack.chain(self.completion.frame_deadline()).chain(self.code_hover.frame_deadline()).min()
    }

    /// The overlays a key goes through, topmost first: the open ones, down to the first
    /// that keeps it (`None`: all let it go on, to what has the keyboard).
    pub(super) fn overlay_for_key(&self, key: &EditKey) -> Option<Overlay> {
        Overlay::ALL
            .into_iter()
            .filter(|o| !o.slot().floats(self) && self.overlay_is_open(*o))
            .find(|o| {
                let slot = o.slot();
                slot.kind(self) == OverlayKind::Modal || !slot.passes(self, key)
            })
    }
}

impl Overlay {
    /// The overlay's behavior.
    pub(super) fn slot(self) -> &'static dyn OverlaySlot {
        match self {
            Overlay::FileDrop => &super::file_drop::FileDropSlot,
            Overlay::Toasts => &super::toasts::ToastsSlot,
            Overlay::StatusPopover => &super::status::StatusPopoverSlot,
            Overlay::MenuDrawer => &super::menu_drawer::MenuDrawerSlot,
            Overlay::ContextMenu => &ContextMenuSlot,
            Overlay::ChoiceMenu => &ChoiceMenuSlot,
            Overlay::Dialog => &DialogSlot,
            Overlay::ModalView => &ModalViewSlot,
            Overlay::About => &AboutSlot,
            Overlay::Settings => &SettingsSlot,
            Overlay::ProjectsMenu => &ProjectsMenuSlot,
            Overlay::ResourcePills => &super::status::ResourcePillsSlot,
        }
    }
}

// ─── The menus ───

struct ContextMenuSlot;

impl OverlaySlot for ContextMenuSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.context_menu.is_open()
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        match key {
            // Esc / ← close the open submenu first; Esc then the menu.
            EditKey::Escape => {
                if !wb.context_menu.enter_submenu(false) {
                    wb.context_menu.close();
                    wb.clear_menu_target();
                }
            }
            EditKey::Move { motion: Motion::Left, .. } => {
                wb.context_menu.enter_submenu(false);
            }
            EditKey::Move { motion: Motion::Right, .. } => {
                wb.context_menu.enter_submenu(true);
            }
            EditKey::Move { motion: Motion::Up, .. } => wb.context_menu.step(-1),
            EditKey::Move { motion: Motion::Down, .. } | EditKey::Tab { .. } => {
                wb.context_menu.step(1)
            }
            EditKey::Enter { .. } => {
                // On a submenu's row it opens instead (the menu stays).
                let switch = wb.context_menu.on_switch();
                if let Some(id) = wb.context_menu.confirm() {
                    wb.request = Some(MouseResult::MenuCommand(id));
                    if !switch {
                        wb.context_menu.close();
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        match wb.context_menu.hit(p) {
            // A row that only tells (the status bar's `…` listing a value): the menu closes.
            Some(ui_workbench::ContextAction::Run(id)) if id.is_empty() => {
                wb.context_menu.close();
                MouseResult::Redraw
            }
            Some(ui_workbench::ContextAction::Run(id)) => {
                wb.context_menu.close();
                MouseResult::MenuCommand(id)
            }
            // A setting's row: run, the menu stays and shows how it stands now.
            Some(ui_workbench::ContextAction::Switch(id)) => MouseResult::MenuCommand(id),
            Some(ui_workbench::ContextAction::Close) => {
                wb.context_menu.close();
                wb.clear_menu_target();
                MouseResult::Redraw
            }
            _ => MouseResult::None,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.context_menu.hover(p), pointer_if(wb.context_menu.over_item(p)))
    }

    /// A two-finger click on a trackpad comes with small scrolls: they must not close the
    /// menu it opened (nothing behind it scrolls either).
    fn scroll(&self, _: &mut Workbench, _: f32, _: f32) -> Option<bool> {
        Some(false)
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        let support = Arc::clone(&wb.support);
        let states: Vec<(&'static str, Option<bool>)> = if wb.context_menu.is_open() {
            support.commands.iter().map(|c| (c.spec.id.0, c.handler.switched(&*wb))).collect()
        } else {
            Vec::new()
        };
        wb.context_menu
            .refresh_switches(&|id| states.iter().find(|(c, _)| *c == id).and_then(|(_, on)| *on));
        wb.context_menu.paint(&wb.icons, text, scene, &wb.palette);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.context_menu.is_animating()
    }
}

struct ChoiceMenuSlot;

impl OverlaySlot for ChoiceMenuSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.choice_menu.is_open()
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        match key {
            EditKey::Escape => wb.choice_menu.close(),
            EditKey::Move { motion: Motion::Up, .. } => wb.choice_menu.step(-1),
            EditKey::Move { motion: Motion::Down, .. } | EditKey::Tab { .. } => {
                wb.choice_menu.step(1)
            }
            EditKey::Enter { .. } => {
                let picked = wb.choice_menu.confirm();
                wb.choice_menu.close();
                wb.request = picked.map(|i| wb.choice_picked(i));
            }
            _ => {}
        }
        true
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        // On its list's scrollbar: the list follows the thumb (a drag goes on).
        if wb.choice_menu.scrollbar_press(p) {
            return MouseResult::StartDrag;
        }
        match wb.choice_menu.hit(p) {
            Some(ui_workbench::ChoiceAction::Pick(i)) => {
                wb.choice_menu.close();
                wb.choice_picked(i)
            }
            Some(ui_workbench::ChoiceAction::Close) => {
                wb.choice_menu.close();
                MouseResult::Redraw
            }
            _ => MouseResult::None,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.choice_menu.hover(p), pointer_if(wb.choice_menu.over_clickable(p)))
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.choice_menu.scroll(dy))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.choice_menu.paint(&wb.icons, text, scene, &wb.palette);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.choice_menu.is_animating()
    }
}

// ─── The modals ───

struct DialogSlot;

impl OverlaySlot for DialogSlot {
    fn is_open(&self, wb: &Workbench) -> bool {
        wb.dialogs.dialog.is_open()
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        match key {
            EditKey::Escape => {
                let answer = wb.dialogs.dialog.cancel();
                wb.answer_dialog(answer)
            }
            EditKey::Enter { .. } => {
                let answer = wb.dialogs.dialog.confirm();
                wb.answer_dialog(answer)
            }
            EditKey::Tab { shift } => wb.dialogs.dialog.move_focus(if shift { -1 } else { 1 }),
            // With a text field, the cursor keys move in it (the common handling).
            EditKey::Move { .. } if wb.dialogs.dialog.input.is_some() => false,
            EditKey::Backspace { word } => {
                if let Some(input) = &mut wb.dialogs.dialog.input {
                    input.backspace(word);
                }
                true
            }
            EditKey::Delete => {
                if let Some(input) = &mut wb.dialogs.dialog.input {
                    input.delete();
                }
                true
            }
            EditKey::Move { motion: Motion::Left, .. } => wb.dialogs.dialog.move_focus(-1),
            EditKey::Move { motion: Motion::Right, .. } => wb.dialogs.dialog.move_focus(1),
            // The list (if any) scrolls a row.
            EditKey::Move { motion: Motion::Up, .. } => {
                wb.dialogs.dialog.scroll_rows(-1.0);
                true
            }
            EditKey::Move { motion: Motion::Down, .. } => {
                wb.dialogs.dialog.scroll_rows(1.0);
                true
            }
            _ => true,
        }
    }

    fn text(&self, wb: &mut Workbench, _text: &mut TextSystem, typed: &str) -> bool {
        match &mut wb.dialogs.dialog.input {
            Some(input) => {
                input.insert(typed);
                true
            }
            // Space toggles the checkbox.
            None => typed == " " && wb.dialogs.dialog.toggle_check(),
        }
    }

    /// It gets all clicks (outside it: none).
    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        // On its list's scrollbar: the list follows the thumb (a drag goes on).
        if wb.dialogs.dialog.scrollbar_press(p) {
            return MouseResult::StartDrag;
        }
        if wb.dialogs.dialog.check_at(p) {
            return MouseResult::Redraw;
        }
        let answer = wb.dialogs.dialog.hit(p);
        if wb.answer_dialog(answer) { MouseResult::Redraw } else { MouseResult::None }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.dialogs.dialog.hover(p), pointer_if(wb.dialogs.dialog.over_button(p)))
    }

    /// The wheel scrolls its list (if any); nothing behind it scrolls.
    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.dialogs.dialog.scroll(dy))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.dialogs.dialog.paint(&wb.icons, text, scene, &wb.palette);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.dialogs.dialog.is_animating()
    }

    fn has_input(&self, wb: &Workbench) -> bool {
        wb.dialogs.dialog.input.is_some()
    }
}

/// A module's view in a modal window (the remote project wizard).
struct ModalViewSlot;

impl OverlaySlot for ModalViewSlot {
    /// A popover when the view opened as one (the color picker), a modal otherwise.
    fn kind(&self, wb: &Workbench) -> OverlayKind {
        if wb.modal_view.as_ref().is_some_and(|m| m.popover) {
            OverlayKind::Popover
        } else {
            OverlayKind::Modal
        }
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.modal_view.is_some()
    }

    /// Floats over the empty screen.
    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    /// What the view doesn't use goes nowhere.
    fn key(&self, wb: &mut Workbench, text: &mut TextSystem, key: EditKey) -> bool {
        wb.modal_view_key(text, &key).unwrap_or(false)
    }

    fn text(&self, wb: &mut Workbench, text: &mut TextSystem, typed: &str) -> bool {
        wb.modal_view_text(text, typed).unwrap_or(false)
    }

    fn click(
        &self,
        wb: &mut Workbench,
        text: &mut TextSystem,
        p: Point,
        clicks: u32,
    ) -> MouseResult {
        wb.click_modal_view(text, p, clicks).unwrap_or(MouseResult::None)
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        wb.hover_modal_view(p).unwrap_or((false, CursorIcon::Default))
    }

    /// The view's own tooltips, inside its card.
    fn tooltip_at(&self, wb: &Workbench, p: Point) -> Option<ui_workbench::Tip> {
        wb.modal_view_tooltip(p)
    }

    /// A popover lets the scroll go on to what's under it, unless the mouse is over what it
    /// scrolls itself (a list).
    fn scroll(&self, wb: &mut Workbench, dx: f32, dy: f32) -> Option<bool> {
        if self.kind(wb) == OverlayKind::Popover && !wb.modal_view_takes_scroll() {
            return None;
        }
        Some(wb.scroll_modal_view(dx, dy).unwrap_or(false))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.paint_modal_view(text, scene);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.modal_view_animating()
    }

    fn has_input(&self, wb: &Workbench) -> bool {
        wb.modal_view_has_input()
    }
}

struct AboutSlot;

impl OverlaySlot for AboutSlot {
    fn is_open(&self, wb: &Workbench) -> bool {
        wb.about.is_open()
    }

    /// Floats over the empty screen.
    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    /// ↵ closes it too (it only shows).
    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        if matches!(key, EditKey::Escape | EditKey::Enter { .. }) {
            wb.about.close();
            crate::resources::release_splash_art();
        }
        true
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        wb.about.click(p);
        if !wb.about.is_open() {
            crate::resources::release_splash_art();
        }
        MouseResult::Redraw
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        if !wb.about.is_open() {
            return;
        }
        let lines = about_lines(wb.support.module_count);
        let art = crate::resources::splash_art();
        wb.about.paint(
            concat!("v", env!("CARGO_PKG_VERSION")),
            &lines,
            art.as_ref(),
            Painter { text, scene, palette: &wb.palette, icons: &wb.icons },
        );
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.about.is_animating()
    }
}

// ─── The app's modals ───

struct SettingsSlot;

impl OverlaySlot for SettingsSlot {
    fn is_open(&self, wb: &Workbench) -> bool {
        wb.settings.is_open()
    }

    /// While a module's text field is written, Enter saves it and Esc gives it up. While
    /// searching, Esc clears the search and ↑↓ walk the results; else ↑↓ change the section.
    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        if wb.settings.editing_text() || wb.settings.shortcuts_have_keys() {
            match key {
                EditKey::Escape => {
                    wb.settings.cancel_text();
                }
                EditKey::Enter { .. } => wb.settings_commit_text(),
                EditKey::Backspace { word } => wb.settings.backspace(word),
                _ => return false,
            }
            return true;
        }
        let searching = wb.settings.has_query();
        match key {
            EditKey::Escape => {
                wb.close_settings();
                true
            }
            EditKey::Move { motion: Motion::Up, .. } if searching => wb.settings.move_result(-1),
            EditKey::Move { motion: Motion::Down, .. } if searching => wb.settings.move_result(1),
            EditKey::Move { motion: Motion::Up, .. } => wb.settings.step_section(-1),
            EditKey::Move { motion: Motion::Down, .. } => wb.settings.step_section(1),
            EditKey::Enter { .. } => {
                let opened = wb.settings.confirm(&wb.settings_model);
                wb.settings_result_opened();
                opened
            }
            EditKey::Backspace { word } => {
                wb.settings.backspace(word);
                true
            }
            _ => false,
        }
    }

    fn text(&self, wb: &mut Workbench, _text: &mut TextSystem, typed: &str) -> bool {
        wb.settings.type_text(typed);
        true
    }

    fn clear_query(&self, wb: &mut Workbench) -> bool {
        // A text field being written: Esc gives up its change first.
        if wb.settings.cancel_text() {
            return true;
        }
        let typed = wb.settings.has_query();
        if typed {
            wb.settings.clear_query();
        }
        typed
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        match wb.settings.hit(p) {
            Some(SettingsAction::Close) => {
                wb.close_settings();
                MouseResult::Redraw
            }
            Some(SettingsAction::OpenResult) => {
                wb.settings.open_selected(&wb.settings_model);
                wb.settings_result_opened();
                MouseResult::Redraw
            }
            Some(action) => MouseResult::Settings(action),
            // Click on a section (or in the content): redraw.
            None => MouseResult::Redraw,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.settings.hover(p), pointer_if(wb.settings.over_clickable(p)))
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.settings.scroll_by(dy))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        wb.settings.paint(&wb.settings_model, &wb.icons, text, scene, &wb.palette);
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.settings.is_animating()
    }

    fn has_input(&self, _wb: &Workbench) -> bool {
        true
    }
}

struct ProjectsMenuSlot;

impl OverlaySlot for ProjectsMenuSlot {
    fn is_open(&self, wb: &Workbench) -> bool {
        wb.home.projects_menu.is_open()
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        match key {
            EditKey::Escape => wb.home.projects_menu.close(),
            EditKey::Move { motion: Motion::Up, .. } | EditKey::Tab { shift: true } => {
                wb.home.projects_menu.step(-1)
            }
            EditKey::Move { motion: Motion::Down, .. } | EditKey::Tab { shift: false } => {
                wb.home.projects_menu.step(1)
            }
            EditKey::Backspace { .. } => wb.home.projects_menu.backspace(),
            EditKey::Move { motion: Motion::Left, .. } if wb.fold_selected_group(true, true) => {}
            EditKey::Move { motion: Motion::Right, .. } if wb.fold_selected_group(true, false) => {}
            EditKey::Enter { .. } => match wb.home.projects_menu.confirm() {
                Some(action) => wb.request = Some(wb.picker_action(action, true)),
                None => wb.home.projects_menu.close(),
            },
            _ => return false,
        }
        true
    }

    fn text(&self, wb: &mut Workbench, _text: &mut TextSystem, typed: &str) -> bool {
        wb.home.projects_menu.type_text(typed);
        true
    }

    fn clear_query(&self, wb: &mut Workbench) -> bool {
        let typed = wb.home.projects_menu.has_query();
        if typed {
            wb.home.projects_menu.clear_query();
        }
        typed
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        match wb.home.projects_menu.hit(p) {
            Some(action) => wb.picker_action(action, true),
            // A press on a project or a group: a click or a drag, known on release.
            None if wb.home.projects_menu.pressed() => MouseResult::StartDrag,
            None => MouseResult::Redraw,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.home.projects_menu.hover(p), pointer_if(wb.home.projects_menu.over_clickable(p)))
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.home.projects_menu.scroll_by(dy))
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        // Open, the list has the keyboard (its field shows the caret / selection).
        wb.home.projects_menu.focused =
            wb.home.projects_menu.is_open() && !wb.dialogs.dialog.is_open();
        let current = wb.current_project();
        wb.home.projects_menu.paint_modal(
            current.as_ref(),
            &wb.home.recent,
            Painter { text, scene, palette: &wb.palette, icons: &wb.icons },
        );
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.home.projects_menu.is_animating()
    }

    fn has_input(&self, _wb: &Workbench) -> bool {
        true
    }
}
