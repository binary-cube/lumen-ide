use super::*;

/// Only a program from the servers' folder whose parent is gone is left behind: one with
/// its app alive (another copy's), or from elsewhere, never.
#[test]
fn only_servers_without_their_app_are_left_behind() {
    let dir = Path::new("/data/lsp");
    let server = Path::new("/data/lsp/php/8.4.22/php");
    assert!(is_left_behind(Some(server), false, dir));
    assert!(!is_left_behind(Some(server), true, dir), "its app is running");
    assert!(!is_left_behind(Some(Path::new("/usr/bin/php")), false, dir), "not ours");
    assert!(!is_left_behind(Some(Path::new("/data/lsp-other/x")), false, dir), "a neighbour");
    assert!(!is_left_behind(None, false, dir), "unknown program");
}
