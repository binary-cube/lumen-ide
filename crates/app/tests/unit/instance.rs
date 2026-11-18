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
