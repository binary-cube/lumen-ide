//! What the engine answers: the pages' lifecycle and the commands for them, called on the
//! host's main thread (CEF's own); what it tells goes out through the [`Outbox`].

use std::sync::mpsc::{Receiver, Sender};

use web_engine_protocol::{Command, Event, Hello, Open, PageId};

use crate::server::ClientId;

/// A page of a client: its id is the client's, unique only with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PageKey {
    pub client: ClientId,
    pub page: PageId,
}

/// What runs if an event never reaches its app's connection — dropped before any of it was
/// written (its app gone or let go, its queue full, too large to send): what the event
/// handed over in its app's process is taken back (a texture's handle, say). An event
/// whose writing began counts as delivered (its app may have read it).
pub struct Undelivered(Option<Box<dyn FnOnce() + Send>>);

impl Undelivered {
    /// `take_back` run unless the event is delivered.
    pub fn new(take_back: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(take_back)))
    }

    /// The event delivered: nothing is taken back.
    pub(crate) fn delivered(mut self) {
        self.0 = None;
    }
}

impl std::fmt::Debug for Undelivered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Undelivered").field("armed", &self.0.is_some()).finish()
    }
}

impl Drop for Undelivered {
    fn drop(&mut self) {
        if let Some(take_back) = self.0.take() {
            take_back();
        }
    }
}

/// An event on its way to a client's writer, with what's taken back if it never gets there.
#[derive(Debug)]
pub(crate) struct Outgoing {
    pub(crate) event: Event,
    pub(crate) undelivered: Option<Undelivered>,
}

/// What the router of the engine's events is told, in order.
#[derive(Debug)]
pub(crate) enum Routed {
    /// An event for a client's writer.
    Event(ClientId, Outgoing),
    /// A client gone: the router lets its queue go (its writer ends with it).
    Forget(ClientId),
}

/// Where the engine's events go: each to its client's writer thread (sent from any thread,
/// never blocking on the client).
#[derive(Debug, Clone)]
pub struct Outbox {
    pub(crate) sender: Sender<Routed>,
}

impl Outbox {
    /// `event` for `client` (gone: dropped).
    pub fn send(&self, client: ClientId, event: Event) {
        let outgoing = Outgoing { event, undelivered: None };
        let _ = self.sender.send(Routed::Event(client, outgoing));
    }

    /// `event` for `client`, `undelivered` run if it never reaches the client's connection.
    pub fn send_tracked(&self, client: ClientId, event: Event, undelivered: Undelivered) {
        let outgoing = Outgoing { event, undelivered: Some(undelivered) };
        // Not sent (the router gone): `undelivered` runs as it drops.
        let _ = self.sender.send(Routed::Event(client, outgoing));
    }

    /// An outbox whose events are kept to be read back (an engine's tests: what it told).
    pub fn kept() -> (Self, Kept) {
        let (sender, routed) = std::sync::mpsc::channel();
        (Self { sender }, Kept { routed })
    }

    /// `client` gone: what was routed to it let go.
    pub(crate) fn forget(&self, client: ClientId) {
        let _ = self.sender.send(Routed::Forget(client));
    }
}

/// What an [`Outbox::kept`] was told.
#[derive(Debug)]
pub struct Kept {
    routed: Receiver<Routed>,
}

impl Kept {
    /// The events told since the last look, in order (each taken as delivered).
    pub fn told(&self) -> Vec<(ClientId, Event)> {
        let mut told = Vec::new();
        while let Ok(routed) = self.routed.try_recv() {
            if let Routed::Event(client, Outgoing { event, undelivered }) = routed {
                if let Some(undelivered) = undelivered {
                    undelivered.delivered();
                }
                told.push((client, event));
            }
        }
        told
    }
}

/// The engine behind the host (CEF in the `web-host` program).
pub trait Engine {
    /// Its version, told to each app (`<Chromium>-<ours>`).
    fn version(&self) -> String;
    /// The GPU it draws on, when it was told one (said in the greeting).
    fn adapter(&self) -> Option<web_engine_protocol::Adapter> {
        None
    }
    /// An app joined (its greeting: where its frames' textures go — its process, its Mach
    /// service).
    fn joined(&mut self, client: ClientId, hello: &Hello) {
        let _ = (client, hello);
    }
    /// An app gone (after its pages were closed).
    fn left(&mut self, client: ClientId) {
        let _ = client;
    }
    /// A page made; its events (frames, cursor, messages) go out through `outbox`.
    fn open(&mut self, page: PageKey, open: Open, outbox: &Outbox);
    /// A command for an open page (never `Hello`, `Open` or `Close`).
    fn command(&mut self, page: PageKey, command: Command);
    /// A page gone (closed by its app, or its app gone).
    fn close(&mut self, page: PageKey);
}
