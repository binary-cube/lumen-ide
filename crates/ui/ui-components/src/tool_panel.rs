//! A side panel's content (e.g. "Structure"), made by its tool window
//! ([`crate::ToolWindowProvider::create`]). The app gives them the editor context (the file, the symbols, the cursor,
//! the problems, the usages), and the panel requests navigation via a [`ToolPanelAction`].

use std::fmt::Debug;
use std::ops::Range;
use std::path::{Path, PathBuf};

use code_intelligence::FileSymbols;
use kernel::{Point, Rect, Rgba};

use crate::viewer::ViewerPaint;

/// A problem in the file (syntax error, warning).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub range: Range<usize>,
    pub line: u32,
    pub message: String,
}

/// A place in the project where the symbol at the cursor is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    /// Relative to the project root.
    pub path: PathBuf,
    /// The line (0-based).
    pub line: u32,
}

/// A text with colored ranges (relative to it).
pub type ColoredText<'a> = (&'a str, &'a [(Range<usize>, Rgba)]);

/// What a panel sees from the editor. Built only when something changed ([`Self::key`]).
pub struct PanelContext<'a> {
    pub path: Option<&'a Path>,
    pub file: Option<&'a FileSymbols>,
    /// What its language writes properties after (`$` in PHP), for their labels.
    pub sigil: Option<char>,
    pub cursor: usize,
    /// The line (0-based) of a document offset.
    pub line_of: &'a dyn Fn(usize) -> u32,
    /// The header of the declaration at the cursor (up to `{`), with syntax colors (ranges
    /// relative to the text).
    pub header: Option<ColoredText<'a>>,
    pub problems: &'a [Problem],
}

impl std::fmt::Debug for PanelContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PanelContext")
            .field("path", &self.path)
            .field("cursor", &self.cursor)
            .field("problems", &self.problems.len())
            .finish_non_exhaustive()
    }
}

/// What a click in the panel requested.
#[derive(Debug, Clone, PartialEq)]
pub enum ToolPanelAction {
    /// Moves the cursor to an offset in the active document.
    Reveal(usize),
    /// Opens a project file at a line.
    Open { path: PathBuf, line: u32 },
    /// What a view can ask for (a file at a line and column, a command run, a request).
    View(crate::viewer::ViewerAction),
    /// It's about to search the project: its files indexed, a fresh `ProjectView` given.
    RefreshProject,
    /// Entry `from` dropped on folder `into`: moved, as a refactoring (previewed).
    Move { from: PathBuf, into: PathBuf },
    /// Entry `from` renamed to `to` (in its folder): as a tab's file is renamed — a class's
    /// file as a refactoring (previewed), the tabs of it or under it following.
    Rename { from: PathBuf, to: PathBuf },
    /// Its own menu, shown under the header button clicked (the "Database" window's `+`):
    /// the row chosen comes back as [`ToolPanel::menu_command`].
    Menu(Vec<crate::viewer::ViewMenuEntry>),
    /// Its own menu, shown under `anchor` (a button of its own: a row's "⋮"); the row chosen
    /// comes back as [`ToolPanel::menu_command`].
    MenuUnder { anchor: Rect, entries: Vec<crate::viewer::ViewMenuEntry> },
}

/// A button of a panel's header (after its title): what it's called when clicked, its icon,
/// its tooltip, its group (buttons of different groups have a separator between them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub tooltip: String,
    pub group: u8,
}

/// How far a key reaches in a text field: by word (⌥ / Ctrl), extending the selection (⇧).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyReach {
    pub word: bool,
    pub extend: bool,
}

impl KeyReach {
    /// An editing key in a one-line field (`input`): erasing, moving, by word or selecting as
    /// far as this reaches. `false`: not one for it.
    pub fn edit(self, input: &mut ui_kit::TextInput, key: crate::viewer::ViewerKey) -> bool {
        use crate::viewer::ViewerKey;
        let Self { word, extend } = self;
        match key {
            ViewerKey::Backspace => input.backspace(word),
            ViewerKey::Delete => input.delete(),
            ViewerKey::Left => input.move_left(word, extend),
            ViewerKey::Right => input.move_right(word, extend),
            ViewerKey::Home => input.home(extend),
            ViewerKey::End => input.end(extend),
            _ => return false,
        }
        true
    }
}

/// What a panel keeps of itself between sessions (its own text: where its splitter is),
/// given back ([`ToolPanel::set_data`]) when it's made again; the project's session keeps
/// it ([`ToolPanel::memento`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelMemento(pub String);

pub trait ToolPanel: Send {
    /// The new editor context (when the file, the text or the cursor changes).
    fn update(&mut self, ctx: &PanelContext<'_>);
    fn paint(&mut self, ctx: ViewerPaint<'_>);
    fn scroll(&mut self, _dy: f32) -> bool {
        false
    }
    /// The wheel both ways (a log scrolled sideways); by default only `dy`.
    fn scroll_by(&mut self, dx: f32, dy: f32) -> bool {
        let _ = dx;
        self.scroll(dy)
    }
    /// What it keeps of itself for the next session ([`PanelMemento`]); `None`: nothing.
    fn memento(&self) -> Option<String> {
        None
    }
    fn click(&mut self, _p: Point) -> Option<ToolPanelAction> {
        None
    }
    /// A press with its click count (a double click in a text field selects it all); by
    /// default a click.
    fn press(&mut self, p: Point, clicks: u32) -> Option<ToolPanelAction> {
        let _ = clicks;
        self.click(p)
    }
    /// A press with the modifiers held (a list's ⇧ / ⌘ selection); by default a
    /// [`Self::press`].
    fn press_with(
        &mut self,
        p: Point,
        clicks: u32,
        mods: crate::viewer::PressMods,
    ) -> Option<ToolPanelAction> {
        let _ = mods;
        self.press(p, clicks)
    }
    /// `true` if the hover state changed (needs a redraw).
    fn hover(&mut self, _p: Point) -> bool {
        false
    }
    /// Something clickable is under the mouse.
    fn clickable(&self, _p: Point) -> bool {
        false
    }
    /// What the app gives every panel once it's created (e.g. where the Task Manager reads
    /// the app's resources); a panel takes what it knows, ignores the rest.
    fn set_data(&mut self, _data: &dyn std::any::Any) {}
    /// Clears what can be rebuilt when needed ("Clear and Rebuild Caches").
    fn purge(&mut self) {}
    /// Back on the UI thread after a wake: `true` if it has something new to draw.
    fn poll(&mut self) -> bool {
        false
    }
    /// Another frame is needed (an animation, work going on).
    /// When it next wants a frame at its own pace (a scrollbar fading); `None`: none.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        None
    }
    fn is_animating(&self) -> bool {
        false
    }
    /// A loader turns in it (work going on: a statement running): frames at a loader's pace
    /// (about 20 a second), not the screen's, as [`crate::Viewer::is_loading`].
    fn is_loading(&self) -> bool {
        false
    }
    /// The mouse dragged after a press in it (a scrollbar's thumb). `true`: a redraw.
    fn drag(&mut self, _p: Point) -> bool {
        false
    }
    fn release(&mut self) -> bool {
        false
    }
    /// It takes typed text (a filter field) while it has the keyboard (clicked into).
    fn wants_text(&self) -> bool {
        false
    }
    fn text_input(&mut self, _text: &str) -> bool {
        false
    }
    /// Text typed while its rows (not a field) have the keyboard: `true` if it took it (the
    /// project tree starts its search with it); `false` leaves it to the editor.
    fn text_on_rows(&mut self, _text: &str) -> bool {
        false
    }
    /// The text selected in its field (Copy / Cut take it) while it takes text.
    fn selected_text(&self) -> Option<String> {
        None
    }
    /// A key while it has the keyboard. `true` if it used it.
    fn edit_key(&mut self, _key: crate::viewer::ViewerKey) -> bool {
        false
    }
    /// A key with how far it reaches (word, selection); by default as [`Self::edit_key`].
    fn edit_key_with(&mut self, key: crate::viewer::ViewerKey, _reach: KeyReach) -> bool {
        self.edit_key(key)
    }
    /// The tooltip for what's under `p` (a mark's meaning, a name cut short).
    fn tooltip_at(&self, _p: Point) -> Option<crate::tooltip::Tip> {
        None
    }
    /// The cursor over `p`, when not the pointer / arrow [`Self::clickable`] gives (text).
    fn cursor_at(&self, _p: Point) -> Option<crate::viewer::ViewerCursor> {
        None
    }
    /// The header the app draws above it (its title and icon from its registration), with
    /// these buttons; `None`: it draws its own.
    fn header(&self) -> Option<Vec<PanelButton>> {
        None
    }
    /// The icon before the header's title, if not its registration's (the tool strip's).
    fn header_icon(&self) -> Option<&'static str> {
        None
    }
    /// A header button clicked.
    fn header_action(&mut self, _id: &str) -> Option<ToolPanelAction> {
        None
    }
    /// An editing command while it has the keyboard: `copy`, `cut`, `paste`, `delete`,
    /// `select_all`; one of its own while [`Self::keyed`]; an answer of the app to what it
    /// asked (`picked-file:<path>`). `true` if it took it (else it goes where it would).
    fn command(&mut self, _id: &str) -> bool {
        false
    }
    /// Its own right-click menu at `p`, when it isn't the contributed rows of a place
    /// ([`Self::context_at`]): the row chosen comes back as [`Self::menu_command`]. `None`:
    /// none there.
    fn menu_at(&mut self, _p: Point) -> Option<Vec<crate::viewer::ViewMenuEntry>> {
        None
    }
    /// A row of its own menu chosen ([`Self::menu_at`], [`ToolPanelAction::Menu`]): `true`
    /// if it used it; what it asks for then is in [`Self::take_action`].
    fn menu_command(&mut self, _id: &str) -> bool {
        false
    }
    /// A click in it gives it the keyboard (its rows move with the arrows), as the left
    /// panels have it; by default only while it takes text.
    fn takes_keys(&self) -> bool {
        false
    }
    /// While it has the keyboard, its own commands work (a `ViewAction` whose view is its
    /// tool window's id: F5 refreshing it), sent to it as [`Self::command`].
    fn keyed(&self) -> bool {
        false
    }
    /// The kind of the view inside it that has its keys (a Services tab's, while clicked
    /// into): that view's actions' keys work then too, before the panel's own.
    fn keyed_view(&self) -> Option<&'static str> {
        None
    }
    /// A right click at `p`: what it's on (the row, selected), for the menu shown there and
    /// its commands. `None`: no menu.
    fn context_at(&mut self, _p: Point) -> Option<workbench_model::ActionContext> {
        None
    }
    /// What the commands run by the keyboard act on while it has it (its selection).
    fn context(&self) -> Option<workbench_model::ActionContext> {
        None
    }
    /// The keyboard went elsewhere (a click outside it): what was being typed in place is
    /// given up (a new entry's name).
    fn focus_lost(&mut self) {}
    /// The page the system's web engine shows over it (a view inside it shows one: a
    /// response's preview), placed and hidden by the app as a tab's.
    fn web_page(&self) -> Option<crate::viewer::WebPage> {
        None
    }
    /// What its app's page told it (see [`crate::Viewer::web_event`]).
    fn web_event(&mut self, _page: u128, _event: crate::viewer::WebEvent) {}
    /// The messages it has for its app's pages (see [`crate::Viewer::take_web_sends`]).
    fn take_web_sends(&mut self) -> Vec<(u128, String)> {
        Vec::new()
    }
    /// Hidden, kept aside until shown again (its state stays): what runs for it stops
    /// (threads, what it holds the app to keep up); it starts again on its next paint.
    fn parked(&mut self) {}
    /// After its action (a click's, a key's): it keeps the keyboard; `false`: the editor
    /// takes it (e.g. a search result opened).
    fn keeps_keyboard(&mut self) -> bool {
        true
    }
    /// What it tells the user (its views' notifications: a run ended, an export written),
    /// shown by the app as toasts.
    fn take_notifications(&mut self) -> Vec<workbench_model::Notification> {
        Vec::new()
    }
    /// What it asks for after a key or a command (as a click's action).
    fn take_action(&mut self) -> Option<ToolPanelAction> {
        None
    }
}
