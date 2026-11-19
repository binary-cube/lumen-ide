//! The pages the system's web engine shows over this window's views ([`WebPage`]): one per
//! page a view has, made when it's first seen, placed on the view's area after each frame,
//! hidden while it isn't seen (another tab, a modal or a menu over it — nothing the app
//! draws can go above it), let go with its view. Only while a view has one: none made
//! before.
//!
//! A page something opens over (About, a dialog, a menu, a notification) isn't left as a
//! blank: its picture is asked for first (the page kept on screen until it comes, at most
//! [`PICTURE_WAIT`]), then the page is hidden and the picture drawn in its place, under what
//! opened, until it closes. The picture goes as the page shows again or its tab does.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use platform_api::WebView;
use ui_workbench::{WebEvent, WebPage, WebSource};

use crate::workbench::Sight;

/// How long a page something opened over stays on screen waiting for its picture: past it,
/// hidden without one (a blank under what opened).
const PICTURE_WAIT: Duration = Duration::from_millis(500);

/// A page's picture asked for, filled on the window's thread when it comes (`Some(None)`:
/// none could be taken).
type Asked = Rc<RefCell<Option<Option<platform_api::WebPicture>>>>;

/// A page on screen.
#[derive(Debug)]
struct Live {
    id: u128,
    source: WebSource,
    view: Box<dyn WebView>,
    area: kernel::Rect,
    visible: bool,
    /// Since when it's been hidden (its engine let go after [`HIDDEN_KEPT`]).
    hidden_since: Option<Instant>,
    /// Its picture asked for (something opened over it), since when.
    asked: Option<(Instant, Asked)>,
    /// What floats over it (the mouse there the app's), as last told, and whether it's cut
    /// out of it.
    holes: Vec<(kernel::Rect, f32)>,
    cut: bool,
    /// Its corners as last rounded (radius, which).
    corners: (f32, u8),
}

/// What the app pages told, not yet handed to their views (filled by the engine's handlers
/// on the window's thread, emptied before the next frame).
#[derive(Debug, Default)]
struct Inbox {
    events: VecDeque<(u128, WebEvent)>,
    /// The bytes they hold.
    bytes: usize,
}

/// The most events kept waiting for their views, and their bytes: past either, the newest
/// is let go (logged) — a page can't fill the app's memory while its window is hidden.
const INBOX_EVENTS: usize = 1024;
const INBOX_BYTES: usize = 256 << 20;

impl Inbox {
    fn push(&mut self, page: u128, event: WebEvent) {
        let size = event_size(&event);
        if size > platform_api::MAX_WEB_MESSAGE
            || self.events.len() >= INBOX_EVENTS
            || self.bytes.saturating_add(size) > INBOX_BYTES
        {
            tracing::warn!(
                "a page's message dropped: too many waiting, or too long ({size} bytes)"
            );
            return;
        }
        self.bytes += size;
        self.events.push_back((page, event));
    }

    /// What `keep` doesn't want is let go (its page is gone).
    fn retain(&mut self, keep: impl Fn(u128) -> bool) {
        self.events.retain(|(id, _)| keep(*id));
        self.bytes = self.events.iter().map(|(_, e)| event_size(e)).sum();
    }
}

fn event_size(event: &WebEvent) -> usize {
    match event {
        WebEvent::Message(text) | WebEvent::OpenLink(text) => text.len(),
    }
}

/// Whether a page made for `live` can show `source`: another HTML in its place (loaded
/// into it), the same app; another app is another page.
fn can_show(live: &WebSource, source: &WebSource) -> bool {
    match (live, source) {
        (WebSource::Html(_), WebSource::Html(_)) => true,
        (a, b) => a == b,
    }
}

/// How long a hidden page keeps its engine (a web engine holds tens of MB): past it, it's
/// let go, and made again when its tab is seen.
const HIDDEN_KEPT: Duration = Duration::from_secs(60);

/// Where the system's web engine keeps its files: the app's cache (writable everywhere;
/// WebView2 would write beside the program otherwise).
pub(super) fn data_dir() -> std::path::PathBuf {
    files::places::cache_dir().unwrap_or_else(std::env::temp_dir).join("web-engine")
}

#[derive(Debug, Default)]
pub(super) struct WebViews {
    live: Vec<Live>,
    /// The pages the system couldn't show (no engine, it didn't start): not tried again on
    /// each frame — their tabs say what they hold, Open in Browser works.
    failed: Vec<u128>,
    /// What the app pages told, for their views (shared with the engine's handlers).
    inbox: Rc<RefCell<Inbox>>,
    /// The pictures drawn in place of the pages something is open over (page id, picture).
    pictures: Vec<(u128, render_api::ImageData)>,
}

impl WebViews {
    /// The pages as the views have them now (each with where it stands), `covered`: if
    /// something is open over the views; `make`: a page made over the window (the window's
    /// [`platform_api::Window::web_view`]) with where its events go. `true`: the pictures
    /// drawn in place of pages changed (a frame needed).
    pub(super) fn sync<S: Into<Sight>>(
        &mut self,
        pages: Vec<(WebPage, S)>,
        covered: bool,
        now: Instant,
        mut make: impl FnMut(
            &WebSource,
            kernel::Rect,
            platform_api::WebInbox,
        ) -> Option<Box<dyn WebView>>,
    ) -> bool {
        if pages.is_empty() && self.live.is_empty() && self.failed.is_empty() {
            return false;
        }
        let pages: Vec<(WebPage, Sight)> = pages
            .into_iter()
            .map(|(page, sight)| match sight.into() {
                Sight::Shown if covered => (page, Sight::Under),
                sight => (page, sight),
            })
            .collect();
        // A picture stays only while its page is under something.
        let pictures = self.pictures.len();
        self.pictures
            .retain(|(id, _)| pages.iter().any(|(p, s)| p.id == *id && *s == Sight::Under));
        let mut changed = self.pictures.len() != pictures;
        // Their views closed: let go; hidden long enough: let go too (made again if seen);
        // an app page showing another app: let go, made anew below.
        let before = self.live.len();
        self.live.retain(|l| {
            pages.iter().any(|(p, _)| p.id == l.id && can_show(&l.source, &p.source))
                && l.hidden_since.is_none_or(|since| now.duration_since(since) < HIDDEN_KEPT)
        });
        self.failed.retain(|id| pages.iter().any(|(p, _)| p.id == *id));
        if self.live.len() != before {
            // What a page let go told is let go with it.
            let live: Vec<u128> = self.live.iter().map(|l| l.id).collect();
            self.inbox.borrow_mut().retain(|id| live.contains(&id));
        }
        for (page, sight) in pages {
            let mut visible = sight == Sight::Shown && page.area.size.width > 0.0;
            match self.live.iter_mut().find(|l| l.id == page.id) {
                Some(live) => {
                    if sight == Sight::Under && live.visible {
                        let has = self.pictures.iter().any(|(id, _)| *id == page.id);
                        visible = !has && waits_for_picture(live, now, &mut self.pictures);
                        changed |= self.pictures.iter().any(|(id, _)| *id == page.id) != has;
                    }
                    if sight != Sight::Under || !live.visible {
                        live.asked = None;
                    }
                    if let (WebSource::Html(shown), WebSource::Html(html)) =
                        (&live.source, &page.source)
                        && !std::sync::Arc::ptr_eq(shown, html)
                    {
                        live.view.load_html(html);
                        live.source = page.source;
                    }
                    if visible && live.area != page.area {
                        live.view.set_bounds(page.area);
                        live.area = page.area;
                    }
                    if live.visible != visible {
                        live.view.set_visible(visible);
                        live.visible = visible;
                    }
                    live.hidden_since =
                        if visible { None } else { live.hidden_since.or(Some(now)) };
                }
                // Made when it's first seen (a page never seen makes nothing), once.
                None if visible && !self.failed.contains(&page.id) => {
                    let inbox = Rc::clone(&self.inbox);
                    let id = page.id;
                    let to_view: platform_api::WebInbox = Box::new(move |event| {
                        let event = match event {
                            platform_api::WebEvent::Message(text) => WebEvent::Message(text),
                            platform_api::WebEvent::OpenLink(url) => WebEvent::OpenLink(url),
                        };
                        inbox.borrow_mut().push(id, event);
                    });
                    match make(&page.source, page.area, to_view) {
                        Some(view) => self.live.push(Live {
                            id: page.id,
                            source: page.source,
                            view,
                            area: page.area,
                            visible: true,
                            hidden_since: None,
                            asked: None,
                            holes: Vec::new(),
                            cut: true,
                            corners: (0.0, 0),
                        }),
                        None => self.failed.push(page.id),
                    }
                }
                None => {}
            }
        }
        changed
    }

    /// What floats over the pages (rect, corners' radius; physical): the mouse there the
    /// app's, and — `cut` (no floating layer to draw it over them) — cut out of each. Told
    /// to a page only when it changes.
    pub(super) fn set_holes(&mut self, holes: &[(kernel::Rect, f32)], cut: bool) {
        for live in &mut self.live {
            if live.holes != holes || live.cut != cut {
                live.view.set_holes(holes, cut);
                live.holes = holes.to_vec();
                live.cut = cut;
            }
        }
    }

    /// Each page's corners rounded as `corners(area)` says (its card's): told to a page only
    /// when they change.
    pub(super) fn set_corners(&mut self, corners: impl Fn(kernel::Rect) -> (f32, u8)) {
        for live in &mut self.live {
            let now = corners(live.area);
            if live.corners != now {
                live.view.set_corners(now.0, now.1);
                live.corners = now;
            }
        }
    }

    /// Whether a page is on screen now (what floats is drawn over it then).
    pub(super) fn any_shown(&self) -> bool {
        self.live.iter().any(|l| l.visible)
    }

    /// The pictures drawn in place of the pages something is open over (page id, picture).
    pub(super) fn pictures(&self) -> &[(u128, render_api::ImageData)] {
        &self.pictures
    }

    /// What the app pages told since the last time, for their views (page id, event), in
    /// the order told.
    pub(super) fn take_events(&mut self) -> Vec<(u128, WebEvent)> {
        let mut inbox = self.inbox.borrow_mut();
        if inbox.events.is_empty() {
            return Vec::new();
        }
        inbox.bytes = 0;
        inbox.events.drain(..).collect()
    }

    /// The views' messages handed to their pages; one for a page not made (not seen yet,
    /// let go) is let go.
    pub(super) fn send(&self, sends: Vec<(u128, String)>) {
        for (id, message) in sends {
            match self.live.iter().find(|l| l.id == id) {
                Some(live) => live.view.send(&message),
                None => tracing::debug!("a message for a page not shown let go"),
            }
        }
    }

    /// When a hidden page is next let go (its engine's memory back): a frame then, or an idle
    /// window would keep it.
    pub(super) fn deadline(&self) -> Option<Instant> {
        let let_go = self.live.iter().filter_map(|l| l.hidden_since).map(|s| s + HIDDEN_KEPT);
        let waits =
            self.live.iter().filter_map(|l| l.asked.as_ref()).map(|(s, _)| *s + PICTURE_WAIT);
        let_go.chain(waits).min()
    }

    /// A click on the window itself (not on a page): the pages give the keyboard back.
    pub(super) fn release_focus(&self) {
        for live in self.live.iter().filter(|l| l.visible) {
            live.view.release_focus();
        }
    }

    /// How many pages are made (tests).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.live.len()
    }
}

/// A shown page something opened over: whether it stays on screen a while longer, waiting
/// for its picture (asked for now, if it wasn't). A picture come is put in `pictures` and
/// the page stays one more frame (the picture drawn under it before it goes); none come
/// (none taken, or not in time), it goes now.
fn waits_for_picture(
    live: &mut Live,
    now: Instant,
    pictures: &mut Vec<(u128, render_api::ImageData)>,
) -> bool {
    let Some((since, asked)) = &live.asked else {
        let asked: Asked = Rc::default();
        let filled = Rc::clone(&asked);
        let taking = live.view.snapshot(Box::new(move |picture| {
            *filled.borrow_mut() = Some(picture);
        }));
        if taking {
            live.asked = Some((now, asked));
        }
        return taking;
    };
    let come = asked.borrow_mut().take();
    match come {
        Some(Some(picture))
            if picture.rgba.len() == picture.width as usize * picture.height as usize * 4 =>
        {
            let image =
                render_api::ImageData::gpu_only(picture.width, picture.height, picture.rgba);
            pictures.push((live.id, image));
            live.asked = None;
            true
        }
        Some(_) => {
            live.asked = None;
            false
        }
        None if now.duration_since(*since) >= PICTURE_WAIT => {
            live.asked = None;
            false
        }
        None => true,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/web_views.rs"]
mod tests;
