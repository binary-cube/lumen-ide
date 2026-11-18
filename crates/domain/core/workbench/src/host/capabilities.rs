//! What only some modules use of the workbench, each capability its own trait: the views
//! and panels, the active editor, the refactorings, the shared services, the desktop.
//! [`WorkbenchHost`](crate::WorkbenchHost) has them all (its supertraits). Every method
//! has a default ("not here"), so a host without a capability (a test's fake) implements it
//! empty.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use diff::CompareInput;

use super::ToolBadge;

/// Opening modules' views (tabs, the group below, tool windows, modals, overlays) and
/// telling the tool windows.
pub trait ViewHost {
    /// Opens a module's view in a tab named `title`: the viewer contributed for files with
    /// `extension`, given `data`.
    fn open_view(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool;
    /// Brings the tab of a module's view with `extension` to the front, in whichever editor
    /// group it is (that group made current): `false` if none is open (then
    /// [`Self::open_view`] opens one). A view meant once per window (the Task Manager).
    fn focus_view(&mut self, extension: &str) -> bool {
        let _ = extension;
        false
    }
    /// Opens a module's view in a tab of the editor's group below (split under the editor
    /// the first time; the next ones join it while it's open), read beside the code (a
    /// refactoring's preview, Find Usages): the code it opens goes to the group above. A host
    /// without editor groups opens it in a tab.
    fn open_view_below(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        self.open_view(title, extension, data)
    }
    /// Shows a module's view in its tool window `tool` at the bottom (as the Git log), under
    /// the editor; a host without tool windows opens it in a tab.
    fn show_in_panel(
        &mut self,
        tool: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let _ = tool;
        self.open_view(title, extension, data)
    }
    /// [`Self::show_in_panel`] with the view found by `key` (the same in every language)
    /// rather than by its title.
    fn show_in_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let _ = key;
        self.show_in_panel(tool, title, extension, data)
    }
    /// [`Self::show_in_panel_keyed`] without showing it: the view made (unless `tool` has one
    /// by `key`) behind the ones shown — nothing else changes (a window's fixed tab made
    /// again when a session lacks it). `false`: not made (a host without tool windows).
    fn add_to_panel_keyed(
        &mut self,
        tool: &str,
        key: &str,
        title: &str,
        extension: &str,
        data: &dyn std::any::Any,
    ) -> bool {
        let _ = (tool, key, title, extension, data);
        false
    }
    /// Tool window `tool` at the bottom has a view shown as `title` (in any of its groups,
    /// even renamed since). A host without tool windows: `false`.
    fn in_panel(&self, tool: &str, title: &str) -> bool {
        let _ = (tool, title);
        false
    }
    /// Opens a module's view in a modal window over everything (a wizard): it alone gets
    /// the keyboard and the mouse until it closes (× / Esc / its buttons, never a click
    /// outside); a host without modals opens it in a tab.
    fn open_modal(&mut self, title: &str, extension: &str, data: &dyn std::any::Any) -> bool {
        self.open_view(title, extension, data)
    }
    /// Opens a module's view bare over the whole window — it draws all of it, the app adds no
    /// frame: a modal (nothing behind it gets anything until it closes) or a popover (it
    /// has the keyboard; it closes itself, e.g. on a click outside it). `false`: a host
    /// without overlays.
    fn open_overlay(&mut self, extension: &str, data: &dyn std::any::Any, popover: bool) -> bool {
        let _ = (extension, data, popover);
        false
    }
    /// Opens bare view `extension` over every window of the app at once, as one (a dimmed
    /// screen, a calm scene: what stands for the whole app, not a window): here with `data`
    /// (as [`Self::open_overlay`], a modal), in the others without; closed in any, it closes
    /// in all. A window with another view over it keeps that one. A host of one window: as
    /// [`Self::open_overlay`].
    fn open_overlay_everywhere(&mut self, extension: &str, data: &dyn std::any::Any) -> bool {
        self.open_overlay(extension, data, false)
    }
    /// The same, on the whole screen (a calm scene): one window a screen shows it — the
    /// host chooses which: several instances of the app on one screen show it once — the
    /// others leave theirs as they are; a key or a click in any closes it everywhere. A host
    /// of one window: as [`Self::open_overlay_everywhere`].
    fn open_full_screen_everywhere(&mut self, extension: &str) -> bool {
        self.open_overlay_everywhere(extension, &())
    }
    /// Opens a comparison in a tab named `title`. `false`: a host without comparisons.
    fn compare(&mut self, input: CompareInput, title: &str) -> bool {
        let _ = (input, title);
        false
    }
    /// Shows / hides tool window `id` (as its button does); a host without tool windows:
    /// `false`.
    fn toggle_tool(&mut self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// Shows the right side panel `id` (a module's, e.g. the Task Manager), opened if it
    /// isn't; a host without side panels: `false`.
    fn show_side_panel(&mut self, id: &str) -> bool {
        let _ = id;
        false
    }
    /// Gives tool window `tool`'s side panel `data` (its `set_data`; shown first if it
    /// isn't), the keyboard too if `focus`. `false`: no such panel.
    fn tell_panel(&mut self, tool: &str, data: &dyn std::any::Any, focus: bool) -> bool {
        let _ = (tool, data, focus);
        false
    }
    /// Puts a count on tool window `tool`'s button (`None`: none), for this project.
    fn set_tool_badge(&mut self, tool: &'static str, badge: Option<ToolBadge>) {
        let _ = (tool, badge);
    }
}

/// The editor's text: the active file's (editing, saving, its document, the type at its
/// cursor) and any file's as the editor has it.
pub trait EditorHost {
    /// File `path`'s text as the editor has it (unsaved changes included), else as on disk.
    fn file_text(&self, path: &Path) -> Option<String> {
        let _ = path;
        None
    }
    /// The active document is locked (read-only): a change about to be made to it (a
    /// formatter's run on its file) is refused, the user told. `true`: refused, do nothing.
    fn refuse_if_locked(&mut self) -> bool {
        false
    }
    /// Replaces `range` (bytes) of the active document with `text`, as one undo step
    /// (`false`, nothing changed: it's locked).
    fn edit_active(&mut self, range: std::ops::Range<usize>, text: &str) -> bool {
        let _ = (range, text);
        false
    }
    /// Saves the active file if it has changes (as ⌘S). `false` if it couldn't (or there's
    /// none).
    fn save_active(&mut self) -> bool {
        false
    }
    /// Selects `range` (bytes) of the active document, shown, the editor given the keys.
    fn select_active(&mut self, range: std::ops::Range<usize>) -> bool {
        let _ = range;
        false
    }
    /// Opens file `path` in a tab of the editor (activated if it's open). `false`: it
    /// couldn't be (or a host without an editor).
    fn open_file(&mut self, path: &Path) -> bool {
        let _ = path;
        false
    }
    /// File `path` opened, its view given `data` (what it's asked to show).
    fn open_file_with(&mut self, path: &std::path::Path, data: &dyn std::any::Any) -> bool {
        let _ = data;
        self.open_file(path)
    }
    /// The active tab's document, unless a menu acts on another entry (a tree's file): what
    /// a module keeps per document (its decorations, see [`crate::LineDecorator`]) it finds
    /// by it.
    fn active_document(&self) -> Option<kernel::DocumentId> {
        None
    }
    /// The FQN of the type (class, interface, trait, enum) named at the cursor, or containing
    /// it.
    fn type_at_cursor(&mut self) -> Option<String> {
        None
    }
}

/// What the refactorings need of the workbench: what they act on (with the project's
/// sources and the language's resolver) and applying / undoing their plans. `Err` with an
/// empty message: a host without refactorings.
pub trait RefactoringHost {
    /// What a rename at the cursor would rename, with the project's sources and the
    /// language's resolver (usable on a background thread). `Err`: why nothing can be
    /// renamed there (a message for the user).
    fn rename_at_cursor(&mut self) -> Result<refactor::application::RenameContext, String> {
        Err(String::new())
    }
    /// What Find Usages at the cursor looks for: the symbol a rename there would rename, but
    /// its places found as Find Usages finds them (a language server's references, where
    /// one answers them, beside the built-in engine's). By default the rename's.
    fn usages_at_cursor(&mut self) -> Result<refactor::application::RenameContext, String> {
        self.rename_at_cursor()
    }
    /// What a Change Signature at the cursor changes (a function, a method, a class's
    /// constructor), with the ports to find its calls. `Err`: why not, for the user.
    fn change_signature_at_cursor(
        &mut self,
    ) -> Result<refactor::signature::SignatureContext, String> {
        Err(String::new())
    }
    /// What moving the menu's target (else the active file) changes: the file / folder and,
    /// following it, its classes' namespaces (PSR-4). `Err`: why not, for the user.
    fn move_context(&mut self) -> Result<refactor::moving::MoveContext, String> {
        Err(String::new())
    }
    /// What moving `from` into folder `into` changes (the tree's drop). `Err`: why not.
    fn move_context_to(
        &mut self,
        from: &Path,
        into: &Path,
    ) -> Result<refactor::moving::MoveContext, String> {
        let _ = (from, into);
        Err(String::new())
    }
    /// What renaming the type declared in file `path` and named after it renames (its
    /// language keeps one type per file, PSR-4): `None` if the file holds no such type.
    fn rename_type_of_file(&mut self, path: &Path) -> Option<refactor::application::RenameContext> {
        let _ = path;
        None
    }
    /// Applies a refactoring's chosen changes: open files are edited (unsaved, one undo step
    /// each), closed ones written; files are renamed. Returns how many places changed.
    fn apply_refactoring(&mut self, plan: &refactor::Plan) -> Result<usize, String> {
        let _ = plan;
        Err(String::new())
    }
    /// Undoes the last applied refactoring (every file, renames included): how many files
    /// were left as they are because they changed since (never overwritten). `None` if
    /// there is none.
    fn undo_refactoring(&mut self) -> Option<usize> {
        None
    }
}

/// The models the host shares with modules: the project's queries, what the app's parts
/// cost, the themes. `None`: a host without it.
pub trait ServicesHost {
    /// The project's queries (paths, declarations, text, files), as a snapshot usable on a
    /// background thread; `None` without a project.
    fn queries(&mut self) -> Option<Arc<dyn crate::ProjectQueries>> {
        None
    }
    /// Where the Task Manager reads what the app's parts cost (none: no Task Manager).
    fn resources(&self) -> Option<Arc<dyn crate::ResourceSource>> {
        None
    }
    /// The themes (id, name, text, without the preview) and the one in use, for the theme
    /// configurator; `None`: a host without themes.
    fn themes(&self) -> Option<theming::studio::StudioInput> {
        None
    }
}

/// The desktop around the workbench: the system's clipboard, its file manager, other
/// projects.
pub trait SystemHost {
    /// The text on the system clipboard, if any.
    fn clipboard_text(&self) -> Option<String> {
        None
    }
    /// Puts `text` in the system's clipboard.
    fn set_clipboard_text(&mut self, text: &str) {
        let _ = text;
    }
    /// The files on the system's clipboard (copied in the file manager).
    fn clipboard_files(&self) -> Vec<PathBuf> {
        Vec::new()
    }
    /// Puts `paths` on the system's clipboard (pasteable in the file manager; as text too).
    fn set_clipboard_files(&mut self, paths: &[PathBuf]) {
        let _ = paths;
    }
    /// Shows `path` in the system's file manager (Finder, Explorer).
    fn reveal_in_file_manager(&mut self, path: &Path) {
        let _ = path;
    }
    /// Shows `path` selected in the project's tree (Select in Project View). `false`: no
    /// project's tree here.
    fn reveal_in_project(&mut self, path: &Path) -> bool {
        let _ = path;
        false
    }
    /// Opens the project at `root` (asking where, if this window has one already).
    fn open_project(&mut self, root: &Path) -> bool {
        let _ = root;
        false
    }
    /// Opens the settings at page `page` (a module's own page, by its id): its "Settings…"
    /// from its button's menu. A host without settings: `false`.
    fn open_settings_page(&mut self, page: &str) -> bool {
        let _ = page;
        false
    }
    /// The projects open in the app's windows, by their numbers (this one's marked).
    fn open_projects(&self) -> Vec<crate::ProjectWindow> {
        Vec::new()
    }
    /// This window's project takes number `number` (1–9) — kept with it; the project that
    /// had it gets this one's old number (they swap). `false`: a host without windows.
    fn set_project_number(&mut self, number: u8) -> bool {
        let _ = number;
        false
    }
    /// A picture of this window as it is now (before a view opens over it), kept with the
    /// others ([`crate::pictures`]) once read. `false`: a host without windows.
    fn capture_window(&mut self) -> bool {
        false
    }
    /// Brings window `window`'s project to the front (its window focused). `false`: no
    /// such window.
    fn focus_project(&mut self, window: u64) -> bool {
        let _ = window;
        false
    }
    /// Closes the app as Quit does — the work still running and the unsaved files asked
    /// about first, Cancel keeping everything — then puts the update staged in `staged` in
    /// its place and starts it again with its windows. `false`: a host that can't.
    fn restart_to_update(&mut self, staged: &Path) -> bool {
        let _ = staged;
        false
    }
}
