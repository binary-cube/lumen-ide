//! File viewers contributed by modules (images, rendered markdown…): an
//! extension point like language intelligence. The app asks the providers whether they can
//! show a file; the returned viewer is drawn instead of the editor or next to it.

use std::fmt::Debug;
use std::path::Path;

use icons::Icons;
use kernel::{Point, Rect};
use render_api::Scene;
use text_layout::TextSystem;

use crate::palette::Palette;

/// How a file is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerKind {
    /// Only the viewer (the file isn't text: images).
    Replace,
    /// The source stays editable; the viewer is a preview of it (markdown), switchable
    /// Editor / Split / Preview.
    Preview,
}

/// When a provider is asked whether it shows a file, among the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum ViewerPriority {
    /// In the order the modules were registered.
    #[default]
    Normal,
    /// After every other one: what shows a file no one else does (the binary files'
    /// notice).
    Fallback,
}

/// A module's contribution: which files it can show and how.
pub trait ViewerProvider: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    /// When it's asked, among the others (the default: in the modules' order).
    fn priority(&self) -> ViewerPriority {
        ViewerPriority::Normal
    }
    /// The viewer kind for the file, or `None` if it can't show it.
    fn kind(&self, path: &Path) -> Option<ViewerKind>;
    /// The file's viewer. For [`ViewerKind::Preview`], the content comes from the editor
    /// via [`Viewer::set_source`].
    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>>;
}

/// What a viewer receives when painting.
pub struct ViewerPaint<'a> {
    pub bounds: Rect,
    pub scale: f32,
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
    /// It has the keyboard (a caret drawn solid, not outlined).
    pub focused: bool,
    /// Wakes the app (a redraw) from a background thread, e.g. when a decode finishes.
    pub waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for ViewerPaint<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerPaint")
            .field("bounds", &self.bounds)
            .field("scale", &self.scale)
            .field("focused", &self.focused)
            .finish_non_exhaustive()
    }
}

/// A page the system's web engine shows over part of a view ([`Viewer::web_page`]): the
/// app places it at `area` while the view is seen and nothing is open over it, hides it
/// otherwise, and lets it go when the view closes.
#[derive(Debug, Clone, PartialEq)]
pub struct WebPage {
    /// Which page it is (a new id: another page in its place).
    pub id: u128,
    pub source: WebSource,
    /// Where it goes (physical pixels, in the window): set as the view paints.
    pub area: Rect,
}

/// What a [`WebPage`] shows.
#[derive(Debug, Clone, PartialEq)]
pub enum WebSource {
    /// HTML given (a response's): shown as it is, no script runs, no link is followed.
    Html(std::sync::Arc<str>),
    /// A trusted app on this computer: the files of `root` (nothing outside it), its scripts
    /// running, no network. `entry`: the page opened first, relative to `root`, with its
    /// query (`index.html?embed=1`). It talks to its view by messages
    /// ([`Viewer::web_event`], [`Viewer::take_web_sends`]); another root or entry is another
    /// page.
    App { root: std::sync::Arc<std::path::Path>, entry: std::sync::Arc<str> },
}

/// What an app's page tells its view ([`Viewer::web_event`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebEvent {
    /// A message it sent (`window.ipc.postMessage(text)`).
    Message(String),
    /// A link it wanted opened in a new window: never opened by the engine; the view
    /// decides (a request to open it in the browser, or nothing).
    OpenLink(String),
}

/// What was requested by a click in the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewerAction {
    /// A link: an external URL or a path relative to the file.
    OpenLink(String),
    /// Opens a file (absolute path) in the editor, at the given line (0-based).
    OpenAt { path: std::path::PathBuf, line: u32 },
    /// Opens a file (absolute path) at a line and column as written (1-based), when given
    /// (a `file:line:column` link).
    OpenFile { path: std::path::PathBuf, line: Option<u32>, column: Option<u32> },
    /// Replaces lines `lines` (0-based, end inclusive) of file `path` with
    /// `text` (e.g. a declaration edited on the canvas).
    Replace { path: std::path::PathBuf, lines: std::ops::RangeInclusive<u32>, text: String },
    /// A request for the app (what only it can do: themes, the screen).
    Request(AppRequest),
    /// Apply a refactoring's chosen changes (the view's tab closes when it's done).
    ApplyRefactoring(refactor::Plan),
    /// Close the view's tab (Esc in a view that is a step, like a refactoring's preview).
    Close,
    /// Run a command (by id), as from its menu (e.g. a view's Pull / Push buttons).
    RunCommand(String),
    /// Replace file `path`'s whole text (its tab, opened if needed; unsaved, one undo step).
    SetText { path: std::path::PathBuf, text: String },
    /// Write `text` to file `path` as a save from its tab (opened if needed): the tab shows
    /// it and the save is announced like any other (the local history, a remote upload).
    SaveText { path: std::path::PathBuf, text: String },
    /// Open a comparison in a tab named by the second field.
    Compare(Box<diff::CompareInput>, String),
    /// Show a comparison beside the view (in the editor group on its right, split off if
    /// there's none), in place of the one shown there before; the view keeps the focus.
    CompareBeside(Box<diff::CompareInput>, String),
    /// A request to whoever answers it (another module); the answer isn't waited for.
    Ask(Asked),
    /// Open another view in a tab (as `WorkbenchHost::open_view`): the viewer contributed
    /// for its extension, given its data (e.g. a conflicted file's merge).
    OpenView(OpenView),
    /// Work only the host can start (a question to confirm), once the action reaches it.
    WithHost(WithHost),
}

/// Work a view hands to the host through its action (it has no host itself): done once.
/// Compared by identity.
#[derive(Clone)]
pub struct WithHost(WithHostSlot);

type WithHostJob = Box<dyn FnOnce(&mut dyn workbench_model::WorkbenchHost) + Send>;
type WithHostSlot = std::sync::Arc<std::sync::Mutex<Option<WithHostJob>>>;

impl WithHost {
    pub fn once(
        job: impl FnOnce(&mut dyn workbench_model::WorkbenchHost) + Send + 'static,
    ) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(Box::new(job)))))
    }

    /// Does the work, the first time it's asked.
    pub fn run(&self, host: &mut dyn workbench_model::WorkbenchHost) {
        let job = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(job) = job {
            job(host);
        }
    }
}

impl std::fmt::Debug for WithHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WithHost")
    }
}

impl PartialEq for WithHost {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for WithHost {}

/// Sends a request made by a view, once it reaches a host.
type AskIt = std::sync::Arc<dyn Fn(&mut dyn workbench_model::WorkbenchHost) + Send + Sync>;

/// A request a view makes through its action (it has no host). Compared by what it asks.
#[derive(Clone)]
pub struct Asked {
    request: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    text: String,
    run: AskIt,
}

impl Asked {
    pub fn new<R>(request: R) -> Self
    where
        R: kernel::Request + Clone + Sync + std::fmt::Debug,
    {
        let text = format!("{request:?}");
        let asked = request.clone();
        let run = move |host: &mut dyn workbench_model::WorkbenchHost| {
            let _ = host.ask(asked.clone());
        };
        Self { request: std::sync::Arc::new(request), text, run: std::sync::Arc::new(run) }
    }

    /// Sent to whoever answers it.
    pub fn send(&self, host: &mut dyn workbench_model::WorkbenchHost) {
        (self.run)(host);
    }

    /// The request, if it's an `R` (what a view's tests look at).
    pub fn request<R: 'static>(&self) -> Option<&R> {
        self.request.downcast_ref()
    }
}

impl std::fmt::Debug for Asked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for Asked {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Eq for Asked {}

/// A view to open in a tab: its title, the extension its viewer is found by, what it's
/// given. Compared by identity (its data is any module's).
#[derive(Clone)]
pub struct OpenView {
    pub title: String,
    pub extension: &'static str,
    pub data: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    /// In a modal over everything instead of a tab.
    pub modal: bool,
    /// Told when no viewer takes it (its module turned off).
    pub unavailable: Option<String>,
}

impl OpenView {
    pub fn new(
        title: impl Into<String>,
        extension: &'static str,
        data: impl std::any::Any + Send + Sync,
    ) -> Self {
        Self::shared(title, extension, std::sync::Arc::new(data))
    }

    /// The same, for data already shared.
    pub fn shared(
        title: impl Into<String>,
        extension: &'static str,
        data: std::sync::Arc<dyn std::any::Any + Send + Sync>,
    ) -> Self {
        Self { title: title.into(), extension, data, modal: false, unavailable: None }
    }

    /// `message` told when no viewer takes it.
    #[must_use]
    pub fn or_tell(mut self, message: impl Into<String>) -> Self {
        self.unavailable = Some(message.into());
        self
    }

    /// The same, in a modal over everything.
    pub fn modal(
        title: impl Into<String>,
        extension: &'static str,
        data: impl std::any::Any + Send + Sync,
    ) -> Self {
        Self { modal: true, ..Self::new(title, extension, data) }
    }
}

impl std::fmt::Debug for OpenView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenView")
            .field("title", &self.title)
            .field("extension", &self.extension)
            .finish_non_exhaustive()
    }
}

impl PartialEq for OpenView {
    fn eq(&self, other: &Self) -> bool {
        self.title == other.title
            && self.extension == other.extension
            && self.modal == other.modal
            && std::sync::Arc::ptr_eq(&self.data, &other.data)
    }
}

impl Eq for OpenView {}

/// A button a modal view puts in its window's footer, right of the keys (the window draws
/// it, as every modal's): a click on it comes back to the view as [`Viewer::command`] `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalButton {
    pub id: &'static str,
    pub label: String,
    /// The main one: in the accent, with `↵` (the view's Enter does it).
    pub main: bool,
    /// What can't be undone (discarding changes): in the danger color.
    pub danger: bool,
    pub enabled: bool,
    /// A `|` between it and the buttons on its right: a group of its own (Test Connection
    /// apart from Cancel / Apply / OK).
    pub separated: bool,
    /// The keys are on it (the view moves them with ⇥ / ← →; ↵ presses it): ringed.
    pub focused: bool,
    /// A main one pressed only by a click: no `↵` on it, the view's Enter doesn't press it
    /// (what the user must choose to do: a push).
    pub keyless: bool,
}

impl ModalButton {
    pub fn main(id: &'static str, label: impl Into<String>, enabled: bool) -> Self {
        Self {
            id,
            label: label.into(),
            main: true,
            danger: false,
            enabled,
            separated: false,
            focused: false,
            keyless: false,
        }
    }

    pub fn other(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            main: false,
            danger: false,
            enabled: true,
            separated: false,
            focused: false,
            keyless: false,
        }
    }

    pub fn danger(id: &'static str, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            main: false,
            danger: true,
            enabled: true,
            separated: false,
            focused: false,
            keyless: false,
        }
    }

    /// The same, enabled or not.
    #[must_use]
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The same with the keys on it, or not (see [`Self::focused`]).
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// The same in the danger color (a main one keeps its `↵`): what only takes away.
    #[must_use]
    pub fn dangerous(mut self, danger: bool) -> Self {
        self.danger = danger;
        self
    }

    /// The same, pressed only by a click (see [`Self::keyless`]).
    #[must_use]
    pub fn keyless(mut self) -> Self {
        self.keyless = true;
        self
    }

    /// The same, apart from the buttons on its right (see [`Self::separated`]).
    #[must_use]
    pub fn separated(mut self) -> Self {
        self.separated = true;
        self
    }
}

/// What a modal's footer says on its left instead of its keys (cut to its room, whole in
/// its tooltip).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModalStatus {
    /// Something running (a spinner beside it).
    Busy(String),
    /// A question or a warning before the main button ("1327 items change").
    Warning(String),
    /// Something that went well ("Connected to PostgreSQL 16 in 42 ms"): a check beside it.
    Done(String),
    /// Something that failed: in the danger color, an alert beside it.
    Failed(String),
    /// A plain note, muted ("Test cancelled").
    Note(String),
}

/// What a viewer can ask the app for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppRequest {
    /// The eyedropper: a color picked from the screen comes back to the viewer.
    SampleScreen,
    /// The theme being built (its TOML), shown in this window only, not saved.
    PreviewTheme(String),
    /// The configurator closes without saving: the previous theme comes back.
    CancelTheme,
    /// Save the theme (its TOML) for every project, or for the open one.
    SaveTheme { source: String, global: bool },
    /// Show the file in the system's file manager (Finder, Explorer).
    Reveal(std::path::PathBuf),
    /// Open this page (an `.html` file the view wrote) in the system's browser.
    OpenInBrowser(std::path::PathBuf),
    /// Open the project at this folder (asking where, if this window has one).
    OpenProject(std::path::PathBuf),
    /// The system's folder picker (opened at this folder): the chosen one comes back to the
    /// viewer as the command `picked-folder:<path>`.
    PickFolder(Option<std::path::PathBuf>),
    /// The system's file picker (opened at this folder): the chosen file comes back as the
    /// command `picked-file:<path>` — to the side panel that takes it, else to the viewer
    /// (nothing when it's dismissed).
    PickFile(Option<std::path::PathBuf>),
    /// The system's "Save As" dialog, opened at folder `start` with `name` filled in: the
    /// path chosen comes back to the viewer as the command `picked-save:<path>` (nothing
    /// when it's dismissed).
    PickSavePath { start: Option<std::path::PathBuf>, name: String },
    /// Replace `range` of the active file's text with `text`, selected after (one undo step;
    /// the editor gets the keyboard back): e.g. a color picked for a literal.
    ReplaceInActive { range: std::ops::Range<usize>, text: String },
    /// Show this folder in the project tree, expanded.
    ShowFolder(std::path::PathBuf),
    /// Open the settings at this page (its id).
    OpenSettings(String),
    /// Bring this window's project to the front (another of the app's windows).
    FocusProject(u64),
    /// The window takes the whole screen while this bare view is open; left again when it
    /// closes (if it wasn't full screen before).
    FullScreenWhileOpen,
    /// Put this text on the system's clipboard (a grid's rows copied as JSON, from its menu).
    CopyText(String),
    /// Put this picture on the system's clipboard (a diagram copied as an image): its
    /// width, height and straight-alpha RGBA pixels.
    CopyImage { width: usize, height: usize, rgba: std::sync::Arc<Vec<u8>> },
    /// The language version chosen in the level picker (for the project or the file).
    SetLevel(crate::LevelPick),
    /// The answer to "still running — close anyway?" (the closing's question, a module's
    /// modal): the closing waiting on it goes on, waits or stops.
    RunningAnswer(workbench_model::CloseAnswer),
    /// Quit as Quit does (asking what it asks), then put the update staged in this folder
    /// in the app's place and start it again.
    RestartToUpdate(std::path::PathBuf),
}

/// Given to a view made again from a saved session ([`Viewer::set_data`]): the project
/// it's in.
#[derive(Clone, Default)]
pub struct Restored {
    pub project: Option<std::path::PathBuf>,
    /// The project's identity (what a view keeps per project, it keeps by this).
    pub id: Option<kernel::ProjectId>,
    /// What it said it needs to be made again ([`Viewer::memento`]).
    pub memento: Option<String>,
    /// The project's queries (a bottom window's view: the files' icons by language).
    pub queries: Option<std::sync::Arc<dyn workbench_model::ProjectQueries>>,
}

impl std::fmt::Debug for Restored {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restored")
            .field("project", &self.project)
            .field("id", &self.id)
            .field("memento", &self.memento)
            .finish_non_exhaustive()
    }
}

/// A key as pressed, for a view taking the whole keyboard ([`Viewer::takes_keyboard`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressedKey {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1–F12.
    F(u8),
}

/// A key press and the modifiers held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyPress {
    pub key: PressedKey,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl KeyPress {
    /// The editing key it is and how far it reaches (by word: ⌥ on macOS, Ctrl elsewhere;
    /// ⇧ held), as the editing keys reach a view that doesn't take the keyboard. `None`:
    /// not an editing key (a character, a function key).
    pub fn viewer_key(self) -> Option<(ViewerKey, crate::tool_panel::KeyReach)> {
        let key = match self.key {
            PressedKey::Enter => ViewerKey::Enter,
            PressedKey::Backspace => ViewerKey::Backspace,
            PressedKey::Delete => ViewerKey::Delete,
            PressedKey::Tab => ViewerKey::Tab,
            PressedKey::Escape => ViewerKey::Escape,
            PressedKey::Up => ViewerKey::Up,
            PressedKey::Down => ViewerKey::Down,
            PressedKey::Left => ViewerKey::Left,
            PressedKey::Right => ViewerKey::Right,
            PressedKey::Home => ViewerKey::Home,
            PressedKey::End => ViewerKey::End,
            PressedKey::PageUp => ViewerKey::PageUp,
            PressedKey::PageDown => ViewerKey::PageDown,
            PressedKey::Char(_) | PressedKey::Insert | PressedKey::F(_) => return None,
        };
        let word = if cfg!(target_os = "macos") { self.alt } else { self.ctrl };
        // ⇧ as the app passes it: extending a selection, ⇧↵ / ⇧⇥ backwards.
        Some((key, crate::tool_panel::KeyReach { word, extend: self.shift }))
    }
}

/// An editing key for viewers that receive text (search, editing on the canvas).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerKey {
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// A cursor a viewer asks for over one of its parts (otherwise: the pointer over a link,
/// the arrow elsewhere).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewerCursor {
    /// The plain arrow: nothing to click or select there.
    Arrow,
    Pointer,
    /// Over text that can be selected (an I-beam).
    Text,
    /// A separator between columns.
    ResizeHorizontal,
    /// A separator between rows (dragged up and down).
    ResizeVertical,
}

/// The modifiers held with a mouse press in a view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PressMods {
    pub shift: bool,
    /// ⌘ on macOS, Ctrl elsewhere.
    pub primary: bool,
    pub alt: bool,
}

/// A row of a view's right-click menu ([`Viewer::context_menu`]): shown by the app as its
/// own menus are; the chosen row's id comes back to the view as a [`Viewer::command`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewMenuEntry {
    Item {
        /// What the view is sent ([`Viewer::command`]).
        id: String,
        label: String,
        /// As the system shows it (`⌘C`).
        shortcut: Option<String>,
        icon: Option<&'static str>,
        /// How it stands: greyed out, checked, a color in its icon's place.
        look: RowLook,
    },
    /// A row opening more rows beside it.
    Submenu {
        label: String,
        icon: Option<&'static str>,
        entries: Vec<ViewMenuEntry>,
    },
    Separator,
}

/// How a menu's row stands, beside what it says: greyed out (shown, never run: what isn't
/// offered here yet), checked (a ✓ at its end: the choice made), a color drawn as a dot in
/// its icon's place (`0xRRGGBB`: a label's color), in the danger color (what can't be
/// undone: a removal).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RowLook {
    pub disabled: bool,
    pub checked: bool,
    pub swatch: Option<u32>,
    pub danger: bool,
}

impl ViewMenuEntry {
    pub fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Item {
            id: id.into(),
            label: label.into(),
            shortcut: None,
            icon: None,
            look: RowLook::default(),
        }
    }

    /// The same row greyed out when `disabled` (shown, never run).
    pub fn disabled_if(mut self, disabled: bool) -> Self {
        if let Self::Item { look, .. } = &mut self {
            look.disabled = disabled;
        }
        self
    }

    /// The same row with a ✓ at its end when `checked`.
    pub fn checked_if(mut self, checked: bool) -> Self {
        if let Self::Item { look, .. } = &mut self {
            look.checked = checked;
        }
        self
    }

    /// The same row in the danger color when `danger` (what can't be undone: a removal).
    pub fn danger_if(mut self, danger: bool) -> Self {
        if let Self::Item { look, .. } = &mut self {
            look.danger = danger;
        }
        self
    }

    /// The same row with a dot of color `rgb` (`0xRRGGBB`) in its icon's place.
    pub fn with_swatch(mut self, rgb: u32) -> Self {
        if let Self::Item { look, .. } = &mut self {
            look.swatch = Some(rgb);
        }
        self
    }

    /// A row opening `entries` beside it.
    pub fn submenu(label: impl Into<String>, entries: Vec<ViewMenuEntry>) -> Self {
        Self::Submenu { label: label.into(), icon: None, entries }
    }

    /// The same row with an icon.
    pub fn with_icon(mut self, name: &'static str) -> Self {
        match &mut self {
            Self::Item { icon, .. } | Self::Submenu { icon, .. } => *icon = Some(name),
            Self::Separator => {}
        }
        self
    }

    /// The same row with its keys shown.
    pub fn with_shortcut(mut self, keys: impl Into<String>) -> Self {
        if let Self::Item { shortcut, .. } = &mut self {
            *shortcut = Some(keys.into());
        }
        self
    }
}

/// What a view holds, for the Task Manager (on its module's row).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewMemory {
    /// Memory (bytes): decoded images, what it laid out.
    pub ram: u64,
    /// GPU memory (bytes): the textures it uploaded.
    pub gpu: u64,
}

pub trait Viewer: Send {
    /// What it holds now (an estimate it keeps as it goes: asked once a second while the
    /// Task Manager is open).
    fn memory(&self) -> ViewMemory {
        ViewMemory::default()
    }
    /// The source text (for the preview), on open and after every change.
    fn set_source(&mut self, _source: &str) {}
    /// Its tab became visible (`true`) or hidden (`false`): another tab activated in its
    /// group, the tab moved, its group closed or merged, the window hidden. Told by the app
    /// only when it changes (a view starts hidden until told); a view that works in the
    /// background while shown (the Task Manager's sampling) pauses on `false`.
    fn set_shown(&mut self, _shown: bool) {}
    fn paint(&mut self, ctx: ViewerPaint<'_>);
    /// Scroll (physical pixels); `true` if something changed.
    fn scroll(&mut self, _dx: f32, _dy: f32) -> bool {
        false
    }
    /// Zoom (e.g. ⌘ + scroll on images): `factor` > 1 enlarges, around `at`.
    fn zoom(&mut self, _factor: f32, _at: Point) -> bool {
        false
    }
    /// The zoom level to display (1 = 100%), if the viewer has zoom.
    fn zoom_level(&self) -> Option<f32> {
        None
    }
    /// Zoom to 100%. `true` if something changed.
    fn reset_zoom(&mut self) -> bool {
        false
    }
    fn click(&mut self, _p: Point) -> Option<ViewerAction> {
        None
    }
    /// A press in a view shown in a tab, `clicks` in a row, with the modifiers held (a
    /// grid's ⇧ / ⌘ selection); by default, a [`Self::click`] (what tabs always sent).
    fn pointer_down(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        let _ = (clicks, mods);
        self.click(p)
    }
    /// A right click at `p`: the menu to show there (`None`: none of its own). The chosen
    /// row's id comes back as a [`Self::command`].
    fn context_menu(&mut self, _p: Point) -> Option<Vec<ViewMenuEntry>> {
        None
    }
    /// A menu it asked for under a button of its own (a row's "⋮", "Export ▾"), taken by
    /// the app after a press or a command: shown under `anchor` as its right-click menu is,
    /// the row chosen coming back as a [`Self::command`]. `None`: none asked.
    fn take_menu(&mut self) -> Option<(Rect, Vec<ViewMenuEntry>)> {
        None
    }
    /// ⌘A in it (when it takes no text): `true` if it selected everything it shows.
    fn select_all(&mut self) -> bool {
        false
    }
    /// A preview that reads its file itself (a data grid) takes no text
    /// ([`Self::set_source`]): it's told whether the editor's text has unsaved changes
    /// ([`Self::set_unsaved`]) instead.
    fn reads_file(&self) -> bool {
        false
    }
    /// Shown inside a panel (a tab of the Services window): the view kind its actions' keys
    /// are bound to (`When::View`), so they work there as in its own tab; `None`: it has none.
    fn actions_view(&self) -> Option<&'static str> {
        None
    }
    /// The breadcrumb of its tab (`Database Consoles › source › console › profile`), in
    /// place of its title; empty: its title.
    fn crumbs(&self) -> Vec<String> {
        Vec::new()
    }
    /// Its file was renamed or moved to `path` (the tab follows): what it holds of the old
    /// path follows too (a data view's file, what it watches).
    fn moved(&mut self, _path: &Path) {}
    /// Its file changed on disk (another program, a checkout): a view that shows the file in
    /// its tab's place reads it again — off the UI thread — unless what's there is what it
    /// wrote itself. The text editor's tabs follow on their own.
    fn file_changed(&mut self) {}
    /// The editor's text of its file has unsaved changes (`true`), or none (a preview
    /// that [`Self::reads_file`]: told on every paint).
    fn set_unsaved(&mut self, _unsaved: bool) {}
    /// Shown beside the text (Split), it takes the keys once clicked (a grid: arrows,
    /// typing over a cell, ⌘C, ⌘F…) until the text is clicked again. A view that doesn't
    /// leaves every key to the editor (a Markdown preview).
    fn keys_in_split(&self) -> bool {
        false
    }
    /// Shown beside the text (Split), once clicked it takes Select All and Copy (its own
    /// selected text, [`Self::select_all`], [`Self::selected_text`]) until the text is
    /// clicked again; every other key stays the editor's (a Markdown preview).
    fn selects_in_split(&self) -> bool {
        false
    }
    /// Shown beside its text (Split): the source line (0-based, fractional) at its top, for
    /// the text to follow it; `None`: it doesn't scroll with its text (none of its own, or
    /// turned off).
    fn source_line_at_top(&self) -> Option<f32> {
        None
    }
    /// Its text scrolled: source line `line` (fractional) brought to its top. `true` if it
    /// moved.
    fn scroll_to_source(&mut self, line: f32) -> bool {
        let _ = line;
        false
    }
    /// The text's caret moved to source line `line`: brought into view only if it's out of
    /// it. `true` if it moved.
    fn reveal_source_line(&mut self, line: f32) -> bool {
        let _ = line;
        false
    }
    /// A preview shown alone when its file opens (the editor a toggle away).
    fn opens_alone(&self) -> bool {
        false
    }
    /// A press of the mouse button, `clicks` in a row (2: a double click), e.g. to select
    /// text; by default, a [`Self::click`].
    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        let _ = clicks;
        self.click(p)
    }
    /// A press in a view of a bottom window, with the modifiers held (a list's ⇧ / ⌘
    /// selection); by default, a [`Self::press`].
    fn press_with(&mut self, p: Point, clicks: u32, mods: PressMods) -> Option<ViewerAction> {
        let _ = mods;
        self.press(p, clicks)
    }
    /// While ⌘ / Ctrl is held: the link under `p` shown as one (`None`: not held, or the
    /// mouse elsewhere). `true` if it changed.
    fn link_hover(&mut self, _p: Option<Point>) -> bool {
        false
    }
    /// ⌘ / Ctrl + click: what the link under `p` opens.
    fn link_at(&self, _p: Point) -> Option<ViewerAction> {
        None
    }
    /// It takes the whole keyboard while it has it (the terminal): every key press
    /// ([`Self::key_press`]) and typed text ([`Self::text_input`]); only the app's shortcuts
    /// (⌘ on macOS, Ctrl+Shift elsewhere) go past it.
    fn takes_keyboard(&self) -> bool {
        false
    }
    /// A key press while it takes the whole keyboard: by default its editing keys go on as
    /// they would otherwise ([`Self::edit_key_with`]: ↵ in a name field, ⌫, the arrows), so a
    /// view that takes the keyboard only to keep the app's one-key shortcuts away loses none
    /// of them. A view that reads every key itself (the terminal) says so here.
    fn key_press(&mut self, key: KeyPress) -> bool {
        match key.viewer_key() {
            Some((key, reach)) => self.edit_key_with(key, reach),
            None => false,
        }
    }
    /// The text selected in it (Copy takes it), if any.
    fn selected_text(&self) -> Option<String> {
        None
    }
    fn clear_selection(&mut self) -> bool {
        false
    }
    /// Paste into it. `true` if it took the text.
    fn paste(&mut self, _text: &str) -> bool {
        false
    }
    /// Reopened at the next start, in its window (the terminal's shells).
    fn restorable(&self) -> bool {
        false
    }
    /// The name its module finds it by in its bottom window, the same in every language and
    /// whatever its tab is called (Git's Log: `git.log`); `None`: the name it was opened by.
    fn panel_key(&self) -> Option<&'static str> {
        None
    }
    /// What a restorable view needs to be made again, kept with the session and given back
    /// in [`Restored::memento`] (a file's history: its path); `None`: nothing but its project.
    fn memento(&self) -> Option<String> {
        None
    }
    /// Its tab can be closed. A bottom window's fixed tab (Git's Log and Changes) says no:
    /// no ×, no Close in its menu, left alone by Close Others / Left / Right / All and Close
    /// Group; it still moves (dragged, split) like any other.
    fn closable(&self) -> bool {
        true
    }
    /// It has ended on its own (its shell exited): its tab closes, shown or not.
    fn closed(&self) -> bool {
        false
    }
    /// The mouse dragged with the button pressed (after `click`), e.g. pan. `true` = needs a
    /// redraw.
    fn drag(&mut self, _p: Point) -> bool {
        false
    }
    /// The mouse button released.
    fn release(&mut self) -> bool {
        false
    }
    /// `true` while the viewer receives typed text (one of its fields or editors).
    fn wants_text(&self) -> bool {
        false
    }
    /// The field with the keyboard takes several lines (a commit's message): what's pasted
    /// comes whole, not only its first line (as a one-line field gets it).
    fn field_takes_lines(&self) -> bool {
        false
    }
    /// Typed text (when [`Viewer::wants_text`]).
    fn text_input(&mut self, _text: &str) -> bool {
        false
    }
    /// An editing key. `true` if it used it.
    fn edit_key(&mut self, _key: ViewerKey) -> bool {
        false
    }
    /// An editing key and how far it reaches in a text field (by word, extending the
    /// selection); a view without text fields gets [`Self::edit_key`].
    fn edit_key_with(&mut self, key: ViewerKey, reach: crate::tool_panel::KeyReach) -> bool {
        let _ = reach;
        self.edit_key(key)
    }
    /// Clears what can be rebuilt when needed (e.g. decoded images), on "Clear and Rebuild
    /// Caches".
    fn purge(&mut self) {}
    /// In a tab: it can be renamed (a session's terminal), its tab's menu offering it.
    fn renamable(&self) -> bool {
        false
    }
    /// Renamed `title` by the user (one line, trimmed, not empty): what it asks then.
    fn rename(&mut self, title: &str) -> Option<ViewerAction> {
        let _ = title;
        None
    }
    /// In a tab: its icon in place of its file's (an agent's session: the agent's), one the
    /// app or its module has. Asked as the tabs are drawn: kept by the view.
    fn tab_icon(&self) -> Option<&'static str> {
        None
    }
    /// In a tab: the name it goes by now, in place of the one it was opened with (a
    /// terminal's program's title); `None`: that one. Asked as the tabs are drawn: kept by
    /// the view, never made here.
    fn live_title(&self) -> Option<&str> {
        None
    }
    /// In a modal window: the line under its title.
    fn subtitle(&self) -> Option<String> {
        None
    }
    /// In a modal window: the keys shown in its footer (the keys, what they do), before
    /// "esc: close".
    fn key_hints(&self) -> Vec<(Vec<&'static str>, String)> {
        Vec::new()
    }
    /// In a modal window: the height its content needs (logical pixels), the window then
    /// as tall as that (at most its size); `None`: the window's full height.
    fn modal_height(&self) -> Option<f32> {
        None
    }
    /// In a modal window: the narrowest its content reads at (logical pixels), when it's a
    /// short one (a question): the window then as wide as that or as its title needs, never
    /// wider than its size; `None`: the size's width.
    fn modal_width(&self) -> Option<f32> {
        None
    }
    /// With [`Self::modal_width`]: whether a long title widens the window (`true`, as a
    /// question's); `false` keeps it as wide as asked, the title cut with `…`.
    fn modal_title_widens(&self) -> bool {
        true
    }
    /// In a modal window: the icon before its title (a name from the app's set).
    fn modal_icon(&self) -> Option<&'static str> {
        None
    }
    /// The color its tab carries (a data source's: production in red, development in
    /// green): a dot on the tab's icon and a thin strip along the view's top, drawn by the
    /// app. Asked at each paint: kept, never worked out there.
    fn accent(&self, palette: &Palette) -> Option<kernel::Rgba> {
        let _ = palette;
        None
    }
    /// In a modal window: it can be closed from its header (`esc`); `false` while work under
    /// way mustn't be left (nothing to click there; the view ignores Esc itself).
    fn modal_closable(&self) -> bool {
        true
    }
    /// In a modal window: asked when its × is clicked; `false` keeps it open (the view asks
    /// first — unsaved changes — and closes itself once answered).
    fn may_close(&mut self) -> bool {
        true
    }
    /// In a modal window: the view draws the footer itself (its keys beside its buttons),
    /// at the bottom of its area; the window adds none.
    fn draws_footer(&self) -> bool {
        false
    }
    /// In a modal window: its footer's buttons, right of the keys, the first the rightmost
    /// (none by default: only the keys).
    fn modal_buttons(&self) -> Vec<ModalButton> {
        Vec::new()
    }
    /// In a modal window: what its footer says instead of the keys (`None`: the keys).
    fn modal_status(&self) -> Option<ModalStatus> {
        None
    }
    /// After the app was woken (by [`ViewerPaint::waker`]): `true` if the viewer has
    /// something new to draw (e.g. a finished background decode).
    fn poll(&mut self) -> bool {
        false
    }
    /// Background work is still going on (a comparison's differences): what it shows isn't
    /// complete yet.
    fn working(&self) -> bool {
        false
    }
    /// An app command sent to the active viewer (e.g. `"search"` on ⌘K).
    fn command(&mut self, _id: &str) -> bool {
        false
    }
    /// What the view tells the user (a change submitted, an export written, why something
    /// couldn't be), taken by the app and shown as its notifications (toasts): a view never
    /// draws them itself.
    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        Vec::new()
    }
    /// An action produced by keys / commands (e.g. editing finished), picked up by the app.
    fn take_action(&mut self) -> Option<ViewerAction> {
        None
    }
    /// Data from the app (e.g. the dependency graph for the canvas), of a type known to the
    /// viewer.
    fn set_data(&mut self, _data: &dyn std::any::Any) {}
    /// Its tab split into another group (a second instance of the view, a mirror): what
    /// that one is given ([`Self::set_data`]) to show the same; `None`: nothing (it starts
    /// as its provider opens it).
    fn mirror_data(&self) -> Option<Box<dyn std::any::Any>> {
        None
    }
    /// The tooltip of the item under the mouse (e.g. a tool strip button): the area and the text.
    fn tooltip_at(&self, _p: Point) -> Option<crate::tooltip::Tip> {
        None
    }
    /// An animation is running (a separator's hover): frames are drawn until it ends.
    fn is_animating(&self) -> bool {
        false
    }
    /// Work is going on and a loader turns (a log read, a transfer): frames come at a
    /// loader's pace (a few a second), not the display's, for as long as it lasts.
    fn is_loading(&self) -> bool {
        false
    }
    /// An animation at its own pace (a full-window scene at 30 or 60 frames a second, not
    /// the display's): when its next frame is due. `None`: none asked.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        None
    }
    /// Opened bare over the window, it draws every pixel of it: nothing behind is drawn
    /// while it's open (and the views behind are told they're hidden).
    fn covers_window(&self) -> bool {
        false
    }
    /// Opened as a popover: it takes the wheel now (the mouse is over its list, as its last
    /// `hover` saw); `false` (the default) lets the scroll go on to what's under it.
    fn takes_scroll(&self) -> bool {
        false
    }
    /// The cursor over `p`, if the viewer wants a particular one there.
    fn cursor_at(&self, _p: Point) -> Option<ViewerCursor> {
        None
    }
    /// A page it has the system's web engine show over part of it (a response's HTML, an
    /// app), as of its last paint. `None`: none.
    fn web_page(&self) -> Option<WebPage> {
        None
    }
    /// What its app's page `page` (a [`WebPage::id`]) told it, in the order told, before
    /// its next paint.
    fn web_event(&mut self, _page: u128, _event: WebEvent) {}
    /// The messages it has for its app's pages (page id, text), taken by the app after each
    /// paint and handed to the page's `window.__hostReceive`; one for a page not shown yet
    /// is let go.
    fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        Vec::new()
    }
    /// `true` if something clickable is under the mouse (the cursor becomes a hand) — or,
    /// for a view whose [`Self::hover_tells_changes`] is `true`, if what it shows changed
    /// (its hand then comes from [`Self::cursor_at`]).
    fn hover(&mut self, _p: Point) -> bool {
        false
    }
    /// Its [`Self::hover`] says whether what it shows changed (a redraw), not whether
    /// something clickable is under the mouse: a tab showing it redraws only then, not at
    /// each move of the mouse over it.
    fn hover_tells_changes(&self) -> bool {
        false
    }
    /// A key (character) with the viewer focused, e.g. `+`, `-`, `0`, `r`.
    fn key(&mut self, _key: char) -> bool {
        false
    }
    /// Has unsaved changes (e.g. a rotated image).
    fn is_dirty(&self) -> bool {
        false
    }
    /// Shown alone in its tab, it edits what it shows and can be locked (read-only) like a
    /// text: the lock in the status bar, the tab's menu. A text's tab (no view, or a
    /// preview beside the text) always can; another view only when it says so.
    fn lockable(&self) -> bool {
        false
    }
    /// Locked (`true`) or unlocked: while locked it refuses every edit (what it shows can
    /// still be selected, copied, searched).
    fn set_locked(&mut self, _locked: bool) {}
    /// Saves the changes to the file.
    fn save(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    /// Info for the status bar (e.g. `1200 × 800 · 245 KB`).
    fn status(&self) -> Option<String> {
        None
    }
}
