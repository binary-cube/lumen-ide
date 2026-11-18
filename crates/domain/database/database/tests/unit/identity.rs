use std::path::Path;

use super::*;

#[test]
fn unix_paths_are_resolved_lexically() {
    let unix = |t| normalize(t, PathRules::Unix);
    assert_eq!(unix("/a/./b/../c.db"), "/a/c.db");
    assert_eq!(unix("/a//b///c"), "/a/b/c");
    assert_eq!(unix("/../a"), "/a");
    assert_eq!(unix("a/../../b"), "../b");
    // The case is kept.
    assert_ne!(unix("/A/b"), unix("/a/b"));
}

#[test]
fn windows_paths_ignore_case_and_separators() {
    let win = |t| normalize(t, PathRules::Windows);
    assert_eq!(win(r"C:\Users\Ana\Shop.SQLite"), r"c:\users\ana\shop.sqlite");
    assert_eq!(win("C:/Users/ana/shop.sqlite"), win(r"c:\USERS\ana\shop.sqlite"));
    assert_eq!(win(r"C:\a\.\b\..\c.db"), r"c:\a\c.db");
    assert_eq!(win(r"C:\..\a"), r"c:\a");
    assert_eq!(win(r"C:\a\\b"), r"c:\a\b");
}

#[test]
fn windows_verbatim_prefixes_are_taken_off() {
    let win = |t| normalize(t, PathRules::Windows);
    assert_eq!(win(r"\\?\C:\Data\shop.db"), win(r"C:\data\shop.db"));
    assert_eq!(win(r"\\?\UNC\server\share\x.db"), r"\\server\share\x.db");
    assert_eq!(win(r"\\Server\Share\..\x.db"), r"\\server\share\x.db");
}

#[test]
fn keys_follow_the_rules() {
    let a = FileKey::with_rules(Path::new("/tmp/./x/../shop.db"), PathRules::Unix);
    assert_eq!(a, FileKey::with_rules(Path::new("/tmp/shop.db"), PathRules::Unix));
    let w = FileKey::with_rules(Path::new(r"\\?\C:\Tmp\Shop.db"), PathRules::Windows);
    assert_eq!(w, FileKey::with_rules(Path::new("c:/tmp/shop.db"), PathRules::Windows));
    assert_eq!(FileKey::lexical(Path::new("a/./b/../c.db")), FileKey::lexical(Path::new("a/c.db")));
}
