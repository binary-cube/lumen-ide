use super::*;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Starts a real shell, runs a command and reads the result from the grid.
#[cfg(unix)]
#[test]
fn runs_a_command_in_the_project_directory() {
    let dir = std::env::temp_dir().join(format!("pty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), "pwd; printf 'gata\\n'; sleep 5".into()],
        cwd: Some(dir.clone()),
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    let canonical = dir.canonicalize().unwrap();
    loop {
        session.snapshot(&mut grid);
        let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
        if text.iter().any(|l| l == "gata") {
            assert!(
                text.iter().any(|l| std::path::Path::new(l) == canonical || std::path::Path::new(l) == dir),
                "{text:?}"
            );
            break;
        }
        assert!(Instant::now() < deadline, "the command didn't finish: {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What a terminal app gives the shell, never over the user's own: colors on, a UTF-8
/// locale when there's none, the terminal's name.
#[test]
fn the_shell_gets_colors_and_a_locale_unless_the_user_set_them() {
    let none = |_: &str| false;
    let env = shell_env(&Default::default(), &none);
    assert_eq!(env["TERM"], "xterm-256color");
    assert_eq!(env["CLICOLOR"], "1");
    assert!(env["LS_COLORS"].contains("di=1;34"));
    assert_eq!(env["LSCOLORS"], LSCOLORS);
    if cfg!(unix) {
        assert_eq!(env["LANG"], "en_US.UTF-8");
    }
    // The user's own (in the app's environment) are kept: not given again.
    let user = |key: &str| matches!(key, "LS_COLORS" | "LC_ALL");
    let env = shell_env(&Default::default(), &user);
    assert!(!env.contains_key("LS_COLORS") && !env.contains_key("LANG"));
    // The profile's win.
    let profile = [("CLICOLOR".to_owned(), "0".to_owned())].into_iter().collect();
    assert_eq!(shell_env(&profile, &none)["CLICOLOR"], "0");
}

/// A real shell gets them: `ls` colored, the locale UTF-8.
#[cfg(unix)]
#[test]
fn a_started_shell_sees_its_environment() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), "printf '%s|%s\\n' \"$CLICOLOR\" \"$TERM\"; sleep 5".into()],
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        session.snapshot(&mut grid);
        let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
        if text.iter().any(|l| l.ends_with("|xterm-256color")) {
            let set_by_user = std::env::var_os("CLICOLOR").is_some();
            assert!(set_by_user || text.iter().any(|l| l == "1|xterm-256color"), "{text:?}");
            break;
        }
        assert!(Instant::now() < deadline, "nothing printed: {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The shells the system has: at least one, the default first, each started with its
/// program.
#[test]
fn the_system_has_a_shell() {
    let shells = shells();
    assert!(!shells.is_empty());
    let default = profile("", None);
    assert_eq!(default.program.as_deref(), Some(shells[0].program.as_str()));
    assert_eq!(profile("not-installed", None).program, default.program);
}

/// The user's options reach a running session: the cursor's shape (the program asked for
/// none).
#[cfg(unix)]
#[test]
fn the_options_change_a_running_session() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), "sleep 5".into()],
        ..Default::default()
    };
    let size = TermSize { cols: 20, rows: 4, cell_width: 8, cell_height: 16 };
    let mut session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    session.snapshot(&mut grid);
    assert_eq!(grid.cursor_shape, CursorShape::Block);
    session.set_options(TermOptions {
        scrollback: 100,
        cursor: CursorShape::Beam,
        ..TermOptions::default()
    });
    session.snapshot(&mut grid);
    assert_eq!(grid.cursor_shape, CursorShape::Beam);
}

/// zsh with the themed prompt: the user's own configuration read first (their `.zshrc`),
/// then our prompt; `ZDOTDIR` back to theirs afterwards.
#[cfg(unix)]
#[test]
fn zsh_gets_the_themed_prompt_after_the_user_s_configuration() {
    if !std::path::Path::new("/bin/zsh").exists() {
        return;
    }
    let home = std::env::temp_dir().join(format!("pty-prompt-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".zshrc"), "export FROM_USER=yes\nPROMPT='plain> '\n").unwrap();
    let mut profile = ShellProfile {
        program: Some("/bin/zsh".into()),
        args: vec!["-l".into()],
        cwd: Some(home.clone()),
        ..Default::default()
    };
    profile.env.insert("HOME".into(), home.to_string_lossy().into_owned());
    let profile = themed_prompt(&profile);
    assert!(profile.env.contains_key("ZDOTDIR"));
    let size = TermSize { cols: 100, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    session.write(b"echo \"read:$FROM_USER zdotdir:${ZDOTDIR:-none}\"\r");
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        session.snapshot(&mut grid);
        let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
        if let Some(line) = text.iter().find(|l| l.starts_with("read:")) {
            assert_eq!(line, "read:yes zdotdir:none", "theirs read, ZDOTDIR back: {text:?}");
            assert!(text.iter().any(|l| l.contains('@') && !l.starts_with("plain>")), "{text:?}");
            break;
        }
        assert!(Instant::now() < deadline, "nothing printed: {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// bash with the themed prompt: the login files read (`.bash_profile`), then our prompt.
#[cfg(unix)]
#[test]
fn bash_gets_the_themed_prompt_after_the_login_files() {
    if !std::path::Path::new("/bin/bash").exists() {
        return;
    }
    let home = std::env::temp_dir().join(format!("pty-prompt-bash-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(home.join(".bash_profile"), "export FROM_USER=yes\n").unwrap();
    let mut profile = ShellProfile {
        program: Some("/bin/bash".into()),
        args: vec!["-l".into()],
        cwd: Some(home.clone()),
        ..Default::default()
    };
    profile.env.insert("HOME".into(), home.to_string_lossy().into_owned());
    let profile = themed_prompt(&profile);
    let size = TermSize { cols: 100, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    session.write(b"echo \"read:$FROM_USER\"\r");
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        session.snapshot(&mut grid);
        let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
        if text.iter().any(|l| l == "read:yes") {
            // The prompt (long: it may wrap): user@host, the directory, `$`.
            let all = text.concat();
            assert!(all.contains('@') && all.contains(" $ echo"), "{text:?}");
            break;
        }
        assert!(Instant::now() < deadline, "nothing printed: {text:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = std::fs::remove_dir_all(&home);
}

/// A long session keeps its history: 5 000 lines, then a program scrolling a region whose
/// top is the first row, a full-screen erase, ink-style redraws (cursor up, erase below) and
/// two resizes — the oldest line is still at the history's top, the scroll's range is the
/// history's length. Only an explicit `ESC[3J` (asked by the program) clears it.
#[test]
fn the_history_survives_regions_erases_redraws_and_resizes() {
    use alacritty_terminal::vte::ansi::Processor;
    struct Quiet;
    impl EventListener for Quiet {}
    let options = TermOptions { scrollback: 10_000, ..TermOptions::default() };
    let mut term = Term::new(term_config(options), &GridSize::new(80, 24), Quiet);
    let mut parser: Processor = Processor::new();
    let mut feed = |term: &mut Term<Quiet>, bytes: &[u8]| parser.advance(term, bytes);
    for i in 0..5000 {
        feed(&mut term, format!("line {i:05}\r\n").as_bytes());
    }
    // A scroll region from the first row (DECSTBM 1;20): what leaves it goes to history.
    feed(&mut term, b"\x1b[1;20r\x1b[20;1H");
    for i in 0..100 {
        feed(&mut term, format!("region {i}\r\n").as_bytes());
    }
    feed(&mut term, b"\x1b[r\x1b[2J\x1b[H");
    for _ in 0..50 {
        feed(&mut term, b"\x1b[10A\x1b[Jframe\r\nframe\r\n");
    }
    term.resize(GridSize::new(60, 30));
    term.resize(GridSize::new(100, 20));
    // Scrolled to the very top: the first line written is the first row shown.
    term.scroll_display(Scroll::Top);
    let mut grid = GridSnapshot::default();
    snapshot_of(&term, &mut grid);
    assert!(grid.history >= 5000, "{}", grid.history);
    assert_eq!(grid.display_offset, grid.history, "the scroll's range is the history");
    assert_eq!(grid.row_text(0), "line 00000");
    // The program asked for it (`ESC[3J`, as Claude Code's Ink sends with its full-screen
    // redraws): only then is the history gone.
    term.scroll_display(Scroll::Bottom);
    feed(&mut term, b"\x1b[2J\x1b[3J\x1b[H");
    snapshot_of(&term, &mut grid);
    assert_eq!(grid.history, 0);
}

/// A real interactive shell (job control on: each command its own process group, the
/// terminal's foreground): a program running for a while that redraws erasing the history
/// keeps it; a short command started from the shell erasing it empties it; Clear Terminal
/// always does.
#[cfg(unix)]
#[test]
fn the_history_against_the_foreground_program() {
    let home = std::env::temp_dir().join(format!("pty-history-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    // No user configuration read: an empty home, no `ENV`.
    let env = [("HOME", home.to_string_lossy().into_owned()), ("ENV", String::new())]
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-i".into()],
        env,
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let mut session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    let mut wait_for = |session: &PtySession, what: &str, done: &dyn Fn(&GridSnapshot) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            session.snapshot(&mut grid);
            if done(&grid) {
                return grid.history;
            }
            let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
            assert!(Instant::now() < deadline, "{what}: {text:?} (history {})", grid.history);
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let shows =
        |line: &'static str| move |g: &GridSnapshot| (0..g.rows).any(|r| g.row_text(r) == line);
    session.write(b"i=0; while [ $i -lt 300 ]; do echo l$i; i=$((i+1)); done; echo filled\r");
    let filled = wait_for(&session, "filled", &shows("filled"));
    assert!(filled >= 290, "{filled}");
    // A program running for over a second, then redrawing as Ink-based ones do.
    session
        .write(b"sh -c 'sleep 1.3; printf \"\\033[2J\\033[3J\\033[H\"; echo redrawn; sleep 30'\r");
    let kept = wait_for(&session, "redrawn", &shows("redrawn"));
    assert!(kept >= filled, "the running program's request was honored: {kept} < {filled}");
    // Stopped; then `clear`'s way: a command just started from the shell.
    session.write(b"\x03");
    session.write(b"sh -c 'printf \"\\033[2J\\033[3J\\033[H\"'; echo cleared\r");
    let cleared = wait_for(&session, "cleared", &shows("cleared"));
    assert!(cleared < 20, "the short command's request was ignored: {cleared}");
    // Filled again, then Clear Terminal.
    session.write(b"i=0; while [ $i -lt 100 ]; do echo m$i; i=$((i+1)); done; echo again\r");
    wait_for(&session, "again", &|g: &GridSnapshot| g.history > 80);
    session.clear_history();
    session.snapshot(&mut grid);
    assert_eq!(grid.history, 0);
    let _ = std::fs::remove_dir_all(&home);
}

/// Clear Terminal: the history gone, the cursor's line (the prompt) at the top, the rest of
/// the screen empty.
#[test]
fn clear_terminal_keeps_the_prompt_line_at_the_top() {
    use alacritty_terminal::vte::ansi::Processor;
    struct Quiet;
    impl EventListener for Quiet {}
    let mut term = Term::new(term_config(TermOptions::default()), &GridSize::new(40, 10), Quiet);
    let mut parser: Processor = Processor::new();
    for i in 0..50 {
        parser.advance(&mut term, format!("out {i}\r\n").as_bytes());
    }
    parser.advance(&mut term, b"$ ls");
    term.scroll_display(Scroll::Delta(5));
    clear_history_of(&mut term);
    let mut grid = GridSnapshot::default();
    snapshot_of(&term, &mut grid);
    assert_eq!(grid.history, 0);
    assert_eq!(grid.display_offset, 0);
    assert_eq!(grid.row_text(0), "$ ls");
    assert!((1..grid.rows).all(|r| grid.row_text(r).is_empty()));
    assert_eq!(grid.cursor, Some((0, 4)));
}

/// A drag of the panel's edge (30 sizes, 10 ms apart): the program is told one size, the
/// last, once they stop for 80 ms — not one per pixel (plan 55m).
#[test]
fn a_drag_tells_the_program_its_last_size_once() {
    let start = Instant::now();
    let size =
        |rows: u16| WindowSize { num_lines: rows, num_cols: 80, cell_width: 8, cell_height: 16 };
    let mut debounce = ResizeDebounce::default();
    let mut told = Vec::new();
    for i in 0..30u16 {
        let now = start + Duration::from_millis(10 * u64::from(i));
        debounce.ask(size(20 + i % 7), now);
        if let Some(s) = debounce.take(now + Duration::from_millis(5)) {
            told.push(s.num_lines);
        }
    }
    assert!(told.is_empty(), "nothing while dragging: {told:?}");
    let last = start + Duration::from_millis(290);
    assert_eq!(debounce.due(), Some(last + RESIZE_QUIET));
    assert!(debounce.take(last + Duration::from_millis(79)).is_none());
    let final_size = debounce.take(last + RESIZE_QUIET).expect("told once it stopped");
    assert_eq!(final_size.num_lines, 20 + 29 % 7);
    assert!(debounce.take(last + Duration::from_secs(1)).is_none(), "once");
}

/// A program asking for application cursor keys and bracketed paste: the modes read without
/// copying the screen are the snapshot's.
#[cfg(unix)]
#[test]
fn the_input_modes_are_read_as_the_program_set_them() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), "printf '\\033[?1h\\033[?2004hgata\\n'; sleep 5".into()],
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        session.snapshot(&mut grid);
        if (0..grid.rows).any(|r| grid.row_text(r) == "gata") {
            break;
        }
        assert!(Instant::now() < deadline, "the program didn't write");
        std::thread::sleep(Duration::from_millis(20));
    }
    let modes = loop {
        if let Some(modes) = session.try_modes() {
            break modes;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(modes, InputModes { app_cursor: true, bracketed_paste: true });
    assert_eq!(modes, InputModes::of(&grid));
}

/// A program asking the background's color (OSC 11, as a TUI does to suit a dark or a
/// light screen) is answered the view's at once, instead of waiting until it gives up.
#[cfg(unix)]
#[test]
fn a_program_asking_a_color_is_answered_the_view_s() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec![
            "-c".into(),
            "stty raw -echo; sleep 0.3; printf '\\033]11;?\\033\\\\'; r=$(head -c 25 | cat -v); \
             stty sane; printf 'got %s\\n' \"$r\"; sleep 5"
                .into(),
        ],
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    session.set_palette(TermPalette { background: (10, 20, 30), ..TermPalette::default() });
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(5);
    let answer = loop {
        session.snapshot(&mut grid);
        if let Some(line) = (0..grid.rows).map(|r| grid.row_text(r)).find(|l| l.starts_with("got "))
        {
            break line;
        }
        assert!(Instant::now() < deadline, "no answer came");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(answer, "got ^[]11;rgb:0a0a/1414/1e1e^[\\");
}

/// A color a program redefined is answered as it set it; the others as the view draws them.
#[test]
fn a_redefined_color_is_answered_as_the_program_set_it() {
    let mut colors = Colors::default();
    assert_eq!(colors.answer(257), None, "nothing drawn yet: nothing answered");
    colors.palette = Some(TermPalette { foreground: (1, 2, 3), ..TermPalette::default() });
    assert_eq!(colors.answer(256), Some((1, 2, 3)));
    assert_eq!(colors.answer(196), Some((255, 0, 0)));
    colors.redefined.insert(256, (9, 9, 9));
    assert_eq!(colors.answer(256), Some((9, 9, 9)));
    assert_eq!(colors.answer(400), None);
}

/// A real shell flooding output (`yes`), a UI taking the change on each wake: woken at most
/// about once per `wakes::INTERVAL`, and the end of the output shown.
#[cfg(unix)]
#[test]
fn a_shell_flooding_output_wakes_the_ui_at_a_bounded_rate() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec!["-c".into(), "yes | head -n 300000; echo flood-done; sleep 5".into()],
        ..Default::default()
    };
    let size = TermSize { cols: 80, rows: 10, cell_width: 8, cell_height: 16 };
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let tx = Mutex::new(tx);
    let session = spawn(
        &profile,
        size,
        Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        }),
    )
    .unwrap();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(20);
    let mut grid = GridSnapshot::default();
    let mut wakes = 0usize;
    // A session starts with its screen to draw: taken, as a view's first frame does.
    session.take_dirty();
    loop {
        if Instant::now() >= deadline {
            session.snapshot(&mut grid);
            let text: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
            panic!("the output's end wasn't shown after {wakes} wakes: {text:?}");
        }
        if rx.recv_timeout(Duration::from_millis(500)).is_err() {
            continue;
        }
        wakes += 1;
        if session.take_dirty() {
            session.snapshot(&mut grid);
            if (0..grid.rows).any(|r| grid.row_text(r) == "flood-done") {
                break;
            }
        }
    }
    let took = started.elapsed();
    let most = (took.as_millis() / wakes::INTERVAL.as_millis()) as usize + 3;
    eprintln!("{wakes} wakes in {took:?} ({:.0} a second)", wakes as f64 / took.as_secs_f64());
    assert!(wakes <= most, "{wakes} wakes in {took:?}");
}

/// A real session not drawn for a while: its history parked (the history's length shown the
/// same, its memory a fraction), output going on meanwhile, then scrolled to the top: every
/// line back, in order.
#[cfg(unix)]
#[test]
fn a_session_not_drawn_parks_its_history_and_gives_it_back() {
    let profile = ShellProfile {
        program: Some("/bin/sh".into()),
        args: vec![
            "-c".into(),
            "i=0; while [ $i -lt 2000 ]; do echo \"line $i\"; i=$((i+1)); done; read x; \
             echo after; sleep 10"
                .into(),
        ],
        ..Default::default()
    };
    let size = TermSize { cols: 60, rows: 10, cell_width: 8, cell_height: 16 };
    let mut session = spawn(&profile, size, Arc::new(|| {})).unwrap();
    let mut grid = GridSnapshot::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while grid.history < 1990 {
        assert!(Instant::now() < deadline, "the output didn't come: {}", grid.history);
        std::thread::sleep(Duration::from_millis(20));
        session.snapshot(&mut grid);
    }
    let history = grid.history;
    let memory = session.memory();
    // Not drawn "for a while" now (then a while again: the parking thread, woken by other
    // sessions, leaves it be).
    session.parking.park_after.store(0, Ordering::Relaxed);
    session.parking.park();
    session.parking.park_after.store(parking::PARK_AFTER.as_millis() as u64, Ordering::Relaxed);
    assert!(session.parking.rows() >= 1990);
    // Measured once the reader lets it (else the last figure is told).
    let deadline = Instant::now() + Duration::from_secs(5);
    while session.memory() * 3 >= memory {
        assert!(Instant::now() < deadline, "{} of {memory}", session.memory());
        std::thread::sleep(Duration::from_millis(10));
    }
    // Output while parked.
    session.write(b"\n");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "no output after");
        std::thread::sleep(Duration::from_millis(20));
        let term = session.term.lock();
        if term.grid().history_size() > 0 {
            break;
        }
    }
    // Scrolled to the top: back at once, the oldest line first.
    session.scroll(i32::MAX / 2);
    assert_eq!(session.parking.rows(), 0);
    session.snapshot(&mut grid);
    assert_eq!(grid.history, history + 2, "the line typed and the one written");
    assert_eq!(grid.display_offset, grid.history);
    assert_eq!(grid.row_text(0), "line 0");
    session.scroll(-1000);
    session.snapshot(&mut grid);
    assert_eq!(grid.row_text(0), "line 1000");
    session.scroll(-10_000);
    session.snapshot(&mut grid);
    assert_eq!(grid.row_text(grid.rows - 2), "after");
}

/// A selection made while the screen scrolled is copied whole, its lines from the history
/// too; a line the program's output wrapped comes back as one.
#[test]
fn a_selection_is_copied_with_what_scrolled_off_the_screen() {
    use alacritty_terminal::vte::ansi::Processor;
    use terminal_session::text::{TermPoint, TermSelection};
    struct Quiet;
    impl EventListener for Quiet {}
    let mut term = Term::new(term_config(TermOptions::default()), &GridSize::new(20, 5), Quiet);
    let mut parser: Processor = Processor::new();
    for i in 0..30 {
        parser.advance(&mut term, format!("row {i:02}\r\n").as_bytes());
    }
    // 30 rows written on a 5-row screen: "row 10" is far up in the history.
    let line_of = |i: i32| i - 26;
    let selection = TermSelection {
        anchor: TermPoint { line: line_of(10), col: 0 },
        head: TermPoint { line: line_of(13), col: 5 },
    };
    let history = term.grid().history_size();
    assert_eq!(selection_text_of(&term, &selection, history), "row 10\nrow 11\nrow 12\nrow 13");
    // Output since the selection was made (as the snapshot drawn had it): the same rows,
    // found where they went.
    parser.advance(&mut term, b"later 1\r\nlater 2\r\nlater 3\r\n");
    assert_eq!(selection_text_of(&term, &selection, history), "row 10\nrow 11\nrow 12\nrow 13");
    // A line longer than the screen's width, wrapped by the terminal: one line again.
    parser.advance(&mut term, b"abcdefghijklmnopqrstuvwxyz\r\n");
    let selection = TermSelection {
        anchor: TermPoint { line: 2, col: 0 },
        head: TermPoint { line: 3, col: 19 },
    };
    let history = term.grid().history_size();
    assert_eq!(selection_text_of(&term, &selection, history), "abcdefghijklmnopqrstuvwxyz");
}

/// A letter's combining mark is copied with it (as drawn).
#[test]
fn a_combining_mark_is_copied_with_its_letter() {
    use alacritty_terminal::vte::ansi::Processor;
    use terminal_session::text::{TermPoint, TermSelection};
    struct Quiet;
    impl EventListener for Quiet {}
    let mut term = Term::new(term_config(TermOptions::default()), &GridSize::new(20, 5), Quiet);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, "cafe\u{301}".as_bytes());
    let selection = TermSelection {
        anchor: TermPoint { line: 0, col: 0 },
        head: TermPoint { line: 0, col: 3 },
    };
    let history = term.grid().history_size();
    assert_eq!(selection_text_of(&term, &selection, history), "cafe\u{301}");
}
