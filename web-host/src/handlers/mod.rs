//! A page's CEF handlers and the state they share: its size and scale, its frames (handed to
//! its app as GPU textures or written as pixels, held until copied: [`render`]), its cursor,
//! context menu and accessibility tree ([`display`]), the links it wants opened (never
//! followed: told to the app), where it may go and what it may load, its first content and
//! its renderer's end ([`request`]).

mod access_tree;
mod display;
mod frames;
mod render;
mod request;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use cef::*;
use web_engine_protocol::{Event, Frame as FrameMessage, Size, Texture};
use web_host_core::{Acks, Outbox, PageKey, Undelivered};

pub use access_tree::ACCESS_EVERY;
use access_tree::{AccessState, Due};
use frames::Frames;

use crate::pixels::{Painted, PixelFiles};
use crate::transfer::Transfer;

/// How long a frame's buffer waits for its app's copy (a copy takes ~1 ms; past this the app
/// is slow or gone, and the buffer goes back to Chromium anyway).
const COPY_WAIT: Duration = Duration::from_millis(100);

/// The browsers made and not closed yet (CEF ends once there's none).
static OPEN_BROWSERS: AtomicUsize = AtomicUsize::new(0);

/// The browsers not closed yet.
pub fn open_browsers() -> usize {
    OPEN_BROWSERS.load(Ordering::Acquire)
}

/// What a page shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// An app's page: its scripts run, its own files.
    App,
    /// A response's HTML, at its address now (the only one it may be at).
    Html(String),
}

/// What a page's state is made of, as it opens.
#[derive(Debug)]
pub struct PageParts {
    pub key: PageKey,
    /// As the app asked it (brought within a page's bounds here).
    pub size: Size,
    /// Where its events go.
    pub outbox: Outbox,
    /// Where its frames' copies are told.
    pub acks: Acks,
    /// How its app gets its frames' textures.
    pub transfer: Rc<Transfer>,
    pub kind: Kind,
    /// Its frames' files, where its app asked for pixels (`None`: GPU textures).
    pub pixels: Option<PixelFiles>,
}

/// A page's state its handlers share (on CEF's main thread).
#[derive(Debug)]
pub struct PageState {
    /// The page, as its app and the host know it.
    key: PageKey,
    /// Its size now (within a page's bounds).
    size: Cell<Size>,
    /// Closed: nothing more is told (CEF lets its handlers go later).
    closed: Cell<bool>,
    /// What it shows (an HTML page's address changes with each HTML given).
    kind: RefCell<Kind>,
    /// Where its events go.
    outbox: Outbox,
    /// Where its frames' copies are told (and whether its app is behind).
    acks: Acks,
    /// How its app gets its frames' textures.
    transfer: Rc<Transfer>,
    /// Its frames' numbers, which are handed over, what each tells changed.
    frames: RefCell<Frames>,
    /// Its frames as pixels, written into these files (the app reads them): `None`, as GPU
    /// textures. Let go (the files removed) as the page closes.
    pixels: RefCell<Option<PixelFiles>>,
    /// Its accessibility tree while the app asks for it (a screen reader runs).
    access: RefCell<Option<AccessState>>,
}

/// A frame painted, as it's told to its app.
struct Painting<'a> {
    popup: bool,
    /// Its number ([`PageState::next_frame`]).
    frame: u64,
    width: u32,
    height: u32,
    texture: Texture,
    /// CEF's changed rectangles.
    dirty: Option<&'a [Rect]>,
}

impl PageState {
    /// A page's state from its parts.
    pub fn new(parts: PageParts) -> Self {
        Self {
            key: parts.key,
            size: Cell::new(crate::engine::sane(parts.size)),
            closed: Cell::new(false),
            kind: RefCell::new(parts.kind),
            outbox: parts.outbox,
            acks: parts.acks,
            transfer: parts.transfer,
            frames: RefCell::default(),
            pixels: RefCell::new(parts.pixels),
            access: RefCell::new(None),
        }
    }

    /// An app's page (its scripts run, its own files), not a response's HTML.
    pub fn is_app(&self) -> bool {
        *self.kind.borrow() == Kind::App
    }

    /// Its frames come as pixels (else as GPU textures).
    pub fn uses_pixels(&self) -> bool {
        self.pixels.borrow().is_some()
    }

    /// Its size now (within a page's bounds).
    pub fn size(&self) -> Size {
        self.size.get()
    }

    /// Its size as the app asks it now (brought within a page's bounds); whether its scale
    /// changed.
    pub fn resize(&self, size: Size) -> bool {
        let size = crate::engine::sane(size);
        let scaled = self.size.get().scale != size.scale;
        self.size.set(size);
        scaled
    }

    /// An HTML page shown at `url` now (the only address it may be at); an app's keeps its
    /// own.
    pub fn show_html(&self, url: &str) {
        if let Kind::Html(at) = &mut *self.kind.borrow_mut() {
            url.clone_into(at);
        }
    }

    /// The page closed: nothing more is told, its pixels' files removed now (CEF lets its
    /// handlers go later).
    pub fn close(&self) {
        self.closed.set(true);
        self.pixels.borrow_mut().take();
    }

    /// The app let its picture go: the next frame of the view is told whole.
    pub fn repaint(&self) {
        self.frames.borrow_mut().missed(false);
    }

    /// Its accessibility tree asked for (`on`) or not: whether Chromium is to be told. Asked
    /// again while on, the tree kept is told again whole (nothing reset).
    pub fn set_access(&self, on: bool) -> bool {
        let mut access = self.access.borrow_mut();
        match (on, access.as_mut()) {
            (true, Some(state)) => {
                state.changed();
                false
            }
            (true, None) => {
                *access = Some(AccessState::default());
                true
            }
            (false, Some(_)) => {
                *access = None;
                true
            }
            (false, None) => false,
        }
    }

    /// Its tree changed by `change` (nothing while it isn't asked for), told now or when
    /// [`ACCESS_EVERY`] has passed ([`Self::tell_access`]).
    pub fn access_changed(&self, change: impl FnOnce(&mut crate::access::Tree)) {
        {
            let mut access = self.access.borrow_mut();
            let Some(state) = access.as_mut() else { return };
            change(&mut state.tree);
            state.changed();
        }
        self.tell_access(Instant::now());
    }

    /// Its tree told if it changed and [`ACCESS_EVERY`] has passed since it was last; else
    /// when it's due (`None`: nothing waits).
    pub fn tell_access(&self, now: Instant) -> Option<Instant> {
        let due = self.access.borrow_mut().as_mut().map_or(Due::Nothing, |s| s.due(now));
        match due {
            Due::Nothing => None,
            Due::At(at) => Some(at),
            Due::Now(nodes) => {
                self.send(Event::Access { page: self.key.page, nodes });
                None
            }
        }
    }

    /// Whether its app caught up since a frame was dropped (the page is to be painted again:
    /// told once).
    pub fn caught_up(&self) -> bool {
        // Most pages dropped nothing: their app's state isn't looked at (it takes a lock).
        if !self.frames.borrow().dropped() {
            return false;
        }
        let behind = self.acks.behind(self.key);
        self.frames.borrow_mut().caught_up(behind)
    }

    /// Whether a frame of the view or the popup is handed over now: always while its app
    /// keeps up; while it's behind, now and then (the others dropped).
    fn hands_over(&self, popup: bool, now: Instant) -> bool {
        let behind = self.acks.behind(self.key);
        self.frames.borrow_mut().hands_over(popup, behind, now)
    }

    /// A frame of the view or the popup not handed over: the next one is told whole.
    fn missed(&self, popup: bool) {
        self.frames.borrow_mut().missed(popup);
    }

    /// The popup gone: the next one shown is told and written whole.
    fn popup_gone(&self) {
        self.missed(true);
        self.forget_pixels(true);
    }

    /// The page may be at `url` (see [`may_go`]).
    fn may_go(&self, url: &str) -> bool {
        may_go(&self.kind.borrow(), url)
    }

    /// The next frame's number.
    fn next_frame(&self) -> u64 {
        self.frames.borrow_mut().next()
    }

    /// A frame told to its app, held until it's copied (at most [`COPY_WAIT`]);
    /// `undelivered`: what its texture handed over is taken back by if its message never
    /// reaches the app.
    fn tell_frame(&self, painting: Painting<'_>, undelivered: Option<Undelivered>) {
        // Closed: nobody copies it, nothing is waited for (what was handed over is taken back
        // as `undelivered` drops).
        if self.closed.get() {
            return;
        }
        let Painting { popup, frame, width, height, texture, dirty } = painting;
        let changed = self.frames.borrow_mut().changed(popup, dirty, (width, height));
        let message =
            FrameMessage { page: self.key.page, frame, width, height, texture, popup, changed };
        let event = Event::Frame(message);
        match undelivered {
            Some(undelivered) => self.outbox.send_tracked(self.key.client, event, undelivered),
            None => self.outbox.send(self.key.client, event),
        }
        if !self.acks.wait(self.key, frame, COPY_WAIT) {
            tracing::debug!(frame, "a frame not copied in time");
        }
    }

    /// `frame` written into the view's or the popup's file (`false`: none, or not written).
    fn write_pixels(
        &self,
        popup: bool,
        frame: Painted<'_>,
        dirty: impl Iterator<Item = (i32, i32)>,
    ) -> bool {
        let mut pixels = self.pixels.borrow_mut();
        pixels
            .as_mut()
            .and_then(|files| files.of(popup))
            .is_some_and(|file| file.write(frame, dirty))
    }

    /// The view's or the popup's file no longer known to hold the frame before the next (one
    /// dropped): the next is written whole.
    fn forget_pixels(&self, popup: bool) {
        if let Some(file) = self.pixels.borrow_mut().as_mut().and_then(|files| files.of(popup)) {
            file.forget();
        }
    }

    /// A link the page wants opened, told to the app only when the user asked (a click):
    /// a page doesn't push links by itself (a redirect, a frame).
    fn link(&self, url: String, user_gesture: bool) {
        if user_gesture {
            self.send(Event::OpenLink { page: self.key.page, url });
        }
    }

    /// `event` told to its app, unless the page closed (nothing more is told then).
    fn send(&self, event: Event) {
        if self.closed.get() {
            return;
        }
        self.outbox.send(self.key.client, event);
    }
}

/// Whether a page may be at `url`: an app's own files, or the HTML given to it (at its
/// address, none while it's empty) — nothing else, not even a `data:` document (it would
/// run without the app's content policy).
pub(crate) fn may_go(kind: &Kind, url: &str) -> bool {
    url == "about:blank"
        || match kind {
            Kind::App => web_pages::allows_navigation(url),
            Kind::Html(at) => !at.is_empty() && url == at,
        }
}

/// A rectangle of CEF's as the protocol tells it.
fn popup_rect(rect: &Rect) -> web_engine_protocol::PopupRect {
    web_engine_protocol::PopupRect { x: rect.x, y: rect.y, width: rect.width, height: rect.height }
}

/// The page's client: its handlers.
pub fn client(state: Rc<PageState>) -> Client {
    let shared = || Shared(Rc::clone(&state));
    ClientBuilder::new(
        shared(),
        render::RenderBuilder::new(shared(), display::AccessBuilder::new(shared())),
        display::DisplayBuilder::new(shared()),
        request::LifeSpanBuilder::new(shared()),
        request::RequestBuilder::new(shared(), request::LoadsBuilder::new(shared())),
        request::LoadBuilder::new(shared()),
        display::MenuBuilder::new(shared()),
    )
}

/// The page's state, as each handler holds it.
#[derive(Debug, Clone)]
pub struct Shared(Rc<PageState>);

wrap_client! {
    pub struct ClientBuilder {
        page: Shared,
        render: RenderHandler,
        display: DisplayHandler,
        life_span: LifeSpanHandler,
        request: RequestHandler,
        load: LoadHandler,
        menu: ContextMenuHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> {
            Some(self.render.clone())
        }
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(self.display.clone())
        }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(self.life_span.clone())
        }
        fn request_handler(&self) -> Option<RequestHandler> {
            Some(self.request.clone())
        }
        fn load_handler(&self) -> Option<LoadHandler> {
            Some(self.load.clone())
        }
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            Some(self.menu.clone())
        }

        /// What its page's renderer sent: an app page's message, on to the app.
        fn on_process_message_received(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _source_process: ProcessId,
            message: Option<&mut ProcessMessage>,
        ) -> ::std::os::raw::c_int {
            let page = &self.page.0;
            if !page.is_app() {
                return 0;
            }
            match crate::ipc::receive(frame, message) {
                Some(text) => {
                    page.send(Event::Message { page: page.key.page, text });
                    1
                }
                None => 0,
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/handlers.rs"]
mod tests;
