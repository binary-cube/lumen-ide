use super::*;

#[test]
fn a_busy_named_thread_is_seen_with_its_cpu_time() {
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let worker = std::thread::Builder::new()
        .name("stats-probe".into())
        .spawn(move || {
            // Busy for a while, then waits to be looked at.
            let start = std::time::Instant::now();
            let mut x = 0u64;
            while start.elapsed() < Duration::from_millis(150) {
                x = x.wrapping_mul(31).wrapping_add(7);
            }
            std::hint::black_box(x);
            let _ = rx.recv();
        })
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let threads = threads();
    let _ = tx.send(());
    worker.join().unwrap();
    assert!(threads.len() >= 2, "{threads:?}");
    let probe = threads.iter().find(|t| t.name == "stats-probe").expect("named");
    assert!(probe.cpu >= Duration::from_millis(50), "{probe:?}");
}

/// What the process wrote counts (as the system sees it).
#[test]
fn the_process_disk_is_counted() {
    let Some((_, before)) = disk_io() else { return };
    let file = std::env::temp_dir().join(format!("process-io-{}", std::process::id()));
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&file).unwrap();
        f.write_all(&vec![7u8; 1 << 20]).unwrap();
        f.sync_all().unwrap();
    }
    let (_, after) = disk_io().unwrap();
    let _ = std::fs::remove_file(&file);
    assert!(after >= before, "counts only go up");
}

/// The test's own program is named as the system names it (on every platform).
#[test]
fn a_program_is_named() {
    let name = process_name(std::process::id()).expect("this program's name");
    let exe = std::env::current_exe().unwrap();
    let file = exe.file_stem().unwrap().to_string_lossy().into_owned();
    // macOS and Linux cut it (16 / 15 characters): its start.
    let short: String = file.chars().take(name.chars().count()).collect();
    assert!(!name.is_empty() && name == short, "{name} for {file}");
    // Read once, kept while it runs.
    let mut trees = ProcessTrees::default();
    assert_eq!(trees.name(std::process::id()), Some(name.clone()));
    assert_eq!(trees.name(std::process::id()), Some(name));
    assert_eq!(process_name(u32::MAX - 1), None, "no such program");
}

/// What the process holds as its own grows with what it allocates and touches.
#[test]
fn the_footprint_counts_what_the_process_holds() {
    let Some(before) = footprint() else { return };
    assert!(before > 0);
    let mut held = vec![0u8; 64 << 20];
    // Touched page by page: only then do the pages count.
    for i in (0..held.len()).step_by(4096) {
        held[i] = 1;
    }
    let after = footprint().unwrap();
    assert!(after >= before + (32 << 20), "{before} → {after}");
    std::hint::black_box(&held);
}

/// A command's first word as a name: its file's name, a login shell's dash and `.exe` taken
/// away, one short line.
#[test]
fn a_command_s_first_word_names_its_program() {
    assert_eq!(invoked_name("/usr/bin/git").as_deref(), Some("git"));
    assert_eq!(invoked_name("-zsh").as_deref(), Some("zsh"));
    assert_eq!(invoked_name(r"C:\Tools\node.exe").as_deref(), Some("node"));
    assert_eq!(invoked_name("claude").as_deref(), Some("claude"));
    assert_eq!(invoked_name("bad\nname\u{7}").as_deref(), Some("badname"));
    assert_eq!(invoked_name(&"x".repeat(100)).map(|n| n.len()), Some(40));
    assert_eq!(invoked_name("/usr/bin/"), None);
    assert_eq!(invoked_name(""), None);
    assert_eq!(invoked_name(r"C:\Tools\Node.Exe").as_deref(), Some("Node"), "any case");
    assert_eq!(invoked_name(".exe").as_deref(), Some(".exe"), "a name, not only the end");
    assert_eq!(invoked_name("sshd: user@pts/3"), None, "a title, not a path");
}

/// A name shown can't be disguised: no bidi overrides, isolates or zero-width marks.
#[test]
fn a_name_loses_what_could_disguise_it() {
    assert_eq!(tidy_name("\u{202E}exe.tsoh").as_deref(), Some("exe.tsoh"));
    assert_eq!(tidy_name("a\u{200B}b\u{2066}c\u{FEFF}").as_deref(), Some("abc"));
    assert_eq!(tidy_name("\u{200F}\u{2069}"), None);
    assert_eq!(tidy_name("ăîșț").as_deref(), Some("ăîșț"), "letters stay");
}

/// Another program's own memory is read as this one's is (the system's monitor's figure);
/// one that ended has none.
#[test]
fn a_started_program_s_own_memory_is_read() {
    assert!(footprint().is_some_and(|b| b > 0) == footprint_of(std::process::id()).is_some());
    #[cfg(unix)]
    let program = std::process::Command::new("sleep").arg("5").spawn();
    // The program itself (not through `cmd`: killing that would leave its child running).
    #[cfg(windows)]
    let program = std::process::Command::new("ping")
        .args(["-n", "6", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .spawn();
    let Ok(mut program) = program else { return };
    let pid = program.id();
    std::thread::sleep(Duration::from_millis(100));
    if cfg!(any(target_os = "macos", target_os = "linux", windows)) {
        assert!(footprint_of(pid).is_some_and(|b| b > 0), "read");
    }
    let _ = program.kill();
    let _ = program.wait();
    // Its handle let go too (Windows keeps an ended process while one is open).
    drop(program);
    assert_eq!(footprint_of(pid), None, "ended");
}
