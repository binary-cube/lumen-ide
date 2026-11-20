//! Composition of the main screen: the project tree, the tabs with editors, the find bar
//! and the status bar.
//!
//! It knows nothing about the window or the renderer (the app handles dialogs and the
//! clipboard), so it is also used for `--snapshot`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use code_intelligence::LanguageIntelligence;
use document::{
    Document, Encoding, FindState, Indent, LineEnding, Motion, SaveCleanup, SearchQuery, commands,
    open_document, save_document, visual_width,
};
use i18n::tr;
use icons::{IconId, Icons};
use kernel::{DocumentId, LanguageId, Memo, PhysicalSize, Point, Purge, Rect, Rgba};
use languages::SNIFF_BYTES;
use platform_api::CursorIcon;
use render_api::{Quad, Scene};
use settings_model::{Charset, EndOfLine, IndentStyle, editorconfig};
use syntax_ts::{
    DocumentSyntax, HighlightSpan, Highlighter, ScopeKey, SyntaxWorker, parse_document,
};
use text_layout::{FontFamily, TextSystem};
use theming::ResolvedTheme;
use ui_kit::{draw_icon, draw_text};
use ui_workbench::{
    Breadcrumbs, ColorSpan, CompletionPopup, Crumb, Decorations, EditorView, FindBar, FindField,
    FindHit, Folds, Navigation, Orientation, Palette, PanelAction, PanelHeader, ProjectsAction,
    ProjectsPicker, RecentProject, ScrollbarStyle, SettingsAction, SettingsModel, SettingsView,
    Splitter, SplitterStyle, StatusBar, StatusItem, TabBar, TabHit, TabTitle, ToolButton,
    ToolStrip, ToolStripStyle, TopBar, TopBarHit, TopBarModel,
};
use workspace::{FileTree, NodeKind, PathQuery, PathResults, open_project};

use crate::modules::LanguageSupport;
use crate::resources::Sample;
use overlay_stack::OverlayKind;
use overlays::Overlay;
use project_data::ProjectData;

/// What a tab's syntax colors were computed for: the document version, the parse number,
/// the theme generation and the area (slightly larger than the visible one).
#[derive(Debug, Clone, PartialEq)]
struct HighlightKey {
    version: u64,
    parses: u64,
    theme: u64,
    range: std::ops::Range<usize>,
    /// The long rows' drawn parts highlighted (`None`: `range` whole).
    windows: Option<Vec<std::ops::Range<usize>>>,
}

/// What the breadcrumbs depend on: the document, its version, the cursor, its symbols,
/// whether the editor shows, the title, the theme (the icon's color) and the file icon set.
type CrumbKey = (DocumentId, u64, usize, Option<u64>, bool, String, u64, Option<u8>);

/// What the colors written in code were searched for: the document, the version, the visible area.
type SwatchKey = (DocumentId, u64, std::ops::Range<usize>);

/// The colors written on the visible lines: (line, color) for the gutter and the literal
/// (absolute range), for the picker.
#[derive(Debug, Default)]
struct Swatches {
    lines: Vec<(u32, Rgba)>,
    literals: Vec<code_intelligence::ColorLiteral>,
}

/// The default width of the project panel and its limits, in logical pixels.
mod assists;
mod caches;
mod chrome;
mod concealed;
mod context_actions;
mod decorations;
mod diagnostics;
mod dialogs;
mod editing;
mod editor_full;
mod editor_toolbars;
mod emphasis;
mod entry_events;
mod everywhere;
mod file_drop;
mod file_format;
mod files;
mod find_search;
pub(crate) mod frameworks;
mod groups;
mod home;
mod host;
mod idle;
mod indenting;
mod input;
mod insight;
mod intelligence;
mod levels;
mod links;
mod listing;
mod locking;
mod menu_drawer;
mod menu_states;
mod menus;
mod modal_view;
mod moving;
mod navigation;
mod overlay_stack;
mod overlays;
mod paint;
mod paint_phases;
mod ports;
mod preview_split;
mod project_data;
mod project_groups;
mod recent_writes;
mod refactorings;
mod running;
mod saving;
mod search;
mod servers;
mod session;
mod side_panel;
mod smart_select;
mod splits;
mod status;
mod syntax;
mod task_data;
mod theme_decorations;
mod toasts;
mod tool_area;
mod tool_windows;
mod tree;
mod viewers;
mod window_menu;
mod zoom;

const TREE_WIDTH: f32 = 280.0;
/// Below this, the header (title + buttons) no longer fits.
const TREE_MIN_WIDTH: f32 = 250.0;
/// The gap between the breadcrumb (or the find strip) and the editor, in logical pixels:
/// the current line doesn't touch the bar.
const EDITOR_GAP: f32 = 4.0;
/// The strip between the tree and the card, where the separator sits.
const SPLITTER_WIDTH: f32 = 16.0;
/// The bottom dock's share of the window's height until it's dragged in the project: a
/// result shows ~8–10 rows below its bars; the editor above keeps at least
/// [`EDITOR_MIN_SHARE`].
const BOTTOM_SHARE: f32 = 0.45;
const EDITOR_MIN_SHARE: f32 = 0.30;
/// The minimum height of the bottom dock (logical).
const TERMINAL_MIN_HEIGHT: f32 = 140.0;
/// The main card's distance from the window edges and the corner radius (logical).
const CARD_MARGIN: f32 = 8.0;
/// How long the hidden top bar takes to come down or go (full screen).
const TOP_BAR_REVEAL: std::time::Duration = std::time::Duration::from_millis(160);
const CARD_RADIUS: f32 = ui_workbench::ds::radius::CARD;
/// The width of the right side panel (logical pixels).
const SIDE_PANEL_WIDTH: f32 = 300.0;
const SIDE_PANEL_MIN: f32 = 220.0;

/// An editing key, after the keymap shortcuts have already been handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Move { motion: Motion, extend: bool },
    Enter { shift: bool },
    Backspace { word: bool },
    Delete,
    Tab { shift: bool },
    Escape,
}

/// The folding commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldCommand {
    /// Collapses the innermost region around the cursor.
    Fold,
    Unfold,
    FoldAll,
    UnfoldAll,
}

/// Dragging the mouse after a multi-click extends by whole words or lines.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DragUnit {
    Word {
        origin: std::ops::Range<usize>,
        word_chars: &'static str,
    },
    Line {
        origin: std::ops::Range<usize>,
    },
    /// ⌥ + drag: column selection from (line, column).
    Column {
        anchor: (usize, usize),
    },
}

/// The symbol under the mouse with ⌘ (Ctrl) held: it can be clicked to go to its declaration.
#[derive(Debug, Clone)]
struct DefinitionLink {
    tab: usize,
    range: std::ops::Range<usize>,
    /// The destination (e.g. `User.php:12`); `None` if the symbol has no known declaration.
    label: Option<String>,
    /// Where a document link leads (a URL, a file named in quotes), when it's one.
    target: Option<code_intelligence::links::LinkTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Editor,
    Find,
    /// A tool window's view at the bottom (the Git log): its keys.
    Panel,
    /// A side panel (clicked into: its keys, its text — the Task Manager's filter).
    Side(ui_workbench::Dock),
}

/// What the choice list (`ChoiceMenu`) is for.
#[derive(Default)]
enum ChoicePurpose {
    #[default]
    None,
    /// "Copy": the values copied.
    Copy(Vec<String>),
    /// A module's list: what to do with the chosen row.
    Contributed(workbench_model::OnChoice),
    /// A navigation's list (implementations, a hierarchy): where each row leads.
    Navigate(Vec<navigation::NavRow>),
    /// The code actions at the cursor: what each row does.
    Actions(Vec<context_actions::ContextAction>),
    /// The file's language, encoding, line terminator or indentation: what each row sets.
    /// A file's format: what each row does, to the document the list was opened for.
    Format(DocumentId, Vec<file_format::FormatPick>),
}

impl std::fmt::Debug for ChoicePurpose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::None => "None",
            Self::Copy(_) => "Copy",
            Self::Contributed(_) => "Contributed",
            Self::Navigate(_) => "Navigate",
            Self::Actions(_) => "Actions",
            Self::Format(..) => "Format",
        })
    }
}

/// The function that wakes the event loop (after background indexing).
#[derive(Clone)]
struct Waker(Arc<dyn Fn() + Send + Sync>);

/// An editor group (split): its tabs and what belongs to them. The current group keeps
/// its state directly in the `Workbench` fields (`tabs`, `active`, `tab_bar`…), so the
/// rest of the code always works with "the current group"; its slot in `editors.groups`
/// keeps only its bounds until a swap ([`Workbench::swap_group`]).
#[derive(Debug, Default)]
struct GroupState {
    tabs: Vec<Tab>,
    active: usize,
    tab_bar: TabBar,
    breadcrumbs: ui_workbench::Breadcrumbs,
    /// The breadcrumbs (and each segment's target offset; `None`: the file), for what they
    /// were computed for (see `crumbs`).
    crumbs: Memo<CrumbKey, (Vec<ui_workbench::Crumb>, Vec<Option<usize>>)>,
    editor_area: Rect,
    swatches: Memo<SwatchKey, Swatches>,
    /// The group's area in the card (not swapped).
    bounds: Rect,
}

pub use crate::split_tree::Split;
pub use editor_toolbars::BAR_MENU;
pub use file_drop::{DropAction, skipped_message};
pub use locking::LockState;
pub use menu_states::RowState;
pub use navigation::Related;
pub use ports::{Disk, Ports};
pub(crate) use project_groups::{RECENT_MENU, delete_project_dialog};
pub use running::RunningRequest;
pub use side_panel::PANEL_MENU;
pub use status::STATUS_MENU;
pub use tool_windows::TOOL_VIEW_MENU;
pub use viewers::{Sight, VIEW_MENU};
pub use window_menu::WINDOW_MENU;

mod disk;
mod tab;
mod tab_icons;
mod text_util;
use disk::*;
use tab::*;
use tab_icons::*;
use text_util::*;

/// What the status bar's cursor position is computed from: the document, its version, the
/// primary cursor (head, anchor), the number of cursors, the tab width.
type StatusCursorKey = (DocumentId, u64, usize, usize, usize, u32);

/// A tab dragged with the mouse (to another group or another position).
#[derive(Debug, Clone)]
struct TabDrag {
    tab: usize,
    title: String,
    icon: Option<(IconId, Rgba)>,
    at: Point,
    /// The group and position it would go into, with the insertion line (on the tab bar).
    target: Option<(usize, usize, Option<Rect>)>,
}

/// The thickness of the strip between split groups (logical pixels), with the handle in the
/// middle.
const SPLIT_GAP: f32 = 10.0;

/// A side panel (one per side), registered by a module: on the right (Structure, the
/// Task Manager) or on the left (in the project panel's place while it's open).
struct SidePanel {
    id: &'static str,
    dock: ui_workbench::Dock,
    panel: Box<dyn ui_workbench::ToolPanel>,
    /// Where it's drawn (its card), its content (below the header the app draws for it,
    /// when it asks for one) and that header.
    bounds: Rect,
    content: Rect,
    header: PanelHeader,
    /// A press started in it (its drags go to it).
    dragging: bool,
    /// The editor state at the last update (the document, version, cursor, symbols,
    /// usages): the panel is rebuilt only when it changes.
    key: Option<(DocumentId, u64, usize, Option<u64>, u64)>,
    /// The project it was last given (its root, the files' generation, the file index):
    /// given again when it changes. `None`: not given yet.
    project: Option<Option<ProjectKey>>,
    /// Hidden since (parked, its data not let go yet): after a while its data goes
    /// (`ToolPanel::purge`), like a hidden bottom window's views'.
    parked_since: Option<std::time::Instant>,
}

/// Which project, and which state of its files, a panel was given.
/// The project a panel was given (by its identity), its files' generation, its index.
type ProjectKey = (kernel::ProjectId, u64, usize);

impl std::fmt::Debug for SidePanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SidePanel").field("id", &self.id).finish()
    }
}

impl std::fmt::Debug for Waker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Waker")
    }
}

/// Reads the system clipboard's text (the window's; read only when a command asks).
#[derive(Clone)]
struct ClipboardReader(std::rc::Rc<dyn Fn() -> Option<String>>);

/// The themes for the configurator (the app's, every window's), given the project's own
/// theme: read only when it's opened.
pub(crate) type ThemeSource = std::rc::Rc<dyn Fn(Option<&str>) -> theming::studio::StudioInput>;

struct Themes(ThemeSource);

impl std::fmt::Debug for Themes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Themes")
    }
}

/// What runs once a module's question is answered "yes".
pub(crate) struct Confirming(pub workbench_model::OnYes);

impl std::fmt::Debug for Confirming {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Confirming")
    }
}

/// Reads the system clipboard's files.
pub(crate) type ReadFiles = std::rc::Rc<dyn Fn() -> Vec<PathBuf>>;
/// Puts files on the system clipboard.
pub(crate) type WriteFiles = std::rc::Rc<dyn Fn(&[PathBuf])>;

/// The system clipboard's files: read (copied in the file manager) and written.
#[derive(Clone)]
pub(crate) struct FilesClipboard {
    pub read: ReadFiles,
    pub write: WriteFiles,
}

impl std::fmt::Debug for FilesClipboard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FilesClipboard")
    }
}

impl std::fmt::Debug for ClipboardReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClipboardReader")
    }
}

/// What is shown in a tab with a viewer: only the editor, the editor and the preview side
/// by side, or only the preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewMode {
    Editor,
    Split,
    Preview,
}

/// A tab's viewer (contributed by a module): the image, the Markdown preview…
struct TabViewer {
    viewer: Box<dyn ui_workbench::Viewer>,
    /// Its viewer's id (where its actions' keys work).
    provider: &'static str,
    kind: ui_workbench::ViewerKind,
    mode: ViewMode,
    /// The document version last sent to the viewer (preview).
    source_version: Option<u64>,
    bounds: Rect,
    /// The Editor / Split / Preview buttons (from the last paint).
    toggles: Vec<(Rect, ViewMode)>,
    /// Opened below the editor (`open_view_below`): the code it opens goes to the group above.
    below: bool,
    /// Split: the view's half was clicked last, so it has the keys (when it takes them
    /// beside the text: [`ui_workbench::Viewer::keys_in_split`]); the editor's half had
    /// them otherwise.
    keys: bool,
    /// What its viewer was last told of its visibility ([`ui_workbench::Viewer::set_shown`]):
    /// hidden until the first frame shows it.
    shown: bool,
    /// Split: the halves' ratio and the separator between them.
    split: preview_split::PreviewSplit,
}

impl std::fmt::Debug for TabViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TabViewer").field("kind", &self.kind).field("mode", &self.mode).finish()
    }
}

impl TabViewer {
    fn new(
        viewer: Box<dyn ui_workbench::Viewer>,
        provider: &'static str,
        kind: ui_workbench::ViewerKind,
        mode: ViewMode,
    ) -> Self {
        Self {
            viewer,
            provider,
            kind,
            mode,
            source_version: None,
            bounds: Rect::default(),
            toggles: Vec::new(),
            below: false,
            keys: false,
            shown: false,
            split: preview_split::PreviewSplit::default(),
        }
    }

    /// The text and the view side by side (a preview's Split).
    fn splits(&self) -> bool {
        self.kind == ui_workbench::ViewerKind::Preview && self.mode == ViewMode::Split
    }

    /// The editor is visible (and editable).
    fn editor_visible(&self) -> bool {
        self.kind == ui_workbench::ViewerKind::Preview && self.mode != ViewMode::Preview
    }

    fn viewer_visible(&self) -> bool {
        self.kind == ui_workbench::ViewerKind::Replace || self.mode != ViewMode::Editor
    }

    /// The view has the keyboard: it's shown without the editor, or beside it (Split)
    /// after a click in it, when it takes keys there (a grid). The editor keeps them in
    /// every other case.
    fn has_keys(&self) -> bool {
        !self.editor_visible()
            || (self.mode == ViewMode::Split
                && self.kind == ui_workbench::ViewerKind::Preview
                && self.keys
                && self.viewer.keys_in_split())
    }
}

/// Something flagged in the text: an error (red squiggle) or a warning (struck through).
#[derive(Debug, Clone)]
struct Mark {
    range: std::ops::Range<usize>,
    message: String,
    /// The code it is about (shown after the message).
    code: Option<String>,
    warning: bool,
    /// Unused code: dimmed, not counted, no mark on the scroll strip.
    weak: bool,
    /// Deprecated: struck through (other warnings get a yellow squiggle).
    struck: bool,
}

/// An open file: the document, its view and the search state.
#[derive(Debug)]
struct Tab {
    /// Pinned: stays first and isn't closed as part of the group.
    pinned: bool,
    /// Its file can't be written (read-only on disk): opened locked, and it stays so until
    /// the file is writable again (`locking`). The lock itself is its document's.
    disk_read_only: bool,
    /// Its file changed on disk while it had unsaved changes: they're kept, and only a
    /// save the user asks for writes over the file (not the automatic one).
    disk_changed: bool,
    /// Its file's size and modification time when last seen (opened, saved, reloaded): the
    /// automatic save checks it right before writing, so a change on disk the watcher hasn't
    /// reported yet (or missed) is never written over.
    disk_stamp: Option<DiskStamp>,
    /// The fingerprint of the text we last wrote to its file: the watcher reporting that
    /// write (late: typing may have gone on since) isn't taken for a change on disk.
    saved_text: Option<u64>,
    /// The same file is also open in the current group: (the document there, its version)
    /// at the last text sync.
    synced_from: Option<(DocumentId, u64)>,
    viewer: Option<TabViewer>,
    /// A module's bar above its text (a query console's).
    toolbar: Option<editor_toolbars::TabToolbar>,
    doc: Document,
    view: EditorView,
    find: FindState,
    find_bar: FindBar,
    find_open: bool,
    /// The number for new, unsaved documents ("Untitled-2").
    untitled: Option<u32>,
    syntax: DocumentSyntax,
    folds: Folds,
    /// The document version the folding regions are aligned to.
    folds_version: u64,
    /// The document version whose symbols are in the index.
    symbols_version: Option<u64>,
    /// What was flagged in its text (syntax errors, newer features, deprecated or unused
    /// names, requirements), with what it was computed for.
    diagnostics: TabDiagnostics,
    /// The occurrences of the selected word and what for (the document version, the word).
    occurrences: Vec<std::ops::Range<usize>>,
    occurrences_key: Option<(u64, String)>,
    /// Bumped whenever the marks or the occurrences are recomputed (the editor's scroll
    /// strip keeps their lines for it).
    marks_generation: u64,
    /// The whole text as one string, for the document version it was made from: the
    /// language services that need it share one copy per version (not one per call).
    text: std::cell::RefCell<Option<(u64, Arc<str>)>>,
    /// The syntax colors of an area slightly larger than the visible one, and what for
    /// (the document version, the parse number, the theme generation): they aren't recomputed
    /// on redraws without changes (cursor, hover, animations) or on small scrolls.
    highlight: Memo<HighlightKey, Vec<ColorSpan>>,
    /// When the tab was last on screen (its syntax tree is freed after a while unseen).
    seen: std::time::Instant,
    /// What "Shrink Selection" goes back through.
    selection_steps: smart_select::SelectionSteps,
    /// Its inlay hints and code lenses.
    insight: insight::TabInsight,
    /// What the modules hide in it (a `.env` file's secrets).
    concealed: concealed::TabConcealed,
}

/// The project panel, the app's own tool window (on the left).
pub const PROJECT_TOOL: &str = workbench_model::PROJECT_PANEL;

#[derive(Debug)]
pub struct Workbench {
    palette: Palette,
    /// The window's renderer draws backdrop blurs (else Glass backdrops are drawn simple).
    blur: bool,
    /// The disk, the session and settings stores, watching: the domain's ports.
    ports: Ports,
    project: Option<FileTree>,
    tabs: Vec<Tab>,
    active: usize,
    tab_bar: TabBar,
    status: StatusBar,
    /// The editor area takes the window: what it hid, brought back after.
    editor_full: Option<editor_full::Hidden>,
    focus: Focus,
    code_font: FontFamily,
    next_untitled: u32,
    size: PhysicalSize,
    scale: f32,
    editor_area: Rect,
    /// The rounded card that holds the tabs and the editor.
    card: Rect,
    support: Arc<LanguageSupport>,
    /// Background parsing; without it (e.g. `--snapshot`), parsing is synchronous.
    worker: Option<SyntaxWorker>,
    highlighter: Highlighter,
    spans: Vec<HighlightSpan>,
    colors: Vec<ColorSpan>,
    theme: Option<ResolvedTheme>,
    /// The color of each syntax capture in the active theme (cleared when the theme changes).
    scope_colors: HashMap<ScopeKey, Option<ScopeStyle>>,
    /// The environment's look (`levels::warm_generation`) the project's levels were loaded
    /// with.
    levels_warmed: u64,
    /// Incremented on each new theme (invalidates the cached syntax colors).
    theme_generation: u64,
    /// The files' icons by name (the tabs', the breadcrumbs', the status bar's).
    file_icons: FileIcons,
    /// The window's usage (the process and the estimated GPU memory), shown in the status bar.
    resources: Option<(Sample, Option<render_api::GpuMemory>)>,
    /// The last minute of them (the bar's meter and its window's graphs).
    resource_trail: status::ResourceTrail,
    /// The resources window pinned (the setting, read once; `None`: not yet).
    resources_pinned: Option<bool>,
    /// The widths the side panels' headers needed when they were last laid out for them.
    laid_for_headers: Vec<f32>,
    icons: Arc<Icons>,
    drag_unit: Option<DragUnit>,
    settings: SettingsView,
    settings_model: SettingsModel,
    /// The language chosen on the settings' Languages page (`None`: its first).
    settings_language: Option<String>,
    /// "Copy": the ways to copy a file's path / reference, with previews.
    choice_menu: ui_workbench::ChoiceMenu,
    /// The right-click menu (drawn by us, the same on every system).
    context_menu: ui_workbench::ContextMenu,
    /// The app's menus in a drawer from the window's bar: made when it first opens, gone
    /// once it has slid out.
    menu_drawer: Option<Box<ui_workbench::MenuDrawer>>,
    /// What the open choice list is for (what its rows do).
    choice_purpose: ChoicePurpose,
    /// The notifications drawn (their layout, where each went, what's under the mouse);
    /// what they are is the project's (`data.notifications`).
    toasts: ui_workbench::toast::ToastStack,
    /// A status bar item's window (the file's details, the resources).
    status_popover: ui_workbench::status_popover::StatusPopover,
    /// The pictures drawn in place of the pages a window is open over (page id, picture):
    /// the app's, given after each frame they change.
    page_pictures: Vec<(u128, render_api::ImageData)>,
    /// The pages can have holes (what floats over them shows through): the window's say.
    page_holes_ok: bool,
    /// The resources pinned: their pills in the editor's corner (apart from the window a
    /// bar item opens over it, which they never keep from opening).
    resource_pills: ui_workbench::status_popover::StatusPopover,
    /// When time last passed for them (a frame).
    toast_clock: Option<std::time::Instant>,
    /// Files from the system's file manager dragged over the window.
    file_drop: Option<file_drop::FileDropHover>,
    /// Where this window's notifications show instead of the setting (the tests).
    toast_position: Option<workbench_model::ToastPosition>,
    /// A search opened a file with a place in its text (`User.php:12:5`): the cursor goes
    /// there once the file is shown (0-based line, column).
    pending_goto: Option<(u32, u32)>,
    /// A navigation list's row chosen: the file opened and the cursor put at the offset at
    /// the next frame.
    pending_offset: Option<(PathBuf, usize)>,
    /// What a menu's commands act on (what was right-clicked), instead of the keyboard's.
    menu_context: Option<workbench_model::ActionContext>,
    /// The menu's target file's text read from disk (not open): read once while the menu
    /// asks its commands whether they apply, not once per command.
    menu_text: std::cell::RefCell<Option<(PathBuf, Arc<str>)>>,
    /// The status bar's cursor position, by what it was computed from.
    status_cursor: std::cell::RefCell<Memo<StatusCursorKey, (usize, String)>>,
    /// The About window.
    about: ui_workbench::AboutView,
    /// The text size in the editor (1 = 100%), for all tabs.
    editor_zoom: f32,
    /// The theme chosen only for the window's project (from the session).
    project_theme: Option<String>,
    /// The language version picker (popover from the status bar) and its language.
    level_language: Option<LanguageId>,
    /// The entries moved or removed by anyone, heard (the tabs follow).
    entry_changes: entry_events::EntryChanges,
    /// Work going on, shown in the status bar (by id).
    progress: Vec<(&'static str, workbench_model::Progress)>,
    /// The system clipboard's files (set by the window).
    files_clipboard: Option<FilesClipboard>,
    /// When the project's session is saved (the last one saved; frozen while the tabs are
    /// being closed: closing the window doesn't overwrite the session with an empty one).
    session: workbench_model::SessionKeeper,
    /// Where project sessions are kept (`<project dir>/session.toml`).
    sessions: session::QueuedSessions,
    /// The project's recent files, written in the background.
    recent_writes: recent_writes::QueuedRecentFiles,
    /// The open project's data (indexes, caches, histories): replaced as a whole when
    /// another project is opened, so nothing is left over from the previous one.
    data: ProjectData,
    /// The last sweep for unseen tabs' syntax trees (see `release_unseen_syntax`).
    syntax_swept: std::time::Instant,
    /// How long the first frames' parts took.
    pub(crate) paint_phases: paint_phases::PaintPhases,
    /// A request for the app produced by keys (e.g. Enter on a project in the list).
    request: Option<MouseResult>,
    /// A module asked about its work still running before restarting it (a reconnect): the
    /// app asks once the current input is handled.
    running_request: Option<running::RunningRequest>,
    /// What the active view asked for with a command run on it that needs the text system
    /// (opening a file at a line): done at its next action's turn.
    held_view_action: Option<ui_workbench::ViewerAction>,
    /// What the bottom views' background work prepared, to be done at this wake.
    panel_actions: Vec<ui_workbench::ViewerAction>,
    /// ⌘ (Ctrl) is held down: the symbols under the mouse become links to their declaration.
    /// ⌘⌥ (Ctrl+Alt) is held: the mouse wheel zooms.
    wheel_zoom: bool,
    link_mode: bool,
    link: Option<DefinitionLink>,
    /// The active document's byte under the pointer (on its text): a hidden part there is
    /// shown.
    pointer_text: Option<(DocumentId, usize)>,
    /// The tooltips (a single layer, above everything).
    tooltips: ui_workbench::Tooltips,
    /// The error under the mouse (its index in the tab) and the mouse position, for the tooltip.
    error_tip: Option<(usize, Point)>,
    /// The card explaining the code under the mouse (a signature, a syntax).
    code_hover: ui_workbench::CodeHover,
    /// The text a language inserted as a character was typed, that the next one may take
    /// back: the document, its version and the cursor after it, and those characters.
    typed_insert: Option<(kernel::DocumentId, u64, usize, &'static [char])>,
    /// The document the frameworks last checked: its id, its version, their models'
    /// generations (summed) — checked again when one changes.
    framework_checked: Option<(kernel::DocumentId, u64, u64)>,
    /// The modules' status signs as last drawn (a change: drawn again).
    shown_indicators: Vec<workbench_model::Indicator>,
    /// The projects open in the app's windows (this one's marked), as the app tells.
    project_windows: Vec<workbench_model::ProjectWindow>,
    /// A module asked for this window's picture (taken before the next frame).
    capture_wanted: bool,
    /// The signature of the call being typed.
    param_hint: ui_workbench::ParamHint,
    /// When the text was last changed (automatic saving waits for a pause).
    last_edit: Option<std::time::Instant>,
    /// When a locked tab's edit was last refused with a word to the user (said once a
    /// while, not on every key).
    lock_hint: Option<std::time::Instant>,
    /// Watches files created / deleted / renamed, for the tree.
    tree_watcher: Option<Box<dyn workspace::FileWatcher>>,
    completion: CompletionPopup,
    /// The text replaced by the chosen suggestion.
    completion_replace: Option<std::ops::Range<usize>>,
    /// The open list came from a module's assist, of this generation: made again when it
    /// changes (the columns it asked for came).
    completion_assist: Option<u64>,
    /// Opened projects are recorded in the recent list (but not in `--snapshot`).
    remember_projects: bool,
    waker: Option<Waker>,
    /// What the modules' services follow of the window itself (a project in it or not):
    /// let go with it.
    window_subscriptions: Vec<kernel::Subscription>,
    /// The automatic saves being written on the worker.
    saving: saving::Saving,
    /// The document links' candidate files the disk was asked about, for one version of one
    /// document (⌘ over its words): whether each is a file.
    link_files: Option<((DocumentId, u64), HashMap<PathBuf, bool>)>,
    /// Files changed on disk read again for their tabs on a worker, back here.
    reloads: (
        std::sync::mpsc::Sender<Vec<tree::Reloaded>>,
        std::sync::mpsc::Receiver<Vec<tree::Reloaded>>,
    ),
    /// The open files' stamps being compared on a worker (the window taken again): another
    /// return meanwhile waits for it.
    files_checking: Arc<std::sync::atomic::AtomicBool>,
    /// Files read again in the encoding the user chose, on a worker, back here.
    format_reads: (
        std::sync::mpsc::Sender<file_format::FormatRead>,
        std::sync::mpsc::Receiver<file_format::FormatRead>,
    ),
    /// A large document's find searched whole on a worker, back here.
    found: (
        std::sync::mpsc::Sender<find_search::Found>,
        std::sync::mpsc::Receiver<find_search::Found>,
    ),
    clipboard: Option<ClipboardReader>,
    themes: Option<Themes>,
    /// The history of active tabs, for the ← → buttons (closed documents are skipped).
    back: Vec<DocumentId>,
    forward: Vec<DocumentId>,
    /// The navigation bar below the tabs (`file › class › method`).
    breadcrumbs: ui_workbench::Breadcrumbs,
    /// Each segment's target: the declaration offset (`None`: the file).
    /// The breadcrumbs (and each segment's target offset; `None`: the file), for what they
    /// were computed for (see `crumbs`).
    crumbs: Memo<CrumbKey, (Vec<ui_workbench::Crumb>, Vec<Option<usize>>)>,
    /// The colors written on the visible lines: (line, color) for the gutter and the literal
    /// (absolute range), for the picker.
    swatches: Memo<SwatchKey, Swatches>,
    /// Click in a viewer with no action: the following moves go to it (pan).
    viewer_drag: bool,
    /// A module's view in a modal window (a wizard).
    modal_view: Option<modal_view::ModalView>,
    /// The view over every window this one shows, or was asked to ([`everywhere`]).
    everywhere: everywhere::Everywhere,
    /// What modules start after a while without input (Relax mode): each one's watch.
    idle: Vec<workbench_model::IdleWatch>,
    /// Files being brought to disk to be opened (a remote project's).
    fetching: Vec<listing::Fetching>,
    /// What the Task Manager reads (the app's parts, the modules' reporters).
    task_data: Arc<task_data::AppResources>,
    task_refreshed: Option<std::time::Instant>,
    /// The window is hidden (occluded, minimized): its views are told they're hidden.
    occluded: bool,
    /// The index's memory as last measured, and when (walking it is measured at most every
    /// few seconds, never waiting for the indexer's lock).
    index_memory: std::cell::Cell<Option<(std::time::Instant, u64)>>,
    /// The comparison a view last showed beside it (replaced by the next one).
    beside_compare: Option<PathBuf>,
    /// The top bar, the tint and the window controls.
    chrome: chrome::Chrome,
    /// The open dialog and its answer.
    dialogs: dialogs::Dialogs,
    /// The empty screen, the projects modal and the recent projects.
    home: home::HomeScreen,
    /// The editor groups (split) and their separators.
    editors: groups::EditorGroups,
    /// The tool strips, the side panels and the bottom tool windows.
    tools: tool_area::ToolArea,
    /// The region being worked in, emphasized while a module asks (Focus mode).
    emphasis: emphasis::WindowEmphasis,
}

impl Workbench {
    pub fn new(support: Arc<LanguageSupport>, ports: Ports) -> Self {
        crate::config::isolate_tests();
        let reporters = support.resource_reporters.clone();
        let modules = Arc::clone(&support.manager);
        let mut settings = SettingsView::default();
        settings.set_card_previews(support.card_previews.clone());
        let idle = vec![
            workbench_model::IdleWatch::new(std::time::Instant::now());
            support.idle_actions.len()
        ];
        let data = ProjectData::new();
        let entry_changes = entry_events::EntryChanges::listen(&data.events);
        let palette = Palette::dark();
        let tools = tool_area::ToolArea::new(&palette);
        let icons = shared_icons(&support);
        let sessions = session::QueuedSessions::new(Arc::clone(&ports.sessions));
        let recent_writes = recent_writes::QueuedRecentFiles::new(Arc::clone(&ports.recent_files));
        Self {
            code_font: palette.code_font.clone(),
            palette,
            blur: true,
            ports,
            project: None,
            tabs: Vec::new(),
            active: 0,
            tab_bar: TabBar::default(),
            status: StatusBar::default(),
            editor_full: None,
            focus: Focus::Editor,
            next_untitled: 1,
            size: PhysicalSize::default(),
            scale: 1.0,
            editor_area: Rect::default(),
            card: Rect::default(),
            support,
            worker: None,
            highlighter: Highlighter::new(),
            spans: Vec::new(),
            colors: Vec::new(),
            theme: None,
            scope_colors: HashMap::new(),
            levels_warmed: 0,
            theme_generation: 0,
            file_icons: FileIcons::default(),
            resources: None,
            resource_trail: Default::default(),
            resources_pinned: None,
            laid_for_headers: Vec::new(),
            icons,
            drag_unit: None,
            settings,
            settings_model: SettingsModel::default(),
            settings_language: None,
            choice_menu: Default::default(),
            context_menu: Default::default(),
            menu_drawer: None,
            choice_purpose: ChoicePurpose::None,
            toasts: Default::default(),
            status_popover: Default::default(),
            page_pictures: Vec::new(),
            page_holes_ok: false,
            resource_pills: Default::default(),
            toast_clock: None,
            file_drop: None,
            toast_position: None,
            pending_goto: None,
            pending_offset: None,
            menu_context: None,
            menu_text: std::cell::RefCell::default(),
            status_cursor: std::cell::RefCell::default(),
            request: None,
            running_request: None,
            held_view_action: None,
            panel_actions: Vec::new(),
            about: Default::default(),
            editor_zoom: 1.0,
            project_theme: None,
            level_language: None,
            entry_changes,
            progress: Vec::new(),
            files_clipboard: None,
            session: workbench_model::SessionKeeper::default(),
            sessions,
            recent_writes,
            data,
            syntax_swept: std::time::Instant::now(),
            paint_phases: paint_phases::PaintPhases::default(),
            wheel_zoom: false,
            link_mode: false,
            link: None,
            pointer_text: None,
            tooltips: ui_workbench::Tooltips::default(),
            error_tip: None,
            code_hover: ui_workbench::CodeHover::default(),
            typed_insert: None,
            framework_checked: None,
            shown_indicators: Vec::new(),
            project_windows: Vec::new(),
            capture_wanted: false,
            param_hint: ui_workbench::ParamHint::default(),
            last_edit: None,
            lock_hint: None,
            tree_watcher: None,
            completion: CompletionPopup::default(),
            completion_replace: None,
            completion_assist: None,
            remember_projects: true,
            waker: None,
            window_subscriptions: Vec::new(),
            saving: saving::Saving::default(),
            reloads: std::sync::mpsc::channel(),
            files_checking: Arc::default(),
            format_reads: std::sync::mpsc::channel(),
            found: std::sync::mpsc::channel(),
            link_files: None,
            clipboard: None,
            themes: None,
            back: Vec::new(),
            forward: Vec::new(),
            breadcrumbs: Default::default(),
            crumbs: Memo::default(),
            swatches: Memo::default(),
            viewer_drag: false,
            modal_view: None,
            everywhere: everywhere::Everywhere::default(),
            idle,
            fetching: Vec::new(),
            task_data: task_data::AppResources::new(reporters, Some(modules)),
            task_refreshed: None,
            occluded: false,
            index_memory: std::cell::Cell::new(None),
            beside_compare: None,
            chrome: chrome::Chrome::default(),
            dialogs: dialogs::Dialogs::default(),
            home: home::HomeScreen::default(),
            tools,
            editors: groups::EditorGroups::default(),
            emphasis: emphasis::WindowEmphasis::default(),
        }
    }

    /// The icon set (also for the renderer, which rasterizes them).
    /// The modules and languages (shared by all windows).
    pub fn support(&self) -> Arc<LanguageSupport> {
        Arc::clone(&self.support)
    }

    pub fn icons(&self) -> &Arc<Icons> {
        &self.icons
    }

    /// Updates the displayed usage. `true` if it changed.
    pub fn set_resources(&mut self, sample: Sample, gpu: Option<render_api::GpuMemory>) -> bool {
        let value = Some((sample, gpu));
        let changed = self.resources != value;
        self.resource_trail.push(
            std::time::Instant::now(),
            sample.cpu,
            sample.ram_bytes as f32,
            gpu.map(|g| g.total() as f32),
        );
        self.resources = value;
        // Only a new sample, and no more than the Task Manager's own pace.
        if changed {
            self.refresh_task_data_if_watched();
        }
        changed
    }

    // ─── Syntax ───

    /// Wakes the event loop when a background job finishes (indexing).
    pub fn set_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
        // The window's own, once: the modules' services follow it, a project or none.
        if self.waker.is_none() {
            let support = Arc::clone(&self.support);
            for service in &support.project_services {
                let subscriptions = service.window_opened(&waker);
                self.window_subscriptions.extend(subscriptions);
            }
        }
        self.waker = Some(Waker(waker));
    }

    /// How the system clipboard's text is read (the window's clipboard).
    /// How the system clipboard's files are read and written (the window's clipboard).
    pub fn set_files_clipboard(&mut self, clipboard: FilesClipboard) {
        self.files_clipboard = Some(clipboard);
    }

    pub fn set_clipboard_reader(&mut self, read: std::rc::Rc<dyn Fn() -> Option<String>>) {
        self.clipboard = Some(ClipboardReader(read));
    }

    /// Where the configurator's themes come from (the app's).
    pub fn set_theme_source(&mut self, themes: ThemeSource) {
        self.themes = Some(Themes(themes));
    }

    /// The configurator's themes, with this project's own theme as the current one.
    pub(crate) fn studio_input(&self) -> Option<theming::studio::StudioInput> {
        self.themes.as_ref().map(|themes| (themes.0)(self.project_theme()))
    }

    // ─── The tree: changes on disk, new files / directories ───

    /// The window's title: the project, the active file, the app.
    pub fn title(&self) -> String {
        // Long names cut in the middle: the Dock's and the taskbar's menus show the whole
        // title on one line.
        let file = self.tabs.get(self.active).map(|t| {
            let dirty = if t.is_dirty() { "● " } else { "" };
            format!("{dirty}{}", kernel::text::shorten_middle(&t.title(), TITLE_FILE))
        });
        let project = self
            .project
            .as_ref()
            .map(|p| kernel::text::shorten_middle(p.name(p.root()), TITLE_PROJECT).into_owned());
        let app = kernel::brand::NAME;
        match (file, project) {
            // The project first: the Dock's and the taskbar's window lists group by it.
            (Some(f), Some(p)) => format!("{p} — {f} — {app}"),
            (Some(f), None) => format!("{f} — {app}"),
            (None, Some(p)) => format!("{p} — {app}"),
            (None, None) => app.to_owned(),
        }
    }

    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Where the diagnostics overlay may go: the work area between the tool strips, below
    /// the top bar and above the status bar, inset by the popover gap (never over chrome).
    pub fn overlay_area(&self) -> Rect {
        let s = self.scale;
        let gap = (ui_workbench::ds::POPOVER_GAP * s).round();
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let top = TopBar::height(s) + gap;
        let bottom = h - StatusBar::height(s) - gap;
        let left = self.tools.strip.bounds().max_x() + gap;
        let right_strip = self.tools.right_strip.bounds();
        let right = if right_strip.size.width > 0.0 { right_strip.min_x() } else { w } - gap;
        Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.tools.set_palette(&palette);
        // The code font is the one found on the system (not the theme's generic family).
        let code_font = self.code_font.clone();
        let backdrop = self.backdrop_style(&palette.backdrop);
        let floating = crate::floating::style(&palette.floating, self.blur);
        self.palette = Palette { code_font, backdrop, floating, ..palette };
        // The icons' colors come from the palette.
        self.file_icons.purge();
    }

    /// Whether the window's renderer draws backdrop blurs.
    pub fn set_backdrop_blur(&mut self, blur: bool) {
        self.blur = blur;
        self.refresh_backdrop();
    }

    /// The backdrop and the floating panels' glass again (the user's choice changed).
    pub fn refresh_backdrop(&mut self) {
        self.palette.backdrop = self.backdrop_style(&self.palette.backdrop);
        self.palette.floating = crate::floating::style(&self.palette.floating, self.blur);
    }

    /// The backdrop a window draws: the theme's (what `style` was resolved from), never
    /// blurred where the renderer can't.
    fn backdrop_style(&self, style: &ui_workbench::BackdropStyle) -> ui_workbench::BackdropStyle {
        crate::backdrop::style(style, self.blur)
    }

    pub fn set_font_family(&mut self, family: FontFamily) {
        // Also in the palette: the terminal and the other components use the same code font.
        self.palette.code_font = family.clone();
        self.code_font = family.clone();
        for tab in &mut self.tabs {
            tab.view.set_font_family(family.clone());
        }
    }

    pub fn background(&self) -> Rgba {
        self.palette.window_background
    }

    pub fn resize(&mut self, size: PhysicalSize, scale: f32) {
        self.size = size;
        self.scale = scale;
        self.layout();
    }

    // ─── Editing ───

    fn push_tab(&mut self, mut tab: Tab) {
        tab.view.set_zoom(self.editor_zoom);
        // Read-only on disk, or the same file already open locked: it opens locked.
        self.lock_on_open(&mut tab);
        // A cancelled close (or a new file after it): the session is tracked again.
        self.session.thaw();
        if let Some(path) = tab.doc.path().map(Path::to_path_buf) {
            self.remember_file(&path);
        }
        // An empty, unmodified "Untitled" tab is replaced by the first opened file.
        let replace = self.tabs.len() == 1
            && self.tabs[0].untitled.is_some()
            && !self.tabs[0].doc.is_dirty()
            && self.tabs[0].doc.buffer().is_empty();
        if replace {
            self.tabs[0] = tab;
            self.active = 0;
        } else {
            self.tabs.insert((self.active + 1).min(self.tabs.len()), tab);
            self.active = (self.active + 1).min(self.tabs.len() - 1);
            if self.tabs.len() == 1 {
                self.active = 0;
            }
        }
        self.focus = Focus::Editor;
        self.tab_bar.reveal_active();
        self.layout();
    }

    /// The left dock's width (logical): the project's, as dragged, else the default — the
    /// same for every tool window shown there.
    /// A width dragged is never narrower than its panel's header needs (one kept from
    /// before it was); the default stays as it is.
    fn left_dock_width(&self) -> f32 {
        match self.data.sizes.get(workbench_model::sizes::DOCK_LEFT) {
            Some(kept) => kept.max(self.header_min(ui_workbench::Dock::Left)),
            None => TREE_WIDTH,
        }
    }

    /// The right dock's width (logical), as the left's.
    fn right_dock_width(&self) -> f32 {
        match self.data.sizes.get(workbench_model::sizes::DOCK_RIGHT) {
            Some(kept) => kept.max(self.header_min(ui_workbench::Dock::Right)),
            None => SIDE_PANEL_WIDTH,
        }
    }

    /// The bottom dock's height (physical) in `available` (the height below the top bar,
    /// physical; `gap`: its separator's): the project's, as dragged, the editor keeping
    /// 120 px; never dragged, [`BOTTOM_SHARE`] of it, the editor keeping
    /// [`EDITOR_MIN_SHARE`]. The same for every tool window shown there.
    fn bottom_dock_height(&self, available: f32, gap: f32) -> f32 {
        let s = self.scale;
        match self.data.sizes.get(workbench_model::sizes::DOCK_BOTTOM) {
            Some(height) => (height * s).round().min((available - gap - 120.0 * s).max(0.0)),
            None => {
                let most = (available * (1.0 - EDITOR_MIN_SHARE) - gap).max(0.0);
                (available * BOTTOM_SHARE).max(TERMINAL_MIN_HEIGHT * s).min(most).round()
            }
        }
    }

    fn layout(&mut self) {
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let s = self.scale;
        let margin = (CARD_MARGIN * s).round();
        let status_h = StatusBar::height(s);
        let body_h = (h - status_h).max(0.0);
        self.settings.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.home.projects_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.choice_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.context_menu.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.dialogs.dialog.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.about.set_viewport(Rect::new(0.0, 0.0, w, h), s);
        self.place_menu_drawer();
        // The top bar (on macOS, in the title bar); the rest starts below it — or at the
        // top, the bar hidden (full screen: it comes over the content, see `paint`).
        let bar_h = TopBar::height(s);
        self.chrome.top_bar.set_bounds(Rect::new(0.0, 0.0, w, bar_h), s);
        let y0 = if self.chrome.top_bar_hides() { 0.0 } else { bar_h };
        let strip_w = ToolStrip::width(s);
        self.tools.strip.set_bounds(Rect::new(0.0, y0, strip_w, (body_h - y0).max(0.0)), s);
        // The right bar (the module panels), stuck to the window edge; with no panels, only the
        // usual margin remains.
        let right = self.support.tools.iter().any(|t| t.dock() == ui_workbench::Dock::Right);
        let right_w = if right { strip_w } else { 0.0 };
        self.tools
            .right_strip
            .set_bounds(Rect::new(w - right_w, y0, right_w, (body_h - y0).max(0.0)), s);
        let right_edge = w - if right_w > 0.0 { right_w } else { margin };
        // The top area (project + editor) and the terminal below it, split horizontally.
        let gap = (SPLITTER_WIDTH * s).round();
        let available = (body_h - y0).max(0.0);
        // The terminal or a module's tool window (Git): the same area, one at a time.
        let bottom = self.tools.bottom.is_some();
        let size = if bottom { Some(self.tools.bottom_size) } else { None };
        let (term_h, top_h) = match size {
            None => (0.0, available),
            // All of it: nothing above, no separator.
            Some(tool_windows::BottomSize::Full) => (available, 0.0),
            Some(tool_windows::BottomSize::Half) => {
                let half = ((available - gap) / 2.0).max(0.0).round();
                (half, (available - half - gap).max(0.0))
            }
            Some(tool_windows::BottomSize::Kept) => {
                let term_h = self.bottom_dock_height(available, gap);
                (term_h, (available - term_h - gap).max(0.0))
            }
        };
        let full = size == Some(tool_windows::BottomSize::Full);
        let full_w = (right_edge - strip_w).max(0.0);
        let splitter_w = if bottom && !full { full_w } else { 0.0 };
        self.tools.terminal_splitter.set_bounds(Rect::new(strip_w, y0 + top_h, splitter_w, gap), s);
        let below = if full { 0.0 } else { gap };
        self.tools.bottom_bounds = Rect::new(strip_w, y0 + top_h + below, full_w, term_h);
        self.layout_tool_groups();
        // The left side: the panel open there (the project panel, or one in its place).
        let left_panel = self.tools.sides.iter().any(|p| p.dock == ui_workbench::Dock::Left);
        let (tree_w, splitter_w) = if left_panel {
            ((self.left_dock_width() * s).round().min(w * 0.6), (SPLITTER_WIDTH * s).round())
        } else {
            (0.0, 0.0)
        };
        // Its card, next to the toolbar.
        self.tools.sidebar = Rect::new(strip_w, y0, tree_w, top_h);
        let border = s.round().max(1.0);
        self.tools
            .splitter
            .set_bounds(Rect::new(self.tools.sidebar.max_x(), y0, splitter_w, top_h), s);
        self.status.set_bounds(Rect::new(0.0, body_h, w, status_h), s);

        // The main card: rounded, with margins from the window (after the separator on the left).
        let left = if tree_w > 0.0 { self.tools.sidebar.max_x() + splitter_w } else { strip_w };
        // The right side panel (if open), as a separate card.
        let right_panel = self.tools.sides.iter().any(|p| p.dock == ui_workbench::Dock::Right);
        let side_w = if right_panel {
            (self.right_dock_width() * s).round().min(((right_edge - left) * 0.5).max(0.0))
        } else {
            0.0
        };
        let side_gap = if side_w > 0.0 { (SPLITTER_WIDTH * s).round() } else { 0.0 };
        let right_bounds = Rect::new(right_edge - side_w, y0, side_w, top_h);
        self.tools
            .side_splitter
            .set_bounds(Rect::new(right_bounds.min_x() - side_gap, y0, side_gap, top_h), s);
        // Each side panel's card, and its content below the header the app draws for it.
        let (sidebar, bottom_bounds) = (self.tools.sidebar, self.tools.bottom_bounds);
        for side in &mut self.tools.sides {
            side.bounds = match side.dock {
                ui_workbench::Dock::Left => sidebar,
                // A panel at the bottom: the bottom windows' area (and their header's height).
                ui_workbench::Dock::Bottom => bottom_bounds,
                _ => right_bounds,
            };
            let header_h = if side.dock == ui_workbench::Dock::Bottom {
                PanelHeader::bottom_height(s)
            } else {
                PanelHeader::height(s)
            };
            let b = side.bounds;
            let inner = Rect::new(
                b.min_x() + border,
                b.min_y() + border,
                (b.size.width - 2.0 * border).max(0.0),
                (b.size.height - 2.0 * border).max(0.0),
            );
            let header = if side.panel.header().is_some() { header_h } else { 0.0 };
            side.header.set_bounds(Rect::new(b.min_x(), b.min_y(), b.size.width, header), s);
            side.content = Rect::new(
                inner.min_x(),
                inner.min_y() + header,
                inner.size.width,
                (inner.size.height - header).max(0.0),
            );
        }
        self.card = Rect::new(left, y0, (right_edge - left - side_w - side_gap).max(0.0), top_h);
        let inner = Rect::new(
            self.card.min_x() + border,
            self.card.min_y() + border,
            (self.card.size.width - 2.0 * border).max(0.0),
            (self.card.size.height - 2.0 * border).max(0.0),
        );
        // The editor groups, following the split tree, with a strip (and handle) between them.
        let gap = (SPLIT_GAP * s).round();
        let palette = self.palette.clone();
        let rects = self.editors.split.layout(&self.editors.tree, inner, gap, s, &palette);
        for (g, rect) in rects {
            if g >= self.editors.groups.len() {
                continue;
            }
            self.editors.groups[g].bounds = rect;
            if g == self.editors.current {
                self.layout_group(rect);
            } else {
                self.swap_group(g);
                self.layout_group(rect);
                self.swap_group(g);
            }
        }
    }
}

/// The color of a syntax capture in the theme: first the language-specific variant
/// (`property.json`, `variable.php`), then the general one (`property`, `variable`), each
/// with hierarchical fallback (`function.method` → `function`).
/// A highlight scope as the theme draws it: its color, bold, italic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ScopeStyle {
    pub color: Rgba,
    pub bold: bool,
    pub italic: bool,
}

impl ScopeStyle {
    /// A colored span of `range` in this style.
    pub(super) fn span(self, range: std::ops::Range<usize>) -> ColorSpan {
        ColorSpan { range, color: self.color, bold: self.bold, italic: self.italic }
    }
}

fn scope_color(
    support: &LanguageSupport,
    theme: Option<&ResolvedTheme>,
    key: ScopeKey,
) -> Option<ScopeStyle> {
    let theme = theme?;
    let (language, capture) = support.syntax.scope(key)?;
    // Theme lookup is hierarchical: `property.json` falls back to `property` on its own.
    let style = theme.syntax(&format!("{capture}.{language}"))?;
    // Only bold / italic (no color of its own): the code's color.
    let color = style.foreground.or_else(|| {
        (style.bold || style.italic).then(|| theme.color("editor.foreground")).flatten()
    })?;
    Some(ScopeStyle { color, bold: style.bold, italic: style.italic })
}

/// What the empty screen's files were made for: the query, the project, its files'
/// generation, its recent files (count, latest), its path index and the look (the theme
/// generation, the file icon set and style).
type HomeKey = (
    String,
    kernel::ProjectId,
    u64,
    usize,
    Option<PathBuf>,
    usize,
    (u64, Option<u8>, icons::IconLook),
);

/// The empty screen's files and whether they were found (a filter) or are the recent ones.
type HomeFiles = (Vec<ui_workbench::HomeFile>, bool);

/// How many file names [`FileIcons`] remembers per generation.
const FILE_ICONS_LIMIT: usize = 256;

/// The files' icons and colors by name ([`file_icon`]: the language rules, then the icon
/// set's lookup), remembered for what they were found for: the theme, the file icon set
/// and the icons' look (shared by the windows, changed from any of them). The tabs, the
/// breadcrumbs and the status bar ask for them on every frame.
#[derive(Debug)]
struct FileIcons {
    stamp: (u64, Option<u8>, icons::IconLook),
    icons: kernel::GenerationalCache<Box<str>, (IconId, Rgba)>,
}

/// The tab's viewer (image, preview) and, for previews, the Editor / Split / Preview
/// switcher in the top-right corner.
fn paint_viewer(
    v: &mut TabViewer,
    doc: &Document,
    area: Rect,
    scale: f32,
    painter: ui_workbench::Painter<'_>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    let ui_workbench::Painter { text, scene, palette, icons } = painter;
    let px = |x: f32| (x * scale).round();
    if v.viewer_visible() {
        // The preview gets the text only when the document changed.
        // A preview reading its file itself only hears whether the text is saved.
        if v.kind == ui_workbench::ViewerKind::Preview && v.viewer.reads_file() {
            v.viewer.set_unsaved(doc.is_dirty());
        } else if v.kind == ui_workbench::ViewerKind::Preview
            && v.source_version != Some(doc.version())
        {
            v.viewer.set_source(&doc.buffer().text());
            v.source_version = Some(doc.version());
        }
        // Its separator dragged: painted at the width it was last laid out at, clipped to
        // its half (laid out again every so often, and once released).
        let live = v.split.live_width().filter(|_| v.splits());
        let bounds = live.map_or(v.bounds, |w| {
            Rect::new(v.bounds.min_x(), v.bounds.min_y(), w, v.bounds.size.height)
        });
        let first = scene.layers().len();
        v.viewer.paint(ui_workbench::ViewerPaint {
            bounds,
            scale,
            text,
            scene,
            palette,
            icons,
            focused: true,
            waker,
        });
        // The view's color (its data source's): a thin strip along its top.
        if let Some(accent) = v.viewer.accent(palette) {
            let strip = Rect::new(v.bounds.min_x(), v.bounds.min_y(), v.bounds.size.width, px(2.0));
            scene.push_layer(None).quad(Quad::filled(strip, accent));
        }
        if live.is_some() {
            preview_split::clip_layers(scene, first, v.bounds);
        }
        // The separator over both halves (its handle reaches over them): painted after both.
        if v.splits() {
            v.split.splitter.set_style(SplitterStyle::from_palette(palette));
            v.split.splitter.paint(scene);
        }
    }
    v.toggles.clear();
    if v.kind != ui_workbench::ViewerKind::Preview {
        return;
    }
    let modes = [
        (ViewMode::Editor, "view-editor"),
        (ViewMode::Split, "view-split"),
        (ViewMode::Preview, "view-preview"),
    ];
    let (bw, bh, pad) = (px(28.0), px(24.0), px(3.0));
    let width = bw * modes.len() as f32 + 2.0 * pad;
    let bar =
        Rect::new(area.max_x() - width - px(22.0), area.min_y() + px(10.0), width, bh + 2.0 * pad);
    let layer = scene.push_layer(None);
    layer.quad(
        Quad::filled(bar, palette.popup_background.with_alpha(0.94))
            .rounded(px(8.0))
            .bordered(scale.round().max(1.0), palette.popup_border),
    );
    for (i, (mode, icon)) in modes.into_iter().enumerate() {
        let r = Rect::new(bar.min_x() + pad + i as f32 * bw, bar.min_y() + pad, bw, bh);
        let active = v.mode == mode;
        if active {
            layer.quad(Quad::filled(r, palette.accent.with_alpha(0.14)).rounded(px(6.0)));
        }
        if let Some(id) = icons.id(icon) {
            let size = px(15.0);
            let ir =
                Rect::new(r.min_x() + (bw - size) / 2.0, r.min_y() + (bh - size) / 2.0, size, size);
            draw_icon(
                layer,
                icons,
                id,
                ir,
                if active { palette.accent } else { palette.text_muted },
            );
        }
        v.toggles.push((r, mode));
    }
}

/// The language level menu (built by the app as a native menu).
#[derive(Debug, Clone)]
pub struct LevelMenu {
    pub language: LanguageId,
    pub name: String,
    pub levels: Vec<&'static str>,
    /// The project's level.
    pub project: String,
    /// The active file's own level, if it has one.
    pub file: Option<String>,
    pub file_name: Option<String>,
    pub auto: bool,
    pub detected: Option<String>,
    /// Where the detected level comes from (e.g. `composer.json`, `php 8.3.6`).
    pub source: Option<String>,
}

/// What the app must do after a click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseResult {
    None,
    Redraw,
    /// Click in the editor: subsequent moves with the button pressed extend the selection.
    StartDrag,
    Open(PathBuf),
    CloseTab(usize),
    /// An app command (the identifier from `commands.rs`), e.g. "file.new".
    Command(&'static str),
    /// A choice in settings (theme etc.), applied by the app.
    Settings(SettingsAction),
    /// A project from the recent list.
    OpenProject(PathBuf),
    /// A past session's windows reopened, one after the other (plan 78).
    RestoreSession(u128),
    /// A menu command (from our context menu), handled like the native menus' commands.
    MenuCommand(String),
    /// Text for the clipboard (e.g. from the "Copy" menu).
    CopyText(String),
    /// Shows the path in the system's file manager.
    Reveal(PathBuf),
    /// Bring this window to the front (another project's).
    FocusWindow(u64),
    /// This window's project takes this number (it swaps with the one that had it).
    NumberProject(u8),
    /// An external link (from a preview): opened in the browser.
    OpenUrl(String),
    /// A viewer's request for the app (e.g. `theme.preview` from the configurator).
    ViewerRequest(ui_workbench::AppRequest),
    /// Click on an empty area of the top bar: moves the window; double click: maximizes.
    DragWindow,
    ToggleMaximize,
    /// The window's own buttons (Windows, drawn in the top bar).
    MinimizeWindow,
    CloseWindow,
}

/// The prefix of the virtual tabs' extensions (modules' views use it too).
pub const VIRTUAL_EXT_PREFIX: &str = "view-";

/// The information in the About window.
fn about_lines(modules: usize) -> Vec<String> {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    vec![
        i18n::t("about.tagline"),
        i18n::tr!("about.modules", count = modules as i64),
        format!("{os} · {} · wgpu", std::env::consts::ARCH),
    ]
}

/// What a choice from the project list does.
fn picker_result(action: ProjectsAction) -> MouseResult {
    match action {
        ProjectsAction::OpenProject => MouseResult::Command("project.open"),
        ProjectsAction::OpenRemote => MouseResult::Command(workbench_model::OPEN_REMOTE_PROJECT.0),
        ProjectsAction::SearchProject => MouseResult::Command("project.search"),
        ProjectsAction::OpenPath(path) => MouseResult::Open(path),
        ProjectsAction::OpenFile => MouseResult::Command("file.open"),
        ProjectsAction::NewFile => MouseResult::Command("file.new"),
        ProjectsAction::OpenRecent(path) => MouseResult::OpenProject(path),
        ProjectsAction::RestoreSession(id) => MouseResult::RestoreSession(id),
        ProjectsAction::CloseProject => MouseResult::Command(crate::commands::CLOSE_PROJECT.0),
        // Opened by its module, before this result (`Workbench::picker_action`).
        ProjectsAction::EditProject(_) => MouseResult::Redraw,
        ProjectsAction::Close
        | ProjectsAction::ToggleGroup(_)
        | ProjectsAction::MoveProject { .. }
        | ProjectsAction::MoveGroup { .. }
        | ProjectsAction::NewGroup
        | ProjectsAction::ToggleSession(_)
        | ProjectsAction::ToggleSessionWindow(..) => MouseResult::Redraw,
    }
}

/// A test's folder as the app keeps paths: canonical (macOS' `/private/var`), in Windows'
/// usual form (`C:\…`, not the `\\?\C:\…` `canonicalize` gives).
#[cfg(test)]
pub(crate) fn canonical(path: &Path) -> PathBuf {
    disk::simplify_path(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
}

#[cfg(test)]
#[path = "../../tests/unit/workbench.rs"]
pub(crate) mod tests;

/// The system's cursor for what a view asks over one of its parts.
pub(crate) fn cursor_icon(cursor: ui_workbench::ViewerCursor) -> CursorIcon {
    use ui_workbench::ViewerCursor as C;
    match cursor {
        C::Arrow => CursorIcon::Default,
        C::Pointer => CursorIcon::Pointer,
        C::Text => CursorIcon::Text,
        C::ResizeHorizontal => CursorIcon::ResizeHorizontal,
        C::ResizeVertical => CursorIcon::ResizeVertical,
    }
}

/// The most characters of the active file's and of the project's names in the window's
/// title (cut in the middle beyond).
const TITLE_FILE: usize = 48;
const TITLE_PROJECT: usize = 40;
