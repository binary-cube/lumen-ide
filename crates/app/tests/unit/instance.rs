use super::*;

/// The running instance takes a later start's paths; with nobody running, the start runs.
#[test]
fn a_later_start_hands_its_paths_to_the_running_one() {
    let tag = format!("test-{}", std::process::id());
    assert!(!hand_over(&tag, &[]), "nobody running");
    listen(&tag).unwrap();
    let dir = std::env::temp_dir();
    let paths = vec![dir.join("project"), dir.join("a.php")];
    assert!(hand_over(&tag, &paths));
    assert!(hand_over(&tag, &[]), "nothing: the app to the front");
    assert_eq!(take(), [paths, Vec::new()]);
    // The test's socket (the listener stays until the process ends).
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_dir().join(format!("editor-instance-{tag}.sock")));
}

#[test]
fn the_channel_is_named_after_the_executable() {
    assert_eq!(fnv(Path::new("/a/editor")), fnv(Path::new("/a/editor")));
    assert_ne!(fnv(Path::new("/a/editor")), fnv(Path::new("/b/editor")));
    assert_eq!(executable_tag().unwrap().len(), 16);
}

/// On Windows every spelling of the program's path is one channel: the long-path prefix,
/// either slash, any case, a trailing separator; elsewhere a path is as it is.
#[test]
fn a_path_has_one_form_on_windows() {
    let one = one_form(r"C:\Program Files\Editor\editor.exe", true);
    for other in [
        r"\\?\C:\Program Files\Editor\editor.exe",
        r"c:\program files\editor\EDITOR.EXE",
        "C:/Program Files/Editor/editor.exe",
        r"C:\Program Files\Editor\editor.exe\",
    ] {
        assert_eq!(one_form(other, true), one, "{other}");
    }
    assert_eq!(one_form(r"\\?\UNC\server\share\editor.exe", true), r"\\server\share\editor.exe");
    assert_eq!(one_form("/Apps/Editor", false), "/Apps/Editor", "elsewhere: as it is");
}
