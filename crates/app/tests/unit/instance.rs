use super::*;

/// The handed paths' queue is the process's: its tests take turns.
static QUEUE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The running instance takes a later start's paths; with nobody running, the start runs.
#[test]
fn a_later_start_hands_its_paths_to_the_running_one() {
    let _turn = QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let given: Vec<Vec<PathBuf>> =
        take().into_iter().map(|h| h.into_iter().map(|h| h.path).collect()).collect();
    assert_eq!(given, [paths, Vec::new()]);
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
    let _turn = QUEUE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::env::temp_dir().join(format!("instance-handed-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("project")).unwrap();
    std::fs::write(dir.join("a.txt"), "").unwrap();
    let _ = take();
    hand_in(vec![dir.join("a.txt")]);
    hand_in(vec![dir.join("project"), dir.join("gone.txt")]);
    // Looked at on a worker, in order.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut ours = Vec::new();
    while ours.len() < 2 && std::time::Instant::now() < until {
        let taken =
            take().into_iter().filter(|h| h.first().is_some_and(|f| f.path.starts_with(&dir)));
        ours.extend(taken);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert_eq!(ours.len(), 2, "{ours:?}");
    assert!(!ours[0][0].is_dir);
    assert_eq!(ours[0][0].real, std::fs::canonicalize(dir.join("a.txt")).unwrap());
    assert!(ours[1][0].is_dir, "a folder: a project");
    assert_eq!(ours[1][1].real, dir.join("gone.txt"), "not there: as given");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A handed file opens in its project's window (the deepest project holding it), else in
/// the frontmost; with no window, nowhere.
#[test]
fn a_handed_file_goes_to_its_projects_window_or_the_front_one() {
    let (outer, inner) = (Path::new("/work/site"), Path::new("/work/site/vendor/lib"));
    let at = |window, root, focus_turn| Candidate { window, root, focus_turn };
    let windows = [at(1, Some(outer), 3), at(2, None, 7), at(3, Some(inner), 1)];
    assert_eq!(window_for(Path::new("/work/site/index.php"), &windows), Some(1));
    assert_eq!(window_for(Path::new("/work/site/vendor/lib/a.php"), &windows), Some(3));
    assert_eq!(window_for(Path::new("/elsewhere/b.txt"), &windows), Some(2), "the front one");
    assert_eq!(window_for(Path::new("/work/sitemap.xml"), &windows), Some(2), "not a prefix");
    assert_eq!(window_for::<u8>(Path::new("/a"), &[]), None);
}
