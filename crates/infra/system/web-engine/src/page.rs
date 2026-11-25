//! A page opened through the connection: driven by its methods, closed when dropped.

use std::sync::Arc;

use web_engine_protocol::{Command, Key, Mouse, Open, PageId, Size, Source, Wheel};

use crate::connection::{Connection, Sink};

/// What a page is opened with ([`WebPage::open`]).
#[derive(Debug, Clone, PartialEq)]
pub struct PageSpec {
    pub source: Source,
    pub size: Size,
    pub hidden: bool,
    pub background: [u8; 4],
    /// Its frames as pixels in this file ([`web_engine_protocol::Open::pixels`]).
    pub pixels: Option<std::path::PathBuf>,
}

/// A web page drawn by the engine.
pub struct WebPage {
    id: PageId,
    connection: Arc<Connection>,
}

impl std::fmt::Debug for WebPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WebPage").field("id", &self.id).finish_non_exhaustive()
    }
}

impl WebPage {
    /// A page opened on `connection`, its events to `sink`.
    pub fn open(connection: Arc<Connection>, spec: PageSpec, sink: Sink) -> Self {
        let id = connection.register(sink);
        let PageSpec { source, size, hidden, background, pixels } = spec;
        connection.send(Command::Open(Open { page: id, source, size, hidden, background, pixels }));
        Self { id, connection }
    }

    pub fn id(&self) -> PageId {
        self.id
    }

    /// Its engine still runs (gone: the page shows it, and is opened again).
    pub fn is_alive(&self) -> bool {
        self.connection.is_alive()
    }

    pub fn resize(&self, size: Size) {
        self.connection.send(Command::Resize { page: self.id, size });
    }

    /// Hidden (its tab behind another, its window minimized) or shown again: nothing
    /// started or stopped, the engine only stops drawing it.
    pub fn set_hidden(&self, hidden: bool) {
        self.connection.send(Command::Hidden { page: self.id, hidden });
    }

    pub fn load_html(&self, html: String) {
        self.connection.send(Command::LoadHtml { page: self.id, html });
    }

    pub fn post(&self, message: String) {
        self.connection.send(Command::Post { page: self.id, message });
    }

    /// A frame asked for (only for a page on screen; one comes if anything changed).
    pub fn begin_frame(&self) {
        self.connection.send(Command::BeginFrame { page: self.id });
    }

    pub fn mouse(&self, mouse: Mouse) {
        self.connection.send(Command::Mouse { page: self.id, mouse });
    }

    pub fn wheel(&self, wheel: Wheel) {
        self.connection.send(Command::Wheel { page: self.id, wheel });
    }

    pub fn key(&self, key: Key) {
        self.connection.send(Command::Key { page: self.id, key });
    }

    /// Its whole view drawn again (shown again after its picture was let go).
    pub fn repaint(&self) {
        self.connection.send(Command::Repaint { page: self.id });
    }

    /// Its accessibility tree wanted (a screen reader runs), or not.
    pub fn access(&self, on: bool) {
        self.connection.send(Command::Access { page: self.id, on });
    }

    /// Files dragged over it.
    pub fn drag(&self, drag: web_engine_protocol::Drag) {
        self.connection.send(Command::Drag { page: self.id, drag });
    }

    /// Text from the input method (being composed, typed, given up).
    pub fn ime(&self, ime: web_engine_protocol::Ime) {
        self.connection.send(Command::Ime { page: self.id, ime });
    }

    /// An editing action on what the page has selected.
    pub fn edit(&self, action: web_engine_protocol::EditAction) {
        self.connection.send(Command::Edit { page: self.id, action });
    }

    pub fn focus(&self, focused: bool) {
        self.connection.send(Command::Focus { page: self.id, focused });
    }
}

impl Drop for WebPage {
    fn drop(&mut self) {
        self.connection.unregister(self.id);
        self.connection.send(Command::Close { page: self.id });
    }
}
