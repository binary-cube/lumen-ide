//! The pages over the views: made when first seen, placed, hidden while covered or not seen,
//! let go with their views; an app page's messages taken for its view, the views' handed to
//! their pages.

use super::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct Calls(Mutex<Vec<String>>);

#[derive(Debug)]
struct Fake(Arc<Calls>);

impl WebView for Fake {
    fn set_bounds(&self, bounds: kernel::Rect) {
        self.0.0.lock().unwrap().push(format!("bounds {}", bounds.min_x()));
    }
    fn set_visible(&self, visible: bool) {
        self.0.0.lock().unwrap().push(format!("visible {visible}"));
    }
    fn load_html(&self, html: &str) {
        self.0.0.lock().unwrap().push(format!("load {html}"));
    }
    fn send(&self, message: &str) {
        self.0.0.lock().unwrap().push(format!("send {message}"));
    }
}

fn page(id: u128, html: &Arc<str>, x: f32) -> WebPage {
    WebPage {
        id,
        source: WebSource::Html(Arc::clone(html)),
        area: kernel::Rect::new(x, 0.0, 100.0, 100.0),
    }
}

fn app(id: u128, entry: &str) -> WebPage {
    WebPage {
        id,
        source: WebSource::App {
            root: Arc::from(std::path::Path::new("/apps/one")),
            entry: entry.into(),
        },
        area: kernel::Rect::new(0.0, 0.0, 100.0, 100.0),
    }
}

type Make =
    Box<dyn FnMut(&WebSource, kernel::Rect, platform_api::WebInbox) -> Option<Box<dyn WebView>>>;

impl WebViews {
    /// [`WebViews::sync`] at a fixed moment.
    fn sync_at(
        &mut self,
        pages: Vec<(WebPage, bool)>,
        covered: bool,
        make: impl FnMut(&WebSource, kernel::Rect, platform_api::WebInbox) -> Option<Box<dyn WebView>>,
    ) {
        self.sync(pages, covered, start(), make);
    }
}

fn start() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

#[test]
fn a_page_is_made_seen_moved_hidden_and_let_go() {
    let calls = Arc::new(Calls::default());
    let made = Arc::new(Mutex::new(0));
    let mut views = WebViews::default();
    let make = |calls: &Arc<Calls>, made: &Arc<Mutex<i32>>| {
        let (calls, made) = (Arc::clone(calls), Arc::clone(made));
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            *made.lock().unwrap() += 1;
            Some(Box::new(Fake(Arc::clone(&calls))) as Box<dyn WebView>)
        }
    };
    let html: Arc<str> = "<p>a</p>".into();
    // Not seen yet: nothing made.
    views.sync_at(vec![(page(1, &html, 0.0), false)], false, make(&calls, &made));
    assert_eq!(views.len(), 0);
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make(&calls, &made));
    assert_eq!((views.len(), *made.lock().unwrap()), (1, 1), "made once seen");
    // The same page again: nothing done.
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make(&calls, &made));
    assert!(calls.0.lock().unwrap().is_empty(), "{:?}", calls.0.lock().unwrap());
    // Moved, then a menu over it, then another page in its place.
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, make(&calls, &made));
    views.sync_at(vec![(page(1, &html, 10.0), true)], true, make(&calls, &made));
    let other: Arc<str> = "<p>b</p>".into();
    views.sync_at(vec![(page(1, &other, 10.0), true)], false, make(&calls, &made));
    assert_eq!(
        *calls.0.lock().unwrap(),
        ["bounds 10", "visible false", "load <p>b</p>", "visible true"]
    );
    assert_eq!(*made.lock().unwrap(), 1, "never made twice");
    // Its view closed: let go.
    views.sync_at(Vec::new(), false, make(&calls, &made));
    assert_eq!(views.len(), 0);
}

#[test]
fn a_page_that_can_t_be_shown_isn_t_tried_again_and_a_hidden_one_lets_its_engine_go() {
    let html: Arc<str> = "<p>a</p>".into();
    let tries = Arc::new(Mutex::new(0));
    let mut views = WebViews::default();
    for _ in 0..3 {
        let tries = Arc::clone(&tries);
        views.sync_at(vec![(page(1, &html, 0.0), true)], false, move |_, _, _| {
            *tries.lock().unwrap() += 1;
            None
        });
    }
    assert_eq!(*tries.lock().unwrap(), 1, "once, not on each frame");
    // A page shown, then hidden past the time kept: its engine let go.
    let calls = Arc::new(Calls::default());
    let fake = |calls: &Arc<Calls>| {
        let calls = Arc::clone(calls);
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            Some(Box::new(Fake(Arc::clone(&calls))) as Box<dyn WebView>)
        }
    };
    let mut views = WebViews::default();
    let t0 = start();
    views.sync(vec![(page(2, &html, 0.0), true)], false, t0, fake(&calls));
    views.sync(vec![(page(2, &html, 0.0), false)], false, t0, fake(&calls));
    assert_eq!(views.len(), 1, "hidden: kept a while");
    views.sync(vec![(page(2, &html, 0.0), false)], false, t0 + HIDDEN_KEPT, fake(&calls));
    assert_eq!(views.len(), 0, "hidden long enough: let go");
}

/// A maker of fake pages keeping each page's inbox (by the order made), so a test can speak
/// for the page.
fn keeping(
    calls: &Arc<Calls>,
    inboxes: &std::rc::Rc<std::cell::RefCell<Vec<platform_api::WebInbox>>>,
) -> Make {
    let (calls, inboxes) = (Arc::clone(calls), std::rc::Rc::clone(inboxes));
    Box::new(move |_, _, inbox| {
        inboxes.borrow_mut().push(inbox);
        Some(Box::new(Fake(Arc::clone(&calls))) as Box<dyn WebView>)
    })
}

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

/// A hidden page says when it's let go (a frame then: an idle window wouldn't draw one).
#[test]
fn a_hidden_page_says_when_it_goes() {
    let html: Arc<str> = Arc::from("<p>x</p>");
    let calls = Arc::new(Calls::default());
    let fake = |calls: &Arc<Calls>| {
        let calls = Arc::clone(calls);
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            Some(Box::new(Fake(Arc::clone(&calls))) as Box<dyn WebView>)
        }
    };
    let mut views = WebViews::default();
    let t0 = start();
    views.sync(vec![(page(3, &html, 0.0), true)], false, t0, fake(&calls));
    assert_eq!(views.deadline(), None, "shown: kept");
    views.sync(vec![(page(3, &html, 0.0), false)], false, t0, fake(&calls));
    assert_eq!(views.deadline(), Some(t0 + HIDDEN_KEPT));
}

/// A page that takes pictures: each `done` kept for the test to answer.
type Pending = Rc<RefCell<Vec<Box<dyn FnOnce(Option<platform_api::WebPicture>)>>>>;

#[derive(Debug)]
struct Pictured(Arc<Calls>, #[allow(dead_code)] PendingDebug);

/// [`Pending`] in a [`Pictured`] (debug-printable).
struct PendingDebug(Pending);

impl std::fmt::Debug for PendingDebug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} asked", self.0.borrow().len())
    }
}

impl WebView for Pictured {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, visible: bool) {
        self.0.0.lock().unwrap().push(format!("visible {visible}"));
    }
    fn load_html(&self, _: &str) {}
    fn snapshot(&self, done: Box<dyn FnOnce(Option<platform_api::WebPicture>)>) -> bool {
        self.0.0.lock().unwrap().push("snapshot".into());
        self.1.0.borrow_mut().push(done);
        true
    }
}

/// Something opened over a page: its picture asked for, the page kept until it comes, then
/// hidden with the picture in its place; shown again, the picture goes. No picture in time:
/// hidden without one.
#[test]
fn a_page_something_opens_over_leaves_its_picture() {
    let calls = Arc::new(Calls::default());
    let pending: Pending = Rc::default();
    let make = |calls: &Arc<Calls>, pending: &Pending| {
        let (calls, pending) = (Arc::clone(calls), Rc::clone(pending));
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            Some(Box::new(Pictured(Arc::clone(&calls), PendingDebug(Rc::clone(&pending))))
                as Box<dyn WebView>)
        }
    };
    let html: Arc<str> = "<p>a</p>".into();
    let t0 = start();
    let mut views = WebViews::default();
    let shown = || vec![(page(1, &html, 0.0), Sight::Shown)];
    let under = || vec![(page(1, &html, 0.0), Sight::Under)];
    assert!(!views.sync(shown(), false, t0, make(&calls, &pending)));
    // A modal over it: the picture asked for, the page still on screen.
    assert!(!views.sync(shown(), true, t0, make(&calls, &pending)));
    assert_eq!(*calls.0.lock().unwrap(), ["snapshot"]);
    assert_eq!(views.deadline(), Some(t0 + PICTURE_WAIT), "a frame when the wait ends");
    // Not come yet: still waiting, nothing asked twice.
    assert!(!views.sync(under(), false, t0, make(&calls, &pending)));
    assert_eq!(calls.0.lock().unwrap().len(), 1);
    // Come: kept, drawn on the next frame, the page still there for it.
    let done = pending.borrow_mut().pop().unwrap();
    done(Some(platform_api::WebPicture { width: 2, height: 1, rgba: vec![9; 8] }));
    assert!(views.sync(under(), false, t0, make(&calls, &pending)), "a picture to draw");
    assert_eq!(views.pictures().len(), 1);
    assert_eq!(calls.0.lock().unwrap().len(), 1, "not hidden yet");
    // The next frame (the picture under it): hidden.
    assert!(!views.sync(under(), false, t0, make(&calls, &pending)));
    assert_eq!(*calls.0.lock().unwrap(), ["snapshot", "visible false"]);
    assert_eq!(views.pictures().len(), 1, "kept while it's under");
    // What was over it closed: the page back, the picture gone.
    assert!(views.sync(shown(), false, t0, make(&calls, &pending)));
    assert!(views.pictures().is_empty());
    assert_eq!(calls.0.lock().unwrap().last().unwrap(), "visible true");
    // Again, with no picture in time: hidden when the wait ends, nothing drawn.
    calls.0.lock().unwrap().clear();
    views.sync(under(), false, t0, make(&calls, &pending));
    views.sync(under(), false, t0 + PICTURE_WAIT, make(&calls, &pending));
    assert_eq!(*calls.0.lock().unwrap(), ["snapshot", "visible false"]);
    assert!(views.pictures().is_empty());
    // One coming late is let go.
    let late = pending.borrow_mut().pop().unwrap();
    late(Some(platform_api::WebPicture { width: 1, height: 1, rgba: vec![0; 4] }));
    assert!(!views.sync(under(), false, t0 + PICTURE_WAIT, make(&calls, &pending)));
    assert!(views.pictures().is_empty());
    // Its tab left: no picture either.
    views.sync(shown(), false, t0, make(&calls, &pending));
    views.sync(vec![(page(1, &html, 0.0), Sight::Away)], false, t0, make(&calls, &pending));
    assert!(views.pictures().is_empty());
}
