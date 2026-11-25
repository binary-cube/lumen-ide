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

    /// When something of it waits for the mouse to rest (a card about to show, a menu's
    /// category the mouse lingers on): one redraw then; open or not yet. `None` by default.
    fn rest_deadline(&self, _wb: &Workbench) -> Option<std::time::Instant> {
        None
    }

    /// The mouse pressed somewhere else (a popover that goes then: the editor's cards);
    /// nothing by default.
    fn press_elsewhere(&self, _wb: &mut Workbench) {}

    /// The wheel turned somewhere else; nothing by default.
    fn scroll_elsewhere(&self, _wb: &mut Workbench) {}

    /// A press of its own is held (the mouse's moves must reach [`Self::drag`]); `false` by
    /// default.
    fn held(&self, _wb: &mut Workbench) -> bool {
        false
    }

    /// The mouse moved while a press of its own is held (a scrollbar's thumb, a slider, a
    /// view's drag): `Some(redraw)` if it holds one; `None` (the default): nothing held here,
    /// the move goes on to what's under it.
    fn drag(&self, _wb: &mut Workbench, _text: &mut TextSystem, _p: Point) -> Option<bool> {
        None
    }

    /// The mouse let go: `Some(redraw)` if it held something (what a release decides is
    /// acted on); `None` (the default): nothing held here.
    fn release(&self, _wb: &mut Workbench, _text: &mut TextSystem) -> Option<bool> {
        None
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

    /// Its text field with the keyboard, when the app edits it (the clipboard, the cursor
    /// keys, a click's caret); `None` (the default): none, or the view edits its own.
    fn input<'a>(&self, _wb: &'a Workbench) -> Option<&'a ui_kit::TextInput> {
        None
    }

    /// [`Self::input`], to change it.
    fn input_mut<'a>(&self, _wb: &'a mut Workbench) -> Option<&'a mut ui_kit::TextInput> {
        None
    }

    /// What it has selected, when it answers the clipboard itself (a module's view: its
    /// field's or its own selection, never what's behind it): `Some` (perhaps nothing
    /// selected); `None` (the default): it doesn't.
    fn selected_text(&self, _wb: &Workbench) -> Option<Option<String>> {
        None
    }

    /// An edit command (`undo`, `redo`, `cut`, `select_all`) for its own field or view:
    /// `Some(used)` if it takes it; `None` (the default): it doesn't.
    fn edit_command(&self, _wb: &mut Workbench, _id: &str) -> Option<bool> {
        None
    }

    /// Text pasted while it's on top: `Some(used)` if it takes it (its view decides);
    /// `None` (the default): what's under it does.
    fn paste(&self, _wb: &mut Workbench, _text: &mut TextSystem, _line: &str) -> Option<bool> {
        None
    }

    /// Its work isn't done yet (a spinner turns); `false` by default.
    fn loading(&self, _wb: &Workbench) -> bool {
        false
    }
}

/// The cursor over something clickable or not.
fn pointer_if(clickable: bool) -> CursorIcon {
    if clickable { CursorIcon::Pointer } else { CursorIcon::Default }
}

impl Workbench {
    /// When each window over the editor next wants a frame at its own pace (a scrollbar
    /// fading, a notification's ring, a modal view's scene, a settings card's picture), by
    /// its name (a profile recording says whose); none at rest.
    pub fn overlays_due(&self) -> impl Iterator<Item = (&'static str, std::time::Instant)> + '_ {
        Overlay::ALL
            .into_iter()
            .filter(|o| self.overlay_is_open(*o))
            .filter_map(|o| Some((o.name(), o.slot().frame_deadline(self)?)))
    }

    /// The mouse pressed or the wheel turned at `p`: each window over the editor not under
    /// it is told, open or about to open (a card waiting for its delay goes too).
    pub(super) fn elsewhere(&mut self, p: Point, press: bool) {
        for o in Overlay::ALL {
            let slot = o.slot();
            if slot.is_open(self) && slot.holds(self, p) {
                continue;
            }
            if press {
                slot.press_elsewhere(self);
            } else {
                slot.scroll_elsewhere(self);
            }
        }
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
    /// Its name where one is said (a profile recording's frame askers).
    pub(super) fn name(self) -> &'static str {
        match self {
            Overlay::CodeHover => "code-hover",
            Overlay::Completion => "completion",
            Overlay::ParamHint => "param-hint",
            Overlay::FileDrop => "file-drop",
            Overlay::Toasts => "toast",
            Overlay::Hint => "hint",
            Overlay::StatusPopover => "status-popover",
            Overlay::MenuDrawer => "menu-drawer",
            Overlay::ContextMenu => "context-menu",
            Overlay::DropdownList => "dropdown-list",
            Overlay::ChoiceMenu => "choice-menu",
            Overlay::Dialog => "dialog",
            Overlay::ModalView => "modal-view",
            Overlay::About => "about",
            Overlay::Settings => "settings",
            Overlay::ProjectsMenu => "projects-menu",
            Overlay::ResourcePills => "resource-pills",
        }
    }

    /// The overlay's behavior.
    pub(super) fn slot(self) -> &'static dyn OverlaySlot {
        match self {
            Overlay::CodeHover => &super::editor_popups::CodeHoverSlot,
            Overlay::Completion => &super::editor_popups::CompletionSlot,
            Overlay::ParamHint => &super::editor_popups::ParamHintSlot,
            Overlay::FileDrop => &super::file_drop::FileDropSlot,
            Overlay::Toasts => &super::toasts::ToastsSlot,
            Overlay::Hint => &super::hints::HintSlot,
            Overlay::StatusPopover => &super::status::StatusPopoverSlot,
            Overlay::MenuDrawer => &super::menu_drawer::MenuDrawerSlot,
            Overlay::ContextMenu => &ContextMenuSlot,
            Overlay::DropdownList => &DropdownListSlot,
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
        // The modules' switches and the app's own (the window kept on top).
        let states: Vec<(&'static str, Option<bool>)> = if wb.context_menu.is_open() {
            let app = crate::commands::SPECS.iter().map(|s| (s.id.0, wb.app_switched(s.id)));
            support
                .commands
                .iter()
                .map(|c| (c.spec.id.0, c.handler.switched(&*wb)))
                .chain(app.filter(|(_, on)| on.is_some()))
                .collect()
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

/// A dropdown's list (a bar's): the shared picker under its field — ↑ ↓ move, ↵ takes,
/// Esc or a click outside closes it; a row taken goes back to the bar as its command.
struct DropdownListSlot;

impl DropdownListSlot {
    /// What the list answered: a row taken is its bar's command.
    fn answered(
        wb: &mut Workbench,
        event: Option<ui_workbench::picker::PickerEvent>,
    ) -> MouseResult {
        match event {
            Some(ui_workbench::picker::PickerEvent::Chosen(i)) => {
                let ids = std::mem::take(&mut wb.dropdown_ids);
                match ids.into_iter().nth(i) {
                    Some(id) => MouseResult::MenuCommand(format!("{BAR_MENU}{id}")),
                    None => MouseResult::Redraw,
                }
            }
            Some(_) => MouseResult::Redraw,
            None => MouseResult::None,
        }
    }
}

impl OverlaySlot for DropdownListSlot {
    fn held(&self, wb: &mut Workbench) -> bool {
        wb.dropdown_list.dragging()
    }

    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.dropdown_list.is_open()
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        let key = match key {
            EditKey::Escape => ui_workbench::ViewerKey::Escape,
            EditKey::Enter { .. } => ui_workbench::ViewerKey::Enter,
            EditKey::Move { motion: Motion::Up, .. } => ui_workbench::ViewerKey::Up,
            EditKey::Move { motion: Motion::Down, .. } => ui_workbench::ViewerKey::Down,
            EditKey::Backspace { .. } => ui_workbench::ViewerKey::Backspace,
            _ => return true,
        };
        let event = wb.dropdown_list.key(key, ui_workbench::KeyReach::default());
        let result = Self::answered(wb, event);
        if !matches!(result, MouseResult::None) {
            wb.request = Some(result);
        }
        true
    }

    fn text(&self, wb: &mut Workbench, _text: &mut TextSystem, typed: &str) -> bool {
        wb.dropdown_list.text_input(typed)
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        let event = wb.dropdown_list.press(p);
        match Self::answered(wb, event) {
            MouseResult::None => MouseResult::Redraw,
            result => result,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        // Its own cursor (a hand on its rows, the text's in its search), never what's under.
        let cursor = wb.dropdown_list.cursor_at(p).map_or(CursorIcon::Default, super::cursor_icon);
        (wb.dropdown_list.hover(p), cursor)
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.dropdown_list.scroll(dy))
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.dropdown_list.dragging().then(|| wb.dropdown_list.drag(p))
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.dropdown_list.release().then_some(true)
    }

    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        if !wb.dropdown_list.is_open() {
            return;
        }
        let ctx = ui_workbench::ds::Ctx::new(&wb.palette, wb.scale);
        let bounds = kernel::Rect::new(0.0, 0.0, wb.size.width as f32, wb.size.height as f32);
        let anchor = wb.dropdown_anchor;
        wb.dropdown_list.paint(scene, text, &wb.icons, &ctx, (anchor, bounds), "");
    }

    /// Nothing of its own moves every frame: its scrollbar fades at its own pace.
    fn animating(&self, _wb: &Workbench) -> bool {
        false
    }

    /// Its scrollbar's next change, while it's open (closed, it isn't drawn).
    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.dropdown_list.is_open().then(|| wb.dropdown_list.next_frame()).flatten()
    }

    fn has_input(&self, wb: &Workbench) -> bool {
        wb.dropdown_list.searches()
    }
}

struct ChoiceMenuSlot;

impl OverlaySlot for ChoiceMenuSlot {
    fn held(&self, wb: &mut Workbench) -> bool {
        wb.choice_menu.scrollbar_held()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.choice_menu.scrollbar_held().then(|| wb.choice_menu.scrollbar_drag(p))
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.choice_menu.scrollbar_release().then_some(true)
    }

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
    fn held(&self, wb: &mut Workbench) -> bool {
        wb.dialogs.dialog.held()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.dialogs.dialog.held().then(|| wb.dialogs.dialog.drag(p))
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.dialogs.dialog.release().then_some(true)
    }

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
        // On a part that holds the mouse (its list's thumb): a drag goes on.
        if wb.dialogs.dialog.press(p) {
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

    fn input<'a>(&self, wb: &'a Workbench) -> Option<&'a ui_kit::TextInput> {
        wb.dialogs.dialog.input.as_ref()
    }

    fn input_mut<'a>(&self, wb: &'a mut Workbench) -> Option<&'a mut ui_kit::TextInput> {
        wb.dialogs.dialog.input.as_mut()
    }
}

/// A module's view in a modal window (the remote project wizard).
struct ModalViewSlot;

impl OverlaySlot for ModalViewSlot {
    /// Its view at its own pace (a full-window scene).
    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.modal_view_deadline()
    }

    fn held(&self, wb: &mut Workbench) -> bool {
        wb.modal_view_dragging()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.drag_modal_view(p)
    }

    /// Its view's press let go: what the view decided on it is acted on.
    fn release(&self, wb: &mut Workbench, text: &mut TextSystem) -> Option<bool> {
        let changed = wb.release_modal_view()?;
        wb.take_modal_action(text);
        Some(changed)
    }

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

    fn loading(&self, wb: &Workbench) -> bool {
        wb.modal_view_loading()
    }

    fn selected_text(&self, wb: &Workbench) -> Option<Option<String>> {
        let modal = wb.modal_view.as_ref()?;
        Some(modal.viewer.selected_text().filter(|s| !s.is_empty()))
    }

    /// ⌘A goes to its view; undo, redo and cut to its field, while it has one.
    fn edit_command(&self, wb: &mut Workbench, id: &str) -> Option<bool> {
        match id {
            "select_all" => wb.modal_view_command(id),
            _ => wb.modal_view_field_command(id),
        }
    }

    fn paste(&self, wb: &mut Workbench, text: &mut TextSystem, line: &str) -> Option<bool> {
        wb.modal_view_text(text, line)
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
    fn input<'a>(&self, wb: &'a Workbench) -> Option<&'a ui_kit::TextInput> {
        Some(wb.settings.query_input())
    }

    fn input_mut<'a>(&self, wb: &'a mut Workbench) -> Option<&'a mut ui_kit::TextInput> {
        Some(wb.settings.query_input_mut())
    }

    /// A settings card playing its picture (Relax mode's scenes), at its pace.
    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.settings_card_deadline()
    }

    fn held(&self, wb: &mut Workbench) -> bool {
        wb.settings.slider_held() || wb.settings.scrollbar_held()
    }

    /// A scrollbar held (the navigation's, the content's, a dropdown list's) follows; a held
    /// slider chooses each stop it reaches (saved, shown).
    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        if wb.settings.scrollbar_held() {
            return Some(wb.settings.drag_scrollbar(p));
        }
        if !wb.settings.slider_held() {
            return None;
        }
        let Some(SettingsAction::Choose { block, key, value }) = wb.settings.drag(p) else {
            return Some(false);
        };
        wb.settings_choose(block, key, value);
        wb.settings_model.blocks = wb.settings_blocks();
        Some(true)
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.settings.release().then_some(true)
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.settings.is_open()
    }

    /// While a module's text field is written, Enter saves it and Esc gives it up. While
    /// searching, Esc clears the search and ↑↓ walk the results; else ↑↓ change the section.
    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        // A dropdown's list open: its keys (↑ ↓ choose, ↵ takes, Esc closes it only).
        if wb.settings.picker_open() {
            let key = match key {
                EditKey::Escape => ui_workbench::ViewerKey::Escape,
                EditKey::Enter { .. } => ui_workbench::ViewerKey::Enter,
                EditKey::Move { motion: Motion::Up, .. } => ui_workbench::ViewerKey::Up,
                EditKey::Move { motion: Motion::Down, .. } => ui_workbench::ViewerKey::Down,
                _ => return true,
            };
            if let Some(action) = wb.settings.picker_key(key) {
                wb.request = Some(MouseResult::Settings(action));
            }
            return true;
        }
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
    /// A row pressed: it may be dragged into a group (the moves follow it).
    fn held(&self, wb: &mut Workbench) -> bool {
        wb.home.projects_menu.pressed()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        let menu = &mut wb.home.projects_menu;
        menu.pressed().then(|| menu.drag(p))
    }

    /// The press ends: the row's action (a click) or a move (a drop).
    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.release_projects_list(true)
    }

    fn input<'a>(&self, wb: &'a Workbench) -> Option<&'a ui_kit::TextInput> {
        Some(&wb.home.projects_menu.query.input)
    }

    fn input_mut<'a>(&self, wb: &'a mut Workbench) -> Option<&'a mut ui_kit::TextInput> {
        Some(&mut wb.home.projects_menu.query.input)
    }

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
