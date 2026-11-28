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
//!
//! A page the app draws itself (its own engine's, [`MadePage::drawn`]) is an image in the
//! scene: what opens over it is drawn over it, so it needs no picture and isn't hidden.
//!
//! A page whose view falls asleep ([`WebPage::freeze`]) is given its picture once
//! ([`WebEvent::Picture`]): a page the app draws, a copy of its last frame on the GPU (one
//! hidden long enough to have none drawn anew by its engine first); a system's page, its
//! snapshot — each waited for at most [`FREEZE_WAIT`]; none could be taken (it was let go,
//! a system's page hidden, the system can't): `None`. The view draws it in its place.
//!
//! Its parts: the input handed to the pages the app draws ([`input`]), files dragged from
//! the system over them ([`files`]), what the pages tell their views ([`inbox`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use platform_api::WebView;
use ui_workbench::{WebEvent, WebPage, WebSource};

use crate::workbench::{PagePopup, Sight};

mod files;
mod inbox;
mod input;

use files::DraggedFiles;
pub(super) use files::FilesOver;
use inbox::Inbox;
use input::PagesInput;
pub(super) use input::Pointer;

/// How long a page something opened over stays on screen waiting for its picture: past it,
/// hidden without one (a blank under what opened).
const PICTURE_WAIT: Duration = Duration::from_millis(500);

/// How long a page falling asleep is waited for: its engine drawing it anew (a page hidden a
/// while has no frame left), or the system taking its snapshot.
const FREEZE_WAIT: Duration = Duration::from_secs(2);

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
    /// Its picture asked for its view falling asleep, since when (a system's page).
    freezing: Option<(Instant, Asked)>,
    /// What floats over it (the mouse there the app's), as last told, and whether it's cut
    /// out of it.
    holes: Vec<(kernel::Rect, f32)>,
    cut: bool,
    /// Its corners as last rounded.
    corners: render_api::ImageCorners,
    /// The image the app draws in its place (a page of its own engine's); `None`: the
    /// system draws it over the window.
    drawn: Option<render_api::ImageData>,
    /// Its popup's image (drawn over it where [`WebView::popup`] says, while it's open).
    popup: Option<render_api::ImageData>,
    /// The frame of `drawn` last seen ([`render_api::ImageData::generation`]).
    frame: u64,
    /// Its accessibility tree's last seen (its generation).
    access: u64,
}

impl Live {
    /// It's drawn by the app (its own engine's), not by the system over the window.
    fn is_drawn(&self) -> bool {
        self.drawn.is_some()
    }

    /// It's drawn by the app and on screen.
    fn shown_drawn(&self) -> bool {
        self.visible && self.is_drawn()
    }

    /// `p` (window) in its own pixels.
    fn local(&self, p: kernel::Point) -> kernel::Point {
        p.relative_to(self.area.origin)
    }

    /// What of it the app draws now ([`Drawn`]): shown, with a frame.
    fn drawn_now(&self) -> Option<Drawn> {
        if !self.visible {
            return None;
        }
        let image = self.drawn.as_ref().filter(|i| i.has_frame())?;
        let popup = self.popup.as_ref().filter(|i| i.has_frame()).and(self.view.popup());
        Some(Drawn { page: self.id, image: image.id, popup: popup.map(rounded) })
    }
}

/// A page the app draws as last told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Drawn {
    page: u128,
    /// Its image's id.
    image: u64,
    /// Where its popup is (rounded) when it shows one with a frame.
    popup: Option<[i32; 4]>,
}

/// What a wake handed over ([`WebViews::poll`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Polled {
    /// A page told its view something, or what's drawn of them changed: a whole frame.
    pub told: bool,
    /// Only a new frame of a page the app draws: drawn again where it is.
    pub framed: bool,
    /// A page asked the app something (its menu, its caret): looked at now.
    pub asked: bool,
}

impl Polled {
    /// Anything new.
    pub(super) fn any(self) -> bool {
        self.told || self.framed || self.asked
    }
}

/// `r` in whole pixels (compared from one frame to the next).
fn rounded(r: kernel::Rect) -> [i32; 4] {
    [r.min_x(), r.min_y(), r.size.width, r.size.height].map(|v| v.round() as i32)
}

/// A page made ([`WebViews::sync`]'s `make`): the engine's view, and the image the app
/// draws for it when it draws it itself.
pub(super) struct MadePage {
    pub view: Box<dyn WebView>,
    pub drawn: Option<render_api::ImageData>,
    /// Its popup's image, for a page the app draws.
    pub popup: Option<render_api::ImageData>,
}

impl From<Box<dyn WebView>> for MadePage {
    fn from(view: Box<dyn WebView>) -> Self {
        Self { view, drawn: None, popup: None }
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
    ::files::places::cache_dir().unwrap_or_else(std::env::temp_dir).join("web-engine")
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
    /// The pages the app draws, shown and with a frame, as last told.
    drawn: Vec<Drawn>,
    /// The input the app hands the pages it draws.
    input: PagesInput,
    /// The window's scale as last told.
    scale: f32,
    /// The pages' accessibility trees are wanted (a screen reader runs).
    access: bool,
    /// Files dragged from the system over the window.
    files: Option<DraggedFiles>,
    /// The engine's install as this window last saw it: each window moves its own system
    /// pages to the engine once it's there.
    engine_seen: super::web_engine::EngineSeen,
    /// The pages whose view was given the picture it asked for (while it still asks).
    pictured: Vec<u128>,
    /// What wakes the window (a system's picture come, for a view falling asleep).
    wake: Option<WindowWake>,
}

/// What wakes a window.
#[derive(Clone)]
struct WindowWake(std::sync::Arc<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for WindowWake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WindowWake")
    }
}

impl WebViews {
    /// The pages as the views have them now (each with where it stands), `covered`: if
    /// something is open over the views; `make`: a page made over the window (the window's
    /// [`platform_api::Window::web_view`]) with where its events go. `true`: the pictures
    /// drawn in place of pages changed (a frame needed).
    pub(super) fn sync<S: Into<Sight>, M: Into<MadePage>>(
        &mut self,
        pages: Vec<(WebPage, S)>,
        covered: bool,
        now: Instant,
        mut make: impl FnMut(&WebSource, kernel::Rect, platform_api::WebInbox) -> Option<M>,
    ) -> bool {
        if pages.is_empty() && self.live.is_empty() && self.failed.is_empty() {
            return false;
        }
        // What the engines gathered meanwhile, handed over; a page whose engine went is let
        // go (made again below).
        for live in &self.live {
            live.view.poll();
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
            !l.view.is_gone()
                && pages.iter().any(|(p, _)| p.id == l.id && can_show(&l.source, &p.source))
                && l.hidden_since.is_none_or(|since| now.duration_since(since) < HIDDEN_KEPT)
        });
        self.failed.retain(|id| pages.iter().any(|(p, _)| p.id == *id));
        self.pictured.retain(|id| pages.iter().any(|(p, _)| p.id == *id && p.freeze));
        if self.live.len() != before {
            self.forget_gone();
        }
        for (page, sight) in pages {
            // Its view falls asleep: its picture, once.
            if page.freeze
                && !self.pictured.contains(&page.id)
                && let Some(picture) = self.picture_of(page.id, now)
            {
                tracing::debug!(taken = picture.is_some(), "a sleeping page's picture given");
                self.pictured.push(page.id);
                self.inbox.borrow_mut().push(page.id, WebEvent::Picture(picture));
                changed = true;
            }
            let mut visible = sight == Sight::Shown && page.area.size.width > 0.0;
            match self.live.iter_mut().find(|l| l.id == page.id) {
                // Drawn by the app: what's over it is drawn over it, it stays.
                Some(live) if live.is_drawn() => {
                    let visible = sight != Sight::Away && page.area.size.width > 0.0;
                    place(live, &page, visible, now);
                }
                Some(live) => {
                    if sight == Sight::Under && live.visible {
                        let has = self.pictures.iter().any(|(id, _)| *id == page.id);
                        visible = !has && waits_for_picture(live, now, &mut self.pictures);
                        changed |= self.pictures.iter().any(|(id, _)| *id == page.id) != has;
                    }
                    if sight != Sight::Under || !live.visible {
                        live.asked = None;
                    }
                    place(live, &page, visible, now);
                }
                // Made when it's first seen (a page never seen makes nothing), once.
                None if visible && !self.failed.contains(&page.id) => {
                    let to_view = inbox::to_view(&self.inbox, page.id);
                    match make(&page.source, page.area, to_view).map(Into::into) {
                        Some(MadePage { view, drawn, popup }) => {
                            view.set_access(self.access);
                            self.live.push(Live {
                                id: page.id,
                                source: page.source,
                                view,
                                drawn,
                                popup,
                                frame: 0,
                                area: page.area,
                                visible: true,
                                hidden_since: None,
                                asked: None,
                                freezing: None,
                                holes: Vec::new(),
                                cut: true,
                                corners: render_api::ImageCorners::NONE,
                                access: 0,
                            });
                        }
                        // Not made (no engine here, it didn't start): its view stops waiting
                        // for it, and says why in its place.
                        None => {
                            self.failed.push(page.id);
                            self.inbox.borrow_mut().push(page.id, WebEvent::Failed);
                        }
                    }
                }
                None => {}
            }
        }
        if self.drawn_changed() {
            self.drawn = self.live.iter().filter_map(Live::drawn_now).collect();
            changed = true;
        }
        changed
    }

    /// Whether `pages` (as the views have them now) ask something of a window that draws no
    /// frames: a page's picture (its view falls asleep), or a page let go (asleep). Nothing
    /// else is done unseen — a covered window's hidden pages are kept as they were.
    pub(in crate::app) fn sleep_work(&self, pages: &[(WebPage, Sight)]) -> bool {
        pages.iter().any(|(p, _)| p.freeze && !self.pictured.contains(&p.id))
            || self.live.iter().any(|l| !pages.iter().any(|(p, _)| p.id == l.id))
    }

    /// What wakes the window: a page's picture come is taken at once (set once).
    pub(in crate::app) fn set_waker(
        &mut self,
        wake: impl FnOnce() -> std::sync::Arc<dyn Fn() + Send + Sync>,
    ) {
        if self.wake.is_none() {
            self.wake = Some(WindowWake(wake()));
        }
    }

    /// Page `id`'s picture as it is now, for its view falling asleep: `Some(picture)` once
    /// it's known (`None` in it: none could be taken), `None` while a system's is awaited.
    fn picture_of(&mut self, id: u128, now: Instant) -> Option<Option<render_api::ImageData>> {
        let Some(live) = self.live.iter_mut().find(|l| l.id == id) else { return Some(None) };
        // Its size in pixels: drawn as it is, never stretched.
        let size = |area: kernel::Rect| {
            (area.size.width.round().max(0.0) as u32, area.size.height.round().max(0.0) as u32)
        };
        let (width, height) = size(live.area);
        if let Some(drawn) = &live.drawn {
            if drawn.has_frame() {
                // Shown for it alone (hidden): hidden again.
                if live.freezing.take().is_some() {
                    live.view.set_visible(live.visible);
                }
                // A copy of its frame, made on the GPU at the renderer's next frame.
                return Some(Some(render_api::ImageData::copy_of(drawn, width, height)));
            }
            // Hidden a while, its frame let go: its engine draws it anew (told it's seen;
            // nothing of it is drawn on screen), waited for.
            return match &live.freezing {
                None => {
                    live.view.set_visible(true);
                    live.freezing = Some((now, Rc::default()));
                    None
                }
                Some((since, _)) if now.duration_since(*since) >= FREEZE_WAIT => {
                    live.freezing = None;
                    live.view.set_visible(live.visible);
                    Some(None)
                }
                Some(_) => None,
            };
        }
        if !live.visible {
            return Some(None);
        }
        let Some((since, asked)) = &live.freezing else {
            let asked: Asked = Rc::default();
            let filled = Rc::clone(&asked);
            // Come, the window woken to take it (it may draw nothing meanwhile).
            let wake = self.wake.clone();
            let taking = live.view.snapshot(Box::new(move |picture| {
                *filled.borrow_mut() = Some(picture);
                if let Some(WindowWake(wake)) = &wake {
                    wake();
                }
            }));
            if !taking {
                return Some(None);
            }
            live.freezing = Some((now, asked));
            return None;
        };
        let come = asked.borrow_mut().take();
        let picture = match come {
            Some(Some(picture)) if Some(picture.rgba.len()) == picture_bytes(&picture) => {
                Some(render_api::ImageData::held(picture.width, picture.height, picture.rgba))
            }
            Some(_) => None,
            None if now.duration_since(*since) >= FREEZE_WAIT => None,
            None => return None,
        };
        live.freezing = None;
        Some(picture)
    }

    /// Whether what the app draws of its pages (shown, with a frame) isn't as last told:
    /// compared as it's walked, nothing gathered (asked at every wake).
    fn drawn_changed(&self) -> bool {
        !self.live.iter().filter_map(Live::drawn_now).eq(self.drawn.iter().copied())
    }

    /// Whether page `id` is one the app draws, shown with a frame (as last told).
    fn is_drawn_shown(&self, id: u128) -> bool {
        self.drawn.iter().any(|d| d.page == id)
    }

    /// The pages let go: what they told, the input pointing at them, files over them, the
    /// pictures in their place let go with them.
    fn forget_gone(&mut self) {
        let live: Vec<u128> = self.live.iter().map(|l| l.id).collect();
        self.inbox.borrow_mut().retain(|id| live.contains(&id));
        self.input.forget(|id| live.contains(&id));
        self.pictures.retain(|(id, _)| live.contains(id));
        self.forget_drag(&live);
    }

    /// What floats over the pages (rect, corners' radius; physical): the mouse there the
    /// app's, and — `cut` (no floating layer to draw it over them) — cut out of each. Told
    /// to a page only when it changes.
    pub(super) fn set_holes(&mut self, holes: &[(kernel::Rect, f32)], cut: bool) {
        // Only the system's pages are drawn over the window (the app draws its own under
        // what floats).
        for live in self.live.iter_mut().filter(|l| !l.is_drawn()) {
            if live.holes != holes || live.cut != cut {
                live.view.set_holes(holes, cut);
                live.holes = holes.to_vec();
                live.cut = cut;
            }
        }
    }

    /// Each page's corners rounded as `corners(area)` says (its card's): told to a page only
    /// when they change.
    pub(super) fn set_corners(
        &mut self,
        corners: impl Fn(kernel::Rect) -> render_api::ImageCorners,
    ) {
        // The app rounds its own as it draws them.
        for live in self.live.iter_mut().filter(|l| !l.is_drawn()) {
            let now = corners(live.area);
            if live.corners != now {
                live.view.set_corners(now.radius, now.which);
                live.corners = now;
            }
        }
    }

    /// A wake: what the engines gathered handed over ([`Polled`]).
    pub(super) fn poll(&mut self) -> Polled {
        if self.live.is_empty() {
            return Polled::default();
        }
        let mut polled = Polled::default();
        for live in &mut self.live {
            live.view.poll();
            // Its menu, its caret; gone, it's made again (a whole frame).
            polled.asked |= live.view.take_changed();
            polled.told |= live.view.is_gone();
            // Its page's frames and its popup's.
            let generation = [&live.drawn, &live.popup]
                .into_iter()
                .flatten()
                .map(render_api::ImageData::generation)
                .sum::<u64>();
            if live.is_drawn() {
                polled.framed |= std::mem::replace(&mut live.frame, generation) != generation;
            }
            // A new accessibility tree: the window's is made again with it (a whole frame).
            let seen = live.view.access().map_or(0, |a| a.generation);
            polled.told |= std::mem::replace(&mut live.access, seen) != seen;
        }
        polled.told |= !self.inbox.borrow().is_empty() || self.drawn_changed();
        if polled.asked {
            kernel::probe::count("web-pages", "asked", 1);
        }
        polled
    }

    /// The engine's install as this window last saw it, for
    /// [`super::web_engine::became_usable`].
    pub(super) fn engine_seen(&mut self) -> &mut super::web_engine::EngineSeen {
        &mut self.engine_seen
    }

    /// The pages the system shows let go (each made again at the next sync, by the app's
    /// engine now that it's there); the system's that couldn't be made tried again too.
    pub(super) fn let_go_system_pages(&mut self) {
        self.live.retain(Live::is_drawn);
        self.failed.clear();
        self.forget_gone();
    }

    /// All pages let go (the GPU device was lost: their importers are dead). Each made again
    /// at the next sync, with a new importer from the new renderer.
    pub(super) fn let_go_all_pages(&mut self) {
        self.live.clear();
        self.failed.clear();
        self.pictures.clear();
        self.drawn.clear();
        self.forget_gone();
    }

    /// Some page the app draws is open (input may go to it).
    pub(super) fn any_drawn(&self) -> bool {
        self.live.iter().any(Live::is_drawn)
    }

    /// The pages' accessibility trees wanted (a screen reader runs) or not: told to each
    /// page as it changes (a page made later is told as it's made).
    pub(super) fn set_access(&mut self, on: bool) {
        if std::mem::replace(&mut self.access, on) == on {
            return;
        }
        for live in &self.live {
            live.view.set_access(on);
        }
    }

    /// The accessibility trees of the pages the app draws and shows (while wanted).
    pub(super) fn access_trees(&self) -> Vec<(u128, platform_api::PageAccess)> {
        if !self.access {
            return Vec::new();
        }
        let shown = self.live.iter().filter(|l| l.shown_drawn());
        shown.filter_map(|l| Some((l.id, l.view.access()?))).collect()
    }

    /// The window's scale, told to the pages it changed for.
    pub(super) fn set_scale(&mut self, scale: f32) {
        if self.scale == scale {
            return;
        }
        self.scale = scale;
        for live in &self.live {
            live.view.set_scale(scale);
        }
    }

    fn live(&self, id: u128) -> Option<&Live> {
        self.live.iter().find(|l| l.id == id)
    }

    /// Whether a page the system shows is on screen now (what floats is drawn over it then;
    /// one the app draws is under the app's drawing anyway).
    pub(super) fn any_shown(&self) -> bool {
        self.live.iter().any(|l| l.visible && !l.is_drawn())
    }

    /// The popups drawn over the pages the app draws (an open `<select>`'s list).
    pub(super) fn popups(&self) -> Vec<PagePopup> {
        let open = |l: &&Live| self.drawn.iter().any(|d| d.page == l.id && d.popup.is_some());
        self.live
            .iter()
            .filter(open)
            .filter_map(|l| {
                Some(PagePopup { page: l.id, image: l.popup.clone()?, at: l.view.popup()? })
            })
            .collect()
    }

    /// The pictures drawn in place of pages (page id, picture): of those something is open
    /// over, and of those the app draws itself.
    pub(super) fn pictures(&self) -> Vec<(u128, render_api::ImageData)> {
        let drawn = self.live.iter().filter(|l| self.is_drawn_shown(l.id));
        let drawn = drawn.filter_map(|l| l.drawn.clone().map(|image| (l.id, image)));
        self.pictures.iter().cloned().chain(drawn).collect()
    }

    /// When a hidden page is next let go (its engine's memory back): a frame then, or an idle
    /// window would keep it.
    pub(super) fn deadline(&self) -> Option<Instant> {
        let let_go = self.live.iter().filter_map(|l| l.hidden_since).map(|s| s + HIDDEN_KEPT);
        let waits =
            self.live.iter().filter_map(|l| l.asked.as_ref()).map(|(s, _)| *s + PICTURE_WAIT);
        let freezing =
            self.live.iter().filter_map(|l| l.freezing.as_ref()).map(|(s, _)| *s + FREEZE_WAIT);
        let_go.chain(waits).chain(freezing).min()
    }

    /// How many pages are made (tests).
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.live.len()
    }
}

/// `live` placed as `page` says, shown or hidden (`visible`): another HTML loaded in it, its
/// area and its visibility told when they change.
fn place(live: &mut Live, page: &WebPage, visible: bool, now: Instant) {
    if let (WebSource::Html(shown), WebSource::Html(html)) = (&live.source, &page.source)
        && !std::sync::Arc::ptr_eq(shown, html)
    {
        live.view.load_html(html);
        live.source = page.source.clone();
    }
    if visible && live.area != page.area {
        live.view.set_bounds(page.area);
        live.area = page.area;
    }
    if live.visible != visible {
        live.view.set_visible(visible);
        live.visible = visible;
    }
    live.hidden_since = if visible { None } else { live.hidden_since.or(Some(now)) };
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
        Some(Some(picture)) if Some(picture.rgba.len()) == picture_bytes(&picture) => {
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

/// The bytes a picture of its size holds (RGBA), `None` past what memory can count (a
/// system's picture is checked against it before it's drawn).
fn picture_bytes(picture: &platform_api::WebPicture) -> Option<usize> {
    (picture.width as usize).checked_mul(picture.height as usize)?.checked_mul(4)
}

#[cfg(test)]
#[path = "../../../tests/unit/app/web_views/mod.rs"]
mod tests;
