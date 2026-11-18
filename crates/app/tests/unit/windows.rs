//! What a past session's window reopens with: its project (if it still exists and isn't
//! open already), or its files alone that still exist.

use super::*;
use workspace::sessions::SessionWindow;

#[test]
fn a_session_window_reopens_what_still_exists_and_isnt_open() {
    let root = std::env::temp_dir().join(kernel::id::new_uuid().to_string());
    let project = root.join("shop");
    std::fs::create_dir_all(&project).unwrap();
    let note = root.join("note.md");
    std::fs::write(&note, "x").unwrap();
    let of = |project: Option<&std::path::Path>, files: &[&std::path::Path]| SessionWindow {
        project: project.map(std::path::Path::to_path_buf),
        files: files.iter().map(|f| f.to_path_buf()).collect(),
    };
    let gone = root.join("gone.md");
    let found = existing(vec![
        of(Some(&project), &[]),
        of(Some(&root.join("gone")), &[]),
        of(None, &[&note, &gone]),
        of(None, &[&gone]),
    ]);
    assert_eq!(found, vec![of(Some(&project), &[]), of(None, &[&note])], "what still exists");
    assert_eq!(reopen_paths(&found[0], &[]), Some(vec![project.clone()]));
    let open = [(WindowId(1), project)];
    assert_eq!(reopen_paths(&found[0], &open), None, "already open");
    assert_eq!(reopen_paths(&found[1], &[]), Some(vec![note]));
    std::fs::remove_dir_all(&root).unwrap();
}
