//! Git's changed files in the shared tree ([`ui_components::changed_files`]): a file's state
//! as the tree shows it, and where the Tree / List choice is kept.

use ui_components::changed_files::ChangeState;
pub use ui_components::changed_files::{ChangedFile, FilesTree, TreeEvent, draw_lit};
use version_control::FileState;

/// The key of the Tree / List choice in the project's sizes (1: a list).
pub const LIST_MODE: &str = "git.changed-files-list";

/// A file of git's, as the tree shows it (`old_path`: where it was, for a rename).
pub fn changed(path: &str, old_path: Option<&str>, state: FileState) -> ChangedFile {
    let state = match state {
        FileState::Added | FileState::Untracked => ChangeState::Added,
        FileState::Modified => ChangeState::Modified,
        FileState::Deleted => ChangeState::Deleted,
        FileState::Renamed => ChangeState::Renamed,
        FileState::Conflicted => ChangeState::Conflicted,
    };
    ChangedFile { old_path: old_path.map(str::to_owned), ..ChangedFile::new(path, state) }
}
