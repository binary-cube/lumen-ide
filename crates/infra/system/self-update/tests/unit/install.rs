use super::*;

fn temp() -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("self-update-install-{}", kernel::id::uuid_text(kernel::id::new_uuid())));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A staged update of a program in `root/updates/2.0.0`, and the app at `root/app/program`.
fn staged(root: &Path, app_is_folder: bool) -> (PathBuf, PathBuf) {
    let dir = root.join("updates").join("2.0.0");
    let unpacked = dir.join("app");
    std::fs::create_dir_all(&unpacked).unwrap();
    let installed = root.join("app");
    std::fs::create_dir_all(&installed).unwrap();
    let (new, target) = if app_is_folder {
        let new = unpacked.join("App.app");
        std::fs::create_dir_all(new.join("Contents")).unwrap();
        std::fs::write(new.join("Contents").join("program"), b"new").unwrap();
        let target = installed.join("App.app");
        std::fs::create_dir_all(target.join("Contents")).unwrap();
        std::fs::write(target.join("Contents").join("program"), b"old").unwrap();
        (new, target)
    } else {
        let new = unpacked.join("program");
        std::fs::write(&new, b"new").unwrap();
        let target = installed.join("program");
        std::fs::write(&target, b"old").unwrap();
        (new, target)
    };
    let rel = new.strip_prefix(&dir).unwrap().to_string_lossy().into_owned();
    // Written as the app writes it (a Windows path's `\\` escaped in TOML).
    let app = toml::Value::String(rel);
    std::fs::write(dir.join(stage::STAGED), format!("version = \"2.0.0\"\napp = {app}\n")).unwrap();
    (dir, target)
}

/// The new app takes the old one's place (a program, a bundle alike); the download goes once
/// it's there, the old one waits beside it (still running) for the next start.
#[test]
fn an_update_takes_the_app_s_place() {
    for folder in [false, true] {
        let root = temp();
        let (dir, target) = staged(&root, folder);
        assert_eq!(apply(&dir, &target).unwrap(), "2.0.0");
        let program = if folder { target.join("Contents").join("program") } else { target.clone() };
        assert_eq!(std::fs::read(program).unwrap(), b"new");
        assert!(aside(&target).exists() && !dir.exists(), "the old one aside, the download gone");
        clean_left(&target);
        assert!(!aside(&target).exists(), "gone at the next start");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

/// Nothing staged: the app stays as it was; how an update went is told once.
#[test]
fn nothing_staged_leaves_the_app_and_the_result_is_told_once() {
    let root = temp();
    let target = root.join("program");
    std::fs::write(&target, b"old").unwrap();
    assert!(apply(&root.join("nothing"), &target).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    note(&root, &Outcome { version: "2.0.0".into(), error: Some("disk full".into()) }).unwrap();
    let told = take_result(&root).unwrap();
    assert_eq!((told.version.as_str(), told.error.as_deref()), ("2.0.0", Some("disk full")));
    assert_eq!(take_result(&root), None, "told once");
    std::fs::remove_dir_all(&root).unwrap();
}

/// What's replaced: the bundle on macOS, the program elsewhere; a folder that takes files
/// is writable.
#[test]
fn the_app_s_place_is_found_and_tried() {
    let exe = Path::new("/Applications/App.app/Contents/MacOS/app");
    let expected = if cfg!(target_os = "macos") { Path::new("/Applications/App.app") } else { exe };
    assert_eq!(target(exe), expected);
    assert_eq!(target(Path::new("/opt/app/app")), Path::new("/opt/app/app"));
    let root = temp();
    assert!(writable(&root.join("program")));
    std::fs::remove_dir_all(&root).unwrap();
}

/// One left aside by an earlier update and still there doesn't block this one; what's left
/// beside the app is cleared at its start.
#[test]
fn what_an_earlier_update_left_never_blocks_the_next() {
    let root = temp();
    let (dir, target) = staged(&root, false);
    std::fs::write(aside(&target), b"older").unwrap();
    assert_eq!(apply(&dir, &target).unwrap(), "2.0.0");
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    std::fs::write(aside(&target), b"left").unwrap();
    std::fs::write(target.with_file_name("program.old-1234"), b"left").unwrap();
    clean_left(&target);
    let left: Vec<_> = std::fs::read_dir(target.parent().unwrap())
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(left, ["program"], "only the app");
    std::fs::remove_dir_all(&root).unwrap();
}

/// Started again: an app bundle opened by the system on macOS (never the program itself:
/// the new app answers for itself), a program at once — told the process to wait for and
/// the session to reopen.
#[test]
fn the_app_is_started_again_waiting_for_the_old_one() {
    let (program, args) = restart_command(Path::new("/opt/my app/app"), 42, Some("ab12"));
    assert_eq!(program, Path::new("/opt/my app/app"));
    assert_eq!(args, ["--wait-for", "42", "--reopen-session", "ab12"]);
    let (program, args) = restart_command(Path::new("/Applications/My App.app"), 7, None);
    if cfg!(target_os = "macos") {
        assert_eq!(program, Path::new("/usr/bin/open"));
        assert_eq!(args, ["-n", "/Applications/My App.app", "--args", "--wait-for", "7"]);
    } else {
        assert!(program.ends_with(Path::new("Contents/MacOS").join(kernel::brand::ID)));
        assert_eq!(args, ["--wait-for", "7"]);
    }
}

/// A process that isn't there (or ended): not waited for.
#[test]
fn an_ended_process_is_not_waited_for() {
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    child.wait().unwrap();
    let since = std::time::Instant::now();
    wait_for(pid);
    assert!(since.elapsed() < std::time::Duration::from_secs(5));
}

/// The installer run silent, told the process to wait for and the session to reopen.
#[test]
fn the_installer_runs_silent_and_reopens_the_session() {
    let args = setup_args(42, Some("ab12"));
    assert_eq!(
        args,
        ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/WAITPID=42", "/REOPEN=ab12"]
    );
    assert_eq!(setup_args(1, None).len(), 4);
}
