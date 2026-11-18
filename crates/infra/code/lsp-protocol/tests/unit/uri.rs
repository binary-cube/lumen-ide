use std::path::{Path, PathBuf};

use super::*;

#[test]
fn a_unix_path_round_trips() {
    let path = Path::new("/Users/me/my project/src/ăț 😀.ts");
    let uri = from_path(path);
    assert_eq!(uri, "file:///Users/me/my%20project/src/%C4%83%C8%9B%20%F0%9F%98%80.ts");
    assert_eq!(to_path(&uri), Some(path.to_path_buf()));
}

#[test]
fn a_windows_drive_is_written_and_read_either_way() {
    assert_eq!(from_path(Path::new("C:\\work\\app\\main.ts")), "file:///C:/work/app/main.ts");
    let expected = if cfg!(windows) { "C:\\work\\a.ts" } else { "C:/work/a.ts" };
    assert_eq!(to_path("file:///C:/work/a.ts"), Some(PathBuf::from(expected)));
    // `vscode-uri` lowers the drive and encodes its colon.
    assert_eq!(to_path("file:///c%3A/work/a.ts"), Some(PathBuf::from(expected)));
}

#[test]
fn reserved_characters_are_encoded() {
    let uri = from_path(Path::new("/a/#b?c%d[e].ts"));
    assert_eq!(uri, "file:///a/%23b%3Fc%25d%5Be%5D.ts");
    assert_eq!(to_path(&uri), Some(PathBuf::from("/a/#b?c%d[e].ts")));
}

#[test]
fn other_uris_arent_paths() {
    assert_eq!(to_path("untitled:Untitled-1"), None);
    assert_eq!(to_path("https://example.com/a.ts"), None);
    assert_eq!(to_path("file:///bad%zz"), None);
    assert_eq!(to_path("file://localhost/etc/hosts"), Some(PathBuf::from("/etc/hosts")));
}
