//! The input the app hands the pages it draws: the mouse to the page under it (or the one
//! a press holds until every button is let go), the keyboard to the one last pressed until
//! the app takes it back (a click elsewhere, its focus moved), a double click counted.

use std::time::{Duration, Instant};

use platform_api::{PageInput, PageMouseAction};

use super::{Live, WebViews};

/// How far (physical pixels) a press may be from the one before to count with it (a double
/// click), as the window's own.
const CLICK_SLOP: f32 = 4.0;

/// A press on a page, to count the next with it (a double click: the same button).
#[derive(Debug, Clone, Copy)]
struct LastPress {
    at: Instant,
    p: kernel::Point,
    button: platform_api::MouseButton,
    /// Its count (1, 2, 3).
    clicks: u8,
}

/// Where the app's input goes among the pages it draws: the one under the mouse, the one
/// a press holds until it's let go (a drag past its edge stays its), the one with the
/// keyboard; the last press, to count a double click.
#[derive(Debug, Default)]
pub(super) struct PagesInput {
    hovered: Option<u128>,
    held: Option<Held>,
    focused: Option<u128>,
    last_press: Option<LastPress>,
    modifiers: platform_api::Modifiers,
}

/// The page a press holds, and the buttons held on it (bits of [`button_bit`]): it keeps
/// the mouse until the last is let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    page: u128,
    buttons: u8,
}

/// The count of a press of `button` at `p` (`now`), `last` the one before: the next of a
/// double or triple click when it's the same button, soon enough and near enough (after
/// three, one again); else one.
fn clicks_of(
    last: Option<LastPress>,
    (button, p): (platform_api::MouseButton, kernel::Point),
    now: Instant,
    within: Duration,
) -> u8 {
    match last {
        Some(last)
            if last.button == button
                && now.duration_since(last.at) < within
                && (last.p.x - p.x).abs() < CLICK_SLOP
                && (last.p.y - p.y).abs() < CLICK_SLOP =>
        {
            last.clicks % 3 + 1
        }
        _ => 1,
    }
}

/// `button`'s bit among those held.
fn button_bit(button: platform_api::MouseButton) -> u8 {
    match button {
        platform_api::MouseButton::Left => 1,
        platform_api::MouseButton::Right => 2,
        platform_api::MouseButton::Middle => 4,
    }
}

impl PagesInput {
    /// The pages `keep` doesn't want are gone: nothing of the input points at them.
    pub(super) fn forget(&mut self, keep: impl Fn(u128) -> bool) {
        self.hovered = self.hovered.filter(|id| keep(*id));
        self.held = self.held.filter(|held| keep(held.page));
        self.focused = self.focused.filter(|id| keep(*id));
    }
}

/// What a page answers the mouse with: it took it (and the pointer it asks), or the
/// window's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum Pointer {
    Page(platform_api::CursorIcon),
    Window,
}

impl WebViews {
    /// The pointer the page under the mouse asks for (`None`: the mouse isn't over one).
    pub(in crate::app) fn hovered_cursor(&self) -> Option<platform_api::CursorIcon> {
        let live = self.live(self.input.hovered?)?;
        Some(live.view.cursor().unwrap_or(platform_api::CursorIcon::Default))
    }

    /// The keys held changed (the mouse's events carry them).
    pub(in crate::app) fn set_modifiers(&mut self, modifiers: platform_api::Modifiers) {
        self.input.modifiers = modifiers;
    }

    /// The page the app draws under `p` that takes input (`open`: nothing of the app's is
    /// over it there).
    pub(super) fn page_at(&self, p: kernel::Point, open: bool) -> Option<&Live> {
        if !open {
            return None;
        }
        self.live.iter().find(|l| l.shown_drawn() && l.view.takes_input() && l.area.contains(p))
    }

    /// The mouse moved to `p` (`open`: nothing of the app's over the pages there): handed to
    /// the page under it, or the one a press holds; the page it left told.
    pub(in crate::app) fn pointer_moved(&mut self, p: kernel::Point, open: bool) -> Pointer {
        let target = match self.input.held {
            Some(held) => self.live(held.page).map(|l| l.id),
            None => self.page_at(p, open).map(|l| l.id),
        };
        if let Some(left) = self.input.hovered.filter(|h| Some(*h) != target) {
            self.mouse(left, PageMouseAction::Leave, p, 1);
        }
        self.input.hovered = target;
        let Some(id) = target else { return Pointer::Window };
        self.mouse(id, PageMouseAction::Move, p, 1);
        let cursor = self.live(id).and_then(|l| l.view.cursor());
        Pointer::Page(cursor.unwrap_or(platform_api::CursorIcon::Default))
    }

    /// The mouse left the window: the page under it told — unless a press holds it (a drag
    /// past the window's edge stays the page's until it's let go).
    pub(in crate::app) fn pointer_left(&mut self, p: kernel::Point) {
        if self.input.held.is_some() {
            return;
        }
        if let Some(left) = self.input.hovered.take() {
            self.mouse(left, PageMouseAction::Leave, p, 1);
        }
    }

    /// The window lost the focus (another program took it): the page with the keyboard is
    /// told it lost it (kept as the one to have it back: [`Self::window_back`]); its button
    /// releases never come here, so the page a press held is told they were let go (at `p`,
    /// the mouse as last seen) and holds the mouse no more.
    pub(in crate::app) fn window_left(&mut self, p: kernel::Point) {
        self.to_focused(true, PageInput::Focus(false));
        let Some(held) = self.input.held.take() else { return };
        let clicks = self.input.last_press.map_or(1, |last| last.clicks);
        for button in [
            platform_api::MouseButton::Left,
            platform_api::MouseButton::Right,
            platform_api::MouseButton::Middle,
        ] {
            if held.buttons & button_bit(button) != 0 {
                self.mouse(held.page, PageMouseAction::Up(button), p, clicks);
            }
        }
    }

    /// The window has the focus again: the page that had the keyboard told it has it back.
    pub(in crate::app) fn window_back(&self) {
        self.to_focused(true, PageInput::Focus(true));
    }

    /// A press at `p` (`open`: nothing of the app's over the pages there; `covered`: a window
    /// of the app's is open over them — a menu, a dialog): `true` if a page took it (it then
    /// holds the mouse until every button is let go, and has the keyboard). A press on the
    /// window itself gives the keyboard back from the pages; one on a window over them keeps
    /// it (a page's own menu's row is the page's editing, Copy reaching it once the menu
    /// closes). A press while a page holds the mouse is that page's, wherever it is.
    pub(in crate::app) fn pointer_down(
        &mut self,
        button: platform_api::MouseButton,
        p: kernel::Point,
        (open, covered): (bool, bool),
    ) -> bool {
        let holding = self.input.held.map(|h| h.page).filter(|id| self.live(*id).is_some());
        let Some(id) = holding.or_else(|| self.page_at(p, open).map(|l| l.id)) else {
            // A click on the window itself: the keyboard back from the pages.
            if button == platform_api::MouseButton::Left && !covered {
                self.release_focus();
            }
            return false;
        };
        let now = Instant::now();
        let clicks = clicks_of(self.input.last_press, (button, p), now, crate::app::MULTI_CLICK);
        self.input.last_press = Some(LastPress { at: now, p, button, clicks });
        if self.input.focused != Some(id) {
            // The keyboard from the others (the system's pages too) to this one.
            self.release_focus_but(Some(id));
            if let Some(live) = self.live(id) {
                live.view.input(PageInput::Focus(true));
            }
            self.input.focused = Some(id);
        }
        let buttons = self.input.held.map_or(0, |h| h.buttons) | button_bit(button);
        self.input.held = Some(Held { page: id, buttons });
        self.mouse(id, PageMouseAction::Down(button), p, clicks);
        true
    }

    /// A button let go at `p`: `true` if a page held the mouse (it gets it; the hold ends
    /// with the last button).
    pub(in crate::app) fn pointer_up(
        &mut self,
        button: platform_api::MouseButton,
        p: kernel::Point,
    ) -> bool {
        let Some(held) = self.input.held else { return false };
        let buttons = held.buttons & !button_bit(button);
        self.input.held = (buttons != 0).then_some(Held { buttons, ..held });
        let clicks = self.input.last_press.map_or(1, |last| last.clicks);
        self.mouse(held.page, PageMouseAction::Up(button), p, clicks);
        true
    }

    /// The wheel at `p` (physical pixels: positive `dy` scrolls up): `true` if a page took it.
    pub(in crate::app) fn wheel(
        &mut self,
        p: kernel::Point,
        delta: (f32, f32),
        open: bool,
    ) -> bool {
        let Some(live) = self.page_at(p, open) else { return false };
        let at = live.local(p);
        let modifiers = self.input.modifiers;
        live.view.input(PageInput::Wheel(platform_api::PageWheel { at, delta, modifiers }));
        true
    }

    /// A key (`open`: no window of the app's has the keyboard): `true` if the page with the
    /// keyboard took it.
    pub(in crate::app) fn key(&self, event: &platform_api::KeyEvent, open: bool) -> bool {
        self.to_focused(open, PageInput::Key(event.clone()))
    }

    /// An editing action (copy, paste…): `true` if the page with the keyboard took it.
    pub(in crate::app) fn edit(&self, action: platform_api::PageEdit, open: bool) -> bool {
        self.to_focused(open, PageInput::Edit(action))
    }

    /// The menu a page asked for since (where in the window, what it's on), taken.
    pub(in crate::app) fn take_menu(&self) -> Option<platform_api::PageMenuAsk> {
        self.live.iter().filter(|l| l.visible).find_map(|l| {
            let ask = l.view.take_menu()?;
            Some(platform_api::PageMenuAsk { at: ask.at.offset(l.area.origin), ..ask })
        })
    }

    /// A screen reader's value for the field of the page with the keyboard: its text chosen,
    /// then the value typed (`true` if a page took it).
    pub(in crate::app) fn set_value(&self, value: &str, open: bool) -> bool {
        self.to_focused(open, PageInput::Edit(platform_api::PageEdit::SelectAll))
            && self.to_focused(open, PageInput::Text(value.to_owned()))
    }

    /// Whether a page the app draws has the keyboard (`open`: no window of the app's has it).
    pub(in crate::app) fn has_keyboard(&self, open: bool) -> bool {
        self.focused(open).is_some()
    }

    /// Text being composed by the input method: `true` if the page with the keyboard took it.
    pub(in crate::app) fn compose(
        &self,
        text: &str,
        cursor: Option<(usize, usize)>,
        open: bool,
    ) -> bool {
        let compose = platform_api::PageCompose { text: text.to_owned(), cursor };
        self.to_focused(open, PageInput::Compose(compose))
    }

    /// Where the page with the keyboard composes text (window's physical pixels), if one does.
    pub(in crate::app) fn caret(&self, open: bool) -> Option<kernel::Rect> {
        let live = self.focused(open)?;
        Some(live.view.caret()?.offset(live.area.origin))
    }

    /// Text from the input method: `true` if the page with the keyboard took it.
    pub(in crate::app) fn text(&self, text: &str, open: bool) -> bool {
        self.to_focused(open, PageInput::Text(text.to_owned()))
    }

    /// `input` handed to the page with the keyboard (`open`: no window of the app's has
    /// it): `true` if there's one.
    fn to_focused(&self, open: bool, input: PageInput) -> bool {
        let Some(live) = self.focused(open) else { return false };
        live.view.input(input);
        true
    }

    /// The page the app draws with the keyboard, if it's still shown.
    fn focused(&self, open: bool) -> Option<&Live> {
        let id = self.input.focused.filter(|_| open)?;
        self.live(id).filter(|l| l.visible && l.view.takes_input())
    }

    /// `action` of the mouse at `p` (window) for page `id`, in its own pixels.
    fn mouse(&self, id: u128, action: PageMouseAction, p: kernel::Point, clicks: u8) {
        let Some(live) = self.live(id) else { return };
        let at = live.local(p);
        let modifiers = self.input.modifiers;
        let mouse = platform_api::PageMouse { action, at, clicks, modifiers };
        live.view.input(PageInput::Mouse(mouse));
    }

    /// The pages give the keyboard back: a click on the window itself (not on a page), or
    /// the app's focus moved (a command, a key of the app's).
    pub(in crate::app) fn release_focus(&mut self) {
        self.release_focus_but(None);
    }

    /// The keyboard back from every page but `keep`: the one the app gave it to (told only
    /// if it had it) and the system's shown (they take it themselves, unknown to the app).
    fn release_focus_but(&mut self, keep: Option<u128>) {
        let focused = self.input.focused.take().filter(|id| Some(*id) != keep);
        let losing =
            |l: &&Live| l.visible && Some(l.id) != keep && (!l.is_drawn() || Some(l.id) == focused);
        for live in self.live.iter().filter(losing) {
            live.view.release_focus();
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_views/input.rs"]
mod tests;
