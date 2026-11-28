//! What the pages tell their views ([`Inbox`]), bounded while they wait, and what the views
//! send their pages.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;

use ui_workbench::WebEvent;

use super::WebViews;

/// What the app pages told, not yet handed to their views (filled by the engine's handlers
/// on the window's thread, emptied before the next frame).
#[derive(Debug, Default)]
pub(super) struct Inbox {
    events: VecDeque<(u128, WebEvent)>,
    /// The bytes they hold.
    bytes: usize,
    /// Messages dropped since the inbox last had room (logged once, as it fills, and
    /// counted when it empties).
    dropped: usize,
}

/// The most events kept waiting for their views: past it, the newest is let go (logged) — a
/// page can't fill the app's memory while its window is hidden.
const INBOX_EVENTS: usize = 1024;
/// The most bytes those events hold (256 MiB): past it, the newest is let go too.
const INBOX_BYTES: usize = 256 << 20;

impl Inbox {
    pub(super) fn push(&mut self, page: u128, event: WebEvent) {
        let size = event_size(&event);
        if size > platform_api::MAX_WEB_MESSAGE
            || self.events.len() >= INBOX_EVENTS
            || self.bytes.saturating_add(size) > INBOX_BYTES
        {
            if self.dropped == 0 {
                tracing::warn!(size, "a page's messages dropped: too many waiting, or too long");
            }
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.bytes += size;
        self.events.push_back((page, event));
    }

    /// Nothing waits.
    pub(super) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// What `keep` doesn't want is let go (its page is gone).
    pub(super) fn retain(&mut self, keep: impl Fn(u128) -> bool) {
        self.events.retain(|(id, _)| keep(*id));
        self.bytes = self.events.iter().map(|(_, e)| event_size(e)).sum();
    }
}

/// Where page `id`'s engine tells its events: into `inbox`, as its view reads them.
pub(super) fn to_view(inbox: &Rc<RefCell<Inbox>>, id: u128) -> platform_api::WebInbox {
    let inbox = Rc::clone(inbox);
    Box::new(move |event| {
        let event = match event {
            platform_api::WebEvent::Message(text) => WebEvent::Message(text),
            platform_api::WebEvent::OpenLink(url) => WebEvent::OpenLink(url),
            platform_api::WebEvent::Shown => WebEvent::Shown,
        };
        inbox.borrow_mut().push(id, event);
    })
}

/// The bytes an event holds (what the inbox counts).
fn event_size(event: &WebEvent) -> usize {
    match event {
        WebEvent::Message(text) | WebEvent::OpenLink(text) => text.len(),
        WebEvent::Shown | WebEvent::Failed | WebEvent::Picture(_) => 0,
    }
}

impl WebViews {
    /// What the app pages told since the last time, for their views (page id, event), in
    /// the order told.
    pub(in crate::app) fn take_events(&mut self) -> Vec<(u128, WebEvent)> {
        let mut inbox = self.inbox.borrow_mut();
        // Room again: the next drop is said anew.
        if inbox.dropped > 0 {
            tracing::debug!(dropped = inbox.dropped, "pages' messages dropped while full");
            inbox.dropped = 0;
        }
        if inbox.events.is_empty() {
            return Vec::new();
        }
        inbox.bytes = 0;
        inbox.events.drain(..).collect()
    }

    /// The views' messages handed to their pages; one for a page not made (not seen yet,
    /// let go) is let go.
    pub(in crate::app) fn send(&self, sends: Vec<(u128, String)>) {
        for (id, message) in sends {
            match self.live(id) {
                Some(live) => live.view.send(&message),
                None => tracing::debug!("a message for a page not shown let go"),
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_views/inbox.rs"]
mod tests;
