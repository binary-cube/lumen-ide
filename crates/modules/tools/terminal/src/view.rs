//! One shell, as a view in the terminal's window (a tab there): its grid drawn through the
//! same pipeline as the editor, in the colors chosen (the theme's, classic, high contrast),
//! the font size, line height and cursor of the settings (Settings → Terminal); text selected with the mouse (a double click: the word), links opened with
//! ⌘ / Ctrl + click (a URL, or `file:line:column` of a file that exists). The shell starts
//! the first time the view is drawn (then its size and the app's waker are known), in the
//! project's directory, and runs on while the view is hidden.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use kernel::{Point, Rect, Rgba};
use render_api::{Layer, Quad};
use terminal_session::text::{LinkTarget, TermSelection, links, word_at};
use terminal_session::{
    CursorShape, GridSnapshot, InputModes, TermColor, TermKey, TermModifiers, TermOptions,
    TermSize, TerminalSession, encode_key, encode_paste, encode_wheel, flags,
};
use text_layout::{Font, TextSystem};
use theming::caret::CaretShape;
use ui_components::control::{Control, Controls, Shown, Thumb};
use ui_components::{
    KeyPress, Palette, PressedKey, Viewer, ViewerAction, ViewerCursor, ViewerPaint,
};

use crate::colors::TermColors;
use crate::config::{Colors, Cursor};
use ui_kit::{centered_baseline, draw_text};

/// Spacing between the view's edge and the grid (logical).
const PADDING: f32 = 12.0;
/// The scrollbar's lane on the right, beside the grid (logical): it never covers text.
const LANE: f32 = 10.0;

/// The shells running now (the Task Manager's detail).
pub(crate) static LIVE: AtomicUsize = AtomicUsize::new(0);

/// The shells running now per project (its identity; `None`: none open), for the tabs'
/// numbers: another project's shells never count in this one's.
pub(crate) static LIVE_IN: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<Option<kernel::ProjectId>, usize>>,
> = std::sync::LazyLock::new(Default::default);

/// Where a new shell starts (given when the view is opened), or what program it runs in
/// its place (a module's: an agent's session) and the tab's icon for it.
#[derive(Debug, Clone, Default)]
pub struct TerminalStart {
    pub cwd: Option<PathBuf>,
    /// The project it's opened in (its identity): its shells are numbered apart.
    pub project: Option<kernel::ProjectId>,
    /// A program run instead of the shell (`None`: the shell chosen in the settings).
    pub launch: Option<agents::Launch>,
    /// Its tab's icon (`None`: the terminal's own).
    pub icon: Option<&'static str>,
    /// Its tab's name (`None`: the one it was opened with).
    pub title: Option<String>,
    /// What it's known by (a program another module runs: renamed by that key).
    pub key: Option<String>,
}

/// The terminals known by a key, each its name slot (set by [`rename`], taken by the view at
/// its poll): weak, gone with their views.
static NAMED: Mutex<Vec<(String, std::sync::Weak<NameSlot>)>> = Mutex::new(Vec::new());

/// What's asked of a terminal by its key, waiting for its view (a new name), and what its
/// view tells (its program still runs).
#[derive(Debug)]
struct Asked {
    name: Option<String>,
    running: bool,
}

impl Default for Asked {
    /// Made to run a program: running until its view says otherwise.
    fn default() -> Self {
        Self { name: None, running: true }
    }
}

type NameSlot = Mutex<Asked>;

/// The longest name a tab is given by its key.
const TITLE_MOST: usize = 200;

/// The live terminal known by `key` (those gone let go first: a tab closed and opened again
/// with the same key is found, not its old one).
fn named(key: &str) -> Option<Arc<NameSlot>> {
    let mut named = NAMED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    named.retain(|(_, slot)| slot.strong_count() > 0);
    named.iter().filter(|(k, _)| k == key).find_map(|(_, s)| s.upgrade())
}

/// The terminal known by `key` called `title`: `false` if there's none (its tab closed).
pub(crate) fn rename(key: &str, title: &str) -> bool {
    let Some(slot) = named(key) else { return false };
    let title: String = title.chars().take(TITLE_MOST).collect();
    slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).name = Some(title);
    true
}

/// Whether the program of the terminal known by `key` still runs (`false`: no such one).
pub(crate) fn running(key: &str) -> bool {
    named(key)
        .is_some_and(|slot| slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).running)
}

/// What a link in the terminal opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalLink {
    Url(String),
    /// An existing file, at a line and column (1-based) when given.
    File {
        path: PathBuf,
        line: Option<u32>,
        column: Option<u32>,
    },
}

/// How long after the wheel's last step a part of a line at the bottom stays (the
/// gesture's momentum still coming).
/// A selection held past the grid's edge scrolls a step this often…
const AUTO_EVERY: std::time::Duration = std::time::Duration::from_millis(50);
/// …by at most this many lines (the farther past the edge, the more).
const AUTO_MOST: i32 = 8;

const SNAP_AFTER: std::time::Duration = std::time::Duration::from_millis(200);

/// How long the screen may stay behind the program's output (its reader busy at every
/// frame) before a frame waits for it once.
const MAX_BEHIND: std::time::Duration = std::time::Duration::from_millis(100);

/// A program another module runs (an agent's session taken up again) prints, as it starts,
/// what it had — maybe a very long conversation. Drawn as it comes, the screen would scroll
/// through all of it (and every frame's copy of the screen would hold up the reader). So
/// at first it isn't drawn: the output is let run, a loader shows, and once the output has
/// been quiet a moment (or after a while at most) the screen shows at its end — all of it
/// in the history, to scroll back to. A key, the wheel: shown at once.
#[derive(Debug, Clone, Copy)]
struct Settling {
    started: std::time::Instant,
    last: std::time::Instant,
}

/// How long the output must be quiet before the screen shows, and how long at most.
const SETTLE_QUIET: std::time::Duration = std::time::Duration::from_millis(300);
const SETTLE_MOST: std::time::Duration = std::time::Duration::from_secs(20);

impl Settling {
    /// When it ends if nothing more comes.
    fn ends(self) -> std::time::Instant {
        (self.last + SETTLE_QUIET).min(self.started + SETTLE_MOST)
    }
}

#[derive(Default)]
pub struct TerminalView {
    session: Option<Box<dyn TerminalSession>>,
    /// Its first output let run without being drawn (a program another module runs).
    settling: Option<Settling>,
    /// Since when a frame found the screen busy (the program's output being parsed): what
    /// was shown stays meanwhile.
    stale_since: Option<std::time::Instant>,
    /// Lines the wheel scrolled while the screen was busy: scrolled at the next frame.
    pending_scroll: i32,
    /// The shell didn't start (said in the view).
    failed: Option<String>,
    cwd: Option<PathBuf>,
    size: TermSize,
    grid_area: Rect,
    /// The grid, reused across frames.
    grid: GridSnapshot,
    /// Size of one cell (physical pixels), from the last paint.
    cell: (f32, f32),
    /// The text selected with the mouse (in lines that survive scrolling).
    selection: Option<TermSelection>,
    /// The mouse button is down, extending the selection.
    selecting: bool,
    /// Where the mouse is while it extends the selection: held past the grid's top or
    /// bottom, the history scrolls (the selection with it).
    drag_at: Option<Point>,
    /// When the history scrolls by the next step while the mouse is held past an edge.
    auto_next: Option<std::time::Instant>,
    /// The link under the mouse while ⌘ / Ctrl is held: its line and columns.
    link: Option<(i32, std::ops::Range<u16>)>,
    action: Option<ViewerAction>,
    /// The options given to the session last (sent again only when the settings change).
    options: Option<TermOptions>,
    /// The colors worked out for a theme's palette and a choice (until either changes).
    colors: Option<(ColorsKey, TermColors)>,
    /// The colors the session answers programs with, as given to it last.
    answered: Option<terminal_session::TermPalette>,
    /// Counted among the live shells while it exists (a view the app made; not the tests'
    /// bare ones).
    _live: Option<Live>,
    /// The project it was opened in (a program another module runs too, though it isn't
    /// counted among the shells): said on its process's row in the Task Manager.
    project: Option<kernel::ProjectId>,
    /// Its shell's process id, listed while it's held.
    shell: Option<ShellPid>,
    /// The wheel's part of a line not reported yet to a program that takes the wheel (a
    /// trackpad's small steps add up).
    wheel_rest: f32,
    /// The history scrolled by pixels: the part of a line beyond the emulator's offset.
    smooth: terminal_session::scroll::SmoothScroll,
    /// When the wheel last moved it (a part of a line at the bottom snaps away once the
    /// gesture is over: [`SNAP_AFTER`]).
    last_wheel: Option<std::time::Instant>,
    /// Where the mouse is (the wheel's reports to a program say over which cell).
    pointer: Option<Point>,
    /// The history's scrollbar, in its lane right of the grid.
    bar: ui_components::EdgeScrollbar,
    bar_area: Rect,
    /// Its parts' state of use (see [`TerminalView::with_controls`]).
    controls: Controls,
    /// The tab's name from its program (as the settings say when): read when the screen
    /// changes, kept for the tabs' drawing.
    program_title: Option<String>,
    /// The title the shell had at its prompt (a command's own title differs from it).
    prompt_title: Option<String>,
    /// What its session is doing (who has the terminal), asked as its title is read.
    watch: Option<std::sync::Arc<dyn terminal_session::SessionWatch>>,
    /// A program it runs instead of the shell (a module asked: an agent's session).
    launch: Option<agents::Launch>,
    /// Its tab's icon, as asked.
    icon: Option<&'static str>,
    /// Its tab's name, as asked (its program's title goes before it, as the settings say).
    title: Option<String>,
    /// What's asked of it by its key (a name, text to type), taken at its poll.
    renamed: Option<Arc<NameSlot>>,
    /// What it's known by (a program another module runs).
    key: Option<String>,
    /// Its own settings in place of the saved ones (tests: never the shared ones).
    #[cfg(test)]
    config: Option<std::sync::Arc<crate::config::TerminalConfig>>,
}

/// A running shell: its process id (the Task Manager measures it, with what it runs), its
/// project, and what tells what runs in it (asked before closing).
#[derive(Clone)]
pub(crate) struct Shell {
    pub pid: u32,
    pub project: Option<kernel::ProjectId>,
    pub watch: Option<std::sync::Arc<dyn terminal_session::SessionWatch>>,
}

/// The running shells.
pub(crate) static SHELLS: std::sync::Mutex<Vec<Shell>> = std::sync::Mutex::new(Vec::new());

/// The running shells' process ids.
pub(crate) fn shell_pids() -> Vec<u32> {
    SHELLS.lock().map(|s| s.iter().map(|s| s.pid).collect()).unwrap_or_default()
}

/// The shells asked to close (the Task Manager's "Close the session", through the module):
/// their views end ([`Viewer::closed`]), their tabs close and their sessions with them.
pub(crate) static CLOSING: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// The running shells' projects, by process id (those opened in one).
pub(crate) fn shell_projects() -> Vec<(u32, kernel::ProjectId)> {
    SHELLS
        .lock()
        .map(|s| s.iter().filter_map(|s| Some((s.pid, s.project?))).collect())
        .unwrap_or_default()
}

/// Shell `pid` (a running one) asked to close: `false` if it isn't one.
pub(crate) fn close_shell(pid: u32) -> bool {
    let running = SHELLS.lock().is_ok_and(|s| s.iter().any(|s| s.pid == pid));
    if running && let Ok(mut closing) = CLOSING.lock() {
        closing.push(pid);
    }
    running
}

/// A shell's process id among [`SHELLS`], for as long as its view holds it.
#[derive(Debug)]
struct ShellPid(u32);

impl ShellPid {
    fn new(shell: Shell) -> Self {
        let pid = shell.pid;
        if let Ok(mut shells) = SHELLS.lock() {
            shells.push(shell);
        }
        Self(pid)
    }
}

impl Drop for ShellPid {
    fn drop(&mut self) {
        if let Ok(mut shells) = SHELLS.lock() {
            shells.retain(|s| s.pid != self.0);
        }
        if let Ok(mut closing) = CLOSING.lock() {
            closing.retain(|p| *p != self.0);
        }
    }
}

/// One of the [`LIVE`] shells (of a project's, [`LIVE_IN`]), for as long as it's held.
#[derive(Debug)]
struct Live(Option<kernel::ProjectId>);

impl Live {
    fn new(project: Option<kernel::ProjectId>) -> Self {
        LIVE.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut all) = LIVE_IN.lock() {
            *all.entry(project).or_default() += 1;
        }
        Self(project)
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        LIVE.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut all) = LIVE_IN.lock()
            && let Some(n) = all.get_mut(&self.0)
        {
            *n = n.saturating_sub(1);
            if *n == 0 {
                all.remove(&self.0);
            }
        }
    }
}

/// The shells running now in `project`.
pub(crate) fn live_in(project: Option<kernel::ProjectId>) -> usize {
    LIVE_IN.lock().ok().and_then(|all| all.get(&project).copied()).unwrap_or(0)
}

impl std::fmt::Debug for TerminalView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalView").field("cwd", &self.cwd).finish_non_exhaustive()
    }
}

/// What the terminal's colors are worked out from: the choice and the theme's colors.
type ColorsKey = (Colors, Rgba, Rgba, Rgba, [Rgba; 16]);

impl TerminalView {
    /// The history scrolled by `lines`, without waiting for the program's output being
    /// parsed: busy, it's scrolled at the next frame (with the lines asked meanwhile).
    fn scroll_lines(&mut self, lines: i32) {
        let Some(session) = self.session.as_mut() else { return };
        let lines = self.pending_scroll + lines;
        self.pending_scroll = if session.try_scroll(lines) { 0 } else { lines };
    }

    /// The screen copied after a scroll, if it's free now (else the next frame copies it).
    fn show_scrolled(&mut self) {
        let Some(session) = self.session.as_mut() else { return };
        if self.pending_scroll == 0 && session.try_snapshot(&mut self.grid) {
            self.stale_since = None;
        } else {
            self.stale_since.get_or_insert_with(std::time::Instant::now);
        }
    }

    pub fn new() -> Self {
        Self { _live: Some(Live::new(None)), ..Self::default() }
    }

    /// The settings it follows: the saved ones (a test's own, in a test).
    fn config(&self) -> std::sync::Arc<crate::config::TerminalConfig> {
        #[cfg(test)]
        if let Some(config) = &self.config {
            return std::sync::Arc::clone(config);
        }
        crate::config::current()
    }

    /// The colors for `palette` and `choice`, worked out once for them.
    fn term_colors(&mut self, palette: &Palette, choice: Colors) -> TermColors {
        let key = (
            choice,
            palette.terminal_foreground,
            palette.terminal_background,
            palette.terminal_cursor,
            palette.terminal_ansi,
        );
        match &self.colors {
            Some((k, colors)) if *k == key => *colors,
            _ => {
                let colors = TermColors::new(palette, choice);
                self.colors = Some((key, colors));
                colors
            }
        }
    }

    /// Size in cells for the grid area.
    fn term_size(&self) -> TermSize {
        let (w, h) = self.cell;
        if w <= 0.0 || h <= 0.0 {
            return TermSize { cols: 80, rows: 24, cell_width: 8, cell_height: 16 };
        }
        TermSize {
            cols: ((self.grid_area.size.width / w).floor() as u16).max(2),
            rows: ((self.grid_area.size.height / h).floor() as u16).max(1),
            cell_width: w.round() as u16,
            cell_height: h.round() as u16,
        }
    }

    /// The shell, started on the first paint (the waker and the size known then).
    fn start(&mut self, waker: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
        if self.session.is_some() || self.failed.is_some() {
            return;
        }
        let waker = waker.unwrap_or_else(|| std::sync::Arc::new(|| {}));
        // The shell chosen in the settings (by default the user's own, a login shell).
        let profile = match &self.launch {
            Some(launch) => crate::config::launched(launch, self.cwd.clone()),
            None => {
                // In a project (opened at its root): its own history.
                let project = self._live.as_ref().and_then(|l| l.0).and(self.cwd.as_deref());
                crate::config::profile(&self.config(), self.cwd.clone(), project)
            }
        };
        let size = self.term_size();
        match pty::spawn(&profile, size, waker) {
            Ok(session) => {
                let project = self.project;
                let watch = session.watch();
                self.watch.clone_from(&watch);
                // Noted for the next start, should the app end without stopping it (plan 83).
                if let Some(pid) = session.pid() {
                    let program = profile.program.as_deref().unwrap_or(&profile.name);
                    let agent = self.launch.is_some();
                    process::launched::shell(pid, program.as_ref(), profile.cwd.as_deref(), agent);
                }
                self.shell = session.pid().map(|pid| ShellPid::new(Shell { pid, project, watch }));
                self.session = Some(Box::new(session));
                self.size = size;
                if self.launch.is_some() {
                    let now = std::time::Instant::now();
                    self.settling = Some(Settling { started: now, last: now });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "the terminal didn't start");
                self.failed = Some(e.to_string());
            }
        }
    }

    /// The visible cell (row, column) under `p`; outside the grid, the nearest one when
    /// `clamp` (dragging a selection past the edge).
    fn cell_at(&self, p: Point, clamp: bool) -> Option<(u16, u16)> {
        let (w, h) = self.cell;
        let a = self.grid_area;
        if w <= 0.0 || h <= 0.0 || self.grid.rows == 0 || (!clamp && !a.contains(p)) {
            return None;
        }
        let col = ((p.x - a.min_x()) / w).floor().clamp(0.0, f32::from(self.grid.cols) - 1.0);
        // The grid drawn shifted down while scrolled by part of a line.
        let top = a.min_y() + self.smooth.shift(h);
        let row = ((p.y - top) / h).floor().clamp(0.0, f32::from(self.grid.rows) - 1.0);
        Some((row as u16, col as u16))
    }

    /// How many lines a step scrolls with the mouse at `p` while selecting: up (positive)
    /// above the grid, down below it, more the farther it is (at most [`AUTO_MOST`]); 0 inside.
    fn edge_lines(&self, p: Point) -> i32 {
        let a = self.grid_area;
        let line = self.cell.1.max(1.0);
        let (past, up) = if p.y < a.min_y() {
            (a.min_y() - p.y, true)
        } else if p.y > a.max_y() {
            (p.y - a.max_y(), false)
        } else {
            return 0;
        };
        let lines = ((past / line).floor() as i32 + 1).min(AUTO_MOST);
        if up { lines } else { -lines }
    }

    /// The history scrolled a step, at `now`, while the mouse is held past an edge of the
    /// grid selecting (one step every [`AUTO_EVERY`]), the selection's end following. `true`
    /// if it scrolled.
    fn auto_scroll(&mut self, now: std::time::Instant) -> bool {
        let Some(p) = self.drag_at.filter(|_| self.selecting) else {
            self.auto_next = None;
            return false;
        };
        let lines = self.edge_lines(p);
        if lines == 0 {
            self.auto_next = None;
            return false;
        }
        if self.auto_next.is_some_and(|due| now < due) {
            return false;
        }
        self.scroll_lines(lines);
        self.show_scrolled();
        self.auto_next = Some(now + AUTO_EVERY);
        self.follow_drag(p);
        true
    }

    /// The mouse pressed on the grid: starts a selection (a double click selects the word).
    pub fn begin_selection(&mut self, p: Point, clicks: u32) -> bool {
        let Some((row, col)) = self.cell_at(p, false) else { return false };
        let at = self.grid.point(row, col);
        self.selection = Some(TermSelection { anchor: at, head: at });
        self.selecting = true;
        if clicks >= 2
            && let Some(word) = word_at(&self.grid.row_chars(row), col)
        {
            self.selection = Some(TermSelection {
                anchor: self.grid.point(row, word.start),
                head: self.grid.point(row, word.end.saturating_sub(1)),
            });
            self.selecting = false;
        }
        true
    }

    /// The mouse moved with the button down: the selection follows it; past the grid's top
    /// or bottom, the history scrolls that way too (and goes on while it's held there).
    pub fn drag_selection(&mut self, p: Point) -> bool {
        if !self.selecting {
            return false;
        }
        self.drag_at = Some(p);
        let scrolled = self.auto_scroll(std::time::Instant::now());
        scrolled | self.follow_drag(p)
    }

    /// The selection's moving end where the mouse is (clamped to the grid's edges).
    fn follow_drag(&mut self, p: Point) -> bool {
        let Some((row, col)) = self.cell_at(p, true) else { return false };
        let head = self.grid.point(row, col);
        match &mut self.selection {
            Some(selection) if selection.head != head => {
                selection.head = head;
                true
            }
            _ => false,
        }
    }

    /// The button released: an empty selection (a plain click) is dropped; another one is
    /// copied at once when the settings say so.
    pub fn end_selection(&mut self) -> bool {
        let was = std::mem::take(&mut self.selecting);
        self.drag_at = None;
        self.auto_next = None;
        if self.selection.is_some_and(|s| s.is_empty()) {
            self.selection = None;
        }
        if self.selection.is_some()
            && self.config().copy_on_select
            && let Some(text) = self.selected_text()
        {
            self.action = Some(ViewerAction::Request(ui_components::AppRequest::CopyText(text)));
        }
        was
    }

    /// The tab's name from its program, read again (the screen changed): as the settings
    /// say — never, while a command runs (its title, else its name), or always. `true` if
    /// it changed.
    fn refresh_title(&mut self) -> bool {
        use terminal_session::history::Foreground;
        let Some(session) = self.session.as_ref() else { return false };
        let title = match self.config().tab_title {
            crate::config::TabTitle::Never => None,
            crate::config::TabTitle::Always => session.title(),
            crate::config::TabTitle::WhileRunning => {
                let watch = self.watch.as_ref();
                let running = watch
                    .as_ref()
                    .is_some_and(|w| matches!(w.foreground(), Foreground::Program(_)));
                if running {
                    // The title a command set, else what's running.
                    let own = session.title().filter(|t| Some(t) != self.prompt_title.as_ref());
                    own.or_else(|| watch.and_then(|w| w.program()))
                } else {
                    self.prompt_title = session.title();
                    None
                }
            }
        };
        let title = title.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
        if title == self.program_title {
            return false;
        }
        self.program_title = title;
        true
    }

    /// The shell ended and its tab stays (the settings say so): its exit code.
    fn ended(&self) -> Option<i32> {
        let code = self.session.as_ref()?.exit_status()?;
        (!self.config().close_on_exit).then_some(code)
    }

    /// The link under `p`: a URL, or a file that exists (relative to the terminal's start
    /// directory, `~` or absolute).
    pub fn link_under(&self, p: Point) -> Option<(i32, std::ops::Range<u16>, TerminalLink)> {
        if !self.config().highlight_links {
            return None;
        }
        let (row, col) = self.cell_at(p, false)?;
        let link = links(&self.grid.row_chars(row)).into_iter().find(|l| l.cols.contains(&col))?;
        let target = match link.target {
            LinkTarget::Url(url) => TerminalLink::Url(url),
            LinkTarget::File { path, line, column } => {
                let path = self.resolve_path(&path)?;
                TerminalLink::File { path, line, column }
            }
        };
        Some((self.grid.point(row, 0).line, link.cols, target))
    }

    fn resolve_path(&self, written: &str) -> Option<PathBuf> {
        // `~/` (and PowerShell's `~\\` on Windows): the home folder.
        let home = written
            .strip_prefix("~/")
            .or_else(|| cfg!(windows).then(|| written.strip_prefix("~\\")).flatten());
        let path = match home {
            Some(rest) => std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(rest))?,
            None => PathBuf::from(written),
        };
        let path = if path.is_absolute() { path } else { self.cwd.as_ref()?.join(path) };
        path.is_file().then_some(path)
    }

    /// The scrollbar's content and offset (physical pixels, from the history's top): `None`
    /// when there's nothing to scroll (no history, or a full-screen program's screen).
    fn bar_metrics(&self) -> Option<(f32, f32)> {
        let g = &self.grid;
        if g.alt_screen || g.history == 0 || g.rows == 0 {
            return None;
        }
        let h = self.cell.1.max(1.0);
        let content = (g.history + g.rows as usize) as f32 * h;
        let offset =
            (g.history.saturating_sub(g.display_offset) as f32 * h - self.smooth.shift(h)).max(0.0);
        // The bar's track is its lane's height: the content scaled to it.
        let k = self.bar_area.size.height / (f32::from(g.rows) * h).max(1.0);
        Some((content * k, offset * k))
    }

    /// Its parts (the history's scrollbar with its offset, shown while there's a history),
    /// with their state of use. The bar moves by its own pixels: the history follows it, by
    /// whole lines. The keys and the text go to the shell, never through them.
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let metrics = self.bar_metrics();
        let before = metrics.map_or(0.0, |(_, offset)| offset);
        let mut offset = before;
        let mut history = Thumb { bar: &mut self.bar, offset: &mut offset };
        let mut history = Shown { part: &mut history, shown: metrics.is_some() };
        let mut parts: [&mut dyn Control; 1] = [&mut history];
        let answer = f(&mut self.controls, &mut parts);
        if offset != before {
            self.scroll_to(offset);
        }
        answer
    }

    /// The history scrolled to `offset` (the bar's, from the top). `true` if it moved.
    fn scroll_to(&mut self, offset: f32) -> bool {
        let g = &self.grid;
        let h = self.cell.1.max(1.0);
        let k = self.bar_area.size.height / (f32::from(g.rows) * h).max(1.0);
        let from_top = (offset / k / h).round() as usize;
        let wanted = g.history.saturating_sub(from_top.min(g.history));
        let delta = wanted as i64 - g.display_offset as i64;
        let Some(session) = self.session.as_mut() else { return false };
        if delta == 0 {
            return false;
        }
        // The bar moves by whole lines.
        self.smooth.reset();
        self.grid.want_above = false;
        session.scroll(delta as i32);
        session.snapshot(&mut self.grid);
        true
    }

    fn write(&mut self, bytes: &[u8]) -> bool {
        let Some(session) = self.session.as_mut() else { return false };
        // Its lock is the reader's too (held while it parses): taken only when there's
        // something to scroll.
        if self.grid.display_offset != 0 {
            session.scroll_to_bottom();
        }
        self.smooth.reset();
        self.grid.want_above = false;
        session.write(bytes);
        true
    }
}

/// The terminal's key for a key press.
fn term_key(key: PressedKey) -> TermKey {
    match key {
        PressedKey::Char(c) => TermKey::Char(c),
        PressedKey::Enter => TermKey::Enter,
        PressedKey::Backspace => TermKey::Backspace,
        PressedKey::Tab => TermKey::Tab,
        PressedKey::Escape => TermKey::Escape,
        PressedKey::Up => TermKey::Up,
        PressedKey::Down => TermKey::Down,
        PressedKey::Left => TermKey::Left,
        PressedKey::Right => TermKey::Right,
        PressedKey::Home => TermKey::Home,
        PressedKey::End => TermKey::End,
        PressedKey::PageUp => TermKey::PageUp,
        PressedKey::PageDown => TermKey::PageDown,
        PressedKey::Insert => TermKey::Insert,
        PressedKey::Delete => TermKey::Delete,
        PressedKey::F(n) => TermKey::F(n),
    }
}

impl Viewer for TerminalView {
    fn memory(&self) -> ui_components::ViewMemory {
        // Its screen and history (what the scrollback setting allows, as filled).
        let ram = self.session.as_ref().map_or(0, |s| s.memory());
        ui_components::ViewMemory { ram, gpu: 0 }
    }

    fn set_data(&mut self, data: &dyn std::any::Any) {
        // Where it starts (only before it has): as asked, or — reopened from the session —
        // in its project.
        if self.session.is_some() {
            return;
        }
        if let Some(start) = data.downcast_ref::<TerminalStart>() {
            self.cwd.clone_from(&start.cwd);
            self.launch.clone_from(&start.launch);
            self.icon = start.icon;
            self.title.clone_from(&start.title);
            self.key.clone_from(&start.key);
            if let Some(key) = &start.key {
                let slot = Arc::new(Mutex::new(Asked::default()));
                NAMED
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push((key.clone(), Arc::downgrade(&slot)));
                self.renamed = Some(slot);
            }
            self.project = start.project;
            // A program another module runs isn't one of the shells (they're numbered apart).
            if start.launch.is_none() {
                self._live = Some(Live::new(start.project));
            }
        } else if let Some(restored) = data.downcast_ref::<ui_components::Restored>() {
            self.cwd.clone_from(&restored.project);
            self._live = Some(Live::new(restored.id));
            self.project = restored.id;
        }
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        // Selecting with the mouse held past an edge: the history goes on scrolling.
        self.auto_scroll(std::time::Instant::now());
        let b = ctx.bounds;
        if b.size.width <= 0.0 || b.size.height <= 0.0 {
            return;
        }
        let palette = ctx.palette;
        let s = ctx.scale;
        let px = |v: f32| (v * s).round();
        let config = self.config();
        let colors = self.term_colors(palette, config.colors);
        ctx.scene.push_layer(Some(b)).quad(Quad::filled(b, colors.background));
        let pad = px(PADDING);
        let lane = px(LANE);
        // On whole pixels: the rows (whole pixels tall) never straddle one, nor overlap.
        let top = (b.min_y() + px(6.0)).round();
        self.grid_area = Rect::new(
            (b.min_x() + pad).round(),
            top,
            (b.size.width - 2.0 * pad - lane).max(0.0),
            (b.max_y() - pad - top).max(0.0),
        );
        // The bar's lane: right of the grid, its whole height (always kept: the columns
        // don't change when history appears).
        self.bar_area = Rect::new(
            b.max_x() - pad / 2.0 - lane,
            b.min_y() + px(6.0),
            lane,
            (b.size.height - px(6.0) - pad / 2.0).max(0.0),
        );
        // Cell size from the code font, at the size and line height chosen.
        let size = config.font_size.unwrap_or(palette.code_font_size);
        let font = Font::new(palette.code_font.clone(), px(size));
        let cell_w = ctx.text.shape_line("M", &font).width;
        let cell_h = (px(size) * config.line_height).round();
        self.cell = (cell_w, cell_h);
        self.start(ctx.waker.clone());
        // Programs asking its colors (a light or a dark background) are answered these.
        let answered = colors.palette();
        if let Some(session) = &self.session
            && self.answered != Some(answered)
        {
            session.set_palette(answered);
            self.answered = Some(answered);
        }
        let layer = ctx.scene.push_layer(Some(self.grid_area));
        if let Some(why) = &self.failed {
            let message = i18n::tr!("terminal.failed", reason = why.as_str());
            let shaped = ctx.text.shape_line(&message, &font);
            let metrics = ctx.text.metrics(&font);
            let baseline = centered_baseline(self.grid_area.min_y(), cell_h, &metrics);
            draw_text(layer, &shaped, self.grid_area.min_x(), baseline, palette.danger);
            return;
        }
        let size = self.term_size();
        // A part of a line at the bottom (where the output is followed) once the gesture is
        // over: gone — the screen exactly at its bottom, its first row whole at the top.
        let gesture_over = self.last_wheel.is_none_or(|at| at.elapsed() >= SNAP_AFTER);
        if self.grid.display_offset == 0 && gesture_over {
            self.smooth.reset();
        }
        let Some(session) = self.session.as_mut() else { return };
        if size != self.size {
            session.resize(size);
            self.size = size;
            // Resized: the screen is the program's again, at a whole line.
            self.smooth.reset();
        }
        let options = TermOptions {
            scrollback: config.scrollback,
            history_clear: config.history_clear,
            cursor: match config.cursor {
                Cursor::Block => CursorShape::Block,
                Cursor::Bar => CursorShape::Beam,
                Cursor::Underline => CursorShape::Underline,
            },
        };
        if self.options != Some(options) {
            session.set_options(options);
            self.options = Some(options);
        }
        // Its first output still coming: not drawn (a loader instead), the reader let run.
        if self.settling.is_some() {
            let ds = ui_components::ds::Ctx::new(palette, s);
            let label = i18n::t("terminal.loading");
            let loader = ui_components::ds::Loader::with_text(&label);
            let width = loader.width(ctx.text, &ds);
            let x = self.grid_area.min_x() + (self.grid_area.size.width - width) / 2.0;
            let mid = self.grid_area.min_y() + self.grid_area.size.height / 2.0;
            ui_components::ds::loader(layer, ctx.text, &ds, x, mid, &loader);
            return;
        }
        self.grid.want_above = self.smooth.partial();
        // Never waiting for the program's output being parsed (keys and frames go on): the
        // screen it had stays, and the next frame tries again — waiting only once it's been
        // behind for a while, so the screen is never further behind than that.
        let behind = self.stale_since.is_some_and(|t| t.elapsed() >= MAX_BEHIND);
        if self.pending_scroll != 0 {
            if behind {
                session.scroll(self.pending_scroll);
                self.pending_scroll = 0;
            } else if session.try_scroll(self.pending_scroll) {
                self.pending_scroll = 0;
            }
        }
        if behind {
            session.snapshot(&mut self.grid);
            self.stale_since = None;
        } else if session.try_snapshot(&mut self.grid) {
            self.stale_since = None;
        } else {
            self.stale_since.get_or_insert_with(std::time::Instant::now);
        }
        // Moved by itself (the output followed, the history cleared, a full-screen program):
        // nothing partial left where there's nothing to show.
        if self.grid.alt_screen {
            self.smooth.reset();
        }
        self.smooth.settle(self.grid.display_offset, self.grid.history);
        // The selection under the text, and the ⌘-hovered link underlined; all of it shifted
        // down by the part of a line scrolled (the line above in the gap).
        let shift = self.smooth.shift(cell_h);
        let area = Rect::new(
            self.grid_area.min_x(),
            self.grid_area.min_y() + shift,
            self.grid_area.size.width,
            self.grid_area.size.height,
        );
        let x_of = |col: u16| (area.min_x() + f32::from(col) * cell_w).round();
        if let Some(selection) = self.selection.filter(|s| !s.is_empty()) {
            for row in 0..self.grid.rows {
                let line = self.grid.point(row, 0).line;
                if let Some(cols) = selection.columns(line, self.grid.cols) {
                    let top = area.min_y() + f32::from(row) * cell_h;
                    layer.quad(Quad::filled(
                        Rect::new(x_of(cols.start), top, x_of(cols.end) - x_of(cols.start), cell_h),
                        palette.selection,
                    ));
                }
            }
        }
        if let Some((line, cols)) = &self.link
            && let Some(row) = self.grid.visible_row(*line)
        {
            let thin = s.round().max(1.0);
            let y = area.min_y() + f32::from(row + 1) * cell_h - 3.0 * thin;
            layer.quad(Quad::filled(
                Rect::new(x_of(cols.start), y.round(), x_of(cols.end) - x_of(cols.start), thin),
                palette.accent,
            ));
        }
        let style = GridStyle {
            cell: (cell_w, cell_h),
            font: &font,
            focused: ctx.focused,
            blink: config.blink,
            caret_id: std::ptr::from_ref(self) as u64,
            colors: &colors,
            bold_bright: config.bold_bright,
            scale: s,
        };
        paint_grid(layer, &self.grid, area, ctx.text, &style);
        // Ended, its tab kept: said on a band at the grid's foot.
        if let Some(code) = self.ended() {
            let notice = i18n::tr!("settings.terminal_ended", code = code);
            let band = Rect::new(
                self.grid_area.min_x(),
                (self.grid_area.max_y() - cell_h).round(),
                self.grid_area.size.width,
                cell_h,
            );
            layer.quad(Quad::filled(band, colors.background));
            let shaped = ctx.text.shape_line(&notice, &font);
            let baseline = centered_baseline(band.min_y(), cell_h, &ctx.text.metrics(&font));
            draw_text(layer, &shaped, band.min_x(), baseline, palette.text_muted);
        }
        // The history's place: where the screen is in it (none without history, nor on a
        // full-screen program's own screen).
        if let Some((content, offset)) = self.bar_metrics() {
            self.bar.paint(ctx.scene, palette, s, self.bar_area, content, offset);
        }
    }

    fn poll(&mut self) -> bool {
        // A frame that found the screen busy asks for the next one.
        let dirty = self.session.as_ref().is_some_and(|s| s.take_dirty());
        // Its first output: the screen shows once it's quiet (else nothing to draw yet).
        if let Some(settling) = &mut self.settling {
            let now = std::time::Instant::now();
            if dirty {
                settling.last = now;
            }
            if now < settling.ends() {
                return false;
            }
            self.settling = None;
            return true;
        }
        // The screen changed: what the program calls itself may have too.
        let mut renamed = dirty && self.refresh_title();
        // Known by a key: renamed by it (its session renamed), and says whether it runs.
        let name = self.renamed.as_ref().and_then(|slot| {
            let running = self.session.as_ref().is_some_and(|s| s.exit_status().is_none());
            let mut asked = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            asked.running = running;
            asked.name.take()
        });
        if let Some(title) = name {
            renamed |= self.title.as_deref() != Some(title.as_str());
            self.title = Some(title);
        }
        self.stale_since.is_some() | dirty | renamed
    }

    /// What the program calls itself, else its name — but a terminal known by a key (an
    /// agent's session) keeps the name it's given (followed, or the user's) over the program's.
    fn live_title(&self) -> Option<&str> {
        if self.key.is_some() {
            return self.title.as_deref().or(self.program_title.as_deref());
        }
        self.program_title.as_deref().or(self.title.as_deref())
    }

    /// Its first output let run: woken when it should be quiet by then.
    fn frame_deadline(&self) -> Option<std::time::Instant> {
        let settling = self.settling.map(Settling::ends);
        // The next step of a selection scrolling past an edge.
        match (settling, self.auto_next) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// A part of a line at the bottom waiting for its gesture to end: frames until it snaps;
    /// its loader turning while its first output comes.
    fn is_animating(&self) -> bool {
        self.settling.is_some()
            || self.grid.display_offset == 0
                && self.smooth.partial()
                && self.last_wheel.is_some_and(|at| at.elapsed() < SNAP_AFTER)
    }

    /// The shell exited (`exit`; its tab stays when the settings say so), or was asked to
    /// close from the Task Manager: its tab closes (its session with it).
    fn closed(&self) -> bool {
        let asked = self
            .shell
            .as_ref()
            .is_some_and(|ShellPid(pid)| CLOSING.lock().is_ok_and(|c| c.contains(pid)));
        let exited = self.session.as_ref().is_some_and(|s| s.exit_status().is_some());
        asked || (exited && self.ended().is_none())
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    /// The wheel: the history scrolls by pixels (a trackpad's steps and its momentum
    /// continuous; a wheel's notch its lines): the emulator by whole lines, the grid drawn
    /// shifted by the rest. A program that asked for the mouse, or a full-screen one, gets it
    /// instead, at once, in lines.
    fn scroll(&mut self, _dx: f32, dy: f32) -> bool {
        self.settling = None;
        let line = self.cell.1.max(1.0);
        let at = self.pointer.and_then(|p| self.cell_at(p, true)).unwrap_or((0, 0));
        // A program that asked for the mouse gets the wheel, unless the settings say not.
        let reported = self.config().mouse_reporting
            && terminal_session::encode_wheel(1, at, &self.grid).is_some();
        if !reported {
            let (offset, history) = (self.grid.display_offset, self.grid.history);
            let before = self.smooth;
            let lines = self.smooth.wheel(dy, line, offset, history);
            self.last_wheel = Some(std::time::Instant::now());
            if self.session.is_none() {
                return false;
            }
            if lines != 0 {
                self.scroll_lines(lines);
            }
            if lines == 0 && self.smooth == before {
                return false;
            }
            // Shown now (not at the next output), the line above with it when partial.
            self.grid.want_above = self.smooth.partial();
            self.show_scrolled();
            return true;
        }
        self.wheel_rest += -dy / line;
        let lines = self.wheel_rest.trunc();
        self.wheel_rest -= lines;
        let lines = lines as i32;
        if lines == 0 {
            return false;
        }
        let at = self.pointer.and_then(|p| self.cell_at(p, true)).unwrap_or((0, 0));
        let report = encode_wheel(lines, at, &self.grid);
        let Some(session) = self.session.as_mut() else { return false };
        match report {
            Some(bytes) => {
                session.write(&bytes);
                false
            }
            None => {
                self.scroll_lines(lines);
                // Shown now (not at the next output).
                self.show_scrolled();
                true
            }
        }
    }

    fn hover(&mut self, p: Point) -> bool {
        self.pointer = Some(p);
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    fn press(&mut self, p: Point, clicks: u32) -> Option<ViewerAction> {
        // Its parts: the scrollbar's thumb taken, or the history jumped there.
        if self.with_controls(|c, parts| c.press(parts, p, clicks)).is_some() {
            return None;
        }
        self.begin_selection(p, clicks);
        None
    }

    /// In an editor's tab (an agent's session): a press as at the bottom — the scrollbar's
    /// thumb taken, a selection started.
    fn pointer_down(
        &mut self,
        p: Point,
        clicks: u32,
        _mods: ui_components::PressMods,
    ) -> Option<ViewerAction> {
        self.press(p, clicks)
    }

    /// The part holding the pointer follows it (`true` if the history moved), else the
    /// selection does.
    fn drag(&mut self, p: Point) -> bool {
        let shown = self.grid.display_offset;
        if self.with_controls(|c, parts| c.drag(parts, p)).is_some() {
            return self.grid.display_offset != shown;
        }
        self.drag_selection(p)
    }

    fn release(&mut self) -> bool {
        let part = self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, c)| c);
        part | self.end_selection()
    }

    fn link_hover(&mut self, p: Option<Point>) -> bool {
        let link = p.and_then(|p| self.link_under(p)).map(|(line, cols, _)| (line, cols));
        let changed = link != self.link;
        self.link = link;
        changed
    }

    fn link_at(&self, p: Point) -> Option<ViewerAction> {
        Some(match self.link_under(p)?.2 {
            TerminalLink::Url(url) => ViewerAction::OpenLink(url),
            TerminalLink::File { path, line, column } => {
                ViewerAction::OpenFile { path, line, column }
            }
        })
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.link.is_some() {
            return Some(ViewerCursor::Pointer);
        }
        self.grid_area.contains(p).then_some(ViewerCursor::Text)
    }

    fn takes_keyboard(&self) -> bool {
        true
    }

    fn key_press(&mut self, key: KeyPress) -> bool {
        self.settling = None;
        self.selection = None;
        // Typing: the cursor shows solid, its blinking starts again.
        ui_kit::caret::reset();
        let Some(session) = self.session.as_ref() else { return false };
        let modes = session.try_modes().unwrap_or_else(|| InputModes::of(&self.grid));
        let mods = TermModifiers { shift: key.shift, ctrl: key.ctrl, alt: key.alt };
        let Some(bytes) = encode_key(term_key(key.key), mods, modes.app_cursor) else {
            return false;
        };
        self.write(&bytes)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.settling = None;
        self.selection = None;
        ui_kit::caret::reset();
        self.write(text.as_bytes())
    }

    fn selected_text(&self) -> Option<String> {
        let selection = self.selection.filter(|s| !s.is_empty())?;
        Some(selection.text(&self.grid)).filter(|t| !t.is_empty())
    }

    fn clear_selection(&mut self) -> bool {
        self.selection.take().is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        let Some(session) = self.session.as_ref() else { return false };
        let modes = session.try_modes().unwrap_or_else(|| InputModes::of(&self.grid));
        let bytes = encode_paste(text, modes.bracketed_paste);
        self.write(&bytes)
    }

    /// A shell is opened again at the next start; a program a module asked for isn't (that
    /// module takes it up again its own way: an agent's session resumed).
    fn restorable(&self) -> bool {
        self.launch.is_none()
    }

    fn tab_icon(&self) -> Option<&'static str> {
        self.icon
    }

    /// A program another module runs by a key (an agent's session) can be renamed.
    fn renamable(&self) -> bool {
        self.key.is_some()
    }

    /// Its tab renamed, and its session asked to be so too (whoever follows it, its agent's
    /// own way).
    fn rename(&mut self, title: &str) -> Option<ViewerAction> {
        let key = self.key.clone()?;
        self.title = Some(title.chars().take(TITLE_MOST).collect());
        let asked = contracts::agents::RenameSession { key, title: title.to_owned() };
        self.action = Some(ViewerAction::Ask(ui_components::Asked::new(asked)));
        None
    }

    /// Clear Terminal (⌘K / Ctrl+Shift+K): the history emptied, whatever programs may do.
    fn command(&mut self, id: &str) -> bool {
        if id != crate::CLEAR_ACTION {
            return false;
        }
        let Some(session) = self.session.as_mut() else { return false };
        session.clear_history();
        self.selection = None;
        session.snapshot(&mut self.grid);
        true
    }
}

/// Color of a cell: defaults and the 16 ANSI colors as chosen, the 6×6×6 cube and grays
/// for 16–255; colors redefined by programs (OSC 4 / 10 / 11) take priority.
fn resolve(color: TermColor, foreground: bool, grid: &GridSnapshot, colors: &TermColors) -> Rgba {
    let rgb = |(r, g, b): (u8, u8, u8)| {
        Rgba::rgb((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
    };
    match color {
        TermColor::Default => {
            let index = if foreground { 256 } else { 257 };
            match grid.palette_overrides.get(&index) {
                Some(&c) => rgb(c),
                None if foreground => colors.foreground,
                None => colors.background,
            }
        }
        TermColor::Rgb(r, g, b) => rgb((r, g, b)),
        TermColor::Indexed(i) => {
            if let Some(&c) = grid.palette_overrides.get(&u16::from(i)) {
                return rgb(c);
            }
            match i {
                0..=15 => colors.ansi[i as usize],
                _ => rgb(terminal_session::indexed_rgb(i)),
            }
        }
    }
}

/// A cell's foreground and background as drawn: bold in the bright colors when chosen (the
/// first 8 become their bright ones), inverse swapped, dim faded.
fn cell_colors(
    c: &terminal_session::TermCell,
    grid: &GridSnapshot,
    colors: &TermColors,
    bold_bright: bool,
) -> (Rgba, Rgba) {
    let fg = match c.fg {
        TermColor::Indexed(i) if bold_bright && i < 8 && c.flags & flags::BOLD != 0 => {
            TermColor::Indexed(i + 8)
        }
        other => other,
    };
    let mut fg = resolve(fg, true, grid, colors);
    let mut bg = resolve(c.bg, false, grid, colors);
    if c.flags & flags::INVERSE != 0 {
        std::mem::swap(&mut fg, &mut bg);
    }
    if c.flags & flags::DIM != 0 {
        fg = fg.with_alpha(fg.a * 0.6);
    }
    (fg, bg)
}

/// How the grid is drawn: the cell's size, the font, the colors and the cursor.
struct GridStyle<'a> {
    cell: (f32, f32),
    font: &'a Font,
    /// The view has the keys (a solid cursor; else its outline).
    focused: bool,
    /// The cursor blinks (as chosen in the settings), and which one it is (its glide and
    /// trail: `ui_kit::caret`).
    blink: bool,
    caret_id: u64,
    colors: &'a TermColors,
    /// Bold text in the bright colors (the first 8 become their bright ones).
    bold_bright: bool,
    scale: f32,
}

fn paint_grid(
    layer: &mut Layer,
    grid: &GridSnapshot,
    area: Rect,
    text: &mut TextSystem,
    style: &GridStyle<'_>,
) {
    let GridStyle { cell: (cell_w, cell_h), font, focused, colors: palette, .. } = *style;
    // Its four faces: regular, bold, italic, bold italic.
    let faces = [
        font.clone(),
        font.clone().with_weight(Font::BOLD),
        font.clone().with_italic(true),
        font.clone().with_weight(Font::BOLD).with_italic(true),
    ];
    let face = |flags: u8| {
        let bold = usize::from(flags & flags::BOLD != 0);
        let italic = usize::from(flags & flags::ITALIC != 0);
        &faces[bold + 2 * italic]
    };
    let metrics = text.metrics(font);
    let default_bg = resolve(TermColor::Default, false, grid, palette);
    let thin = style.scale.round().max(1.0);
    let colors = |c: &terminal_session::TermCell| cell_colors(c, grid, palette, style.bold_bright);
    // The bits that choose the face.
    let face_bits = |c: &terminal_session::TermCell| c.flags & (flags::BOLD | flags::ITALIC);

    let mut run = String::new();
    // The history line above the screen first, while it's scrolled by part of a line (the
    // grid shifted down: `area` starts lower, the layer's clip cuts what's outside).
    let first: i32 = if grid.above.is_empty() { 0 } else { -1 };
    for row in first..i32::from(grid.rows) {
        let cells = if row < 0 { &grid.above[..] } else { grid.row(row as u16) };
        let top = area.min_y() + row as f32 * cell_h;
        let baseline = centered_baseline(top, cell_h, &metrics);
        let x_of = |col: usize| (area.min_x() + col as f32 * cell_w).round();

        // Backgrounds, merged into runs of the same color.
        let mut col = 0;
        while col < cells.len() {
            let (_, bg) = colors(&cells[col]);
            let start = col;
            while col < cells.len() && colors(&cells[col]).1 == bg {
                col += 1;
            }
            if bg != default_bg {
                layer.quad(Quad::filled(
                    Rect::new(x_of(start), top, x_of(col) - x_of(start), cell_h),
                    bg,
                ));
            }
        }

        // Text, in ASCII runs of the same style (aligned exactly to cells); the remaining
        // characters (box-drawing, emoji, CJK) one at a time, at their cell.
        let mut col = 0;
        while col < cells.len() {
            let cell = cells[col];
            let (fg, _) = colors(&cell);
            if cell.flags & flags::HIDDEN != 0 || cell.ch == ' ' {
                col += 1;
                continue;
            }
            let bits = face_bits(&cell);
            let f = face(cell.flags);
            if !cell.ch.is_ascii() {
                let shaped = text.shape_line(cell.ch.encode_utf8(&mut [0; 4]), f);
                draw_text(layer, &shaped, x_of(col), baseline, fg);
                col += 1;
                continue;
            }
            let start = col;
            run.clear();
            while col < cells.len() {
                let c = cells[col];
                let same = c.ch.is_ascii()
                    && c.flags & flags::HIDDEN == 0
                    && face_bits(&c) == bits
                    && colors(&c).0 == fg;
                if !same {
                    break;
                }
                run.push(c.ch);
                col += 1;
            }
            let trimmed = run.trim_end();
            let shaped = text.shape_line(trimmed, f);
            draw_text(layer, &shaped, x_of(start), baseline, fg);
            for (i, c) in cells[start..col].iter().enumerate() {
                if c.flags & (flags::UNDERLINE | flags::STRIKE) != 0 {
                    let y = if c.flags & flags::UNDERLINE != 0 {
                        baseline + thin * 2.0
                    } else {
                        top + cell_h / 2.0
                    };
                    layer.quad(Quad::filled(
                        Rect::new(x_of(start + i), y.round(), cell_w.ceil(), thin),
                        fg,
                    ));
                }
            }
        }
    }

    // Cursor: drawn as the caret's look says when focused (blinking as chosen here, its glow,
    // glide and trail), an outline when not; its shape the program's, else the settings'.
    let Some((row, col)) = grid.cursor else { return };
    if grid.cursor_shape == CursorShape::Hidden || grid.display_offset > 0 {
        return;
    }
    let x = (area.min_x() + f32::from(col) * cell_w).round();
    let y = (area.min_y() + f32::from(row) * cell_h).round();
    let rect = Rect::new(x, y, cell_w.round(), cell_h);
    let cursor = palette.cursor;
    if !focused {
        layer.quad(Quad::filled(rect, Rgba::TRANSPARENT).bordered(thin, cursor));
        return;
    }
    let shape = match grid.cursor_shape {
        CursorShape::Beam => CaretShape::Bar,
        CursorShape::Underline => CaretShape::Underline,
        _ => CaretShape::Block,
    };
    let drawn = ui_kit::caret::paint(
        layer,
        ui_kit::caret::CaretPaint {
            id: style.caret_id,
            cell: rect,
            color: cursor,
            scale: style.scale,
            shape: Some(shape),
            thickness: Some((2.0 * style.scale).round()),
            steady: !style.blink,
        },
    );
    // A block shows its character over it (while it's mostly shown).
    if shape == CaretShape::Block && drawn.is_some() && (!style.blink || ui_kit::caret::visible()) {
        let cell = grid.row(row).get(col as usize).copied().unwrap_or_default();
        if cell.ch != ' ' {
            let shaped = text.shape_line(cell.ch.encode_utf8(&mut [0; 4]), face(cell.flags));
            let baseline = centered_baseline(y, cell_h, &metrics);
            // Glyphs are drawn over quads in the same layer.
            draw_text(layer, &shaped, x, baseline, default_bg);
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/view.rs"]
mod tests;
