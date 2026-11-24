//! What a click asks of the app ([`MouseResult`]), a project list's choice turned into one,
//! and the system's cursor for what a view asks.

use std::path::PathBuf;

use platform_api::CursorIcon;
use ui_workbench::{ProjectsAction, SettingsAction};

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

/// What a choice from the project list does.
pub(super) fn picker_result(action: ProjectsAction) -> MouseResult {
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
