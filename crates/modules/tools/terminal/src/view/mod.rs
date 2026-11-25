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

mod asleep;
mod grid;
mod selection;
mod shells;
mod viewer;

use grid::*;
pub(crate) use shells::*;

/// Spacing between the view's edge and the grid (logical).
const PADDING: f32 = 12.0;
/// How the screen is drawn this frame: its font, colors, cells, and the settings' looks.
struct ScreenLook<'a> {
    font: &'a Font,
    colors: &'a TermColors,
    palette: &'a Palette,
    cell: (f32, f32),
    focused: bool,
    scale: f32,
    blink: bool,
    bold_bright: bool,
}

/// Why a session can't be put to sleep by hand now, as told to the user.
pub(crate) fn cant_sleep(can: contracts::terminal::CanSleep) -> workbench_model::Notification {
    let why = match can {
        contracts::terminal::CanSleep::Typed => "terminal.cant_sleep_typed",
        contracts::terminal::CanSleep::Sleeping => "terminal.cant_sleep_sleeping",
        contracts::terminal::CanSleep::Waking => "terminal.cant_sleep_waking",
        _ => "terminal.cant_sleep",
    };
    workbench_model::Notification::warning(i18n::t(why))
}

/// What a key or a text does to what waits unsent ([`TerminalView::note_typed`]).
#[derive(Debug, Clone, Copy)]
enum Typing {
    Typed(usize),
    Erased,
    Sent,
}

/// A notice drawn on a band at the grid's foot: its text, its font, the band's color.
struct Band<'a> {
    text: &'a str,
    font: &'a Font,
    background: Rgba,
}

/// How much of the terminal's background covers its last screen under the asleep card (the
/// screen still seen, plainly behind it).
const SCRIM_ALPHA: f32 = 0.78;

/// A sleeping terminal's tab icon (in place of its own).
pub(crate) const ASLEEP_ICON: &str = "moon";
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
    /// Its project's root (`None`: none open).
    pub root: Option<PathBuf>,
    /// Its tab is in the editor (a program another module runs there).
    pub in_editor: bool,
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
    /// Its project's root (its history spills under the project's data folder).
    root: Option<PathBuf>,
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
    /// Tells it's gone when the view goes (a keyed terminal only).
    _gone: Option<Gone>,
    /// What it's known by (a program another module runs).
    key: Option<String>,
    /// Its program stopped while unseen and idle (`crate::sleep`): its last screen kept, its
    /// program started again with `launch` (how it resumes) when it's shown.
    asleep: bool,
    /// Started again after a sleep: its last screen stays drawn while the new output settles.
    resuming: bool,
    /// Asleep by hand: not started again when drawn, only once the user acts on it (a key,
    /// a click, the tab shown again after leaving it).
    held: bool,
    /// Its tab is the active one of its group (told by the app: what the user chose).
    active: bool,
    /// What it tells the user, taken by the app (why it couldn't be put to sleep).
    notices: Vec<workbench_model::Notification>,
    /// Characters typed since the last send, an estimate (the program's own line editing —
    /// a word erased, history recalled — isn't seen): anything left keeps it from sleeping.
    typed: usize,
    /// Why it sleeps (said on its card until it's back), and where its card's "Wake Up"
    /// button was drawn last.
    slept: Option<asleep::Slept>,
    wake_button: Option<Rect>,
    /// Its own settings in place of the saved ones (tests: never the shared ones).
    #[cfg(test)]
    config: Option<std::sync::Arc<crate::config::TerminalConfig>>,
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
        if self.session.is_some() || self.failed.is_some() || self.held {
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
                if let Some(slot) = &self.renamed {
                    let mut asked = asked(slot);
                    asked.pid = session.pid();
                    asked.watch.clone_from(&self.watch);
                    asked.asleep = false;
                    asked.typed = false;
                }
                self.typed = 0;
                // A new session: the options and colors are told to it at its first paint; its
                // screen at the bottom, where the program goes on.
                self.options = None;
                self.smooth.reset();
                self.pending_scroll = 0;
                self.selection = None;
                self.answered = None;
                let mut session: Box<dyn TerminalSession> = Box::new(session);
                session.set_spill(crate::spill::store(self.root.as_deref()));
                self.session = Some(session);
                self.size = size;
                // Woken from a sleep: its last screen stays until the new one settles.
                self.resuming = std::mem::take(&mut self.asleep);
                if let Some(slot) = &self.renamed {
                    asked(slot).waking = self.resuming;
                }
                if self.launch.is_some() {
                    let now = std::time::Instant::now();
                    self.settling = Some(Settling { started: now, last: now });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "the terminal didn't start");
                self.failed = Some(e.to_string());
                // Woken and not started: no longer asleep (its failure shows instead; no key
                // is taken to wake it, nobody's told it sleeps).
                self.asleep = false;
                self.held = false;
                self.slept = None;
                if let Some(slot) = &self.renamed {
                    let mut asked = asked(slot);
                    asked.asleep = false;
                    asked.waking = false;
                }
            }
        }
    }

    /// Put to sleep (`crate::sleep`): its program stopped (its process ends as when its tab
    /// closes), its last screen kept, `launch` how it starts again once shown. Nothing done
    /// if its program ended meanwhile.
    fn fall_asleep(&mut self, launch: agents::Launch, by_hand: bool) {
        let Some(session) = &self.session else { return };
        if session.exit_status().is_some() {
            return;
        }
        self.session = None;
        self.watch = None;
        self.shell = None;
        self.settling = None;
        self.stale_since = None;
        self.selection = None;
        self.launch = Some(launch);
        self.asleep = true;
        self.selecting = false;
        self.drag_at = None;
        self.link = None;
        // Its last screen without a cursor: nothing runs there to type to.
        self.grid.cursor = None;
        self.held = by_hand;
        // What the next program is told comes from the settings again (its options, the
        // colors it's answered): a new session knows none of them.
        self.options = None;
        self.answered = None;
        let freed = self.renamed.as_ref().and_then(|slot| asked(slot).memory);
        let idle = if by_hand { None } else { self.config().sleep_after };
        self.slept = Some(asleep::Slept { by_hand, idle, freed });
        if let Some(slot) = &self.renamed {
            let mut asked = asked(slot);
            asked.asleep = true;
            asked.running = false;
            asked.pid = None;
            asked.watch = None;
            asked.memory = None;
        }
        self.announce();
        tracing::info!(by_hand, "a session was put to sleep");
    }

    /// Asleep, or coming back (its last screen shown while the new one settles): the screen
    /// stays still — no scroll, selection, link or scrollbar.
    pub(super) fn frozen(&self) -> bool {
        self.asleep || (self.resuming && self.settling.is_some())
    }

    /// Asleep by hand and acted on by the user: resumed at its next paint. `true` if so.
    fn wake_by_user(&mut self) -> bool {
        if !self.asleep {
            return false;
        }
        self.held = false;
        if let Some(slot) = &self.renamed {
            asked(slot).waking = true;
        }
        self.announce();
        true
    }

    /// How it stands told on its project's bus, if that changed (a keyed terminal only).
    pub(super) fn announce(&self) {
        if let (Some(slot), Some(key)) = (&self.renamed, &self.key) {
            shells::announce(slot, key);
        }
    }

    /// Drawn now: its slot says when (a terminal shown never sleeps).
    fn mark_shown(&self) {
        if let Some(slot) = &self.renamed {
            asked(slot).shown = Some(std::time::Instant::now());
        }
    }

    /// What was typed since the last send, as an estimate ([`Self::typed`]): `Typed` adds
    /// characters, `Erased` takes one back (⌫), `Sent` empties it (↵, Ctrl+C, Ctrl+U, Esc, a
    /// paste ending in a new line). Told to its slot as "text waits unsent".
    fn note_typed(&mut self, typed: Typing) {
        self.typed = match typed {
            Typing::Typed(n) => self.typed.saturating_add(n),
            Typing::Erased => self.typed.saturating_sub(1),
            Typing::Sent => 0,
        };
        if let Some(slot) = &self.renamed {
            asked(slot).typed = self.typed > 0;
        }
    }

    /// The screen as it stands (the grid copied last): the selection, the ⌘-hovered link,
    /// the cells, and `notice` on a band at its foot.
    fn paint_screen(&mut self, layer: &mut Layer, text: &mut TextSystem, look: &ScreenLook<'_>) {
        let (cell_w, cell_h) = look.cell;
        let (palette, s) = (look.palette, look.scale);
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
            cell: look.cell,
            font: look.font,
            focused: look.focused,
            blink: look.blink,
            caret_id: std::ptr::from_ref(self) as u64,
            colors: look.colors,
            bold_bright: look.bold_bright,
            scale: s,
        };
        paint_grid(layer, &self.grid, area, text, &style);
    }

    /// The last screen dimmed under what's said over it (a sleeping session's card): the
    /// terminal's background over it, partly.
    fn paint_scrim(&self, layer: &mut Layer, colors: &TermColors) {
        let scrim = colors.background.with_alpha(SCRIM_ALPHA);
        layer.quad(Quad::filled(self.grid_area, scrim));
    }

    /// The history's place: where the screen is in it (none without history, nor on a
    /// full-screen program's own screen).
    fn paint_bar(&mut self, scene: &mut render_api::Scene, palette: &Palette, scale: f32) {
        if let Some((content, offset)) = self.bar_metrics() {
            self.bar.paint(scene, palette, scale, self.bar_area, content, offset);
        }
    }

    /// A notice on a band at the grid's foot (the session ended, it's being resumed).
    fn paint_band(
        &self,
        layer: &mut Layer,
        text: &mut TextSystem,
        (band, color): (Band<'_>, Rgba),
    ) {
        let h = self.cell.1;
        let rect = Rect::new(
            self.grid_area.min_x(),
            (self.grid_area.max_y() - h).round(),
            self.grid_area.size.width,
            h,
        );
        layer.quad(Quad::filled(rect, band.background));
        let shaped = text.shape_line(band.text, band.font);
        let baseline = centered_baseline(rect.min_y(), h, &text.metrics(band.font));
        draw_text(layer, &shaped, rect.min_x(), baseline, color);
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

    /// The scrollbar's content and offset (physical pixels, from the history's top): `None`
    /// when there's nothing to scroll (no history, or a full-screen program's screen).
    fn bar_metrics(&self) -> Option<(f32, f32)> {
        let g = &self.grid;
        // Asleep: no bar (nothing to scroll on a still screen).
        if self.frozen() || g.alt_screen || g.history == 0 || g.rows == 0 {
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

#[cfg(test)]
#[path = "../../tests/unit/view.rs"]
mod tests;
