//! A page drawn by the engine ([`EnginePage`]): opened on the engine once it's connected
//! (what's sent meanwhile kept, bounded), its input turned into the engine's, what its sink
//! queued read at each poll.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Weak};

use platform_api::{PageInput, PageMouseAction, WebInbox, WebSource};
use render_api::{FrameImporter, ImageData};
use web_engine::protocol::{
    Button, Drag, EditAction, Ime, Mouse, MouseAction, PopupRect, Size, Source, Wheel,
};
use web_engine::{Connection, PageSpec, Sink, WebPage};

use super::access::access_node;
use super::ime::{Composition, ImeStep, TextStep, utf16_at};
use super::sink::{self, Told};
use super::{
    PIXELS, Wake, adapter_of, connection, failed_lately, frames_as_pixels, host_gone, page_gone,
    pixels_wanted,
};
use crate::app::web_keys;

/// The most messages kept for a page not open yet: past it, the oldest is let go (a view
/// sending while the engine never connects can't fill the app's memory).
const WAITING_MESSAGES: usize = 256;
/// The most bytes those messages hold (64 MiB): past it, the oldest are let go too.
const WAITING_BYTES: usize = 64 << 20;

/// The messages sent to a page before it was open, the oldest let go past
/// [`WAITING_MESSAGES`] or [`WAITING_BYTES`].
#[derive(Debug, Default)]
struct Waiting {
    messages: VecDeque<String>,
    /// The bytes they hold.
    bytes: usize,
}

impl Waiting {
    fn push(&mut self, message: String) {
        self.bytes = self.bytes.saturating_add(message.len());
        self.messages.push_back(message);
        while self.messages.len() > WAITING_MESSAGES || self.bytes > WAITING_BYTES {
            let Some(oldest) = self.messages.pop_front() else { break };
            self.bytes = self.bytes.saturating_sub(oldest.len());
            tracing::debug!("a message for a page not open yet let go: too many waiting");
        }
    }

    /// The messages, in the order sent.
    fn take(self) -> VecDeque<String> {
        self.messages
    }
}

/// A page drawn by the engine: a [`platform_api::WebView`] whose picture is
/// [`Self::image`], drawn by the app in its place.
pub(in crate::app) struct EnginePage {
    source: RefCell<Source>,
    image: ImageData,
    wake: Wake,
    importer: Arc<dyn FrameImporter>,
    /// The window's scale (the page's device pixels per CSS pixel).
    scale: Cell<f32>,
    area: Cell<kernel::Rect>,
    visible: Cell<bool>,
    page: RefCell<Option<WebPage>>,
    /// The host it's open on (its end counted once for all its pages: [`host_gone`]).
    host: RefCell<Weak<Connection>>,
    /// The messages sent before the page was open: posted once it is ([`Waiting`]).
    waiting: RefCell<Waiting>,
    told: (Sender<Told>, Receiver<Told>),
    inbox: WebInbox,
    gone: Cell<bool>,
    /// The pointer it asked for last.
    cursor: Cell<platform_api::CursorIcon>,
    /// Its frames' file when they come as pixels (made and removed by the engine, in the
    /// channel's folder); `None`: as GPU textures.
    pixels: Option<PathBuf>,
    /// Its popup's image (an open `<select>`'s list) and where it is (CSS pixels).
    popup_image: ImageData,
    popup: Cell<Option<PopupRect>>,
    /// The menu it asked for, not taken yet.
    menu: Cell<Option<web_engine::protocol::PageMenu>>,
    /// Where its text being composed is.
    caret: Cell<Option<PopupRect>>,
    /// What the input method composes in it ([`Composition`]).
    composition: Cell<Composition>,
    /// Something the app reads of it changed since it last looked (its menu asked, its
    /// caret, it went): [`platform_api::WebView::take_changed`]. Its pointer isn't: the app
    /// reads it at each wake, and a pointer needs no frame.
    changed: Cell<bool>,
    /// Its engine was asked to draw it anew, its picture let go ([`Self::repaint_if_lost`]).
    repainting: Cell<bool>,
    /// Its accessibility tree is wanted (a screen reader runs).
    access_on: Cell<bool>,
    /// …and as last told.
    access: RefCell<platform_api::PageAccess>,
}

impl std::fmt::Debug for EnginePage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnginePage").field("page", &self.page).finish_non_exhaustive()
    }
}

/// What a page is made with ([`EnginePage::new`]).
pub(in crate::app) struct Made<'a> {
    pub source: &'a WebSource<'a>,
    pub area: kernel::Rect,
    pub scale: f32,
    pub inbox: WebInbox,
    pub wake: Wake,
    pub importer: Arc<dyn FrameImporter>,
}

impl EnginePage {
    pub(in crate::app) fn new(made: Made<'_>) -> Self {
        let source = match made.source {
            WebSource::Html(html) => Source::Html((*html).to_owned()),
            WebSource::App { root, entry } => {
                Source::App { root: root.to_path_buf(), entry: (*entry).to_owned() }
            }
        };
        let pixels = pixels_wanted(made.importer.as_ref()).then(|| {
            web_engine::channel().dir.join(web_engine::protocol::pixels_name(uuid::Uuid::new_v4()))
        });
        let page = Self {
            popup_image: ImageData::live(),
            popup: Cell::new(None),
            menu: Cell::new(None),
            caret: Cell::new(None),
            composition: Cell::new(Composition::default()),
            changed: Cell::new(false),
            repainting: Cell::new(false),
            access_on: Cell::new(false),
            access: RefCell::default(),
            pixels,
            source: RefCell::new(source),
            image: ImageData::live(),
            wake: made.wake,
            importer: made.importer,
            scale: Cell::new(made.scale),
            area: Cell::new(made.area),
            visible: Cell::new(true),
            page: RefCell::new(None),
            host: RefCell::new(Weak::new()),
            waiting: RefCell::default(),
            told: std::sync::mpsc::channel(),
            inbox: made.inbox,
            gone: Cell::new(false),
            cursor: Cell::new(platform_api::CursorIcon::Default),
        };
        page.open();
        page
    }

    /// The image the app draws in its place (no frame yet: nothing drawn, its view's own
    /// paint shows).
    pub(in crate::app) fn image(&self) -> &ImageData {
        &self.image
    }

    /// Its popup's image, drawn over it while one is open.
    pub(in crate::app) fn popup_image(&self) -> &ImageData {
        &self.popup_image
    }

    /// Opened on the engine if it's connected (else asked for: [`Self::poll`] opens it once
    /// it is).
    fn open(&self) {
        if self.page.borrow().is_some() {
            return;
        }
        // Its frames turned to pixels since it was made: made again as such (by poll), not
        // connected for nothing.
        if self.pixels.is_none() && PIXELS.load(Ordering::Acquire) {
            return;
        }
        // Pixels go to any GPU: the engine's own is no matter then.
        let adapter = self.pixels.is_none().then(|| adapter_of(self.importer.as_ref())).flatten();
        let Some(connection) = connection(&self.wake, adapter) else { return };
        let source = self.source.borrow().clone();
        // A response's HTML on white, as the system's engine shows one without a background
        // of its own; an app paints its own (transparent until it does).
        let background = match source {
            Source::Html(_) => OPAQUE_WHITE,
            Source::App { .. } => TRANSPARENT,
        };
        let pixels = self.pixels.clone();
        let size = self.size();
        let spec = PageSpec { source, size, hidden: !self.visible.get(), background, pixels };
        *self.host.borrow_mut() = Arc::downgrade(&connection);
        let page = WebPage::open(connection, spec, self.sink());
        for message in self.waiting.take().take() {
            page.post(message);
        }
        if self.access_on.get() {
            page.access(true);
        }
        *self.page.borrow_mut() = Some(page);
    }

    /// Shown with its picture let go (unshown a while): its engine draws it anew, once (it
    /// sends frames only when something changes).
    fn repaint_if_lost(&self) {
        if !self.visible.get() || !self.image.frame_lost() {
            self.repainting.set(false);
            return;
        }
        if self.repainting.replace(true) {
            return;
        }
        if let Some(page) = self.page.borrow().as_ref() {
            page.repaint();
        }
    }

    /// Its engine went (its renderer crashed, its frames never open): told to its owner
    /// (it's made again), counted.
    fn go(&self) {
        self.changed.set(true);
        if !self.gone.replace(true) {
            page_gone();
        }
    }

    /// Its host ended: told to its owner (it's made again), counted once for every page the
    /// host had.
    fn host_ended(&self) {
        self.changed.set(true);
        if !self.gone.replace(true) {
            host_gone(&self.host.borrow());
        }
    }

    /// Let go to be made again, not counted as its engine failing (it never connected, its
    /// frames turned to pixels).
    fn remade(&self) {
        self.changed.set(true);
        self.gone.set(true);
    }

    /// `p` (physical pixels in the page) in the page's own CSS pixels.
    fn css(&self, p: kernel::Point) -> kernel::Point {
        p.scaled(1.0 / self.scale.get())
    }

    /// `rect` (CSS pixels in the page) in physical pixels in it.
    fn physical(&self, rect: PopupRect) -> kernel::Rect {
        let [x, y, width, height] = [rect.x, rect.y, rect.width, rect.height].map(|v| v as f32);
        kernel::Rect::new(x, y, width, height).scaled(self.scale.get())
    }

    /// A composition given up just before (the input method's empty one) not followed by
    /// its text: told to the page now, before what comes instead.
    fn settle_composition(&self, page: &WebPage) {
        let (settled, cancel) = self.composition.get().settle();
        self.composition.set(settled);
        if cancel {
            page.ime(Ime::Cancel);
        }
    }

    /// The page's size in its pixels.
    fn size(&self) -> Size {
        let area = self.area.get();
        let side = |v: f32| v.round().max(1.0) as u32;
        let scale = self.scale.get();
        Size { width: side(area.size.width), height: side(area.size.height), scale }
    }

    /// Where its events go, on the reader thread ([`sink::sink`]).
    fn sink(&self) -> Sink {
        sink::sink(sink::SinkOf {
            image: self.image.clone(),
            popup_image: self.popup_image.clone(),
            importer: Arc::clone(&self.importer),
            told: self.told.0.clone(),
            wake: Arc::clone(&self.wake),
            pixels: self.pixels.clone(),
        })
    }
}

/// A page's background where it paints none: white, opaque (RGBA).
const OPAQUE_WHITE: [u8; 4] = [255, 255, 255, 255];

/// An app's page's background until it paints its own: nothing (RGBA).
const TRANSPARENT: [u8; 4] = [0, 0, 0, 0];

impl platform_api::WebView for EnginePage {
    fn set_bounds(&self, bounds: kernel::Rect) {
        let before = self.size();
        self.area.set(bounds);
        let size = self.size();
        if size != before
            && let Some(page) = &*self.page.borrow()
        {
            page.resize(size);
        }
    }

    fn set_visible(&self, visible: bool) {
        if self.visible.replace(visible) != visible
            && let Some(page) = &*self.page.borrow()
        {
            page.set_hidden(!visible);
        }
        self.repaint_if_lost();
    }

    fn load_html(&self, html: &str) {
        match &*self.page.borrow() {
            Some(page) => page.load_html(html.to_owned()),
            // Not open yet: it opens with this one.
            None => *self.source.borrow_mut() = Source::Html(html.to_owned()),
        }
    }

    fn send(&self, message: &str) {
        match &*self.page.borrow() {
            Some(page) => page.post(message.to_owned()),
            None => self.waiting.borrow_mut().push(message.to_owned()),
        }
    }

    fn poll(&self) {
        if self.gone.get() {
            return;
        }
        self.open();
        self.repaint_if_lost();
        let mut latest = None;
        while let Ok(told) = self.told.1.try_recv() {
            match told {
                Told::Event(event) => (self.inbox)(event),
                Told::Gone => self.go(),
                // Read by the app at each wake (applied to the window at once): no frame.
                Told::Cursor(cursor) => self.cursor.set(cursor),
                Told::Popup(rect) => self.popup.set(rect),
                Told::Menu(menu) => {
                    self.changed.set(true);
                    self.menu.set(Some(menu));
                }
                Told::Caret(rect) => {
                    self.changed.set(true);
                    self.caret.set(Some(rect));
                }
                // Only the latest tree matters: the ones before it aren't read.
                Told::Access(nodes) if self.access_on.get() => latest = Some(nodes),
                Told::Access(_) => {}
                // Its frames as pixels already, and they still fail: gone (counted, so a page
                // failing so is given up, never remade without end).
                Told::AsPixels if self.pixels.is_some() => self.go(),
                // Made again, its frames as pixels (not counted as the engine failing).
                Told::AsPixels => {
                    frames_as_pixels("the web engine's textures can't be opened here");
                    self.remade();
                }
            }
        }
        if let Some(nodes) = latest {
            let mut access = self.access.borrow_mut();
            access.generation += 1;
            access.nodes = nodes.into_iter().map(access_node).collect();
        }
        // Its engine went before it could say so (it ended as the page opened), or never
        // came (it didn't start): made again, by the system's engine if ours failed.
        // A page never connected isn't counted as one gone (the engine's failure is
        // already; counting it would put off the next try).
        let page = self.page.borrow();
        let ended = page.as_ref().is_some_and(|p| !p.is_alive());
        let never = page.is_none() && failed_lately();
        // Not open yet while frames turned to pixels: made again as such.
        let as_pixels = page.is_none() && self.pixels.is_none() && PIXELS.load(Ordering::Acquire);
        drop(page);
        if as_pixels || never {
            self.remade();
        } else if ended {
            self.host_ended();
        }
    }

    fn take_changed(&self) -> bool {
        self.changed.replace(false)
    }

    fn set_scale(&self, scale: f32) {
        if self.scale.replace(scale) != scale
            && let Some(page) = &*self.page.borrow()
        {
            page.resize(self.size());
        }
    }

    fn is_gone(&self) -> bool {
        self.gone.get()
    }

    fn takes_input(&self) -> bool {
        true
    }

    fn input(&self, input: PageInput) {
        let page = self.page.borrow();
        let Some(page) = page.as_ref() else { return };
        // The input method's own steps keep a composition given up waiting for its text.
        if !matches!(input, PageInput::Text(_) | PageInput::Compose(_)) {
            self.settle_composition(page);
        }
        // The focus moving, a press: what was composed ends there (its text then typed anew).
        if Composition::ends_at(&input) {
            self.composition.set(Composition::default());
        }
        match input {
            PageInput::Mouse(mouse) => {
                let at = self.css(mouse.at);
                let button = |b: platform_api::MouseButton| match b {
                    platform_api::MouseButton::Left => Button::Left,
                    platform_api::MouseButton::Middle => Button::Middle,
                    platform_api::MouseButton::Right => Button::Right,
                };
                let action = match mouse.action {
                    PageMouseAction::Move => MouseAction::Move,
                    PageMouseAction::Leave => MouseAction::Leave,
                    PageMouseAction::Down(b) => MouseAction::Down(button(b)),
                    PageMouseAction::Up(b) => MouseAction::Up(button(b)),
                };
                let modifiers = web_keys::modifiers(mouse.modifiers);
                page.mouse(Mouse { action, x: at.x, y: at.y, clicks: mouse.clicks, modifiers });
            }
            PageInput::Wheel(wheel) => {
                let at = self.css(wheel.at);
                let delta = self.css(kernel::Point::new(wheel.delta.0, wheel.delta.1));
                let modifiers = web_keys::modifiers(wheel.modifiers);
                page.wheel(Wheel { x: at.x, y: at.y, dx: delta.x, dy: delta.y, modifiers });
            }
            PageInput::Key(event) => {
                for key in web_keys::key_events(&event) {
                    page.key(key);
                }
            }
            // The input method's text: in place of what it composed, else typed at the caret
            // (Chromium takes a commit only while something is composed).
            PageInput::Text(text) => {
                let (composition, step) = self.composition.get().text();
                self.composition.set(composition);
                match step {
                    TextStep::Commit => page.ime(Ime::Commit { text }),
                    TextStep::Type => {
                        let modifiers = web_keys::modifiers(Default::default());
                        for key in web_keys::text_events(&text, modifiers) {
                            page.key(key);
                        }
                    }
                }
            }
            PageInput::Compose(compose) => {
                let (composition, step) = self.composition.get().compose(compose.text.is_empty());
                self.composition.set(composition);
                if step == ImeStep::Compose {
                    let text = compose.text;
                    let selection = compose
                        .cursor
                        .map(|(from, to)| (utf16_at(&text, from), utf16_at(&text, to)));
                    page.ime(Ime::Compose { text, selection });
                }
            }
            PageInput::Focus(focused) => {
                // Its caret goes with the keyboard (told again as it comes back).
                if !focused {
                    self.caret.set(None);
                }
                page.focus(focused);
            }
            PageInput::Drag(drag) => {
                let at = |p: kernel::Point| {
                    let css = self.css(p);
                    (css.x as i32, css.y as i32)
                };
                page.drag(match drag {
                    platform_api::PageDrag::Enter { files, at: p } => {
                        let (x, y) = at(p);
                        Drag::Enter { files, x, y }
                    }
                    platform_api::PageDrag::Over { at: p } => {
                        let (x, y) = at(p);
                        Drag::Over { x, y }
                    }
                    platform_api::PageDrag::Leave => Drag::Leave,
                    platform_api::PageDrag::Drop { at: p } => {
                        let (x, y) = at(p);
                        Drag::Drop { x, y }
                    }
                });
            }
            PageInput::Edit(edit) => page.edit(match edit {
                platform_api::PageEdit::Undo => EditAction::Undo,
                platform_api::PageEdit::Redo => EditAction::Redo,
                platform_api::PageEdit::Cut => EditAction::Cut,
                platform_api::PageEdit::Copy => EditAction::Copy,
                platform_api::PageEdit::Paste => EditAction::Paste,
                platform_api::PageEdit::SelectAll => EditAction::SelectAll,
            }),
        }
    }

    fn cursor(&self) -> Option<platform_api::CursorIcon> {
        Some(self.cursor.get())
    }

    fn set_access(&self, on: bool) {
        if self.access_on.replace(on) == on {
            return;
        }
        // Its generation only grows (a tree after a screen reader came back is new).
        if !on {
            let mut access = self.access.borrow_mut();
            access.generation += 1;
            access.nodes = Arc::from([]);
        }
        if let Some(page) = self.page.borrow().as_ref() {
            page.access(on);
        }
    }

    fn access(&self) -> Option<platform_api::PageAccess> {
        self.access_on.get().then(|| self.access.borrow().clone())
    }

    fn takes_files(&self) -> bool {
        matches!(*self.source.borrow(), Source::App { .. })
    }

    fn caret(&self) -> Option<kernel::Rect> {
        Some(self.physical(self.caret.get()?))
    }

    fn take_menu(&self) -> Option<platform_api::PageMenuAsk> {
        let menu = self.menu.take()?;
        let at = self.physical(PopupRect { x: menu.x, y: menu.y, width: 0, height: 0 }).origin;
        Some(platform_api::PageMenuAsk { at, selection: menu.selection, editable: menu.editable })
    }

    fn popup(&self) -> Option<kernel::Rect> {
        Some(self.physical(self.popup.get()?))
    }

    fn release_focus(&self) {
        // Its caret goes with the keyboard (as any focus lost).
        self.input(PageInput::Focus(false));
    }
}

impl Drop for EnginePage {
    fn drop(&mut self) {
        // Its textures let go at the renderer's next frame (the page closes as it drops).
        self.image.release();
        self.popup_image.release();
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_engine/page.rs"]
mod tests;
