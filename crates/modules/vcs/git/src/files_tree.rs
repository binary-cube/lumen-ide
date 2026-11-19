//! Git's changed files in the shared tree ([`ui_components::changed_files`]): a file's state
//! as the tree shows it, and where the Tree / List choice is kept.

use i18n::t;
use ui_components::changed_files::ChangeState;
pub use ui_components::changed_files::{ChangedFile, FilesTree, TreeEvent, draw_lit};
use version_control::FileState;

/// The key of the Tree / List choice in the project's sizes (1: a list).
pub const LIST_MODE: &str = "git.changed-files-list";

/// A file of git's, as the tree shows it (`old_path`: where it was, for a rename).
pub fn changed(path: &str, old_path: Option<&str>, state: FileState) -> ChangedFile {
    let state = change_state(state);
    ChangedFile { old_path: old_path.map(str::to_owned), ..ChangedFile::new(path, state) }
}

/// Git's state as the lists' (their badge's).
pub(crate) fn change_state(state: FileState) -> ChangeState {
    match state {
        FileState::Modified => ChangeState::Modified,
        FileState::Added => ChangeState::Added,
        FileState::Untracked => ChangeState::Untracked,
        FileState::Deleted => ChangeState::Deleted,
        FileState::Renamed => ChangeState::Renamed,
        FileState::Conflicted => ChangeState::Conflicted,
    }
}

/// The state in words (a badge's tooltip).
pub(crate) fn state_name(state: FileState) -> String {
    t(match state {
        FileState::Modified => "git.state_modified",
        FileState::Added => "git.state_added",
        FileState::Untracked => "git.state_untracked",
        FileState::Deleted => "git.state_deleted",
        FileState::Renamed => "git.state_renamed",
        FileState::Conflicted => "git.state_conflicted",
    })
}
