//! The app's menus in a drawer from the window's bar (☰): where the system gives no menu
//! bar of ours (Windows, Linux) it's the only menu; on macOS it's there when asked for. The
//! menus are the native menu bar's (`commands::drawer_menus`), read when it opens; nothing
//! of it exists before it first opens, and it's gone again once it has slid out. A command
//! chosen goes the way the native menu's do (`PlatformEvent::Command`).

use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;
use ui_workbench::{DrawerAction, MenuDrawer, NavKey, NavOutcome};

impl Workbench {
    /// The window's bar has the menu button: the setting's, unless this window was told.
    pub(super) fn menu_button_shown(&self) -> bool {
        self.chrome.menu_button.unwrap_or_else(crate::window_menu::shown)
    }

    #[cfg(test)]
    /// This window's bar has the menu button whatever the setting (tests).
    pub fn set_menu_button(&mut self, shown: bool) {
        self.chrome.menu_button = Some(shown);
        self.layout();
    }

    /// The drawer is open (not sliding out).
    pub fn menu_drawer_open(&self) -> bool {
        self.menu_drawer.as_ref().is_some_and(|d| d.is_open())
    }

    /// Opens the drawer (where the bar has its button, nothing else over the window), or
    /// closes it. `true` if it did either.
    pub fn toggle_menu_drawer(&mut self) -> bool {
        if self.menu_drawer_open() {
            self.close_menu_drawer();
            return true;
        }
        if !self.menu_button_shown() || self.overlay_open() {
            return false;
        }
        self.code_hover.hide();
        self.completion.close();
        self.tooltips.hide();
        self.param_hint.hide();
        // What doesn't apply now: a row for another language's files left out (it means
        // nothing here — the native menu greys it, as its system wants); any other greyed
        // out, with why.
        let context = self.menu_context();
        let (elsewhere, disabled): (Vec<_>, Vec<_>) =
            self.disabled_rows(&context).into_iter().partition(|(_, missing)| {
                matches!(missing, Some(command_model::Requires::Language(_)))
            });
        // A row that turns something on or off shows how it stands (the window on top).
        let switched = |id: &str| crate::commands::by_id(id).and_then(|c| self.command_switched(c));
        let menus =
            crate::commands::drawer_menus(&|id| elsewhere.iter().any(|(e, _)| e == id), &switched);
        let disabled = disabled
            .into_iter()
            .map(|(id, missing)| (id, crate::commands::disabled_hint(missing)))
            .collect();
        match &mut self.menu_drawer {
            // Opened again while it slides out.
            Some(drawer) => drawer.reopen(menus),
            None => self.menu_drawer = Some(Box::new(MenuDrawer::new(menus))),
        }
        if let Some(drawer) = &mut self.menu_drawer {
            drawer.set_disabled(disabled);
        }
        self.place_menu_drawer();
        true
    }

    pub(super) fn close_menu_drawer(&mut self) {
        if let Some(drawer) = &mut self.menu_drawer {
            drawer.close();
        }
    }

    /// Its room: the whole window, top to bottom (its header row over the top bar).
    pub(super) fn place_menu_drawer(&mut self) {
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let scale = self.scale;
        if let Some(drawer) = &mut self.menu_drawer {
            drawer.set_area(Rect::new(0.0, 0.0, w, h), scale);
        }
    }

    /// The menu button's tooltip shortcut: F10 (as bound), and Alt where it opens it too.
    pub(super) fn menu_shortcut(&self) -> Option<String> {
        let key = crate::commands::shortcut_label(crate::commands::MENU);
        if cfg!(target_os = "macos") {
            return key;
        }
        Some(match key {
            Some(key) => format!("Alt · {key}"),
            None => "Alt".to_owned(),
        })
    }

    /// A held scrollbar of the drawer dragged: `Some(redraw)` while one is held.
    pub(super) fn drag_menu_drawer(&mut self, p: Point) -> Option<bool> {
        let drawer = self.menu_drawer.as_mut().filter(|d| d.scrollbar_held())?;
        Some(drawer.drag_scrollbar(p))
    }

    /// The mouse released over the drawer: `true` if a scrollbar was let go.
    pub(super) fn release_menu_drawer(&mut self) -> bool {
        self.menu_drawer.as_mut().is_some_and(|d| d.release())
    }

    #[cfg(feature = "snapshot")]
    /// The drawer as the snapshots show it: open, `keys` pressed in it, `typed` in its
    /// filter, its transitions ended.
    pub fn show_menu_drawer(&mut self, keys: &[NavKey], typed: Option<&str>) {
        if !self.menu_drawer_open() {
            self.toggle_menu_drawer();
        }
        let Some(drawer) = &mut self.menu_drawer else { return };
        if let Some(typed) = typed {
            drawer.type_text(typed);
        }
        for key in keys {
            drawer.key(*key);
        }
        drawer.settle();
    }
}

/// The drawer on the overlays' stack: nothing behind it gets the keys, the text, the scroll
/// or the mouse while it's open — but it's a menu, not a window with the user's work in
/// it: a click outside it closes it.
pub(super) struct MenuDrawerSlot;

impl OverlaySlot for MenuDrawerSlot {
    /// A category the mouse rests on: shown then.
    fn rest_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.menu_drawer.as_ref().and_then(|d| d.deadline())
    }

    fn input<'a>(&self, wb: &'a Workbench) -> Option<&'a ui_kit::TextInput> {
        Some(&wb.menu_drawer.as_ref()?.query.input)
    }

    fn input_mut<'a>(&self, wb: &'a mut Workbench) -> Option<&'a mut ui_kit::TextInput> {
        Some(&mut wb.menu_drawer.as_mut()?.query.input)
    }

    fn held(&self, wb: &mut Workbench) -> bool {
        wb.menu_drawer.as_ref().is_some_and(|d| d.scrollbar_held())
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.drag_menu_drawer(p)
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.release_menu_drawer().then_some(true)
    }

    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Modal
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.menu_drawer_open()
    }

    /// The empty screen stays under it.
    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn key(&self, wb: &mut Workbench, _text: &mut TextSystem, key: EditKey) -> bool {
        let Some(drawer) = &mut wb.menu_drawer else { return false };
        let nav = match key {
            EditKey::Escape => {
                wb.close_menu_drawer();
                return true;
            }
            EditKey::Move { motion: Motion::Up, .. } | EditKey::Tab { shift: true } => NavKey::Up,
            EditKey::Move { motion: Motion::Down, .. } | EditKey::Tab { shift: false } => {
                NavKey::Down
            }
            EditKey::Move { motion: Motion::Left, extend: false } => NavKey::Left,
            EditKey::Move { motion: Motion::Right, extend: false } => NavKey::Right,
            EditKey::Enter { .. } => NavKey::Enter,
            EditKey::Backspace { word } => {
                drawer.backspace(word);
                return true;
            }
            EditKey::Delete => {
                drawer.delete();
                return true;
            }
            // The rest moves in the filter's field (a word, Home / End, a selection).
            _ => return false,
        };
        match drawer.key(nav) {
            NavOutcome::Used => true,
            // ← → while filtering: the field's caret.
            NavOutcome::Unused => false,
            NavOutcome::Run(id) => {
                wb.close_menu_drawer();
                wb.request = Some(MouseResult::MenuCommand(id));
                true
            }
        }
    }

    fn text(&self, wb: &mut Workbench, _text: &mut TextSystem, typed: &str) -> bool {
        if let Some(drawer) = &mut wb.menu_drawer {
            drawer.type_text(typed);
        }
        true
    }

    fn clear_query(&self, wb: &mut Workbench) -> bool {
        let Some(drawer) = &mut wb.menu_drawer else { return false };
        let typed = drawer.has_query();
        if typed {
            drawer.clear_query();
        }
        typed
    }

    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, clicks: u32) -> MouseResult {
        // On the top bar beside it: it closes; the window's own buttons and its empty area
        // (moving the window) still do what they do.
        let on_drawer = wb.menu_drawer.as_ref().is_some_and(|d| d.contains(p));
        if !on_drawer && let Some(hit) = wb.chrome.top_bar.hit(p) {
            wb.close_menu_drawer();
            return match hit {
                TopBarHit::Minimize | TopBarHit::Maximize | TopBarHit::Close | TopBarHit::Empty => {
                    wb.click_top_bar(p, clicks).unwrap_or(MouseResult::Redraw)
                }
                _ => MouseResult::Redraw,
            };
        }
        let Some(drawer) = &mut wb.menu_drawer else { return MouseResult::None };
        match drawer.hit_with(p, clicks) {
            DrawerAction::Run(id) => {
                wb.close_menu_drawer();
                MouseResult::MenuCommand(id)
            }
            DrawerAction::Close => {
                wb.close_menu_drawer();
                MouseResult::Redraw
            }
            DrawerAction::Drag => MouseResult::StartDrag,
            DrawerAction::Inside => MouseResult::Redraw,
        }
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        let on_button = wb.chrome.top_bar.hit(p) == Some(TopBarHit::Menu);
        let Some(drawer) = &mut wb.menu_drawer else { return (false, CursorIcon::Default) };
        let changed = drawer.hover(p);
        let clickable = on_button || drawer.over_clickable(p);
        (changed, if clickable { CursorIcon::Pointer } else { CursorIcon::Default })
    }

    fn scroll(&self, wb: &mut Workbench, _dx: f32, dy: f32) -> Option<bool> {
        Some(wb.menu_drawer.as_mut().is_some_and(|d| d.scroll_by(dy)))
    }

    /// Drawn while open and while it slides out; gone (dropped) after.
    fn paint(&self, wb: &mut Workbench, text: &mut TextSystem, scene: &mut Scene) {
        let Some(drawer) = &mut wb.menu_drawer else { return };
        // The bar it covers, as just drawn: its height, its menu button, the window's own.
        let bar = &wb.chrome.top_bar;
        let button = bar.menu_button().unwrap_or_default();
        drawer.set_bar(TopBar::height(wb.scale), button, bar.window_controls());
        drawer.paint(&wb.icons, text, scene, &wb.palette);
        if drawer.is_gone() {
            wb.menu_drawer = None;
        }
    }

    fn animating(&self, wb: &Workbench) -> bool {
        wb.menu_drawer.as_ref().is_some_and(|d| d.is_animating())
    }

    fn has_input(&self, wb: &Workbench) -> bool {
        wb.menu_drawer_open()
    }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/menu_drawer.rs"]
mod tests;
