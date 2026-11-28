use super::*;
use terminal_session::text::TermPoint;

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
    /// How many times a selection's text was built.
    built: std::sync::Arc<std::sync::atomic::AtomicUsize>,
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
        // What it shows, in a number: its lines and where it's scrolled.
        out.fingerprint = (self.lines.len() as u64) << 20 | (self.offset as u64) << 4 | 1;
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
    fn selection_text(&self, selection: &TermSelection, history: usize) -> Option<String> {
        self.try_selection_text(selection, history)
    }
    /// Whole rows of what the selection covers, history included (busy: none).
    fn try_selection_text(&self, selection: &TermSelection, history: usize) -> Option<String> {
        if self.busy.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        self.built.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (start, end) = selection.ordered();
        let at = |line: i32| usize::try_from(history as i64 + i64::from(line)).unwrap_or(0);
        let last = self.lines.len().checked_sub(1)?;
        let rows = self.lines.get(at(start.line)..=at(end.line).min(last))?;
        Some(rows.join("\n"))
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
    // Over the bar, and held over the text: the bar's cursor, never the text's.
    assert_eq!(view.cursor_at(top), Some(ui_components::scrollbar::CURSOR), "the bar's");
    let text_at = view.grid_area.center();
    view.drag(text_at);
    view.hover(text_at);
    assert_eq!(view.cursor_at(text_at), Some(ui_components::scrollbar::CURSOR), "held: the bar's");
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
    // Its lane pressed: no bar there to take it (the press is the screen's).
    let lane = Point::new(view.bar_area.min_x() + 4.0, view.bar_area.min_y() + 2.0);
    view.hover(lane);
    view.press(lane, 1);
    assert!(!view.controls.held(), "no bar held");
    view.release();
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

/// A program another module runs (an agent's session) keeps the project it was opened in,
/// though it isn't counted among the shells: its process's row says that project.
#[test]
fn a_program_run_for_a_module_keeps_its_project() {
    let project = Some(kernel::ProjectId::new());
    let start = TerminalStart {
        project,
        launch: Some(agents::Launch::new("agent", &[], None)),
        ..TerminalStart::default()
    };
    let mut view = TerminalView::default();
    ui_components::Viewer::set_data(&mut view, &start);
    assert_eq!(view.project, project);
    assert!(view._live.is_none(), "not one of the numbered shells");
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
    assert!(view.settling.is_some() && view.is_loading(), "the loader turns (a loader's pace)");
    assert!(!view.is_animating(), "not every frame the display could draw");
    assert_eq!(view.frame_deadline(), Some(now + SETTLE_QUIET));
    let earlier = now - SETTLE_QUIET * 2;
    view.settling = Some(Settling { started: earlier, last: earlier });
    assert!(view.poll(), "quiet: the screen shows");
    assert!(view.settling.is_none());
    view.settling = Some(Settling { started: now, last: now });
    view.text_input("y");
    assert!(view.settling.is_none(), "typed into: shown at once");
}

/// Selecting with the mouse held past the grid's top: the history scrolls up a step at a
/// time (the farther, the more), the selection growing with it; back inside, it stops; the
/// button let go, nothing more scrolls.
#[test]
fn a_selection_held_past_an_edge_scrolls_the_history() {
    let mut view =
        with_config(FakeSession::new(200, 80, 24), crate::config::TerminalConfig::default());
    let mut text = TextSystem::new();
    paint(&mut view, &mut text);
    let (w, h) = view.cell;
    let area = view.grid_area;
    let inside = Point::new(area.min_x() + 2.0 * w, area.min_y() + 5.0 * h + 1.0);
    assert!(view.begin_selection(inside, 1));
    let before = view.selected_text().map_or(0, |t| t.lines().count());
    // Held a line and a half above the top: a step of two lines, then another once due.
    let above = Point::new(inside.x, area.min_y() - 1.5 * h);
    assert_eq!(view.edge_lines(above), 2);
    assert!(view.drag_selection(above));
    let now = std::time::Instant::now();
    assert!(!view.auto_scroll(now), "not before its step is due");
    assert!(view.frame_deadline().is_some(), "a frame asked for the next step");
    assert!(view.auto_scroll(now + AUTO_EVERY));
    view.show_scrolled();
    assert!(view.grid.display_offset >= 4, "scrolled up: {}", view.grid.display_offset);
    let after = view.selected_text().map_or(0, |t| t.lines().count());
    assert!(after > before, "the selection grew with it: {before} → {after}");
    // Far below the bottom: down, by at most AUTO_MOST.
    let far_below = Point::new(inside.x, area.max_y() + 100.0 * h);
    assert_eq!(view.edge_lines(far_below), -AUTO_MOST);
    // Back inside: no more steps.
    view.drag_selection(inside);
    assert!(!view.auto_scroll(now + AUTO_EVERY * 10));
    assert_eq!(view.frame_deadline(), None);
    // Let go past the edge: nothing scrolls after.
    view.drag_selection(above);
    view.end_selection();
    assert!(!view.auto_scroll(now + AUTO_EVERY * 20));
}

/// A keyed program's terminal (an agent's session) with a fake session drawn once, started as
/// far as its sleeping knows (its program not running: nothing to put to sleep yet).
fn keyed_view(key: &str) -> (TerminalView, TextSystem) {
    let start = TerminalStart {
        key: Some(key.into()),
        launch: Some(agents::Launch::new("agent", &[], None)),
        ..TerminalStart::default()
    };
    let mut view = TerminalView::default();
    ui_components::Viewer::set_data(&mut view, &start);
    view.session = Some(Box::new(FakeSession::new(100, 80, 24)));
    if let Some(part) = &view.sleep {
        part.started(None);
    }
    crate::sleep::tell_veto(&named(key).unwrap());
    let mut text = TextSystem::new();
    paint(&mut view, &mut text);
    (view, text)
}

/// Its sleeping slot.
fn sleeping(view: &TerminalView) -> Arc<ui_components::sleep::Slot> {
    Arc::clone(view.sleep.as_ref().expect("a keyed terminal").slot())
}

/// An order to sleep given to `view` now (`why`), as the sleeping gives it.
fn order(view: &TerminalView, why: ui_components::sleep::SleepWhy) {
    let at = std::time::Instant::now();
    sleeping(view).lock().order = Some(ui_components::sleep::Order { at, why });
}

const IDLE_ORDER: ui_components::sleep::SleepWhy =
    ui_components::sleep::SleepWhy::Idle(std::time::Duration::from_secs(3600));

/// How the terminal known by `key` stands, as told to others (`None`: none such).
fn state(key: &str) -> Option<ui_components::sleep::SleepState> {
    ui_components::sleep::find(key).map(|slot| slot.lock().state())
}

/// The terminal known by `key` put to sleep by hand from elsewhere (`false`: not now).
fn sleep_by_key(key: &str) -> bool {
    ui_components::sleep::find(key).is_some_and(|slot| slot.sleep_by_hand().is_ok())
}

/// The terminal known by `key` woken from elsewhere (`false`: not asleep, or none such).
#[cfg(unix)]
fn wake_by_key(key: &str) -> bool {
    ui_components::sleep::find(key).is_some_and(|slot| slot.wake())
}

/// Put to sleep: its program stopped at its poll, its last screen kept, a moon on its tab,
/// how its owner said it resumes kept as its launch; its slot says it's asleep.
#[test]
fn a_sleeping_terminal_keeps_its_screen_and_how_it_resumes() {
    let (mut view, _text, slot) = resumable("sleep-test-key", std::time::Duration::from_secs(60));
    asked(&slot).resume = Some(agents::Launch::new("agent", &["--resume", "id-1"], None));
    let screen = view.grid.cells.clone();
    order(&view, ui_components::sleep::SleepWhy::ByHand);
    assert!(view.poll(), "asleep: its tab changes");
    assert!(view.session.is_none() && view.asleep);
    assert_eq!(view.launch.as_ref().unwrap().args, ["--resume", "id-1"]);
    assert_eq!(view.grid.cells, screen, "its last screen kept");
    assert_eq!(view.tab_icon(), Some(ASLEEP_ICON));
    assert!(!view.closed(), "its tab stays");
    assert_eq!(state("sleep-test-key").map(|s| s.asleep), Some(true));
    assert!(!running("sleep-test-key"), "its program doesn't run");
    assert!(view.options.is_none() && view.answered.is_none(), "told again to the next one");
}

/// A terminal sleeps only untouched: its tab chosen counts as touched, and an order given
/// before a key (or a click, the wheel) is let go; one given while it's left alone is
/// taken, on screen or not.
#[test]
fn a_terminal_sleeps_only_untouched() {
    let (mut view, _text, _slot) = resumable("on-screen-key", std::time::Duration::from_secs(60));
    let before = std::time::Instant::now();
    sleeping(&view).lock().touched = None;
    ui_components::Viewer::set_active(&mut view, true);
    let touched = sleeping(&view).lock().touched;
    assert!(touched.is_some_and(|at| at >= before), "its tab chosen: touched");
    // Ordered, then a key before its poll: awake.
    order(&view, IDLE_ORDER);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let key = KeyPress { key: PressedKey::Char('x'), shift: false, ctrl: false, alt: false };
    view.key_press(key);
    view.poll();
    assert!(view.session.is_some() && !view.asleep, "touched since the order: awake");
    // Ordered while left alone: asleep.
    order(&view, IDLE_ORDER);
    view.poll();
    assert!(view.asleep, "untouched: asleep");
}

/// Drawn again since it was asked to sleep (its window uncovered, a frame), never touched:
/// it sleeps all the same; its tab left, the pointer passing over it touch nothing either.
#[test]
fn drawn_left_or_hovered_a_terminal_is_not_touched() {
    let (mut view, mut text, _slot) =
        resumable("drawn-since-key", std::time::Duration::from_secs(60));
    sleeping(&view).lock().touched = None;
    ui_components::Viewer::set_active(&mut view, false);
    ui_components::Viewer::hover(&mut view, Point::new(5.0, 5.0));
    ui_components::Viewer::link_hover(&mut view, Some(Point::new(5.0, 5.0)));
    assert!(sleeping(&view).lock().touched.is_none(), "left, hovered: not touched");
    order(&view, IDLE_ORDER);
    std::thread::sleep(std::time::Duration::from_millis(2));
    paint(&mut view, &mut text);
    view.poll();
    assert!(view.asleep, "drawn, untouched: asleep");
}

/// Something the user does in a terminal, named.
type Act = (&'static str, fn(&mut TerminalView));

/// What the user does in a terminal counts as touching it (its time untouched starts again):
/// a click, the wheel, text typed, a paste, a command.
#[test]
fn acting_on_a_terminal_touches_it() {
    let (mut view, _text) = keyed_view("touched-key");
    let acts: [Act; 5] = [
        ("a click", |v| drop(v.press(Point::new(5.0, 5.0), 1))),
        ("the wheel", |v| {
            ui_components::Viewer::scroll(v, 0.0, 10.0);
        }),
        ("text", |v| {
            v.text_input("a");
        }),
        ("a paste", |v| {
            v.paste("b");
        }),
        ("a command (Clear)", |v| {
            ui_components::Viewer::command(v, crate::CLEAR_ACTION);
        }),
    ];
    for (what, act) in acts {
        sleeping(&view).lock().touched = None;
        act(&mut view);
        assert!(sleeping(&view).lock().touched.is_some(), "{what}");
    }
}

/// Text typed and not sent keeps a terminal awake: ↵ (or Ctrl+C) clears it.
#[test]
fn text_typed_and_not_sent_is_told() {
    let (mut view, _text) = keyed_view("typed-test-key");
    let slot = named("typed-test-key").unwrap();
    assert!(!asked(&slot).typed);
    view.text_input("refactor the");
    assert!(asked(&slot).typed);
    // ⇧↵ / ⌥↵ only add a line to the draft.
    let shift_enter = KeyPress { key: PressedKey::Enter, shift: true, ctrl: false, alt: false };
    view.key_press(shift_enter);
    assert!(asked(&slot).typed, "a line added, nothing sent");
    let alt_enter = KeyPress { key: PressedKey::Enter, shift: false, ctrl: false, alt: true };
    view.key_press(alt_enter);
    assert!(asked(&slot).typed, "a line added, nothing sent");
    let enter = KeyPress { key: PressedKey::Enter, shift: false, ctrl: false, alt: false };
    view.key_press(enter);
    assert!(!asked(&slot).typed, "sent");
    view.paste("more");
    assert!(asked(&slot).typed);
    let ctrl_c = KeyPress { key: PressedKey::Char('c'), shift: false, ctrl: true, alt: false };
    view.key_press(ctrl_c);
    assert!(!asked(&slot).typed, "dropped");
}

/// A program another module runs starts only when its tab is first drawn: opened and never
/// shown (a tab behind others), nothing runs; it's never reopened at the next start (its
/// module resumes it its own way).
#[test]
fn a_program_starts_only_when_its_tab_is_first_drawn() {
    let start = TerminalStart {
        key: Some("lazy-test-key".into()),
        launch: Some(agents::Launch::new("/bin/sh", &["-c", "sleep 5"], None)),
        ..TerminalStart::default()
    };
    let mut view = TerminalView::default();
    ui_components::Viewer::set_data(&mut view, &start);
    assert!(view.session.is_none(), "not drawn: nothing started");
    assert!(!ui_components::Viewer::restorable(&view), "not reopened by the terminal");
    assert_eq!(ui_components::Viewer::memory(&view).ram, 0);
    assert_eq!(
        state("lazy-test-key").map(|s| s.can_sleep),
        Some(ui_components::sleep::CanSleep::No)
    );
    #[cfg(unix)]
    {
        let mut text = TextSystem::new();
        paint(&mut view, &mut text);
        assert!(view.session.is_some(), "drawn: started");
        assert!(view.settling.is_some(), "its first output let run");
        assert!(sleeping(&view).lock().touched.is_some(), "started: its time left alone counts");
    }
}

/// Put to sleep when idle, it stays asleep when drawn again (its tab shown); woken (a key),
/// its program started the way it resumes, its last screen drawn meanwhile.
#[cfg(unix)]
#[test]
fn a_sleeping_terminal_resumes_when_woken() {
    let (mut view, mut text, slot) = resumable("wake-test-key", std::time::Duration::from_secs(60));
    order(&view, IDLE_ORDER);
    view.poll();
    let screen = view.grid.cells.clone();
    paint(&mut view, &mut text);
    assert!(view.session.is_none() && view.asleep, "shown: still asleep");
    assert!(ui_components::Viewer::text_input(&mut view, "x"), "a key wakes it");
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "started again");
    assert!(!view.asleep && view.resuming);
    assert_eq!(view.grid.cells, screen, "its last screen while the new one settles");
    assert_eq!(state("wake-test-key").map(|s| s.asleep), Some(false));
    assert!(asked(&slot).pid.is_some(), "measured again");
    assert!(view.options.is_some() && view.answered.is_some(), "the new session told them");
}

/// What a keyed terminal holds is answered from memory (its memory as last measured), and
/// its program is called by its tab's title in the Task Manager, renamed with it.
#[test]
fn a_keyed_terminal_tells_what_it_holds_and_its_title() {
    let start = TerminalStart {
        key: Some("holds-test-key".into()),
        title: Some("Fix the login form".into()),
        ..TerminalStart::default()
    };
    let mut view = TerminalView::default();
    ui_components::Viewer::set_data(&mut view, &start);
    let slot = named("holds-test-key").unwrap();
    asked(&slot).pid = Some(4_000_777);
    sleeping(&view).set_memory(Some(300 << 20));
    let held = state("holds-test-key").unwrap();
    assert_eq!((held.memory, held.asleep), (Some(300 << 20), false));
    assert!(program_titles().contains(&(4_000_777, "Fix the login form".to_owned())));
    assert!(rename("holds-test-key", "Review"));
    view.poll();
    assert!(program_titles().contains(&(4_000_777, "Review".to_owned())));
    assert_eq!(state("no-such-key"), None);
}

/// Asked to sleep but its tab chosen since: it stays awake, the order let go.
#[test]
fn a_terminal_chosen_since_it_was_asked_to_sleep_stays_awake() {
    let (mut view, mut text, _slot) =
        resumable("shown-since-key", std::time::Duration::from_secs(60));
    order(&view, IDLE_ORDER);
    std::thread::sleep(std::time::Duration::from_millis(2));
    paint(&mut view, &mut text);
    ui_components::Viewer::set_active(&mut view, true);
    view.poll();
    assert!(view.session.is_some() && !view.asleep, "awake");
    assert!(sleeping(&view).lock().order.is_none(), "the order let go");
}

/// A program quiet for a given time (what "at work" is read from).
struct QuietFor(std::time::Duration);

impl terminal_session::SessionWatch for QuietFor {
    fn foreground(&self) -> terminal_session::history::Foreground {
        terminal_session::history::Foreground::Unknown
    }
    fn program(&self) -> Option<String> {
        None
    }
    fn title(&self) -> Option<String> {
        None
    }
    fn quiet_for(&self) -> Option<std::time::Duration> {
        Some(self.0)
    }
}

/// A keyed terminal its owner knows how to resume, quiet for `quiet`, its tab chosen.
fn resumable(key: &str, quiet: std::time::Duration) -> (TerminalView, TextSystem, Arc<NameSlot>) {
    let (mut view, text) = keyed_view(key);
    ui_components::Viewer::set_active(&mut view, true);
    let slot = named(key).unwrap();
    let watch: Arc<dyn terminal_session::SessionWatch> = Arc::new(QuietFor(quiet));
    {
        let mut asked = asked(&slot);
        asked.resume = Some(agents::Launch::new("/bin/sh", &["-c", "sleep 5"], None));
        asked.watch = Some(Arc::clone(&watch));
        asked.pid = Some(4_000_888);
    }
    if let Some(part) = &view.sleep {
        part.started(Some(Arc::new(WatchQuiet(watch))));
    }
    crate::sleep::tell_veto(&slot);
    (view, text, slot)
}

/// The first row of its tab's menu: its action, greyed out or not.
fn first_row(view: &TerminalView) -> Option<(String, bool)> {
    use ui_components::{ViewMenuEntry, Viewer};
    view.tab_menu().into_iter().next().map(|e| match e {
        ViewMenuEntry::Item { id, look, .. } => (id, look.disabled),
        _ => panic!("a row"),
    })
}

/// "Put to Sleep" in its tab's menu: only when its owner knows how it resumes; greyed out,
/// saying why, while something typed isn't sent. Its program writing (a spinner every
/// moment) doesn't stop the user: they chose.
#[test]
fn put_to_sleep_is_offered_as_it_can_be() {
    use ui_components::sleep::CanSleep;
    let (view, _text) = keyed_view("menu-none-key");
    assert_eq!(first_row(&view), None, "nothing runs, nobody knows how it resumes: hidden");
    let (mut view, _text, slot) = resumable("menu-key", std::time::Duration::from_secs(60));
    assert_eq!(first_row(&view), Some((crate::SLEEP_ACTION.to_owned(), false)));
    view.text_input("draft");
    assert_eq!(first_row(&view).map(|r| r.1), Some(true), "something typed: greyed out");
    let typed = CanSleep::NotNow(crate::sleep::TYPED);
    assert_eq!(state("menu-key").map(|s| s.can_sleep), Some(typed));
    assert!(!sleep_by_key("menu-key"), "not now");
    asked(&slot).typed = false;
    crate::sleep::tell_veto(&slot);
    asked(&slot).owner_busy = true;
    crate::sleep::tell_veto(&slot);
    assert_eq!(first_row(&view).map(|r| r.1), Some(false), "its owner at work: still offered");
    assert!(!sleep_by_key("no-such-key"));
}

/// One click puts it to sleep, even while its program writes every moment (a spinner): the
/// order is taken at once (the click's own poll), never left for a later one; refused (text
/// typed and not sent), it says why — never silently nothing.
#[test]
fn one_click_puts_a_busy_looking_session_to_sleep() {
    use ui_components::Viewer;
    let (mut view, _text, _slot) = resumable("one-click-key", std::time::Duration::from_millis(50));
    assert!(view.command(crate::SLEEP_ACTION));
    assert!(view.asleep && view.session.is_none(), "asleep after one click");
    assert!(view.take_notifications().is_empty());
    let (mut view, _text, _slot) = resumable("typed-click-key", std::time::Duration::from_secs(60));
    view.text_input("draft");
    assert!(view.command(crate::SLEEP_ACTION));
    assert!(!view.asleep, "something typed: not put to sleep");
    assert_eq!(view.take_notifications().len(), 1, "told why");
}

/// Asked from elsewhere (the panel), its window is woken at once: the order is taken at the
/// poll that wake brings, not at an unrelated one; even while its first output settles.
#[test]
fn a_sleep_asked_from_elsewhere_wakes_its_window() {
    use ui_components::Viewer;
    let (mut view, _text, _slot) = resumable("far-click-key", std::time::Duration::from_millis(50));
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    sleeping(&view).lock().waker = Some(Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }));
    let now = std::time::Instant::now();
    view.settling = Some(Settling { started: now, last: now });
    assert!(sleep_by_key("far-click-key"));
    assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), 1, "its window woken");
    assert!(view.poll(), "taken at that poll");
    assert!(view.asleep, "though its first output was still settling");
}

/// Put to sleep by hand while on screen: it stays asleep (its last screen, the band) through
/// paints, until a key resumes it (the key itself sent nowhere).
#[cfg(unix)]
#[test]
fn a_tab_put_to_sleep_by_hand_waits_for_the_user() {
    use ui_components::Viewer;
    let (mut view, mut text, _slot) = resumable("by-hand-key", std::time::Duration::from_secs(60));
    assert!(view.command(crate::SLEEP_ACTION), "asleep now");
    assert!(view.asleep && view.session.is_none());
    for _ in 0..3 {
        paint(&mut view, &mut text);
    }
    assert!(view.session.is_none(), "shown, still asleep: not resumed at a paint");
    let key = KeyPress { key: PressedKey::Char('x'), shift: false, ctrl: false, alt: false };
    assert!(view.key_press(key));
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "a key resumed it");
}

/// Asleep by hand: a click resumes it; covering and uncovering its window, or moving through
/// the tabs onto it, doesn't (nothing woken by mistake).
#[cfg(unix)]
#[test]
fn a_click_resumes_a_tab_asleep_by_hand_moving_onto_it_doesnt() {
    use ui_components::Viewer;
    let (mut view, mut text, _slot) = resumable("click-key", std::time::Duration::from_secs(60));
    assert!(sleep_by_key("click-key"), "asked from elsewhere (the panel)");
    view.poll();
    assert!(view.asleep && view.held);
    view.press(Point::new(10.0, 10.0), 1);
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "a click resumed it");
    let (mut view, mut text, _slot) = resumable("back-key", std::time::Duration::from_secs(60));
    view.command(crate::SLEEP_ACTION);
    view.set_shown(false);
    view.set_shown(true);
    paint(&mut view, &mut text);
    assert!(view.session.is_none(), "covered and uncovered: still asleep");
    view.set_active(false);
    paint(&mut view, &mut text);
    view.set_active(true);
    paint(&mut view, &mut text);
    assert!(view.session.is_none(), "chosen again: still asleep, nothing woken by mistake");
    assert!(view.text_input("x"));
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "a key resumed it");
}

/// Asleep: its tab's menu offers "Wake Up" in place of "Put to Sleep", which resumes it; a
/// wake asked from elsewhere (the panel) is taken at its poll; one not asleep refuses.
#[cfg(unix)]
#[test]
fn an_asleep_tab_offers_wake_up_and_wakes() {
    use ui_components::Viewer;
    let (mut view, mut text, _slot) =
        resumable("wake-menu-key", std::time::Duration::from_secs(60));
    assert_eq!(first_row(&view).map(|r| r.0).as_deref(), Some(crate::SLEEP_ACTION));
    assert!(!wake_by_key("wake-menu-key"), "awake: nothing to wake");
    view.command(crate::SLEEP_ACTION);
    assert_eq!(first_row(&view).map(|r| r.0).as_deref(), Some(crate::WAKE_ACTION), "asleep");
    assert!(view.command(crate::WAKE_ACTION));
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "woken from its menu");
    // From elsewhere.
    let (mut view, mut text, _slot) = resumable("wake-far-key", std::time::Duration::from_secs(60));
    view.command(crate::SLEEP_ACTION);
    assert!(wake_by_key("wake-far-key"));
    assert!(view.poll());
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "woken from the panel");
}

/// Asleep by hand: a card over its last screen says so — why, the memory it freed — with a
/// "Wake Up" button (a hand over it) that resumes it (a press there, or ↵).
#[cfg(unix)]
#[test]
fn an_asleep_tab_shows_its_card_and_its_button_wakes_it() {
    use ui_components::Viewer;
    let (mut view, mut text, _slot) = resumable("card-key", std::time::Duration::from_secs(60));
    sleeping(&view).set_memory(Some(412 << 20));
    view.command(crate::SLEEP_ACTION);
    let slept = view.sleep.as_ref().and_then(|p| p.slept()).unwrap();
    assert_eq!(slept.why, ui_components::sleep::SleepWhy::ByHand);
    assert_eq!(slept.freed, Some(412 << 20), "what it held, said freed");
    paint(&mut view, &mut text);
    let button = view.sleep.as_ref().and_then(|p| p.wake_button()).expect("its button drawn");
    let on = Point::new(button.min_x() + 2.0, button.min_y() + 2.0);
    assert!(view.grid_area.contains(on));
    assert_eq!(view.cursor_at(on), Some(ViewerCursor::Pointer), "a hand on its button");
    view.press(on, 1);
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "its button woke it");
    // ↵ too.
    let (mut view, mut text, _slot) =
        resumable("card-enter-key", std::time::Duration::from_secs(60));
    view.command(crate::SLEEP_ACTION);
    paint(&mut view, &mut text);
    let enter = KeyPress { key: PressedKey::Enter, shift: false, ctrl: false, alt: false };
    assert!(view.key_press(enter));
    paint(&mut view, &mut text);
    assert!(view.session.is_some(), "↵ woke it");
}

/// Woken but its program can't start again (gone): no longer asleep — its failure shows,
/// keys aren't eaten to wake it, nobody's told it sleeps.
#[cfg(unix)]
#[test]
fn a_session_that_cant_resume_is_no_longer_asleep() {
    use ui_components::Viewer;
    let (mut view, mut text, slot) = resumable("no-resume-key", std::time::Duration::from_secs(60));
    let gone = std::env::temp_dir().join(format!("gone-{:032x}", kernel::id::new_uuid()));
    let program = gone.to_string_lossy().into_owned();
    asked(&slot).resume = Some(agents::Launch::new(program, &[], None));
    assert!(view.command(crate::SLEEP_ACTION));
    let key = KeyPress { key: PressedKey::Char('x'), shift: false, ctrl: false, alt: false };
    assert!(view.key_press(key), "the key wakes it");
    paint(&mut view, &mut text);
    assert!(view.session.is_none() && view.failed.is_some(), "it didn't start");
    assert!(!view.asleep && !view.held);
    assert_eq!(state("no-resume-key").map(|s| s.asleep), Some(false));
    assert!(!wake_by_key("no-resume-key"), "nothing to wake");
    assert!(!view.key_press(key), "keys no longer eaten to wake it");
}

/// The asleep card is drawn over the screen's text, never under it: in a layer after the
/// grid's (a layer draws its quads before its glyphs), opaque, the screen dimmed under it.
#[cfg(unix)]
#[test]
fn the_asleep_card_covers_the_screen_s_text() {
    let (mut view, mut text, _slot) = resumable("cover-key", std::time::Duration::from_secs(60));
    view.command(crate::SLEEP_ACTION);
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 2400.0, 1000.0),
        scale: 2.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let layers = scene.layers();
    let grid = layers.iter().position(|l| !l.glyphs.is_empty()).expect("the screen's text");
    let surface = palette.popup_background.with_alpha(1.0);
    let card = layers.iter().rposition(|l| l.quads.iter().any(|q| q.color == surface));
    assert!(card.is_some_and(|c| c > grid), "the card's layer after the grid's text");
    let scrim = layers[card.unwrap()].quads.iter().any(|q| q.bounds == view.grid_area);
    assert!(scrim, "the screen dimmed under it");
}

/// An order waiting is said (the app polls the view even unseen); one too old is let go,
/// never applied late — when its tab is shown again, it doesn't fall asleep.
#[test]
fn an_order_not_taken_in_time_is_let_go() {
    use ui_components::Viewer;
    let (mut view, _text, _slot) = resumable("stale-key", std::time::Duration::from_secs(60));
    assert!(!view.wants_poll());
    let long_ago = std::time::Instant::now() - ui_components::sleep::ORDER_FRESH * 2;
    let why = ui_components::sleep::SleepWhy::ByHand;
    sleeping(&view).lock().order = Some(ui_components::sleep::Order { at: long_ago, why });
    assert!(view.wants_poll(), "an order waits: polled even unseen");
    view.poll();
    assert!(view.session.is_some() && !view.asleep, "too old: let go");
    assert!(!view.wants_poll());
}

/// Asleep or coming back, its last screen stays still: the wheel neither scrolls it nor
/// reaches a program, no selection by drag, no link, no scrollbar, the arrow (a hand on the
/// card's button); back awake, all of it works again.
#[test]
fn a_sleeping_screen_stays_still() {
    use ui_components::Viewer;
    let session = FakeSession::new(5000, 200, 50);
    let written = Arc::clone(&session.written);
    let (mut view, mut text) = painted(session);
    // Coming back from a sleep: its first output settling under its last screen.
    let now = std::time::Instant::now();
    view.resuming = true;
    view.settling = Some(Settling { started: now, last: now + std::time::Duration::from_secs(60) });
    let before = view.grid.display_offset;
    assert!(!view.scroll(0.0, -400.0), "the wheel does nothing");
    assert_eq!(view.grid.display_offset, before);
    assert!(written.lock().unwrap().is_empty(), "nothing sent to the program");
    assert!(view.bar_metrics().is_none(), "no scrollbar");
    let at = Point::new(view.grid_area.min_x() + 40.0, view.grid_area.min_y() + 40.0);
    assert!(view.press(at, 1).is_none());
    assert!(!view.drag(Point::new(at.x + 300.0, at.y + 200.0)));
    view.release();
    assert!(view.selected_text().is_none(), "no selection");
    assert!(!view.link_hover(Some(at)) || view.link.is_none());
    assert_eq!(view.cursor_at(at), Some(ViewerCursor::Arrow));
    // Back awake: the wheel scrolls again.
    view.settling = None;
    view.resuming = false;
    paint(&mut view, &mut text);
    assert!(view.bar_metrics().is_some());
    assert!(view.scroll(0.0, -400.0), "scrolls again");
    assert!(view.grid.display_offset > before, "into the history");
}

/// What waits unsent, as an estimate: ⌫ takes back what was typed (all of it: nothing
/// waits); Esc, Ctrl+U drop it; a paste ending in a new line is sent as it lands.
#[test]
fn what_waits_unsent_follows_erasing_and_dropping() {
    use ui_components::Viewer;
    let (mut view, _text) = keyed_view("estimate-key");
    let slot = named("estimate-key").unwrap();
    let key = |key, ctrl| KeyPress { key, shift: false, ctrl, alt: false };
    view.text_input("ab");
    view.key_press(key(PressedKey::Backspace, false));
    assert!(asked(&slot).typed, "one left");
    view.key_press(key(PressedKey::Backspace, false));
    assert!(!asked(&slot).typed, "all erased: nothing waits");
    view.text_input("draft");
    view.key_press(key(PressedKey::Escape, false));
    assert!(!asked(&slot).typed, "Esc drops it");
    view.text_input("draft");
    view.key_press(key(PressedKey::Char('u'), true));
    assert!(!asked(&slot).typed, "Ctrl+U drops it");
    view.paste("make test\n");
    assert!(!asked(&slot).typed, "a paste ending in a new line is sent");
    view.paste("half a line");
    assert!(asked(&slot).typed);
}

/// On its way to sleep or back, its menu still offers the row, greyed out (never gone, never
/// the opposite of what's happening).
#[test]
fn the_menu_greys_out_sleep_and_wake_while_they_happen() {
    let (view, _text, _slot) = resumable("menu-changing-key", std::time::Duration::from_secs(60));
    assert!(sleep_by_key("menu-changing-key"), "the order given, not yet taken");
    assert_eq!(first_row(&view), Some((crate::SLEEP_ACTION.to_owned(), true)), "going to sleep");
    {
        let slot = sleeping(&view);
        let mut state = slot.lock();
        state.order = None;
        state.waking = true;
    }
    assert_eq!(first_row(&view), Some((crate::WAKE_ACTION.to_owned(), true)), "waking up");
}

/// Just started, its owner not having said yet how it resumes: "Put to Sleep" is there,
/// greyed out (never missing from its menu), until it's known.
#[test]
fn a_session_just_started_offers_sleep_greyed_out() {
    use ui_components::sleep::CanSleep;
    let (view, _text, slot) = resumable("starting-key", std::time::Duration::from_secs(60));
    asked(&slot).resume = None;
    crate::sleep::tell_veto(&slot);
    let can = || state("starting-key").map(|s| s.can_sleep);
    assert_eq!(can(), Some(CanSleep::No), "not started by this view: not starting");
    asked(&slot).started = Some(std::time::Instant::now());
    crate::sleep::tell_veto(&slot);
    assert_eq!(can(), Some(CanSleep::NotNow(crate::sleep::STARTING)));
    assert_eq!(first_row(&view), Some((crate::SLEEP_ACTION.to_owned(), true)));
    assert!(!sleep_by_key("starting-key"), "not yet");
    // Still not known long after it started: it can't sleep (never "starting" for good).
    asked(&slot).started = std::time::Instant::now().checked_sub(crate::sleep::STARTING_WITHIN);
    crate::sleep::tell_veto(&slot);
    assert_eq!(can(), Some(CanSleep::No));
}

/// A session just started is followed (its window woken each second) only while it's
/// starting: known how it resumes, past its time (then told it no longer is), or gone, it's
/// let go — and its project named once.
#[test]
fn a_starting_session_is_followed_only_while_it_starts() {
    use ui_components::sleep::CanSleep;
    let (view, _text, slot) = resumable("followed-key", std::time::Duration::from_secs(60));
    asked(&slot).resume = None;
    asked(&slot).started = Some(std::time::Instant::now());
    crate::sleep::tell_veto(&slot);
    let project = Some(kernel::ProjectId::new());
    let now = std::time::Instant::now();
    let until = now + crate::sleep::STARTING_WITHIN;
    let gone = std::sync::Arc::new(NameSlot::default());
    let mut list = vec![
        (Arc::downgrade(&slot), project, until),
        (Arc::downgrade(&slot), project, until),
        (Arc::downgrade(&gone), None, until),
    ];
    drop(gone);
    assert_eq!(crate::sleep::still_starting(&mut list, now), vec![project], "once, its project");
    assert_eq!(list.len(), 2, "the one gone let go");
    asked(&slot).started = std::time::Instant::now().checked_sub(crate::sleep::STARTING_WITHIN);
    assert!(crate::sleep::still_starting(&mut list, until).is_empty(), "past its time");
    assert_eq!(state("followed-key").map(|s| s.can_sleep), Some(CanSleep::No), "told");
    let mut list = vec![(Arc::downgrade(&slot), project, until)];
    asked(&slot).resume = Some(agents::Launch::default());
    assert!(crate::sleep::still_starting(&mut list, now).is_empty(), "known: let go");
    drop(view);
}

/// Asleep and brought forward again by its key (a click on its session in the panel: its
/// start told again), it stays as it is: asleep, the same slot.
#[test]
fn a_sleeping_tab_asked_again_by_its_key_stays_asleep() {
    use ui_components::Viewer;
    let (mut view, _text, slot) = resumable("again-key", std::time::Duration::from_secs(60));
    assert!(view.command(crate::SLEEP_ACTION));
    assert!(view.asleep);
    let start = TerminalStart { key: Some("again-key".into()), ..TerminalStart::default() };
    view.set_data(&start);
    assert!(view.asleep, "still asleep");
    assert!(view.renamed.as_ref().is_some_and(|s| Arc::ptr_eq(s, &slot)), "the same slot");
    assert_eq!(state("again-key").map(|s| s.asleep), Some(true));
}

/// A selection reaching off the screen: its whole text is made ready with each snapshot, so
/// a copy that finds the emulator busy takes all of it, not the screen's part — only with
/// the snapshot it was made with; one wholly on screen, dragged or too long keeps nothing.
#[test]
fn a_copy_while_the_emulator_is_busy_takes_the_whole_selection() {
    let session = FakeSession::new(200, 80, 24);
    let busy = std::sync::Arc::clone(&session.busy);
    let built = std::sync::Arc::clone(&session.built);
    let (mut view, mut text) = painted(session);
    // From 50 lines up in the history to the screen's 10th row.
    view.selection = Some(TermSelection {
        anchor: TermPoint { line: -50, col: 0 },
        head: TermPoint { line: 10, col: 5 },
    });
    paint(&mut view, &mut text);
    assert!(view.ready_copy.is_some(), "made ready while free");
    let made = built.load(std::sync::atomic::Ordering::Relaxed);
    paint(&mut view, &mut text);
    paint(&mut view, &mut text);
    assert_eq!(built.load(std::sync::atomic::Ordering::Relaxed), made, "repaints build nothing");
    busy.store(true, std::sync::atomic::Ordering::Relaxed);
    let copied = view.selected_text().unwrap_or_default();
    assert_eq!(copied.lines().count(), 61, "all of it, not the screen's 11 rows");
    let first = format!("{:05}", view.grid.history - 50);
    assert!(copied.starts_with(&first), "from the history's line: {copied:.20}");
    // Shown since with other text (output came, the history cleared): what's highlighted
    // may be other text, the ready one isn't taken.
    view.grid.fingerprint += 1;
    assert_eq!(view.selected_text().map(|t| t.lines().count()), Some(11), "the screen's part");
    view.grid.fingerprint -= 1;
    // Another selection, not made ready yet: the screen's part.
    view.selection = Some(TermSelection {
        anchor: TermPoint { line: -40, col: 0 },
        head: TermPoint { line: 2, col: 5 },
    });
    assert_eq!(view.selected_text().map(|t| t.lines().count()), Some(3));
    // Wholly on screen: nothing kept.
    busy.store(false, std::sync::atomic::Ordering::Relaxed);
    view.selection = Some(TermSelection {
        anchor: TermPoint { line: 1, col: 0 },
        head: TermPoint { line: 4, col: 5 },
    });
    paint(&mut view, &mut text);
    assert!(view.ready_copy.is_none());
    // Dragged: made once it ends.
    view.selection = Some(TermSelection {
        anchor: TermPoint { line: -30, col: 0 },
        head: TermPoint { line: 4, col: 5 },
    });
    view.selecting = true;
    paint(&mut view, &mut text);
    assert!(view.ready_copy.is_none(), "not while it's dragged");
    // Too long to build at each frame: left to the copy itself.
    view.selecting = false;
    let most = i32::try_from(COPY_READY_MOST).unwrap();
    view.selection = Some(TermSelection {
        anchor: TermPoint { line: -most, col: 0 },
        head: TermPoint { line: 4, col: 5 },
    });
    paint(&mut view, &mut text);
    assert!(view.ready_copy.is_none(), "too long");
}
