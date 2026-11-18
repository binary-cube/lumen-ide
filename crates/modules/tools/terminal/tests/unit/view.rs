use super::*;

#[test]
fn indexed_colors_cover_ansi_cube_and_grays() {
    let palette = TermColors::new(&Palette::dark(), Colors::Theme);
    let grid = GridSnapshot::default();
    assert_eq!(resolve(TermColor::Indexed(1), true, &grid, &palette), palette.ansi[1]);
    assert_eq!(resolve(TermColor::Indexed(16), true, &grid, &palette), Rgba::rgb(0x000000));
    assert_eq!(resolve(TermColor::Indexed(231), true, &grid, &palette), Rgba::rgb(0xffffff));
    assert_eq!(resolve(TermColor::Indexed(232), true, &grid, &palette), Rgba::rgb(0x080808));
    assert_eq!(resolve(TermColor::Default, false, &grid, &palette), palette.background);
    let mut grid = GridSnapshot::default();
    grid.palette_overrides.insert(257, (1, 2, 3));
    assert_eq!(resolve(TermColor::Default, false, &grid, &palette), Rgba::rgb(0x010203));
}

#[test]
fn selection_double_click_and_file_links() {
    use terminal_session::TermCell;
    let dir = std::env::temp_dir().join(format!("term-links-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/A.php"), "<?php\n").unwrap();
    let rows = ["$ phpunit", "FAIL src/A.php:12 and x.php:3"];
    let cols = 30u16;
    let mut cells = Vec::new();
    for row in rows {
        let mut chars: Vec<char> = row.chars().collect();
        chars.resize(cols as usize, ' ');
        cells.extend(chars.into_iter().map(|ch| TermCell { ch, ..TermCell::default() }));
    }
    let mut panel = TerminalView {
        grid: GridSnapshot { cols, rows: 2, cells, ..Default::default() },
        grid_area: Rect::new(0.0, 0.0, 300.0, 40.0),
        cell: (10.0, 20.0),
        cwd: Some(dir.clone()),
        ..Default::default()
    };
    let at = |col: f32, row: f32| Point::new(col * 10.0 + 5.0, row * 20.0 + 5.0);
    // Drag from "phpunit" to "FAIL".
    assert!(panel.begin_selection(at(2.0, 0.0), 1));
    assert!(panel.drag_selection(at(3.0, 1.0)));
    panel.end_selection();
    assert_eq!(panel.selected_text().as_deref(), Some("phpunit\nFAIL"));
    // A plain click clears it; a double click selects the word.
    panel.begin_selection(at(1.0, 0.0), 1);
    panel.end_selection();
    assert_eq!(panel.selected_text(), None);
    panel.begin_selection(at(7.0, 1.0), 2);
    assert_eq!(panel.selected_text().as_deref(), Some("src/A.php:12"));
    // Links: an existing file opens at its line; a missing one isn't a link.
    let (_, cols, link) = panel.link_under(at(8.0, 1.0)).unwrap();
    assert_eq!(cols, 5..17);
    assert_eq!(
        link,
        TerminalLink::File { path: dir.join("src/A.php"), line: Some(12), column: None }
    );
    assert!(panel.link_under(at(24.0, 1.0)).is_none(), "x.php doesn't exist");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Bold as bright: a bold cell in one of the first 8 colors is drawn in its bright one, only
/// when chosen (and only bold ones); inverse swaps.
#[test]
fn bold_text_in_the_bright_colors_when_chosen() {
    let colors = TermColors::new(&Palette::dark(), Colors::Theme);
    let grid = GridSnapshot::default();
    let cell = |flags| terminal_session::TermCell {
        ch: 'a',
        fg: TermColor::Indexed(1),
        bg: TermColor::Default,
        flags,
    };
    let bold = cell(flags::BOLD);
    assert_eq!(cell_colors(&bold, &grid, &colors, false).0, colors.ansi[1]);
    assert_eq!(cell_colors(&bold, &grid, &colors, true).0, colors.ansi[9]);
    assert_eq!(cell_colors(&cell(0), &grid, &colors, true).0, colors.ansi[1], "not bold");
    let inverse = cell(flags::INVERSE);
    assert_eq!(cell_colors(&inverse, &grid, &colors, false), (colors.background, colors.ansi[1]));
}

/// "Close the session" from the Task Manager: each shell is offered it (asked first); its
/// owner closes it — the view ends (its tab closes, its session with it) — and a shell that
/// isn't running isn't one to close.
#[test]
fn a_session_is_closed_from_the_task_manager() {
    use workbench_model::{ActOutcome, ActionKind, ActionTarget, ResourceReporter as _};
    // A shell's id as its view holds it (no real shell started).
    let pid = 3_999_123;
    let shell = Shell { pid, project: None, watch: None };
    let view = TerminalView { shell: Some(ShellPid::new(shell)), ..TerminalView::default() };
    assert!(!view.closed());
    let part = crate::Shells.report().remove(0);
    let close = part.actions.iter().find(|a| a.target == ActionTarget::Process(pid)).unwrap();
    assert_eq!(close.kind, ActionKind::Close);
    assert!(close.confirm, "asked first: what runs in it ends");
    let named = workbench_model::PartRef { owner: part.owner, name: part.name, project: None };
    assert_eq!(
        crate::Shells.act(&named, close.id, ActionTarget::Process(pid + 1)),
        ActOutcome::Unsupported
    );
    assert_eq!(crate::Shells.act(&named, close.id, close.target), ActOutcome::Done);
    assert!(view.closed(), "its view ends: its tab closes");
    drop(view);
    assert!(!shell_pids().contains(&pid), "gone from the shells");
    assert!(!CLOSING.lock().unwrap().contains(&pid), "nothing left asked");
    // No session left: no part (or one without it).
    assert!(crate::Shells.report().iter().all(|p| !p.processes.contains(&pid)));
}

/// A shell's screen in memory: `lines` of history and screen, scrolled by `offset`; what's
/// written to it kept.
#[derive(Debug, Default)]
struct FakeSession {
    lines: Vec<String>,
    cols: u16,
    rows: u16,
    offset: usize,
    alt_screen: bool,
    mouse: bool,
    written: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    /// A program's block logo in its first rows (half blocks, orange on a dark ground).
    logo: bool,
    /// Its output being parsed: a frame's copy that doesn't wait finds it busy.
    busy: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The program asked for application cursor keys.
    app_cursor: bool,
    /// Copies asked while busy (each would have waited for the reader).
    waited: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// What the program says it is (OSC 0 / 2).
    title: Option<String>,
    /// The shell ended, with this code.
    exit: Option<i32>,
    /// New output since last asked.
    dirty: std::sync::atomic::AtomicBool,
}

impl FakeSession {
    fn new(count: usize, cols: u16, rows: u16) -> Self {
        let lines = (0..count)
            .map(|i| format!("{i:05} drwxr-xr-x  staff  src/module_{i}.rs  {}", "x".repeat(i % 90)))
            .collect();
        Self { lines, cols, rows, ..Self::default() }
    }

    fn history(&self) -> usize {
        if self.alt_screen { 0 } else { self.lines.len().saturating_sub(self.rows as usize) }
    }
}

impl TerminalSession for FakeSession {
    fn write(&self, bytes: &[u8]) {
        self.written.lock().unwrap().extend_from_slice(bytes);
    }
    fn resize(&mut self, size: TermSize) {
        self.cols = size.cols;
        self.rows = size.rows;
    }
    fn snapshot(&self, out: &mut GridSnapshot) {
        if self.busy.load(std::sync::atomic::Ordering::Relaxed) {
            self.waited.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        out.app_cursor = self.app_cursor;
        out.cols = self.cols;
        out.rows = self.rows;
        out.cells.clear();
        let history = self.history();
        let top = history - self.offset.min(history);
        for r in 0..self.rows as usize {
            let line = self.lines.get(top + r).map_or("", String::as_str);
            let mut chars: Vec<char> = line.chars().take(self.cols as usize).collect();
            chars.resize(self.cols as usize, ' ');
            out.cells.extend(
                chars.into_iter().map(|ch| terminal_session::TermCell { ch, ..Default::default() }),
            );
        }
        out.above.clear();
        if out.want_above && top > 0 {
            let line = self.lines.get(top - 1).map_or("", String::as_str);
            let mut chars: Vec<char> = line.chars().take(self.cols as usize).collect();
            chars.resize(self.cols as usize, ' ');
            out.above.extend(
                chars.into_iter().map(|ch| terminal_session::TermCell { ch, ..Default::default() }),
            );
        }
        if self.logo {
            let orange = terminal_session::TermColor::Rgb(0xd9, 0x77, 0x57);
            let ground = terminal_session::TermColor::Rgb(0x20, 0x20, 0x20);
            for row in 0..3usize.min(self.rows as usize) {
                for col in 0..(self.cols as usize).min(12) {
                    let ch = ['▀', '▄', '█'][(row + col) % 3];
                    out.cells[row * self.cols as usize + col] =
                        terminal_session::TermCell { ch, fg: orange, bg: ground, flags: 0 };
                }
            }
        }
        out.display_offset = self.offset;
        out.history = history;
        out.alt_screen = self.alt_screen;
        out.mouse = if self.mouse {
            terminal_session::MouseReport::Sgr
        } else {
            terminal_session::MouseReport::Off
        };
        out.alternate_scroll = true;
        out.cursor = Some((self.rows - 1, 0));
    }
    fn try_snapshot(&self, out: &mut GridSnapshot) -> bool {
        if self.busy.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        self.snapshot(out);
        true
    }
    fn try_scroll(&mut self, lines: i32) -> bool {
        if self.busy.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        self.scroll(lines);
        true
    }
    fn scroll(&mut self, lines: i32) {
        let history = self.history() as i64;
        self.offset = (self.offset as i64 + i64::from(lines)).clamp(0, history) as usize;
    }
    fn scroll_to_bottom(&mut self) {
        self.offset = 0;
    }
    fn clear_history(&mut self) {
        let keep = self.lines.len().saturating_sub(1);
        self.lines.drain(..keep);
        self.offset = 0;
    }
    fn title(&self) -> Option<String> {
        self.title.clone()
    }
    fn exit_status(&self) -> Option<i32> {
        self.exit
    }
    fn take_dirty(&self) -> bool {
        self.dirty.swap(false, std::sync::atomic::Ordering::Relaxed)
    }
}

fn painted(session: FakeSession) -> (TerminalView, TextSystem) {
    let mut view = TerminalView { session: Some(Box::new(session)), ..TerminalView::default() };
    let mut text = TextSystem::new();
    paint(&mut view, &mut text);
    (view, text)
}

fn paint(view: &mut TerminalView, text: &mut TextSystem) {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 2400.0, 1000.0),
        scale: 2.0,
        text,
        scene: &mut render_api::Scene::default(),
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
}

/// A trackpad's small steps (each under half a line) add up: none is lost (they were all
/// rounded away), and the screen shows the new place at once — each step by its pixels.
#[test]
fn a_trackpads_small_steps_scroll_the_history_line_by_line() {
    let (mut view, _text) = painted(FakeSession::new(5000, 200, 50));
    let line = view.cell.1;
    let mut moved = 0;
    for _ in 0..240 {
        if view.scroll(0.0, -3.0) {
            moved += 1;
        }
    }
    let lines = (240.0 * 3.0 / line).floor() as usize;
    assert_eq!(view.grid.display_offset, lines, "every step counted");
    assert_eq!(moved, 240, "a frame for each step (the grid shifted by its pixels)");
    // Back down to the bottom, never past it.
    for _ in 0..1000 {
        view.scroll(0.0, 30.0);
    }
    assert_eq!(view.grid.display_offset, 0);
}

/// The scrollbar shows where the screen is in the history, in its own lane beside the grid,
/// and dragging it scrolls the history.
#[test]
fn the_scrollbar_follows_the_history_and_drags_it() {
    let (mut view, mut text) = painted(FakeSession::new(1050, 200, 50));
    assert!(view.bar_area.min_x() >= view.grid_area.max_x(), "never over the text");
    let (content, offset) = view.bar_metrics().expect("history to scroll");
    assert!((offset + view.bar_area.size.height - content).abs() < 1.0, "at the bottom");
    // The thumb pressed at the top of the lane, then dragged: back into the history.
    let top = Point::new(view.bar_area.min_x() + 4.0, view.bar_area.min_y() + 2.0);
    view.hover(top);
    paint(&mut view, &mut text);
    view.press(top, 1);
    assert!(view.grid.display_offset > 0, "the history jumped toward its start");
    view.release();
    // Nothing to scroll: no bar.
    let (view, _) = painted(FakeSession::new(10, 200, 50));
    assert!(view.bar_metrics().is_none());
}

/// A full-screen program (vim, htop): no history there, so no bar, and the wheel reaches
/// the program as ↑ / ↓ at once; one that asked for the mouse gets its wheel reports.
#[test]
fn a_full_screen_program_gets_the_wheel() {
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let session = FakeSession {
        alt_screen: true,
        written: std::sync::Arc::clone(&sent),
        ..FakeSession::new(200, 80, 24)
    };
    let (mut view, _text) = painted(session);
    assert!(view.bar_metrics().is_none(), "no bar on its screen");
    let line = view.cell.1;
    assert!(!view.scroll(0.0, -2.0 * line), "nothing scrolled here");
    assert_eq!(std::mem::take(&mut *sent.lock().unwrap()), b"\x1b[A\x1b[A");
    assert_eq!(view.grid.display_offset, 0);
}

/// Clear Terminal (⌘K / Ctrl+Shift+K, sent to the view as its action): the history gone,
/// shown at once; other commands aren't its.
#[test]
fn clear_terminal_empties_the_history() {
    let (mut view, _text) = painted(FakeSession::new(500, 80, 24));
    view.scroll(0.0, -200.0);
    assert!(view.grid.history > 0 && view.grid.display_offset > 0);
    assert!(!view.command("find"));
    assert!(view.command(crate::CLEAR_ACTION));
    assert_eq!((view.grid.history, view.grid.display_offset), (0, 0));
}

/// Scrolled by part of a line (plan 55l): the emulator stays on its whole line, the grid is
/// drawn shifted down by the rest with the history line above in the gap; the mouse's cell
/// follows the shift; typing brings it back to the bottom exactly (following the output:
/// nothing shifted, nothing asked above).
#[test]
fn part_of_a_line_shifts_the_grid_with_the_line_above_in_the_gap() {
    let (mut view, mut text) = painted(FakeSession::new(500, 80, 24));
    let line = view.cell.1;
    assert!(view.scroll(0.0, -0.4 * line));
    assert_eq!(view.grid.display_offset, 0, "the emulator on its whole line");
    let shift = view.smooth.shift(line);
    assert!(shift > 0.0 && shift < line, "{shift}");
    paint(&mut view, &mut text);
    let history = view.grid.history;
    let above: String = view.grid.above.iter().map(|c| c.ch).collect();
    assert!(above.starts_with(&format!("{:05}", history - 1)), "the line above: {above}");
    // The mouse's cell: the grid starts lower.
    let a = view.grid_area;
    let x = a.min_x() + 2.0;
    assert_eq!(view.cell_at(Point::new(x, a.min_y() + shift + 0.5 * line), false), Some((0, 0)));
    assert_eq!(view.cell_at(Point::new(x, a.min_y() + shift + 1.5 * line), false), Some((1, 0)));
    // A wheel's notch: whole lines, the part kept.
    view.scroll(0.0, -3.0 * line);
    assert_eq!(view.grid.display_offset, 3);
    // Typing: back at the bottom exactly.
    view.write(b"x");
    paint(&mut view, &mut text);
    assert_eq!((view.grid.display_offset, view.smooth.shift(line)), (0, 0.0));
    assert!(view.grid.above.is_empty(), "nothing asked above at the bottom");
}

/// What a scrolling frame costs (`cargo test --release -p terminal-panel scroll_frame_cost
/// -- --ignored --nocapture`): a 200 × 50 grid painted at a whole line and shifted by half.
#[test]
#[ignore = "a measurement"]
fn scroll_frame_cost() {
    let (mut view, mut text) = painted(FakeSession::new(5000, 200, 50));
    view.scroll(0.0, -400.0);
    let line = view.cell.1;
    for (name, dy) in [("whole lines", 0.0), ("half a line", -0.5 * line)] {
        view.smooth.reset();
        view.scroll(0.0, dy);
        let started = std::time::Instant::now();
        for _ in 0..500 {
            paint(&mut view, &mut text);
        }
        println!("{name}: {:?} a frame", started.elapsed() / 500);
    }
}

/// A program's block logo (half blocks with their colors) in its first rows, the panel
/// resized 30 times (growing and shrinking, at a fractional scale and origin): each frame
/// the logo is the first row, at the grid's top, every row's backgrounds on whole pixels
/// and none over another row's (plan 55m: the header seen cut after such resizes).
#[test]
fn a_block_logo_at_the_top_stays_whole_across_resizes() {
    let session = FakeSession { logo: true, ..FakeSession::new(8, 80, 24) };
    let mut view = TerminalView { session: Some(Box::new(session)), ..TerminalView::default() };
    let mut text = TextSystem::new();
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    for i in 0..30 {
        let height = if i % 2 == 0 { 300.0 + 13.7 * i as f32 } else { 900.0 - 11.3 * i as f32 };
        let mut scene = render_api::Scene::default();
        view.paint(ViewerPaint {
            bounds: Rect::new(10.5, 37.3, 1200.0, height),
            scale: 1.25,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
        let area = view.grid_area;
        assert_eq!(area.min_y().fract(), 0.0, "the grid on a whole pixel");
        let h = view.cell.1;
        assert_eq!(h.fract(), 0.0, "rows whole pixels tall");
        let row: String = view.grid.row(0).iter().take(3).map(|c| c.ch).collect();
        assert_eq!(row, "▀▄█", "the logo first");
        let layer = scene.layers().iter().find(|l| l.clip == Some(area)).expect("the grid's layer");
        let mut backgrounds: Vec<Rect> = layer
            .quads
            .iter()
            .map(|q| q.bounds)
            .filter(|b| (b.size.height - h).abs() < 0.01)
            .collect();
        assert!(!backgrounds.is_empty());
        backgrounds.sort_by(|a, b| a.min_y().total_cmp(&b.min_y()));
        assert_eq!(backgrounds[0].min_y(), area.min_y(), "the logo's row at the top, whole");
        for b in &backgrounds {
            assert_eq!(b.min_y().fract(), 0.0, "{b:?}");
            let row = (b.min_y() - area.min_y()) / h;
            assert_eq!(row.fract(), 0.0, "on its row: {b:?}");
        }
        for pair in backgrounds.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a.min_y() == b.min_y() || a.max_y() <= b.min_y(), "{a:?} over {b:?}");
        }
    }
}

/// A key pressed while the program's output is being parsed doesn't wait for it: it's sent
/// in the modes of the screen last copied (the arrows as the program asked).
#[test]
fn a_key_doesn_t_wait_for_the_output_being_parsed() {
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let session = FakeSession {
        app_cursor: true,
        written: std::sync::Arc::clone(&sent),
        ..FakeSession::new(40, 80, 24)
    };
    let (busy, waited) =
        (std::sync::Arc::clone(&session.busy), std::sync::Arc::clone(&session.waited));
    let (mut view, _text) = painted(session);
    busy.store(true, std::sync::atomic::Ordering::Relaxed);
    let up = KeyPress { key: PressedKey::Up, shift: false, ctrl: false, alt: false };
    assert!(view.key_press(up));
    assert_eq!(std::mem::take(&mut *sent.lock().unwrap()), b"\x1bOA");
    assert_eq!(waited.load(std::sync::atomic::Ordering::Relaxed), 0, "never waited");
    // Free: the modes read now.
    busy.store(false, std::sync::atomic::Ordering::Relaxed);
    view.grid.app_cursor = false;
    assert!(view.key_press(up));
    assert_eq!(std::mem::take(&mut *sent.lock().unwrap()), b"\x1bOA");
}

/// The wheel over a busy screen (output being parsed) doesn't wait: the lines are scrolled
/// at the next frame that finds it free, none lost.
#[test]
fn the_wheel_doesn_t_wait_for_the_output_being_parsed() {
    let session = FakeSession::new(500, 80, 24);
    let (busy, waited) =
        (std::sync::Arc::clone(&session.busy), std::sync::Arc::clone(&session.waited));
    let (mut view, mut text) = painted(session);
    let line = view.cell.1;
    busy.store(true, std::sync::atomic::Ordering::Relaxed);
    view.scroll(0.0, -3.0 * line);
    view.scroll(0.0, -2.0 * line);
    assert_eq!(view.grid.display_offset, 0, "not scrolled yet");
    assert!(view.poll(), "a frame asked for");
    assert_eq!(waited.load(std::sync::atomic::Ordering::Relaxed), 0, "never waited");
    busy.store(false, std::sync::atomic::Ordering::Relaxed);
    paint(&mut view, &mut text);
    assert_eq!(view.grid.display_offset, 5, "every line scrolled");
}

/// A frame never waits for the program's output being parsed (keys and drawing go on): the
/// screen it had stays and the next frame is asked for; behind for long, a frame waits once.
#[test]
fn a_busy_screen_doesn_t_hold_the_frame() {
    let session = FakeSession::new(40, 80, 24);
    let busy = std::sync::Arc::clone(&session.busy);
    let (mut view, mut text) = painted(session);
    let cols = view.grid.cols;
    assert!(cols > 0, "copied when free");
    view.grid.cols = 0;
    busy.store(true, std::sync::atomic::Ordering::Relaxed);
    paint(&mut view, &mut text);
    assert_eq!(view.grid.cols, 0, "busy: the screen it had stays");
    assert!(view.poll(), "the next frame asked for");
    view.stale_since = Some(std::time::Instant::now() - MAX_BEHIND);
    paint(&mut view, &mut text);
    assert_eq!(view.grid.cols, cols, "behind for long: waited once");
    assert!(view.stale_since.is_none() && !view.poll());
}

/// A view following `config` (its own: the saved ones aren't touched).
fn with_config(session: FakeSession, config: crate::config::TerminalConfig) -> TerminalView {
    TerminalView {
        session: Some(Box::new(session)),
        config: Some(std::sync::Arc::new(config)),
        ..TerminalView::default()
    }
}

/// Selected with the mouse and "copy on selection" on: copied at once (off: not).
#[test]
fn a_selection_is_copied_at_once_when_chosen() {
    use crate::config::TerminalConfig;
    for copy in [true, false] {
        let config = TerminalConfig { copy_on_select: copy, ..TerminalConfig::default() };
        let mut view = with_config(FakeSession::new(30, 80, 24), config);
        let mut text = TextSystem::new();
        paint(&mut view, &mut text);
        let (w, h) = view.cell;
        let at = |col: f32, row: f32| {
            Point::new(
                view.grid_area.min_x() + col * w + 1.0,
                view.grid_area.min_y() + row * h + 1.0,
            )
        };
        let (from, to) = (at(0.0, 0.0), at(4.0, 0.0));
        assert!(view.begin_selection(from, 1));
        view.drag_selection(to);
        view.end_selection();
        let copied = match view.take_action() {
            Some(ViewerAction::Request(ui_components::AppRequest::CopyText(t))) => Some(t),
            _ => None,
        };
        assert_eq!(copied.is_some(), copy, "copy on selection: {copy}");
        if copy {
            assert_eq!(copied, view.selected_text());
        }
    }
}

/// "Highlight links" off: nothing under the pointer is a link.
#[test]
fn links_are_only_found_when_chosen() {
    use crate::config::TerminalConfig;
    let session = FakeSession {
        lines: vec!["see https://example.com/x now".into()],
        ..FakeSession::new(0, 80, 24)
    };
    let config = TerminalConfig { highlight_links: false, ..TerminalConfig::default() };
    let mut view = with_config(session, config);
    let mut text = TextSystem::new();
    paint(&mut view, &mut text);
    let (w, h) = view.cell;
    let p = Point::new(view.grid_area.min_x() + 8.0 * w, view.grid_area.min_y() + h / 2.0);
    assert!(view.link_under(p).is_none());
    view.config = Some(std::sync::Arc::new(TerminalConfig::default()));
    assert!(view.link_under(p).is_some(), "on by default");
}

/// "Mouse reporting" off: a program that asked for the mouse doesn't get the wheel; the
/// history scrolls instead.
#[test]
fn the_wheel_scrolls_the_history_without_mouse_reporting() {
    use crate::config::TerminalConfig;
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let session = FakeSession {
        mouse: true,
        written: std::sync::Arc::clone(&sent),
        ..FakeSession::new(200, 80, 24)
    };
    let config = TerminalConfig { mouse_reporting: false, ..TerminalConfig::default() };
    let mut view = with_config(session, config);
    let mut text = TextSystem::new();
    paint(&mut view, &mut text);
    let line = view.cell.1;
    assert!(view.scroll(0.0, -3.0 * line), "the history scrolled");
    assert!(sent.lock().unwrap().is_empty(), "nothing reported to the program");
}

/// The shell ended: its tab closes, unless "close the session" is off (then it stays).
#[test]
fn an_ended_session_stays_when_chosen() {
    use crate::config::TerminalConfig;
    for close in [true, false] {
        let session = FakeSession { exit: Some(3), ..FakeSession::new(10, 80, 24) };
        let config = TerminalConfig { close_on_exit: close, ..TerminalConfig::default() };
        let mut view = with_config(session, config);
        let mut text = TextSystem::new();
        paint(&mut view, &mut text);
        assert_eq!(view.closed(), close);
        assert_eq!(view.ended(), (!close).then_some(3));
    }
}

/// The tab named after the program's title: always, or never; read when the screen changes.
#[test]
fn the_tab_takes_the_program_s_title_when_chosen() {
    use crate::config::{TabTitle, TerminalConfig};
    for (choice, expected) in [(TabTitle::Always, Some("npm run dev")), (TabTitle::Never, None)] {
        let session = FakeSession {
            title: Some(" npm run dev ".into()),
            dirty: std::sync::atomic::AtomicBool::new(true),
            ..FakeSession::new(10, 80, 24)
        };
        let config = TerminalConfig { tab_title: choice, ..TerminalConfig::default() };
        let mut view = with_config(session, config);
        assert_eq!(view.live_title(), None, "nothing read yet");
        view.poll();
        assert_eq!(view.live_title(), expected, "{choice:?}");
    }
}

/// In an editor's tab the scrollbar is pressed as at the bottom (the tab's press is
/// `pointer_down`): the lane's top pressed, the history jumps toward its start.
#[test]
fn the_scrollbar_is_pressed_in_an_editor_tab_too() {
    let (mut view, mut text) = painted(FakeSession::new(1050, 200, 50));
    let top = Point::new(view.bar_area.min_x() + 4.0, view.bar_area.min_y() + 2.0);
    view.hover(top);
    paint(&mut view, &mut text);
    assert!(view.pointer_down(top, 1, ui_components::PressMods::default()).is_none());
    assert!(view.grid.display_offset > 0, "the history jumped toward its start");
    view.release();
}

/// A terminal known by a key is renamed by it (its session renamed elsewhere): its tab's
/// name at its next poll; an unknown key, or one whose tab closed, refused.
#[test]
fn a_terminal_is_renamed_by_its_key() {
    let mut view = TerminalView::default();
    let start = TerminalStart {
        key: Some("rename-test-key".into()),
        title: Some("Claude Code".into()),
        ..TerminalStart::default()
    };
    ui_components::Viewer::set_data(&mut view, &start);
    assert_eq!(view.live_title(), Some("Claude Code"));
    assert!(rename("rename-test-key", "test-new-tab"));
    assert!(view.poll(), "renamed: drawn again");
    assert_eq!(view.live_title(), Some("test-new-tab"));
    assert!(!rename("no-such-key", "x"));
    drop(view);
    assert!(!rename("rename-test-key", "y"), "its tab closed");
}

/// A keyed terminal's name wins over what its program calls itself; one closed and opened
/// again with the same key is the one typed into (its old slot let go).
#[test]
fn a_keyed_terminal_keeps_its_name_and_is_found_again() {
    let start = TerminalStart {
        key: Some("reopened-key".into()),
        title: Some("Mine".into()),
        ..TerminalStart::default()
    };
    let mut old = TerminalView::default();
    ui_components::Viewer::set_data(&mut old, &start);
    drop(old);
    let mut view = TerminalView::default();
    ui_components::Viewer::set_data(&mut view, &start);
    view.program_title = Some("✳ Claude Code".into());
    assert_eq!(view.live_title(), Some("Mine"));
    assert!(running("reopened-key"), "the new tab, not the closed one");
    view.poll();
    assert!(!running("reopened-key"), "no program runs in it (none started)");
    drop(view);
    assert!(!running("reopened-key"), "its tab closed");
}

/// A program another module runs, just started: its first output isn't drawn while it
/// comes (a loader), the screen shows once it's been quiet a moment — or at once on a key.
#[test]
fn a_started_program_s_first_output_is_let_run_unseen() {
    let mut view = TerminalView::default();
    let now = std::time::Instant::now();
    view.settling = Some(Settling { started: now, last: now });
    assert!(!view.poll(), "still coming: nothing to draw yet");
    assert!(view.settling.is_some() && view.is_animating(), "the loader turns");
    assert_eq!(view.frame_deadline(), Some(now + SETTLE_QUIET));
    let earlier = now - SETTLE_QUIET * 2;
    view.settling = Some(Settling { started: earlier, last: earlier });
    assert!(view.poll(), "quiet: the screen shows");
    assert!(view.settling.is_none());
    view.settling = Some(Settling { started: now, last: now });
    view.text_input("y");
    assert!(view.settling.is_none(), "typed into: shown at once");
}
