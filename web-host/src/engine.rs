//! The engine behind the host: each app's page a windowless CEF browser, drawn off screen
//! (its frames handed to the app as GPU textures: [`crate::transfer`]), driven by the app's
//! commands. Called on the host's main thread, CEF's own.

use std::collections::HashMap;
use std::rc::Rc;

use cef::*;
use web_engine_protocol::{
    Button, Command, Drag, EditAction, Hello, Ime, Key, KeyAction, Modifiers, Mouse, MouseAction,
    Open, Size, Source, Wheel,
};
use web_host_core::{Acks, ClientId, Engine, Outbox, PageKey};

use crate::handlers::{self, Kind, PageParts, PageState};
use crate::pixels::PixelFiles;
use crate::transfer::Transfer;

/// Our build of the engine for one Chromium: raised for every package made again with the
/// same Chromium (a fix of ours, another configuration), back to 1 with a new Chromium. The
/// engine's version is Chromium's, then this (`156.0.8078.100-2`).
pub const BUILD: u32 = 5;

/// The engine's version told to the apps and written in its package: Chromium's, then ours.
pub fn version() -> String {
    format!(
        "{}.{}.{}.{}-{BUILD}",
        sys::CHROME_VERSION_MAJOR,
        sys::CHROME_VERSION_MINOR,
        sys::CHROME_VERSION_BUILD,
        sys::CHROME_VERSION_PATCH,
    )
}

/// What the package says of itself (`engine.json`): its version and the protocol it speaks.
pub fn manifest() -> String {
    let info = web_engine_protocol::EngineInfo {
        version: version(),
        protocol: web_engine_protocol::PROTOCOL,
    };
    serde_json::to_string(&info).unwrap_or_default()
}

/// The largest page side an app may ask (pixels; a larger one is brought down to it).
const MAX_SIDE: u32 = 16_384;
/// The least scale an app may ask (device pixels per CSS pixel; a smaller one is brought up).
const MIN_SCALE: f32 = 0.5;
/// The largest scale an app may ask (device pixels per CSS pixel).
const MAX_SCALE: f32 = 8.0;

/// The most frames a second Chromium draws a page at: a ceiling only (it draws only when
/// something changes), a display's usual rate.
const FRAME_RATE_CEILING: i32 = 60;

/// `size` brought within what a page may be.
pub(crate) fn sane(size: Size) -> Size {
    let scale = if size.scale.is_finite() { size.scale.clamp(MIN_SCALE, MAX_SCALE) } else { 1.0 };
    Size { width: size.width.clamp(1, MAX_SIDE), height: size.height.clamp(1, MAX_SIDE), scale }
}

/// A page open.
struct Page {
    browser: Browser,
    state: Rc<PageState>,
    hidden: bool,
    /// The mouse buttons held (CEF's flags: a move carries them, or a drag is a hover).
    buttons: u32,
    /// The wheel's movement not sent yet (CEF takes whole pixels: a trackpad's fractions
    /// add up).
    wheel: (f32, f32),
    /// An HTML page's text, kept while it shows.
    html: Option<crate::scheme::HtmlPage>,
}

impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Page").field("hidden", &self.hidden).finish_non_exhaustive()
    }
}

/// CEF behind the host.
#[derive(Debug)]
pub struct CefEngine {
    pages: HashMap<PageKey, Page>,
    /// How each app gets its frames' textures (its process, its Mach service).
    transfers: HashMap<ClientId, Rc<Transfer>>,
    acks: Acks,
    channel_dir: std::path::PathBuf,
}

impl CefEngine {
    /// `channel_dir`: the channel's folder, the only place a page's pixels' file may be.
    pub fn new(acks: Acks, channel_dir: std::path::PathBuf) -> Self {
        Self { pages: HashMap::new(), transfers: HashMap::new(), acks, channel_dir }
    }

    /// The pages shown (not hidden): CEF's work runs on a timer only while there's one.
    pub fn shown_pages(&self) -> usize {
        self.pages.values().filter(|p| !p.hidden).count()
    }

    /// The pages' accessibility trees held back ([`handlers::ACCESS_EVERY`]) told once due;
    /// the soonest one still waiting is when.
    pub fn tell_access(&self) -> Option<std::time::Instant> {
        let now = std::time::Instant::now();
        self.pages.values().filter_map(|p| p.state.tell_access(now)).min()
    }

    /// The browsers not closed yet (CEF ends once they are).
    pub fn open_browsers(&self) -> usize {
        handlers::open_browsers()
    }

    /// The pages whose apps caught up since frames of theirs were dropped painted again (the
    /// last frame they were given may be older than the page).
    pub fn catch_up(&self) {
        for page in self.pages.values().filter(|page| page.state.caught_up()) {
            if let Some(host) = page.browser.host() {
                host.invalidate(PaintElementType::VIEW);
                host.invalidate(PaintElementType::POPUP);
            }
        }
    }

    /// Every page closed (the host ends).
    pub fn close_all(&mut self) {
        let keys: Vec<PageKey> = self.pages.keys().copied().collect();
        for key in keys {
            self.close(key);
        }
    }
}

impl Engine for CefEngine {
    fn version(&self) -> String {
        version()
    }

    fn adapter(&self) -> Option<web_engine_protocol::Adapter> {
        crate::app::adapter()
    }

    fn joined(&mut self, client: ClientId, hello: &Hello) {
        self.transfers.insert(client, Rc::new(Transfer::to(hello)));
    }

    fn left(&mut self, client: ClientId) {
        self.transfers.remove(&client);
    }

    fn open(&mut self, key: PageKey, open: Open, outbox: &Outbox) {
        let Some(transfer) = self.transfers.get(&key.client).cloned() else { return };
        // Its frames as pixels where the app asked (its files made here, in the channel's
        // folder only), else as GPU textures. Pixels asked and refused: the page isn't made
        // (its app can't take textures, and would get nothing it can show).
        let pixels = match open.pixels.as_deref() {
            None => None,
            Some(path) => match PixelFiles::open(&self.channel_dir, path) {
                Some(files) => Some(files),
                None => {
                    tracing::warn!(page = key.page.0, "a page's pixels' file wasn't made");
                    outbox.send(key.client, web_engine_protocol::Event::Crashed { page: key.page });
                    return;
                }
            },
        };
        // An HTML page opens at its own address (found by its token); an app's first on a
        // blank page, its address loaded once its folder is known (its first requests may
        // come while the browser is made, before its identity is).
        let (url, root, html) = match open.source {
            Source::App { root, entry } => (web_pages::entry_url(&entry), Some(root), None),
            Source::Html(html) => {
                let html = crate::scheme::put_html(html);
                (html.url.clone(), None, Some(html))
            }
        };
        let kind = match &html {
            Some(html) => Kind::Html(html.url.clone()),
            None => Kind::App,
        };
        let parts = PageParts {
            key,
            size: open.size,
            outbox: outbox.clone(),
            acks: self.acks.clone(),
            transfer,
            kind,
            pixels,
        };
        let state = Rc::new(PageState::new(parts));
        let first = if root.is_some() { "about:blank" } else { url.as_str() };
        let Some(browser) = make_browser(&state, first, open.background) else {
            tracing::warn!("a page's browser wasn't made");
            // Its files go now, whoever still holds its state.
            state.close();
            outbox.send(key.client, web_engine_protocol::Event::Crashed { page: key.page });
            return;
        };
        if let Some(root) = root {
            crate::scheme::set_root(browser.identifier(), Some(root));
            if let Some(frame) = browser.main_frame() {
                frame.load_url(Some(&url.as_str().into()));
            }
        }
        if open.hidden
            && let Some(host) = browser.host()
        {
            host.was_hidden(1);
        }
        tracing::debug!(page = key.page.0, "a page opened");
        let page =
            Page { browser, state, hidden: open.hidden, buttons: 0, wheel: (0.0, 0.0), html };
        self.pages.insert(key, page);
    }

    fn command(&mut self, key: PageKey, command: Command) {
        let Some(page) = self.pages.get_mut(&key) else { return };
        // Something happens on the page (a frame asked changes nothing by itself).
        if !matches!(command, Command::BeginFrame { .. }) {
            crate::pump::Pump::touch();
        }
        let Some(host) = page.browser.host() else { return };
        match command {
            Command::Resize { size, .. } => {
                if page.state.resize(size) {
                    host.notify_screen_info_changed();
                }
                host.was_resized();
            }
            Command::Hidden { hidden, .. } => {
                page.hidden = hidden;
                host.was_hidden(i32::from(hidden));
            }
            Command::LoadHtml { html, .. } => {
                // Only an HTML page takes another HTML (an app keeps its own).
                if page.state.is_app() {
                    return;
                }
                let html = crate::scheme::put_html(html);
                page.state.show_html(&html.url);
                if let Some(frame) = page.browser.main_frame() {
                    frame.load_url(Some(&html.url.as_str().into()));
                }
                // The text before let go as this one takes its place.
                page.html = Some(html);
            }
            Command::Post { message, .. } => {
                if let Some(frame) = page.browser.main_frame() {
                    let script = web_pages::receive_script(&message);
                    frame.execute_java_script(Some(&script.as_str().into()), None, 0);
                }
            }
            // Chromium schedules its own frames (external begin frames are off: `open`): an
            // app's request for one needs nothing.
            Command::BeginFrame { .. } => {}
            // Shown again after the app let its picture go: all of it drawn anew.
            Command::Repaint { .. } => {
                page.state.repaint();
                host.invalidate(PaintElementType::VIEW);
            }
            Command::Mouse { mouse, .. } => send_mouse(&host, &mut page.buttons, mouse),
            Command::Wheel { wheel, .. } => send_wheel(&host, &mut page.wheel, wheel),
            Command::Key { key, .. } => send_key(&host, key),
            Command::Focus { focused, .. } => host.set_focus(i32::from(focused)),
            Command::Edit { action, .. } => send_edit(&page.browser, action),
            Command::Ime { ime, .. } => send_ime(&host, ime),
            // Files from the system: only an app's page takes them (an HTML page runs
            // nothing: it would only try to open the file).
            Command::Drag { drag, .. } if page.state.is_app() => send_drag(&host, drag),
            Command::Drag { .. } => {}
            // Chromium tells its tree only while it's on; what was kept goes when off.
            Command::Access { on, .. } => {
                if page.state.set_access(on) {
                    host.set_accessibility_state(if on { State::ENABLED } else { State::DISABLED });
                }
            }
            Command::Hello(_)
            | Command::Open(_)
            | Command::Close { .. }
            | Command::FrameDone { .. } => {}
        }
    }

    fn close(&mut self, key: PageKey) {
        let Some(page) = self.pages.remove(&key) else { return };
        crate::scheme::set_root(page.browser.identifier(), None);
        // Nothing more told; its pixels' files go with it.
        page.state.close();
        if let Some(host) = page.browser.host() {
            host.close_browser(1);
        }
    }
}

/// A page's browser made, drawn off screen (its frames as GPU textures unless they go as
/// pixels), on `background`, first at `url` (`None`: it wasn't).
fn make_browser(state: &Rc<PageState>, url: &str, background: [u8; 4]) -> Option<Browser> {
    let window_info = WindowInfo {
        windowless_rendering_enabled: 1,
        shared_texture_enabled: i32::from(!state.uses_pixels()),
        // Chromium schedules its own frames: its compositor draws only when something
        // changes (measured: a still page 3.9 % of a core with frames asked 60 times a
        // second, 1.8 % with its own; plan 99).
        external_begin_frame_enabled: 0,
        ..Default::default()
    };
    let [r, g, b, a] = background;
    let settings = BrowserSettings {
        windowless_frame_rate: FRAME_RATE_CEILING,
        background_color: u32::from_be_bytes([a, r, g, b]),
        // A response's HTML runs no script.
        javascript: if state.is_app() { State::ENABLED } else { State::DISABLED },
        ..Default::default()
    };
    let mut client = handlers::client(Rc::clone(state));
    browser_host_create_browser_sync(
        Some(&window_info),
        Some(&mut client),
        Some(&url.into()),
        Some(&settings),
        None,
        None,
    )
}

/// An edit asked of the page's focused frame (its main one if none is).
fn send_edit(browser: &Browser, action: EditAction) {
    let Some(frame) = browser.focused_frame().or_else(|| browser.main_frame()) else { return };
    match action {
        EditAction::Undo => frame.undo(),
        EditAction::Redo => frame.redo(),
        EditAction::Cut => frame.cut(),
        EditAction::Copy => frame.copy(),
        EditAction::Paste => frame.paste(),
        EditAction::SelectAll => frame.select_all(),
    }
}

/// The input method's text: composed (its selection, or the caret at its end), committed or
/// cancelled.
fn send_ime(host: &BrowserHost, ime: Ime) {
    // "Nothing to replace" as CEF says it (an invalid range, not none).
    let nothing = Range { from: u32::MAX, to: u32::MAX };
    match ime {
        Ime::Compose { text, selection } => {
            let end = u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX);
            let (from, to) = selection.unwrap_or((end, end));
            let chosen = Range { from, to };
            let text = text.as_str().into();
            host.ime_set_composition(Some(&text), None, Some(&nothing), Some(&chosen));
        }
        Ime::Commit { text } => {
            host.ime_commit_text(Some(&text.as_str().into()), Some(&nothing), 0);
        }
        Ime::Cancel => host.ime_cancel_composition(),
    }
}

/// One of CEF's event flags, as its events carry them.
fn flag(flag: sys::cef_event_flags_t) -> u32 {
    // The enum's value is an `i32` on Windows (C's enums there), a `u32` elsewhere; a flag is
    // never negative, its bits are kept as they are.
    #[allow(clippy::unnecessary_cast, clippy::cast_sign_loss)]
    let bits = flag.0 as u32;
    bits
}

/// CEF's event flags for the keys held (`cef_event_flags_t`).
fn flags(modifiers: Modifiers) -> u32 {
    use sys::cef_event_flags_t as F;
    let held = |bit| modifiers.0 & bit != 0;
    [
        (Modifiers::SHIFT, F::EVENTFLAG_SHIFT_DOWN),
        (Modifiers::CONTROL, F::EVENTFLAG_CONTROL_DOWN),
        (Modifiers::ALT, F::EVENTFLAG_ALT_DOWN),
        (Modifiers::META, F::EVENTFLAG_COMMAND_DOWN),
    ]
    .into_iter()
    .filter(|(modifier, _)| held(*modifier))
    .fold(0, |all, (_, cef)| all | flag(cef))
}

/// Files dragged from the system, told to the page as CEF's drag target (copied, never
/// moved).
fn send_drag(host: &BrowserHost, drag: Drag) {
    let copy = DragOperationsMask::from(sys::cef_drag_operations_mask_t::DRAG_OPERATION_COPY);
    let at = |x: i32, y: i32| MouseEvent { x, y, modifiers: 0 };
    match drag {
        Drag::Enter { files, x, y } => {
            let Some(mut data) = drag_data_create() else { return };
            for file in &files {
                let path = file.to_string_lossy();
                let name =
                    file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                data.add_file(Some(&path.as_ref().into()), Some(&name.as_str().into()));
            }
            host.drag_target_drag_enter(Some(&mut data), Some(&at(x, y)), copy);
        }
        Drag::Over { x, y } => host.drag_target_drag_over(Some(&at(x, y)), copy),
        Drag::Leave => host.drag_target_drag_leave(),
        Drag::Drop { x, y } => host.drag_target_drop(Some(&at(x, y))),
    }
}

/// CEF's flag for a held mouse button (`EVENTFLAG_*_MOUSE_BUTTON`).
fn button_flag(button: Button) -> u32 {
    use sys::cef_event_flags_t as F;
    flag(match button {
        Button::Left => F::EVENTFLAG_LEFT_MOUSE_BUTTON,
        Button::Middle => F::EVENTFLAG_MIDDLE_MOUSE_BUTTON,
        Button::Right => F::EVENTFLAG_RIGHT_MOUSE_BUTTON,
    })
}

/// The mouse, with the buttons it holds (`buttons`, kept per page) on every event: a move
/// with one held is a drag.
fn send_mouse(host: &BrowserHost, buttons: &mut u32, mouse: Mouse) {
    let button = |b: Button| match b {
        Button::Left => MouseButtonType::LEFT,
        Button::Middle => MouseButtonType::MIDDLE,
        Button::Right => MouseButtonType::RIGHT,
    };
    match mouse.action {
        MouseAction::Down(b) => *buttons |= button_flag(b),
        MouseAction::Up(b) => *buttons &= !button_flag(b),
        MouseAction::Leave => *buttons = 0,
        MouseAction::Move => {}
    }
    let modifiers = flags(mouse.modifiers) | *buttons;
    let event = MouseEvent { x: mouse.x as i32, y: mouse.y as i32, modifiers };
    let clicks = i32::from(mouse.clicks.max(1));
    match mouse.action {
        MouseAction::Move => host.send_mouse_move_event(Some(&event), 0),
        MouseAction::Leave => host.send_mouse_move_event(Some(&event), 1),
        MouseAction::Down(b) => host.send_mouse_click_event(Some(&event), button(b), 0, clicks),
        MouseAction::Up(b) => host.send_mouse_click_event(Some(&event), button(b), 1, clicks),
    }
}

/// The wheel in whole pixels, what's left of a trackpad's fractions kept (`rest`).
fn send_wheel(host: &BrowserHost, rest: &mut (f32, f32), wheel: Wheel) {
    let modifiers = flags(wheel.modifiers);
    let event = MouseEvent { x: wheel.x as i32, y: wheel.y as i32, modifiers };
    let (whole, left) = whole_pixels(*rest, (wheel.dx, wheel.dy));
    *rest = left;
    if whole != (0, 0) {
        host.send_mouse_wheel_event(Some(&event), whole.0, whole.1);
    }
}

/// `delta` added to what was left: the whole pixels to send, and what's left now (a delta
/// that isn't a number, or one past any, moves nothing and leaves nothing).
fn whole_pixels(rest: (f32, f32), delta: (f32, f32)) -> ((i32, i32), (f32, f32)) {
    let axis = |rest: f32, delta: f32| {
        let total = rest + delta;
        if !total.is_finite() {
            return (0, 0.0);
        }
        let whole = total.trunc();
        (whole as i32, total - whole)
    };
    let ((x, rest_x), (y, rest_y)) = (axis(rest.0, delta.0), axis(rest.1, delta.1));
    ((x, y), (rest_x, rest_y))
}

/// A key: pressed and let go as one event each, its character with them; a character typed
/// as CEF takes it, one event per UTF-16 unit (an emoji: two).
fn send_key(host: &BrowserHost, key: Key) {
    let type_ = match key.action {
        KeyAction::Down => KeyEventType::RAWKEYDOWN,
        KeyAction::Up => KeyEventType::KEYUP,
        KeyAction::Char => KeyEventType::CHAR,
    };
    let (units, length) = utf16_of(key.character);
    let events = if matches!(key.action, KeyAction::Char) { length } else { 1 };
    for &character in &units[..events] {
        let event = KeyEvent {
            type_,
            modifiers: flags(key.modifiers),
            windows_key_code: key.windows_code,
            native_key_code: key.native_code,
            character,
            unmodified_character: character,
            ..Default::default()
        };
        host.send_key_event(Some(&event));
    }
}

/// A character as UTF-16: its units and how many (one, or two past the BMP: an emoji; not a
/// character: one, zero).
fn utf16_of(character: u32) -> ([u16; 2], usize) {
    let mut units = [0; 2];
    let length = char::from_u32(character).map_or(1, |c| c.encode_utf16(&mut units).len());
    (units, length)
}

#[cfg(test)]
#[path = "../tests/unit/engine.rs"]
mod tests;
