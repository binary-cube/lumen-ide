//! A `.drawio` file's draw.io tab. While draw.io isn't installed it offers the download
//! (its size, its version, the attribution; then its progress and Stop; offline or failed,
//! why, and Retry), as the language servers' bar does. Once it is, our page around draw.io
//! fills the tab ([`WebPage`]: the app shows it while the tab is seen, lets it go when it
//! closes) and the two talk ([`crate::protocol`]).
//!
//! What touches the disk runs on the `diagram-web-file` worker (the file's reads and
//! writes, our page written into the install); its results, and the project's events about
//! the file (written by someone, renamed), come back through one inbox that wakes the app.
//! The theme's colors are worked out once per palette; a theme changed while the tab is
//! open makes the page anew (nothing unsaved), or after the next write. Left alone, it may
//! sleep ([`asleep`]).

mod asleep;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use i18n::{t, tr};
use kernel::{Point, Rect, Rgba};
use render_api::Quad;
use ui_components::ds::{self, Ctx};
use ui_components::{
    AppRequest, Palette, PressMods, Tip, ViewMemory, Viewer, ViewerAction, ViewerCursor,
    ViewerPaint, WebEvent, WebPage, WebSource,
};
use ui_kit::{draw_icon, draw_text};
use workbench_model::Notification;

use crate::install::{self, Status};
use crate::protocol::{Effect, Session};
use crate::theme::{self, Theme};
use crate::{file, package, page};

pub use asleep::DIAGRAMS;
use asleep::{LastChange, Slumber};

/// The extension its tabs are opened by.
pub const EXTENSION: &str = "view-diagram-web";

/// The worker the file's reads and writes run on.
pub const FILE_THREAD: &str = "diagram-web-file";

/// How often the file is looked at while the tab is shown (a change no event told).
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

type Wake = Arc<dyn Fn() + Send + Sync>;

/// What its tab is opened with: the file, and its project's events (its changes on disk).
#[derive(Debug, Clone)]
pub struct OpenData {
    pub path: PathBuf,
    pub events: Option<kernel::EventBus>,
}

/// What comes back to the view (from its workers and its project's events).
#[derive(Debug)]
enum Done {
    /// The file read (`serial`: the read it answers; an older one is let go).
    Read { serial: u64, path: PathBuf, result: Result<Option<Arc<str>>, String> },
    /// A write of `xml` ended.
    Written { xml: Arc<str>, result: Result<file::Wrote, String> },
    /// The file's stamp, looked at (`None`: not there).
    Looked(Option<(u64, std::time::SystemTime)>),
    /// Our page written into the install's folder.
    PageWritten { dir: Arc<Path>, result: Result<(), String> },
    /// The file was written on disk (by anyone: our own writes too).
    DiskChanged(PathBuf),
    /// The file was renamed or moved.
    Renamed { from: PathBuf, to: PathBuf },
    /// An export written where the user chose.
    Exported { path: PathBuf, result: Result<(), String> },
}

/// The offer's buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    Download,
    Stop,
    Retry,
}

/// The palette's colors the theme is made from (compared at each paint: a few numbers).
type ThemeKey = [Rgba; 8];

fn theme_key(p: &Palette) -> ThemeKey {
    [
        p.editor_background,
        p.panel_background,
        p.window_background,
        p.text,
        p.text_muted,
        p.accent,
        p.border,
        p.popup_background,
    ]
}

pub struct DiagramWebView {
    path: Option<PathBuf>,
    session: Session,
    /// The project's events followed (while it lives).
    subscriptions: Vec<kernel::Subscription>,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    wake: Arc<Mutex<Option<Wake>>>,
    /// The install as last looked at (its generation, what it said).
    seen: Option<u64>,
    status: Status,
    /// The folder our page was written into (the page is shown from it).
    page_dir: Option<Arc<Path>>,
    writing_page: bool,
    /// The page's identity (another: the page made anew).
    page_id: u128,
    /// Where its page stands: opening (a loader turns under it), drawn, or never to be.
    page: PageState,
    entry: Arc<str>,
    area: Rect,
    theme: Option<(ThemeKey, Theme)>,
    /// The theme changed while something was unsaved: the page is made anew after the write.
    reload_after_save: bool,
    read_serial: u64,
    /// The file was read once (later reads are changes on disk).
    read_once: bool,
    /// The file's stamp as last looked at, when, and whether a look is under way.
    stamp: Option<(u64, std::time::SystemTime)>,
    looked_at: Option<std::time::Instant>,
    looking: bool,
    /// Its tab isn't seen (another tab, a window hidden): the file isn't looked at, no frame
    /// is asked for it.
    hidden: bool,
    sends: Vec<(u128, String)>,
    /// An export waiting for where it goes (the system's Save dialog).
    export: Option<Vec<u8>>,
    actions: Vec<ViewerAction>,
    notifications: Vec<Notification>,
    buttons: Vec<(Rect, Button)>,
    hovered: Option<Button>,
    /// Its part of the sleeping (a tab with a file: [`Self::in_tab`]).
    sleep: Option<ui_components::sleep::SleepPart>,
    /// When the diagram last changed: how long it's been quiet.
    quiet: Arc<LastChange>,
    /// Where its page stands as it sleeps.
    slumber: Slumber,
}

impl Default for DiagramWebView {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            path: None,
            session: Session::default(),
            subscriptions: Vec::new(),
            tx,
            rx,
            wake: Arc::default(),
            seen: None,
            status: Status::Looking,
            page_dir: None,
            writing_page: false,
            page_id: kernel::id::new_uuid(),
            page: PageState::Opening,
            entry: Arc::from(""),
            area: Rect::default(),
            theme: None,
            reload_after_save: false,
            read_serial: 0,
            read_once: false,
            stamp: None,
            looked_at: None,
            looking: false,
            hidden: false,
            sends: Vec::new(),
            export: None,
            actions: Vec::new(),
            notifications: Vec::new(),
            buttons: Vec::new(),
            hovered: None,
            sleep: None,
            quiet: Arc::default(),
            slumber: Slumber::Awake,
        }
    }
}

impl std::fmt::Debug for DiagramWebView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiagramWebView")
            .field("path", &self.path)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// `job`'s result sent to the view's inbox, the app woken.
fn deliver(tx: &Sender<Done>, wake: &Arc<Mutex<Option<Wake>>>, done: Done) {
    let _ = tx.send(done);
    let wake = wake.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(wake) = wake {
        wake();
    }
}

impl DiagramWebView {
    /// The file's name, shown by draw.io.
    fn title(path: &Path) -> String {
        path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// A view of `file` (`None`: given later, by the command's data).
    pub fn for_file(file: Option<PathBuf>) -> Self {
        let mut view = Self::default();
        if let Some(file) = file {
            view.open(file, None);
        }
        view
    }

    /// The file `path` shown from now on: read anew, its events followed.
    fn open(&mut self, path: PathBuf, events: Option<&kernel::EventBus>) {
        let config = self.theme.as_ref().map(|(_, th)| theme::config(th)).unwrap_or_default();
        self.session = Session::new(Self::title(&path), config);
        self.read_once = false;
        self.path = Some(path);
        self.new_page();
        if let Some(events) = events {
            self.follow(events);
        }
        self.read();
    }

    /// The project's events about files: what concerns this one comes to the inbox.
    fn follow(&mut self, events: &kernel::EventBus) {
        self.subscriptions.clear();
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        self.subscriptions.push(events.subscribe(move |e: &workspace::events::FilesWritten| {
            for path in &e.paths {
                deliver(&tx, &wake, Done::DiskChanged(path.clone()));
            }
        }));
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        self.subscriptions.push(events.subscribe(move |e: &workspace::events::FileRenamed| {
            deliver(&tx, &wake, Done::Renamed { from: e.from.clone(), to: e.to.clone() });
        }));
    }

    /// Its file `from` is `to` now (renamed or moved): followed there.
    fn renamed(&mut self, from: &Path, to: PathBuf) {
        if self.path.as_deref() != Some(from) || from == to {
            return;
        }
        self.session.set_title(Self::title(&to));
        self.path = Some(to);
        // Its first read was of the old path (let go): read where it is now.
        if !self.read_once {
            self.read();
        }
    }

    /// The file read on the worker.
    fn read(&mut self) {
        let Some(path) = self.path.clone() else { return };
        self.read_serial += 1;
        let serial = self.read_serial;
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        kernel::work::run(FILE_THREAD, move || {
            let result = file::read(&path);
            deliver(&tx, &wake, Done::Read { serial, path, result });
        });
    }

    /// The page made anew (another id): it asks for its configuration and the file again.
    fn new_page(&mut self) {
        // Made anew while falling asleep (a theme changed): it stays awake (the picture asked
        // of the old page would never come).
        if matches!(self.slumber, Slumber::Freezing(_)) {
            self.slumber = Slumber::Awake;
        }
        self.page_id = kernel::id::new_uuid();
        self.page = PageState::Opening;
        self.session.new_page();
        self.reload_after_save = false;
        if let Some((_, theme)) = &self.theme {
            self.entry = Arc::from(page::entry(&i18n::locale(), theme.look()));
        }
    }

    /// What the conversation asks for, done.
    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Send(text) => self.sends.push((self.page_id, text)),
                Effect::Write(xml) => {
                    self.quiet.now();
                    self.write(xml);
                }
                Effect::Touched => self.touch(),
                Effect::Close => self.actions.push(ViewerAction::Close),
                Effect::OpenLink(url) => self.actions.push(ViewerAction::OpenLink(url)),
                Effect::SaveAs { name, bytes } => {
                    // Where it goes: asked with the system's dialog, beside the diagram.
                    self.export = Some(bytes);
                    let start = self.path.as_deref().and_then(Path::parent).map(Path::to_path_buf);
                    self.actions
                        .push(ViewerAction::Request(AppRequest::PickSavePath { start, name }));
                }
            }
        }
    }

    /// `xml` written as the file on the worker.
    fn write(&mut self, xml: Arc<str>) {
        let Some(path) = self.path.clone() else { return };
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        // Never over someone else's change (a `git pull`, another editor): checked there.
        let expected = self.session.on_disk();
        kernel::work::run(FILE_THREAD, move || {
            let result = file::write_over(&path, &xml, expected.as_deref());
            deliver(&tx, &wake, Done::Written { xml, result });
        });
    }

    /// The file looked at on the worker, at most every [`LOOK_EVERY`] while the tab is
    /// shown: a change on disk is read (a tab made again from a session hears no events; a
    /// changed stamp is the only sign).
    fn look_at_file(&mut self) {
        let due = self.looked_at.is_none_or(|t| t.elapsed() >= LOOK_EVERY);
        let Some(path) = self.path.clone().filter(|_| due && self.read_once && !self.looking)
        else {
            return;
        };
        self.looking = true;
        self.looked_at = Some(std::time::Instant::now());
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        kernel::work::run(FILE_THREAD, move || {
            let stamp =
                std::fs::metadata(&path).ok().and_then(|m| Some((m.len(), m.modified().ok()?)));
            deliver(&tx, &wake, Done::Looked(stamp));
        });
    }

    /// The palette's colors followed: the configuration made once per palette; a change
    /// while the tab is open makes the page anew (now, or after the write under way).
    fn follow_theme(&mut self, palette: &Palette) {
        let key = theme_key(palette);
        if self.theme.as_ref().is_some_and(|(k, _)| *k == key) {
            return;
        }
        let first = self.theme.is_none();
        let made = Theme::from_palette(palette);
        self.session.set_config(theme::config(&made));
        self.theme = Some((key, made));
        if first {
            if let Some((_, theme)) = &self.theme {
                self.entry = Arc::from(page::entry(&i18n::locale(), theme.look()));
            }
        } else if self.session.dirty() {
            // What the page holds and hasn't saved isn't lost to new colors: made anew after
            // its next save (it saves each change at once).
            self.reload_after_save = true;
        } else {
            self.new_page();
        }
    }

    /// The install looked at again when it changed: our page written into it once it's
    /// there. `true` if what's shown changed.
    fn follow_install(&mut self) -> bool {
        let now = install::generation();
        if self.seen == Some(now) {
            return false;
        }
        self.seen = Some(now);
        let status = install::status();
        let changed = status != self.status;
        self.status = status;
        if let Status::Installed(dir) = &self.status
            && self.page_dir.as_ref() != Some(dir)
            && !self.writing_page
        {
            self.writing_page = true;
            let dir = Arc::clone(dir);
            let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
            kernel::work::run(FILE_THREAD, move || {
                let result = page::write(&dir).map_err(|e| e.to_string());
                deliver(&tx, &wake, Done::PageWritten { dir, result });
            });
        }
        changed
    }

    /// What came back, handled. `true` if anything did.
    fn drain(&mut self) -> bool {
        let mut any = false;
        while let Ok(done) = self.rx.try_recv() {
            any = true;
            match done {
                Done::Read { serial, path, result } => {
                    if serial != self.read_serial || self.path.as_ref() != Some(&path) {
                        continue;
                    }
                    match result {
                        // Not there yet: a new diagram.
                        Ok(xml) if !self.read_once => {
                            self.read_once = true;
                            let xml = xml.unwrap_or_else(|| Arc::from(""));
                            let effects = self.session.file_read(xml);
                            self.apply(effects);
                        }
                        Ok(Some(xml)) => {
                            self.quiet.now();
                            let effects = self.session.disk_changed(xml);
                            self.apply(effects);
                        }
                        // Gone since (deleted, moved): what the page shows stays.
                        Ok(None) => {}
                        Err(reason) => self.notifications.push(Notification::error(tr!(
                            "diagram_web.read_failed",
                            name = Self::title(&path),
                            reason = reason
                        ))),
                    }
                }
                Done::Written { xml, result } => {
                    let name = self.path.as_deref().map(Self::title).unwrap_or_default();
                    let effects = match result {
                        Ok(file::Wrote::Done) => self.session.written(&xml, true),
                        // Changed by someone else: theirs stays, ours beside it, told.
                        Ok(file::Wrote::Conflict { disk, kept }) => {
                            self.notifications.push(Notification::info(tr!(
                                "diagram_web.conflict",
                                name = name,
                                kept = Self::title(&kept)
                            )));
                            self.session.conflict(disk)
                        }
                        Err(reason) => {
                            self.notifications.push(Notification::error(tr!(
                                "diagram_web.write_failed",
                                name = name,
                                reason = reason
                            )));
                            self.session.written(&xml, false)
                        }
                    };
                    self.apply(effects);
                    if self.reload_after_save && !self.session.dirty() {
                        self.new_page();
                    }
                }
                Done::PageWritten { dir, result } => {
                    self.writing_page = false;
                    match result {
                        Ok(()) => self.page_dir = Some(dir),
                        Err(reason) => {
                            tracing::warn!(%reason, "draw.io's page wasn't written");
                            self.notifications.push(Notification::error(tr!(
                                "diagram_web.page_failed",
                                reason = reason
                            )));
                        }
                    }
                }
                Done::DiskChanged(path) => {
                    if self.read_once && self.path.as_ref() == Some(&path) {
                        self.read();
                    }
                }
                Done::Looked(stamp) => {
                    self.looking = false;
                    // Gone (deleted, moved without our hearing it): nothing read in its place.
                    if stamp.is_some() && self.stamp.is_some() && stamp != self.stamp {
                        self.read();
                    }
                    if stamp.is_some() {
                        self.stamp = stamp;
                    }
                }
                Done::Exported { path, result } => match result {
                    Ok(()) => self.notifications.push(Notification::success(tr!(
                        "diagram_web.exported",
                        name = Self::title(&path)
                    ))),
                    Err(reason) => self.notifications.push(Notification::error(tr!(
                        "diagram_web.write_failed",
                        name = Self::title(&path),
                        reason = reason
                    ))),
                },
                Done::Renamed { from, to } => self.renamed(&from, to),
            }
        }
        any
    }

    /// The offer's text, by where the install stands.
    fn offer_text(&self) -> String {
        let version = package::VERSION;
        match &self.status {
            Status::Looking => t("diagram_web.looking"),
            Status::Missing => tr!(
                "diagram_web.offer",
                version = version,
                size = i18n::bytes(package::DOWNLOAD_SIZE),
                disk = i18n::bytes(package::INSTALLED_SIZE)
            ),
            Status::Downloading { received, total } => tr!(
                "diagram_web.downloading",
                version = version,
                received = i18n::bytes(*received),
                total = i18n::bytes(*total)
            ),
            Status::Unpacking => t("diagram_web.unpacking"),
            Status::Failed { offline: true, .. } => t("diagram_web.offline"),
            Status::Failed { reason, .. } => tr!("diagram_web.failed", reason = reason.as_str()),
            Status::Installed(_) => t("diagram_web.opening"),
        }
    }

    fn button_list(&self) -> Vec<Button> {
        match self.status {
            Status::Missing => vec![Button::Download],
            Status::Downloading { .. } => vec![Button::Stop],
            Status::Failed { .. } => vec![Button::Retry],
            _ => Vec::new(),
        }
    }

    fn label(button: Button) -> String {
        t(match button {
            Button::Download => "diagram_web.download",
            Button::Stop => "diagram_web.stop",
            Button::Retry => "diagram_web.retry",
        })
    }

    fn button_at(&self, p: Point) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| r.contains(p)).map(|(_, b)| *b)
    }

    /// The page is shown (installed, our page written, a file to show).
    fn page_ready(&self) -> bool {
        self.path.is_some()
            && self.page_dir.is_some()
            && matches!(self.status, Status::Installed(_))
            && !self.entry.is_empty()
    }

    /// The offer, in the middle of the tab.
    fn paint_offer(&mut self, ctx: &mut ViewerPaint<'_>) {
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let b = ctx.bounds;
        let px = |v: f32| ds.px(v);
        let layer = ctx.scene.push_layer(Some(b));
        layer.quad(Quad::filled(b, p.editor_background));
        let width = px(440.0).min(b.size.width - px(32.0)).max(0.0);
        let x = (b.min_x() + (b.size.width - width) / 2.0).round();
        let mut y = (b.min_y() + b.size.height * 0.32).round();
        let title_font = ds.ui_font(2.0).with_weight(600);
        let font = ds.ui_font(-0.5);
        let small = ds.ui_font(-1.5);
        let text = &mut *ctx.text;
        // Its icon and title.
        let s = px(20.0);
        if let Some(id) = ctx.icons.id("file-pen") {
            draw_icon(layer, ctx.icons, id, Rect::new(x, y, s, s), p.accent);
        }
        let title = ds::ellipsize(text, &t("diagram_web.offer_title"), &title_font, width - s);
        let line = text.shape_line(&title, &title_font);
        draw_text(layer, &line, x + s + px(10.0), ds::optical_baseline(y, s, &title_font), p.text);
        y += s + px(14.0);
        // What it says, on as many lines as it needs (at most four).
        let said = self.offer_text();
        let color = if matches!(self.status, Status::Failed { .. }) { p.warning } else { p.text };
        for part in wrap(&said, 4, |l| text.shape_line(l, &font).width <= width) {
            let shown = ds::ellipsize(text, &part, &font, width);
            let line = text.shape_line(&shown, &font);
            draw_text(layer, &line, x, ds::optical_baseline(y, px(20.0), &font), color);
            y += px(20.0);
        }
        // The attribution, muted.
        y += px(4.0);
        let credit = ds::ellipsize(text, &t("diagram_web.attribution"), &small, width);
        let line = text.shape_line(&credit, &small);
        draw_text(layer, &line, x, ds::optical_baseline(y, px(18.0), &small), p.text_muted);
        y += px(30.0);
        // The progress.
        if let Status::Downloading { received, total } = self.status {
            let fraction = (total > 0).then(|| received as f32 / total as f32);
            ds::ProgressBar::from(fraction).paint_bar(layer, &ds, Rect::new(x, y, width, px(4.0)));
            y += px(16.0);
        } else if matches!(self.status, Status::Unpacking | Status::Looking) {
            ds::ProgressBar::unknown().paint_bar(layer, &ds, Rect::new(x, y, width, px(4.0)));
            y += px(16.0);
        }
        // The buttons.
        let mut buttons = Vec::new();
        let mut left = x;
        let h = px(28.0);
        let button_font = ds::button_font(&ds);
        for button in self.button_list() {
            let label = Self::label(button);
            let w = (text.shape_line(&label, &button_font).width + px(32.0)).round();
            let r = Rect::new(left, y, w, h);
            let hovered = self.hovered == Some(button);
            if button == Button::Stop {
                ds::secondary_button(layer, text, &ds, r, &label, true, hovered);
            } else {
                ds::primary_button(layer, text, &ds, r, &label, true, hovered);
            }
            buttons.push((r, button));
            left = r.max_x() + px(8.0);
        }
        self.buttons = buttons;
    }
}

/// Where a view's page stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageState {
    /// Being made, its content not drawn yet.
    Opening,
    /// Its content drawn.
    Shown,
    /// It can't be made here (no web engine).
    Failed,
}

/// `text` on at most `max` lines, as many words on each as `fits` allows (at least one).
fn wrap(text: &str, max: usize, mut fits: impl FnMut(&str) -> bool) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split(' ') {
        let tried = if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if current.is_empty() || fits(&tried) || lines.len() + 1 >= max {
            current = tried;
        } else {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

impl Viewer for DiagramWebView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<OpenData>() else { return };
        // Its own file (known from the tab's path): the project's events followed now.
        if self.path.as_ref() == Some(&data.path) {
            if self.subscriptions.is_empty()
                && let Some(events) = &data.events
            {
                self.follow(events);
            }
            return;
        }
        // Another file given: shown in its place, unless this one has unsaved changes (they're
        // written first; the user opens the other again).
        if self.path.is_some() && self.session.dirty() {
            return;
        }
        self.open(data.path.clone(), data.events.as_ref());
    }

    /// The file it shows renamed or moved (its tab is the file's own: draw.io the file's
    /// default).
    fn moved(&mut self, path: &Path) {
        if let Some(from) = self.path.clone() {
            self.renamed(&from, path.to_path_buf());
        }
    }

    /// The file changed on disk (its tab is the file's own): read again, merged in as a
    /// change heard from the project's events is.
    fn file_changed(&mut self) {
        if self.read_once {
            self.read();
        }
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        self.look_at_file();
        if let Some(waker) = &ctx.waker {
            let mut wake = self.wake.lock().unwrap_or_else(PoisonError::into_inner);
            if wake.is_none() {
                *wake = Some(Arc::clone(waker));
                drop(wake);
                install::follow(waker);
            }
        }
        // What wakes its window, for the sleeping's orders (taken at once, not at an unrelated
        // poll); with the keyboard, what "Put Tab to Sleep" acts on.
        if let Some(part) = &self.sleep {
            if let Some(waker) = &ctx.waker {
                part.give_waker(waker);
            }
            part.focus(ctx.focused);
        }
        self.follow_theme(ctx.palette);
        self.follow_install();
        self.drain();
        // Asleep or coming back: its picture and its card in its page's place.
        if self.paint_asleep(&mut ctx) {
            self.area = ctx.bounds;
            self.buttons.clear();
            return;
        }
        if self.page_ready() {
            self.area = ctx.bounds;
            self.buttons.clear();
            // Seen only while the page isn't over it: the loader while it opens, why it
            // never will; once drawn, only the background (what opens over a page later
            // uncovers no loader stopped mid-turn).
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let layer = ctx.scene.push_layer(Some(ctx.bounds));
            layer.quad(Quad::filled(ctx.bounds, ctx.palette.editor_background));
            match self.page {
                PageState::Opening => ds::loading_state(
                    layer,
                    ctx.text,
                    &ds,
                    ctx.bounds,
                    &t("diagram_web.opening"),
                    Some(&t("diagram_web.attribution")),
                ),
                PageState::Failed => ds::empty_state(
                    layer,
                    ctx.text,
                    &ds,
                    ctx.bounds,
                    &t("diagram_web.no_engine"),
                    Some(&t("diagram_web.no_engine_detail")),
                ),
                PageState::Shown => {}
            }
        } else {
            self.area = Rect::default();
            self.paint_offer(&mut ctx);
        }
    }

    fn web_page(&self) -> Option<WebPage> {
        let dir = self.page_dir.as_ref()?;
        let freeze = self.page_wanted()?;
        (self.page_ready() && self.area.size.height > 0.0 && self.area.size.width > 0.0).then(
            || WebPage {
                id: self.page_id,
                source: WebSource::App { root: Arc::clone(dir), entry: Arc::clone(&self.entry) },
                area: self.area,
                freeze,
            },
        )
    }

    fn web_event(&mut self, page: u128, event: WebEvent) {
        if page != self.page_id {
            return;
        }
        match event {
            WebEvent::Message(text) => {
                // A draw.io starting in a page that was drawn (made again after its tab was
                // hidden a while): its loader under it until it's drawn again.
                if self.page == PageState::Shown && text.contains(r#""event":"configure""#) {
                    self.page = PageState::Opening;
                }
                let effects = self.session.page_message(&text);
                tracing::debug!(bytes = text.len(), answers = effects.len(), "draw.io said");
                self.apply(effects);
            }
            WebEvent::OpenLink(url) if crate::protocol::web_link(&url) => {
                self.actions.push(ViewerAction::OpenLink(url));
            }
            WebEvent::OpenLink(_) => {}
            WebEvent::Shown => {
                self.page = PageState::Shown;
                self.page_shown();
            }
            WebEvent::Failed => {
                self.page = PageState::Failed;
                // Coming back and it can't be made: no longer shown as coming back.
                if let Slumber::Waking(picture) = std::mem::take(&mut self.slumber) {
                    if let Some(picture) = picture {
                        picture.release();
                    }
                    if let Some(part) = &mut self.sleep {
                        part.waking(false);
                    }
                }
            }
            WebEvent::Picture(picture) => self.got_picture(picture),
        }
    }

    fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        let id = self.page_id;
        let mut sends = std::mem::take(&mut self.sends);
        sends.retain(|(page, _)| *page == id);
        sends
    }

    fn poll(&mut self) -> bool {
        let installed = self.follow_install();
        let changed = self.drain() | installed;
        let slept = self.take_sleep_orders();
        self.tell_veto();
        changed | slept | !self.sends.is_empty() | !self.actions.is_empty()
    }

    /// An order to sleep or to wake waits (given while it's unseen): polled.
    fn wants_poll(&self) -> bool {
        self.sleep.as_ref().is_some_and(ui_components::sleep::SleepPart::order_waits)
    }

    /// Its tab chosen: touched (its time left alone starts again). Asleep, it stays so: only
    /// a key, a click in it or "Wake Up" wakes it.
    fn set_active(&mut self, active: bool) {
        if active {
            self.touch();
        }
    }

    /// Asleep, the keys are its own (any wakes it); else its page's.
    fn takes_keyboard(&self) -> bool {
        matches!(self.slumber, Slumber::Asleep { .. })
    }

    fn wants_text(&self) -> bool {
        self.takes_keyboard()
    }

    fn key_press(&mut self, _key: ui_components::KeyPress) -> bool {
        self.wake_by_user()
    }

    fn text_input(&mut self, _text: &str) -> bool {
        self.wake_by_user()
    }

    /// Its sleeping's rows ("Put to Sleep"; asleep, "Wake Up").
    fn tab_menu(&self) -> Vec<ui_components::ViewMenuEntry> {
        self.sleep_menu()
    }

    /// "Wake Up" brings its tab to the front first: shown, it comes back.
    fn tab_row_activates(&self, id: &str) -> bool {
        id == ui_components::sleep::WAKE_ACTION
    }

    fn pointer_down(&mut self, p: Point, _clicks: u32, _mods: PressMods) -> Option<ViewerAction> {
        // Asleep: a click wakes it.
        if self.wake_by_user() {
            return None;
        }
        match self.button_at(p)? {
            Button::Download | Button::Retry => install::start(),
            Button::Stop => install::stop(),
        }
        None
    }

    fn hover(&mut self, p: Point) -> bool {
        let on = self.button_at(p);
        std::mem::replace(&mut self.hovered, on) != on
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.button_at(p).map(|_| ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<Tip> {
        let (r, button) = self.buttons.iter().find(|(r, _)| r.contains(p))?;
        let label = match button {
            Button::Download | Button::Retry => tr!(
                "diagram_web.download_tip",
                path = install::root().map_or_else(String::new, |r| r.display().to_string())
            ),
            Button::Stop => t("diagram_web.stop_tip"),
        };
        Some(Tip::new(*r, label))
    }

    fn is_loading(&self) -> bool {
        // Only an unknown amount sweeps (the download wakes it as it goes); the page loading
        // turns the loader under it until it's drawn (coming back from a sleep: its card,
        // still).
        match self.page_ready() {
            true => self.page == PageState::Opening && !self.hidden && !self.sleeping(),
            false => matches!(self.status, Status::Unpacking | Status::Looking),
        }
    }

    /// The Save dialog's answer for an export: written there on the worker; "Put to Sleep",
    /// "Wake Up".
    fn command(&mut self, id: &str) -> bool {
        if let Some(done) = self.sleep_command(id) {
            return done;
        }
        let Some(path) = id.strip_prefix("picked-save:") else { return false };
        let Some(bytes) = self.export.take() else { return false };
        let path = PathBuf::from(path);
        let (tx, wake) = (self.tx.clone(), Arc::clone(&self.wake));
        kernel::work::run(FILE_THREAD, move || {
            let result = files::write_atomic(&path, &bytes).map_err(|e| e.to_string());
            deliver(&tx, &wake, Done::Exported { path, result });
        });
        true
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        (!self.actions.is_empty()).then(|| self.actions.remove(0))
    }

    fn take_notifications(&mut self) -> Vec<Notification> {
        std::mem::take(&mut self.notifications)
    }

    fn is_dirty(&self) -> bool {
        self.session.dirty()
    }

    /// The file's next look (while the tab is shown: its paints).
    /// The next look at the file, while its tab is seen (a look happens as it paints: a
    /// hidden tab's would be due forever, a frame asked for at every turn).
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        // Asleep, its picture's copy not made yet: a frame (the renderer makes it then; its
        // page is let go at the one after).
        if self.copy_waits() {
            return Some(std::time::Instant::now());
        }
        (self.read_once && !self.hidden)
            .then(|| self.looked_at.map_or_else(std::time::Instant::now, |t| t + LOOK_EVERY))
    }

    fn set_shown(&mut self, shown: bool) {
        self.hidden = !shown;
    }

    /// Saved now (the tab closing, the app quitting): the latest text the page sent, written
    /// here (a small file), never over someone else's change.
    fn save(&mut self) -> std::io::Result<()> {
        let (Some(path), Some(xml)) = (self.path.clone(), self.session.unsaved()) else {
            return Ok(());
        };
        let expected = self.session.on_disk();
        match file::write_over(&path, &xml, expected.as_deref()).map_err(std::io::Error::other)? {
            file::Wrote::Done => {
                let effects = self.session.written(&xml, true);
                self.apply(effects);
            }
            file::Wrote::Conflict { disk, kept } => {
                self.notifications.push(Notification::info(tr!(
                    "diagram_web.conflict",
                    name = Self::title(&path),
                    kept = Self::title(&kept)
                )));
                let effects = self.session.conflict(disk);
                self.apply(effects);
            }
        }
        Ok(())
    }

    fn tab_icon(&self) -> Option<&'static str> {
        let asleep = matches!(self.slumber, Slumber::Asleep { .. });
        Some(if asleep { ui_components::sleep::ASLEEP_ICON } else { "file-pen" })
    }

    fn memory(&self) -> ViewMemory {
        let held = self.session.xml().map_or(0, |x| x.len() as u64);
        ViewMemory { ram: held.saturating_mul(2), gpu: 0 }
    }
}

/// The tab closing with a write waiting (the page's latest text, not on disk yet): written
/// on the worker, never lost (a write under way finishes on its own).
impl Drop for DiagramWebView {
    fn drop(&mut self) {
        // Its picture (kept on the GPU till it's let go), let go with it.
        if let Slumber::Asleep { picture: Some(picture) } | Slumber::Waking(Some(picture)) =
            &self.slumber
        {
            picture.release();
        }
        if !self.session.dirty() {
            return;
        }
        let (Some(path), Some(xml)) = (self.path.clone(), self.session.unsaved()) else { return };
        let expected = self.session.on_disk();
        kernel::work::run(FILE_THREAD, move || {
            if let Err(e) = file::write_over(&path, &xml, expected.as_deref()) {
                tracing::warn!(error = %e, "a closed draw.io tab's diagram wasn't written");
            }
        });
    }
}

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
