//! What the pages tell their views, and what the views send them.

use std::sync::Arc;

use ui_workbench::WebEvent;

use super::super::WebViews;
use super::super::tests::{Calls, app, keeping};
use super::*;

#[test]
fn an_app_pages_messages_reach_its_view_and_the_views_reach_their_pages() {
    let calls = Arc::new(Calls::default());
    let inboxes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut views = WebViews::default();
    let pages = || vec![(app(1, "index.html"), true), (app(2, "index.html?b=1"), true)];
    views.sync_at(pages(), false, keeping(&calls, &inboxes));
    assert_eq!(views.len(), 2);
    assert!(views.take_events().is_empty());
    {
        let inboxes = inboxes.borrow();
        inboxes[1](platform_api::WebEvent::Message("from two".into()));
        inboxes[0](platform_api::WebEvent::Message("from one".into()));
        inboxes[0](platform_api::WebEvent::OpenLink("https://example.com/".into()));
    }
    assert_eq!(
        views.take_events(),
        [
            (2, WebEvent::Message("from two".into())),
            (1, WebEvent::Message("from one".into())),
            (1, WebEvent::OpenLink("https://example.com/".into())),
        ]
    );
    assert!(views.take_events().is_empty(), "taken once");
    // The views' messages: to their pages; one for a page not made, let go.
    views.send(vec![(2, "to two".into()), (9, "nobody".into()), (1, "to one".into())]);
    assert_eq!(*calls.0.lock().unwrap(), ["send to two", "send to one"]);
    // The same app again: nothing made or loaded.
    views.sync_at(pages(), false, keeping(&calls, &inboxes));
    assert_eq!((views.len(), inboxes.borrow().len()), (2, 2));
}

#[test]
fn a_page_let_go_lets_its_messages_go() {
    let calls = Arc::new(Calls::default());
    let inboxes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut views = WebViews::default();
    views.sync_at(
        vec![(app(1, "index.html"), true), (app(2, "index.html"), true)],
        false,
        keeping(&calls, &inboxes),
    );
    inboxes.borrow()[0](platform_api::WebEvent::Message("one".into()));
    inboxes.borrow()[1](platform_api::WebEvent::Message("two".into()));
    // The first one's view closed before its message was taken.
    views.sync_at(vec![(app(2, "index.html"), true)], false, keeping(&calls, &inboxes));
    assert_eq!(views.take_events(), [(2, WebEvent::Message("two".into()))]);
    // Another app in the second's place: another page, the first let go.
    views.sync_at(vec![(app(2, "other.html"), true)], false, keeping(&calls, &inboxes));
    assert_eq!((views.len(), inboxes.borrow().len()), (1, 3), "made anew");
    assert!(
        calls.0.lock().unwrap().iter().all(|c| !c.starts_with("load")),
        "no HTML loaded into an app"
    );
}

#[test]
fn waiting_messages_are_bounded() {
    let calls = Arc::new(Calls::default());
    let inboxes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut views = WebViews::default();
    views.sync_at(vec![(app(1, "index.html"), true)], false, keeping(&calls, &inboxes));
    for i in 0..INBOX_EVENTS + 10 {
        inboxes.borrow()[0](platform_api::WebEvent::Message(i.to_string()));
    }
    assert_eq!(views.take_events().len(), INBOX_EVENTS, "the newest past the bound let go");
    let long = "x".repeat(platform_api::MAX_WEB_MESSAGE + 1);
    inboxes.borrow()[0](platform_api::WebEvent::Message(long));
    inboxes.borrow()[0](platform_api::WebEvent::Message("short".into()));
    assert_eq!(views.take_events(), [(1, WebEvent::Message("short".into()))]);
}

/// A full inbox says so once, and counts the rest until it has room again.
#[test]
fn a_full_inbox_says_so_once() {
    let mut inbox = Inbox::default();
    for i in 0..INBOX_EVENTS + 3 {
        inbox.push(1, WebEvent::Message(i.to_string()));
    }
    assert_eq!((inbox.events.len(), inbox.dropped), (INBOX_EVENTS, 3));
}
