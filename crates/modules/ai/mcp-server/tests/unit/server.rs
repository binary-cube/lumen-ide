use super::*;
use serde_json::json;

/// A path an agent gives: in the project (relative or absolute), never out of it.
#[test]
fn a_path_stays_in_the_project() {
    let root = Path::new("/work/p");
    assert_eq!(inside(root, "src/a.php"), Some(PathBuf::from("/work/p/src/a.php")));
    assert_eq!(inside(root, "/work/p/src/a.php"), Some(PathBuf::from("/work/p/src/a.php")));
    assert_eq!(inside(root, "../q/a.php"), None);
    assert_eq!(inside(root, "src/../../etc/passwd"), None);
    assert_eq!(inside(root, "/etc/passwd"), None);
    assert_eq!(inside(root, "/work/pp/a.php"), None, "another folder starting the same");
}

/// A call about a file goes to the window whose project holds it; a call about none, or
/// about a file of no open project, to the window worked in last (that one closed: the
/// first open).
#[test]
fn a_call_goes_to_its_project_s_window() {
    let (a, b) = (kernel::ProjectId::new(), kernel::ProjectId::new());
    let windows = [(a, PathBuf::from("/a")), (b, PathBuf::from("/b"))];
    let to = |path: Option<&str>, id, root: &str, focused| {
        for_window(path.map(Path::new), id, Path::new(root), &windows, focused)
    };
    assert!(to(Some("/a/x.php"), a, "/a", Some(b)), "its own project's, focused or not");
    assert!(!to(Some("/b/x.php"), a, "/a", Some(a)));
    assert!(to(None, b, "/b", Some(b)) && !to(None, a, "/a", Some(b)), "to the focused one");
    assert!(to(Some("/c/x.php"), b, "/b", Some(b)) && !to(Some("/c/x.php"), a, "/a", Some(b)));
    let gone = kernel::ProjectId::new();
    assert!(to(None, a, "/a", Some(gone)) && !to(None, b, "/b", Some(gone)), "the first open");
    // Absolute as the system has them (a drive on Windows).
    let absolute = if cfg!(windows) { r"C:\a\x" } else { "/a/x" };
    assert_eq!(path_of(&json!({"path": absolute})), Some(PathBuf::from(absolute)));
    assert_eq!(path_of(&json!({"path": "rel"})), None);
}
