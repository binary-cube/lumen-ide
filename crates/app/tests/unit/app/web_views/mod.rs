//! The pages over the views: made when first seen, placed, hidden while covered or not seen,
//! let go with their views; an app page's messages taken for its view, the views' handed to
//! their pages.

use super::*;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
pub(super) struct Calls(pub(super) Mutex<Vec<String>>);

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

pub(super) fn page(id: u128, html: &Arc<str>, x: f32) -> WebPage {
    WebPage {
        id,
        source: WebSource::Html(Arc::clone(html)),
        area: kernel::Rect::new(x, 0.0, 100.0, 100.0),
        freeze: false,
    }
}

pub(super) fn app(id: u128, entry: &str) -> WebPage {
    WebPage {
        id,
        source: WebSource::App {
            root: Arc::from(std::path::Path::new("/apps/one")),
            entry: entry.into(),
        },
        area: kernel::Rect::new(0.0, 0.0, 100.0, 100.0),
        freeze: false,
    }
}

type Make =
    Box<dyn FnMut(&WebSource, kernel::Rect, platform_api::WebInbox) -> Option<Box<dyn WebView>>>;

impl WebViews {
    /// [`WebViews::sync`] at a fixed moment.
    pub(super) fn sync_at<M: Into<MadePage>>(
        &mut self,
        pages: Vec<(WebPage, bool)>,
        covered: bool,
        make: impl FnMut(&WebSource, kernel::Rect, platform_api::WebInbox) -> Option<M>,
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
            None::<MadePage>
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
pub(super) fn keeping(
    calls: &Arc<Calls>,
    inboxes: &std::rc::Rc<std::cell::RefCell<Vec<platform_api::WebInbox>>>,
) -> Make {
    let (calls, inboxes) = (Arc::clone(calls), std::rc::Rc::clone(inboxes));
    Box::new(move |_, _, inbox| {
        inboxes.borrow_mut().push(inbox);
        Some(Box::new(Fake(Arc::clone(&calls))) as Box<dyn WebView>)
    })
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

/// A page that goes when told (its engine ended).
#[derive(Debug)]
struct Going(Arc<Calls>, Arc<std::sync::atomic::AtomicBool>);

impl WebView for Going {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, visible: bool) {
        self.0.0.lock().unwrap().push(format!("visible {visible}"));
    }
    fn load_html(&self, _: &str) {}
    fn is_gone(&self) -> bool {
        self.1.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// A page the app draws: under a menu it stays (drawn under what's over it, no picture);
/// its frames make a poll ask for a frame, and its image is a picture in its place once it
/// has one; its engine gone, it's made again.
#[test]
fn a_page_the_app_draws_stays_under_what_opens_and_is_made_again_when_gone() {
    let calls = Arc::new(Calls::default());
    let gone = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let image = render_api::ImageData::live();
    let made = Arc::new(Mutex::new(0));
    let make = || {
        let (calls, gone, image, made) =
            (Arc::clone(&calls), Arc::clone(&gone), image.clone(), Arc::clone(&made));
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            *made.lock().unwrap() += 1;
            let view = Box::new(Going(Arc::clone(&calls), Arc::clone(&gone)));
            Some(MadePage { view, drawn: Some(image.clone()), popup: None })
        }
    };
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make());
    assert!(!views.poll().any(), "nothing new");
    assert!(views.pictures().is_empty(), "no frame yet: nothing drawn for it");
    image.frame_copied(render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 });
    assert!(views.poll().told, "a first frame: drawn in its place");
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make());
    assert_eq!(views.pictures().len(), 1);
    image.frame_copied(render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 });
    let polled = views.poll();
    assert!(polled.framed && !polled.told, "another frame: drawn again only");
    views.sync_at(vec![(page(1, &html, 0.0), true)], true, make());
    assert!(calls.0.lock().unwrap().is_empty(), "covered, never hidden");
    assert_eq!(*made.lock().unwrap(), 1);
    gone.store(true, std::sync::atomic::Ordering::Relaxed);
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make());
    assert_eq!((*made.lock().unwrap(), views.len()), (2, 1), "gone: let go, made again");
}

/// A page the app draws that records what it's handed.
#[derive(Debug)]
pub(super) struct Taking(pub(super) Arc<Calls>);

impl WebView for Taking {
    fn takes_files(&self) -> bool {
        true
    }
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn takes_input(&self) -> bool {
        true
    }
    fn input(&self, input: platform_api::PageInput) {
        let said = match input {
            platform_api::PageInput::Mouse(m) => {
                format!("{:?} {} {} x{}", m.action, m.at.x, m.at.y, m.clicks)
            }
            platform_api::PageInput::Wheel(w) => format!("wheel {} {}", w.at.x, w.delta.1),
            platform_api::PageInput::Key(k) => format!("key {:?}", k.key),
            platform_api::PageInput::Text(t) => format!("text {t}"),
            platform_api::PageInput::Focus(f) => format!("focus {f}"),
            platform_api::PageInput::Edit(e) => format!("edit {e:?}"),
            platform_api::PageInput::Compose(c) => format!("compose {}", c.text),
            platform_api::PageInput::Drag(d) => match d {
                platform_api::PageDrag::Enter { files, at } => {
                    format!("enter {} {} {}", files.len(), at.x, at.y)
                }
                platform_api::PageDrag::Over { at } => format!("over {} {}", at.x, at.y),
                platform_api::PageDrag::Leave => "leave".into(),
                platform_api::PageDrag::Drop { at } => format!("drop {} {}", at.x, at.y),
            },
        };
        self.0.0.lock().unwrap().push(said);
    }
    fn cursor(&self) -> Option<platform_api::CursorIcon> {
        Some(platform_api::CursorIcon::Crosshair)
    }
    fn release_focus(&self) {
        self.0.0.lock().unwrap().push("released".into());
    }
}

/// A page the app draws with a popup open (a `<select>`'s list).
#[derive(Debug)]
struct WithPopup;

impl WebView for WithPopup {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn popup(&self) -> Option<kernel::Rect> {
        Some(kernel::Rect::new(5.0, 6.0, 40.0, 30.0))
    }
}

/// A page's popup is drawn over it once it has a frame, where the page says; its new
/// frame asks for one of the window.
#[test]
fn a_pages_popup_is_drawn_over_it_once_it_has_a_frame() {
    let page_image = render_api::ImageData::live();
    let popup_image = render_api::ImageData::live();
    let (image, popup) = (page_image.clone(), popup_image.clone());
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let make = move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
        let view: Box<dyn WebView> = Box::new(WithPopup);
        Some(MadePage { view, drawn: Some(image.clone()), popup: Some(popup.clone()) })
    };
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make.clone());
    page_image.frame_copied(render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 });
    assert!(views.poll().told);
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make.clone());
    assert!(views.popups().is_empty(), "no frame of its own yet");
    popup_image.frame_copied(render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 });
    assert!(views.poll().told, "its popup shows: a whole frame");
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, make);
    let popups = views.popups();
    assert_eq!(popups.len(), 1);
    assert_eq!(popups[0].at, kernel::Rect::new(5.0, 6.0, 40.0, 30.0));
}

/// A page the app draws whose popup opens and closes as the test says.
#[derive(Debug)]
struct Toggled(Rc<std::cell::Cell<bool>>);

impl WebView for Toggled {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn popup(&self) -> Option<kernel::Rect> {
        self.0.get().then(|| kernel::Rect::new(5.0, 6.0, 40.0, 30.0))
    }
}

/// A popup's frames: opening and closing change what's drawn (a whole frame: told), a new
/// frame of one open only draws it again where it is (framed, nothing told).
#[test]
fn a_popups_new_frame_is_only_drawn_again_its_opening_and_closing_told() {
    let copied = || render_api::CopiedFrame { size: (1, 1), changed: None, bytes: 0 };
    let (page_image, popup_image) = (render_api::ImageData::live(), render_api::ImageData::live());
    let (image, popup) = (page_image.clone(), popup_image.clone());
    let open = Rc::new(std::cell::Cell::new(true));
    let shared = Rc::clone(&open);
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let make = move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
        let view: Box<dyn WebView> = Box::new(Toggled(Rc::clone(&shared)));
        Some(MadePage { view, drawn: Some(image.clone()), popup: Some(popup.clone()) })
    };
    let shown = || vec![(page(1, &html, 0.0), true)];
    views.sync_at(shown(), false, make.clone());
    page_image.frame_copied(copied());
    views.poll();
    views.sync_at(shown(), false, make.clone());
    popup_image.frame_copied(copied());
    let opened = views.poll();
    assert!(opened.told, "opened: {opened:?}");
    views.sync_at(shown(), false, make.clone());
    popup_image.frame_copied(copied());
    let framed = views.poll();
    assert!(framed.framed && !framed.told, "a new frame of it: {framed:?}");
    views.sync_at(shown(), false, make.clone());
    open.set(false);
    let closed = views.poll();
    assert!(closed.told, "closed: a whole frame: {closed:?}");
    views.sync_at(shown(), false, make);
    assert!(views.popups().is_empty());
}

/// A page whose accessibility tree is a shared generation (the test bumps it, as a new tree
/// told by its engine).
#[derive(Debug)]
struct Reading(Arc<Calls>, Arc<std::sync::atomic::AtomicU64>);

impl WebView for Reading {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn set_access(&self, on: bool) {
        self.0.0.lock().unwrap().push(format!("access {on}"));
    }
    fn access(&self) -> Option<platform_api::PageAccess> {
        let generation = self.1.load(std::sync::atomic::Ordering::Relaxed);
        Some(platform_api::PageAccess { generation, nodes: Arc::from([]) })
    }
}

/// The pages are told when a screen reader comes and goes (one made meanwhile too); a new
/// tree asks for a whole frame; their trees are handed over only while one reads.
#[test]
fn pages_tell_their_trees_only_while_a_screen_reader_runs() {
    let calls = Arc::new(Calls::default());
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let mut views = WebViews::default();
    let html: Arc<str> = "<p>a</p>".into();
    let make = |calls: &Arc<Calls>, generation: &Arc<std::sync::atomic::AtomicU64>| {
        let (calls, generation) = (Arc::clone(calls), Arc::clone(generation));
        move |_: &WebSource, _, _| {
            let view: Box<dyn WebView> =
                Box::new(Reading(Arc::clone(&calls), Arc::clone(&generation)));
            Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
        }
    };
    views.set_access(true);
    views.sync_at(vec![(page(1, &html, 10.0), true)], false, make(&calls, &generation));
    assert_eq!(*calls.0.lock().unwrap(), ["access true"], "made while one reads: told");
    assert_eq!(views.access_trees().len(), 1);
    let _ = views.poll();
    generation.store(5, std::sync::atomic::Ordering::Relaxed);
    assert!(views.poll().told, "a new tree: a whole frame");
    views.set_access(false);
    assert_eq!(calls.0.lock().unwrap().last().map(String::as_str), Some("access false"));
    assert!(views.access_trees().is_empty(), "nothing handed over without a reader");
}

/// A page the app draws that asks the app things: a menu, a pointer (each told once as it
/// changes), or that goes.
#[derive(Debug, Default)]
struct Asking {
    changed: std::cell::Cell<bool>,
    gone: std::cell::Cell<bool>,
}

impl WebView for Asking {
    fn set_bounds(&self, _: kernel::Rect) {}
    fn set_visible(&self, _: bool) {}
    fn load_html(&self, _: &str) {}
    fn takes_input(&self) -> bool {
        true
    }
    fn take_changed(&self) -> bool {
        self.changed.replace(false)
    }
    fn is_gone(&self) -> bool {
        self.gone.get()
    }
    fn cursor(&self) -> Option<platform_api::CursorIcon> {
        Some(platform_api::CursorIcon::Text)
    }
}

/// A page made by `make` the app draws, kept for the test (the views own theirs).
fn asking(views: &mut WebViews) -> Rc<Asking> {
    #[derive(Debug)]
    struct Shared(Rc<Asking>);
    impl WebView for Shared {
        fn set_bounds(&self, _: kernel::Rect) {}
        fn set_visible(&self, _: bool) {}
        fn load_html(&self, _: &str) {}
        fn takes_input(&self) -> bool {
            true
        }
        fn take_changed(&self) -> bool {
            self.0.take_changed()
        }
        fn is_gone(&self) -> bool {
            self.0.is_gone()
        }
        fn cursor(&self) -> Option<platform_api::CursorIcon> {
            self.0.cursor()
        }
    }
    let page = Rc::new(Asking::default());
    let html: Arc<str> = "<p>a</p>".into();
    let kept = Rc::clone(&page);
    views.sync_at(vec![(self::page(1, &html, 10.0), true)], false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Shared(Rc::clone(&kept)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
    page
}

/// What a page asks the app (its menu, its pointer, its caret) is handed over at the wake
/// it comes, with nothing else new; a page gone asks for a whole frame (made again then);
/// the pointer it asks is the one under the mouse.
#[test]
fn what_a_page_asks_is_looked_at_as_it_comes() {
    let mut views = WebViews::default();
    let page = asking(&mut views);
    assert!(!views.poll().any());
    page.changed.set(true);
    let polled = views.poll();
    assert!(polled.asked && !polled.told, "{polled:?}");
    assert!(!views.poll().any(), "taken once");
    assert_eq!(views.hovered_cursor(), None, "the mouse isn't over it");
    views.pointer_moved(kernel::Point::new(30.0, 5.0), true);
    assert_eq!(views.hovered_cursor(), Some(platform_api::CursorIcon::Text));
    page.gone.set(true);
    assert!(views.poll().told, "gone: made again at a whole frame");
}

/// A page that can't be made tells its view so (it never shows); one the system showed let
/// go for the app's engine takes its messages and its picture with it.
#[test]
fn a_page_not_made_is_told_failed_and_one_let_go_takes_what_it_left() {
    let html: Arc<str> = "<p>a</p>".into();
    let mut views = WebViews::default();
    views.sync_at(vec![(page(1, &html, 0.0), true)], false, |_, _, _| None::<MadePage>);
    assert_eq!(views.take_events(), [(1, WebEvent::Failed)]);
    let calls = Arc::new(Calls::default());
    let inboxes = Rc::new(RefCell::new(Vec::new()));
    let mut views = WebViews::default();
    views.sync_at(vec![(app(2, "index.html"), true)], false, keeping(&calls, &inboxes));
    inboxes.borrow()[0](platform_api::WebEvent::Message("left".into()));
    views.pictures.push((2, render_api::ImageData::live()));
    views.let_go_system_pages();
    assert_eq!(views.len(), 0);
    assert!(views.take_events().is_empty(), "its messages let go with it");
    assert!(views.pictures.is_empty(), "its picture too");
}

/// A page the app draws, recording what it's handed, made at `x` by `views`.
pub(super) fn taking_at(views: &mut WebViews, calls: &Arc<Calls>, pages: &[(u128, f32)]) {
    let html: Arc<str> = "<p>a</p>".into();
    let made = Arc::clone(calls);
    let pages = pages.iter().map(|(id, x)| (page(*id, &html, *x), true)).collect();
    views.sync_at(pages, false, move |_: &WebSource, _, _| {
        let view: Box<dyn WebView> = Box::new(Taking(Arc::clone(&made)));
        Some(MadePage { view, drawn: Some(render_api::ImageData::live()), popup: None })
    });
}

/// A page whose view falls asleep is given its picture once: a page the app draws, a copy
/// of its frame at its own size (taken from its frame on the GPU as it's first drawn); one
/// with no frame (hidden a while), or let go, none — told, never waited for.
#[test]
fn a_page_falling_asleep_is_given_its_picture_once() {
    let image = render_api::ImageData::live();
    let make = {
        let image = image.clone();
        move |_: &WebSource, _: kernel::Rect, _: platform_api::WebInbox| {
            let view: Box<dyn WebView> = Box::new(WithPopup);
            Some(MadePage { view, drawn: Some(image.clone()), popup: None })
        }
    };
    let html: Arc<str> = "<p>a</p>".into();
    let freezing = |id, freeze| WebPage { freeze, ..page(id, &html, 0.0) };
    let mut views = WebViews::default();
    views.sync_at(vec![(freezing(1, false), true)], false, make.clone());
    image.frame_copied(render_api::CopiedFrame { size: (100, 100), changed: None, bytes: 0 });
    views.sync_at(vec![(freezing(1, true), true)], false, make.clone());
    let told = views.take_events();
    let [(1, WebEvent::Picture(Some(picture)))] = told.as_slice() else { panic!("{told:?}") };
    assert_eq!((picture.width, picture.height), (100, 100), "its own size");
    assert!(picture.is_held() && picture.copy_source().is_some_and(|s| s == image));
    views.sync_at(vec![(freezing(1, true), true)], false, make.clone());
    assert!(views.take_events().is_empty(), "once");
    // Awake again, then asleep once more: asked again — its frame let go meanwhile (hidden
    // a while), drawn anew by its engine first, then copied.
    views.sync_at(vec![(freezing(1, false), true)], false, make.clone());
    image.evicted();
    views.sync_at(vec![(freezing(1, true), true)], false, make.clone());
    assert!(views.take_events().is_empty(), "its frame waited for");
    image.frame_copied(render_api::CopiedFrame { size: (100, 100), changed: None, bytes: 0 });
    views.sync_at(vec![(freezing(1, true), true)], false, make.clone());
    let told = views.take_events();
    assert!(matches!(told.as_slice(), [(1, WebEvent::Picture(Some(_)))]), "{told:?}");
    // Its frame never coming: none, past the wait.
    views.sync_at(vec![(freezing(1, false), true)], false, make.clone());
    image.evicted();
    views.sync_at(vec![(freezing(1, true), true)], false, make.clone());
    let later = start() + std::time::Duration::from_secs(60);
    views.sync(vec![(freezing(1, true), true)], false, later, make.clone());
    assert_eq!(views.take_events(), [(1, WebEvent::Picture(None))], "not in time: none");
    // A page never made (or let go): none.
    views.sync_at(vec![(freezing(2, true), false)], false, make);
    assert_eq!(views.take_events(), [(2, WebEvent::Picture(None))]);
}
