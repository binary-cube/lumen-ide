use super::*;

/// The running instance takes a later start's paths; with nobody running, the start runs.
#[test]
fn a_later_start_hands_its_paths_to_the_running_one() {
    let tag = format!("test-{}", std::process::id());
    let none = hand_over(&tag, &[]);
    assert!(
        none.as_ref().is_err_and(|why| why.starts_with("no channel")),
        "nobody running: {none:?}"
    );
    listen(&tag).unwrap();
    let dir = std::env::temp_dir();
    let paths = vec![dir.join("project"), dir.join("a.php")];
    assert_eq!(hand_over(&tag, &paths), Ok(()));
    assert_eq!(hand_over(&tag, &[]), Ok(()), "nothing: the app to the front");
    assert_eq!(take(), [paths, Vec::new()]);
    // The test's socket (the listener stays until the process ends).
    #[cfg(unix)]
    let _ = std::fs::remove_file(socket_dir().join(format!("editor-instance-{tag}.sock")));
}

#[test]
fn the_channel_is_named_after_the_executable() {
    assert_eq!(fnv(Path::new("/a/editor")), fnv(Path::new("/a/editor")));
    assert_ne!(fnv(Path::new("/a/editor")), fnv(Path::new("/b/editor")));
    let (tag, exe) = executable_tag().unwrap();
    assert_eq!(tag.len(), 16);
    assert!(!exe.is_empty());
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

/// Paths the system asks the app to open (macOS' "Open With") are queued as a later
/// start's: taken once, in order.
#[test]
fn paths_the_system_opens_are_queued_as_a_later_start_s() {
    let _ = take();
    hand_in(vec![PathBuf::from("/a.txt")]);
    hand_in(vec![PathBuf::from("/b.txt"), PathBuf::from("/c")]);
    let taken = take();
    let ours: Vec<_> = taken
        .iter()
        .filter(|p| p.first().is_some_and(|f| f.starts_with("/a.txt") || f.starts_with("/b.txt")))
        .collect();
    assert_eq!(ours.len(), 2);
    assert_eq!(ours[1].len(), 2);
}
