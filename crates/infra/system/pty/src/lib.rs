//! Terminal sessions: the system PTY (openpty on Unix, ConPTY on Windows) and
//! the VT emulator, both from `alacritty_terminal`. Implements the domain's
//! [`TerminalSession`] port.
//!
//! The shell gets what a terminal app gives it: colors on (`CLICOLOR`, `LS_COLORS` on the
//! ANSI colors, so the theme's), a UTF-8 locale, its name — each only where the user's
//! environment doesn't set it already ([`shell_env`]). The shells the system has are looked
//! for once ([`shells`]).
//!
//! Reading from the PTY runs on its own thread (the alacritty event loop). On
//! new content a frame is requested via `waker`, at most once until the next
//! grid read (`take_dirty`) and at most once per [`wakes::INTERVAL`] ([`wakes`]), so a
//! `cat` on a large file doesn't flood the UI.
//!
//! A program's request to erase the history (`ESC[3J`) is honored or not as the user chose
//! and by who has the terminal ([`filtered`], [`foreground`]).
//!
//! The history of a terminal not drawn for a while is parked in a compact form and put back
//! before anything reads it ([`parking`]).

mod filtered;
mod foreground;
mod parked;
mod parking;
mod wakes;

use std::borrow::Cow;
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize as GridSize;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape as VtCursor, NamedColor};
use terminal_session::shells::{Platform, Shell, ShellProbe};
use terminal_session::{
    CursorShape, GridSnapshot, InputModes, MouseReport, Rgb8, ShellProfile, TermCell, TermColor,
    TermOptions, TermPalette, TermSize, TerminalSession, flags,
};

pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// State shared between the PTY thread (via the listener) and the session.
struct Shared {
    wakes: wakes::Wakes,
    dirty: AtomicBool,
    title: Mutex<Option<String>>,
    exit: Mutex<Option<i32>>,
    /// The channel to the PTY, for the emulator's replies (e.g. the cursor position).
    sender: OnceLock<EventLoopSender>,
    /// What programs' requests to erase the history get (`HistoryClear::to_u8`), read by
    /// the PTY's reader.
    history_clear: Arc<AtomicU8>,
    /// The colors programs are answered (see [`Colors`]).
    colors: Mutex<Colors>,
}

/// What a program asking a color (OSC 10 / 11 / 12 / 4) is answered: the colors redefined
/// by programs (as last read from the screen), else the view's own (`None` until it's
/// drawn once: nothing answered then). Kept here so the answer goes back while the
/// question is read, before what follows it: a program that asks a sentinel next (the
/// cursor's place, the device attributes) would take a later answer for no answer.
#[derive(Debug, Default)]
struct Colors {
    palette: Option<TermPalette>,
    redefined: std::collections::HashMap<u16, Rgb8>,
}

impl Colors {
    fn answer(&self, index: usize) -> Option<Rgb8> {
        let redefined = u16::try_from(index).ok().and_then(|i| self.redefined.get(&i));
        redefined.copied().or_else(|| self.palette?.color(index))
    }
}

impl Shared {
    fn mark_dirty(&self) {
        if !self.dirty.swap(true, Ordering::AcqRel) {
            self.wakes.wake();
        }
    }

    fn colors(&self) -> std::sync::MutexGuard<'_, Colors> {
        self.colors.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The colors programs redefined, as the screen was just read (copied only when they
    /// changed).
    fn redefined(&self, now: &std::collections::HashMap<u16, Rgb8>) {
        let mut colors = self.colors();
        if colors.redefined != *now {
            colors.redefined.clone_from(now);
        }
    }
}

#[derive(Clone)]
struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        match event {
            Event::Wakeup => shared.mark_dirty(),
            Event::Title(title) => {
                if let Ok(mut t) = shared.title.lock() {
                    *t = Some(title);
                }
                shared.mark_dirty();
            }
            Event::ResetTitle => {
                if let Ok(mut t) = shared.title.lock() {
                    *t = None;
                }
            }
            Event::PtyWrite(text) => {
                if let Some(sender) = shared.sender.get() {
                    let _ = sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
                }
            }
            // A program asks a color: answered at once, in the order of its questions.
            Event::ColorRequest(index, reply) => {
                let answer = shared.colors().answer(index);
                if let (Some((r, g, b)), Some(sender)) = (answer, shared.sender.get()) {
                    let text = reply(alacritty_terminal::vte::ansi::Rgb { r, g, b });
                    let _ = sender.send(Msg::Input(Cow::Owned(text.into_bytes())));
                }
            }
            Event::ChildExit(status) => {
                if let Ok(mut e) = shared.exit.lock() {
                    *e = Some(status.code().unwrap_or(-1));
                }
                shared.mark_dirty();
            }
            Event::Exit => {
                if let Ok(mut e) = shared.exit.lock() {
                    e.get_or_insert(0);
                }
                shared.mark_dirty();
            }
            _ => {}
        }
    }
}

/// A started terminal session.
pub struct PtySession {
    term: Arc<FairMutex<Term<Listener>>>,
    sender: EventLoopSender,
    shared: Arc<Shared>,
    /// The shell's process id.
    pid: Option<u32>,
    /// Who has the terminal (shared with the PTY's reader).
    probe: Arc<Mutex<foreground::Probe>>,
    /// The program redrawing over its whole screen, if one does (see `filtered::Redrawing`).
    redrawing: filtered::Redrawing,
    /// The sizes the PTY is told, a drag's last one only (see [`ResizeDebounce`]).
    resizes: OnceLock<std::sync::mpsc::Sender<WindowSize>>,
    /// What its grid held last time it was free to be measured (the reader busy: told
    /// again, not waited for).
    memory: AtomicU64,
    /// Its history, parked while it isn't drawn.
    parking: Arc<parking::Parking<Listener>>,
}

/// How long the panel's size must stay still before the program is told it (`TIOCSWINSZ`,
/// its `SIGWINCH`): a drag of the panel's edge is one resize for it, not one per pixel —
/// each one makes a program like Claude Code redraw, and redraws racing the next sizes (its
/// cursor-relative erases computed for a width already gone) left its top rows stale.
const RESIZE_QUIET: Duration = Duration::from_millis(80);

/// What a run of sizes asked within [`RESIZE_QUIET`] of each other comes to: the last one,
/// once they stop (pure: the time given).
#[derive(Debug, Default)]
pub(crate) struct ResizeDebounce {
    pending: Option<(WindowSize, Instant)>,
}

impl ResizeDebounce {
    /// A size asked at `now`.
    pub(crate) fn ask(&mut self, size: WindowSize, now: Instant) {
        self.pending = Some((size, now));
    }

    /// When to look again (`None`: nothing waits).
    pub(crate) fn due(&self) -> Option<Instant> {
        self.pending.map(|(_, at)| at + RESIZE_QUIET)
    }

    /// The size to tell now, if the sizes have stopped.
    pub(crate) fn take(&mut self, now: Instant) -> Option<WindowSize> {
        match self.pending {
            Some((size, at)) if now >= at + RESIZE_QUIET => {
                self.pending = None;
                Some(size)
            }
            _ => None,
        }
    }
}

impl std::fmt::Debug for PtySession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySession").finish_non_exhaustive()
    }
}

/// The rows a grid keeps made ahead of its history (alacritty's cache).
const ROWS_AHEAD: usize = 1000;

fn window_size(size: TermSize) -> WindowSize {
    WindowSize {
        num_lines: size.rows.max(1),
        num_cols: size.cols.max(1),
        cell_width: size.cell_width.max(1),
        cell_height: size.cell_height.max(1),
    }
}

fn grid_size(size: TermSize) -> GridSize {
    GridSize::new(size.cols.max(1) as usize, size.rows.max(1) as usize)
}

/// GNU `ls`'s colors (`LS_COLORS`) on the ANSI colors — the theme's: folders blue and bold,
/// links cyan, programs green, sockets / pipes / devices as usual.
const LS_COLORS: &str =
    "di=1;34:ln=36:so=35:pi=33:ex=32:bd=34;46:cd=34;43:su=30;41:sg=30;46:tw=30;42:ow=34;42";
/// BSD `ls`'s (macOS), the same: folders bold blue, links cyan, programs green.
const LSCOLORS: &str = "ExGxfxdxCxegedabagacad";

/// The shell's environment: the profile's, then what a terminal app gives it — the terminal
/// (`TERM`, `COLORTERM`, its name and version), colors on (`CLICOLOR`, `LS_COLORS`,
/// `LSCOLORS`), a UTF-8 locale (Unix) — each only if neither the profile nor the user's
/// environment (`set`: is that variable set in the app's) has it already.
pub fn shell_env(
    profile: &std::collections::HashMap<String, String>,
    set: &dyn Fn(&str) -> bool,
) -> std::collections::HashMap<String, String> {
    let mut env = profile.clone();
    let mut give = |key: &str, value: &str| {
        if !env.contains_key(key) && !set(key) {
            env.insert(key.into(), value.into());
        }
    };
    give("COLORTERM", "truecolor");
    give("TERM_PROGRAM_VERSION", kernel::brand::version());
    give("CLICOLOR", "1");
    give("LS_COLORS", LS_COLORS);
    give("LSCOLORS", LSCOLORS);
    // Started from the Finder / a desktop launcher, the app may have no locale: the shell
    // would show accented letters as `?`.
    if cfg!(unix) && !["LANG", "LC_ALL", "LC_CTYPE"].iter().any(|k| env.contains_key(*k) || set(k))
    {
        env.insert("LANG".into(), "en_US.UTF-8".into());
    }
    // The terminal's own: always ours (the app's environment may come from another one).
    env.entry("TERM".into()).or_insert_with(|| "xterm-256color".into());
    env.entry("TERM_PROGRAM".into()).or_insert_with(|| kernel::brand::NAME.into());
    env
}

/// The system, as the shell detection asks it.
struct SystemProbe;

impl ShellProbe for SystemProbe {
    fn platform(&self) -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else {
            Platform::Linux
        }
    }

    fn login_shell(&self) -> Option<String> {
        std::env::var("SHELL").ok().filter(|s| !s.is_empty())
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }

    fn which(&self, program: &str) -> Option<String> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|dir| dir.join(program))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().into_owned())
    }
}

/// The shells the system has, the default first; looked for once (a few file checks).
pub fn shells() -> &'static [Shell] {
    static SHELLS: OnceLock<Vec<Shell>> = OnceLock::new();
    SHELLS.get_or_init(|| terminal_session::shells::available(&SystemProbe))
}

/// How a tab starts the shell chosen (by its id; `""` or one gone: the default), in `cwd`.
/// None found: the system's default, as the PTY picks it.
pub fn profile(shell: &str, cwd: Option<std::path::PathBuf>) -> ShellProfile {
    match terminal_session::shells::chosen(shells(), shell) {
        Some(shell) => shell.profile(cwd),
        None => ShellProfile { name: "Local".into(), cwd, ..Default::default() },
    }
}

/// `profile` with the prompt in the theme's colors (`terminal_session::prompt`), its startup
/// files written in a folder of the app's in the user's own cache (never a temp folder other
/// users share: the shell runs what's there); unchanged for a shell it doesn't know, or when
/// they can't be written.
pub fn themed_prompt(profile: &ShellProfile) -> ShellProfile {
    let Some(cache) = files::places::cache_dir() else { return profile.clone() };
    let dir = cache.join("terminal-prompt");
    // As the shell reads it (Git Bash too: forward slashes).
    let shown = dir.to_string_lossy().replace('\\', "/");
    let user_zdotdir = std::env::var("ZDOTDIR").ok().filter(|d| !d.is_empty());
    let Some(themed) = terminal_session::prompt::themed(profile, &shown, user_zdotdir.as_deref())
    else {
        return profile.clone();
    };
    let written = std::fs::create_dir_all(&dir).and_then(|()| {
        themed.files.iter().try_for_each(|(name, text)| std::fs::write(dir.join(name), text))
    });
    match written {
        Ok(()) => themed.profile,
        Err(e) => {
            tracing::warn!(error = %e, "the prompt's files can't be written");
            profile.clone()
        }
    }
}

/// `profile` keeping the commands typed in the project at `root` apart (its own history file,
/// in the app's data folder: `terminal-history/<the project's key>/`, made only for the
/// user); unchanged for a shell that keeps none of its own, or when it can't be made.
pub fn project_history(profile: &ShellProfile, root: &std::path::Path) -> ShellProfile {
    use terminal_session::shell_history as history;
    let Some(program) = profile.program.as_deref() else { return profile.clone() };
    let kind = history::kind(program);
    let key = history::project_key(&root.to_string_lossy());
    let file = match history::file_name(kind) {
        Some(name) => {
            let Some(data) = files::places::data_dir() else { return profile.clone() };
            let dir = data.join("terminal-history").join(&key);
            let file = dir.join(name);
            if let Err(e) = private_file(&dir, &file) {
                tracing::warn!(error = %e, "the project's shell history can't be made");
                return profile.clone();
            }
            file.to_string_lossy().into_owned()
        }
        None => String::new(),
    };
    let wslenv = std::env::var("WSLENV").ok();
    let Some(moved) = history::per_project(profile, &file, &key, wslenv.as_deref()) else {
        return profile.clone();
    };
    // zsh: named last, after the system's files and the user's (macOS's `/etc/zshrc` sets
    // its own `HISTFILE`): through startup files of the app's.
    if kind != history::Kind::Zsh {
        return moved;
    }
    let Some(cache) = files::places::cache_dir() else { return moved };
    let dir = cache.join("terminal-history-zsh");
    let user_zdotdir = std::env::var("ZDOTDIR").ok().filter(|d| !d.is_empty());
    let shown = dir.to_string_lossy().into_owned();
    let Some(wrapped) =
        terminal_session::prompt::zsh_history_only(&moved, &shown, user_zdotdir.as_deref())
    else {
        return moved;
    };
    let written = std::fs::create_dir_all(&dir).and_then(|()| {
        wrapped.files.iter().try_for_each(|(name, text)| std::fs::write(dir.join(name), text))
    });
    match written {
        Ok(()) => wrapped.profile,
        Err(e) => {
            tracing::warn!(error = %e, "zsh's startup files for the project's history can't be written");
            moved
        }
    }
}

/// `file` in `dir`, both made if missing, for this user only (as `~/.bash_history`).
fn private_file(dir: &std::path::Path, file: &std::path::Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    options.open(file).map(drop)
}

/// Starts the shell from `profile` in a PTY of size `size`.
pub fn spawn(profile: &ShellProfile, size: TermSize, waker: Waker) -> io::Result<PtySession> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    // Instead of `tty::setup_env()` (which changes the environment of the whole process).
    let env = shell_env(&profile.env, &|key| std::env::var_os(key).is_some());
    let options = tty::Options {
        shell: profile.program.clone().map(|p| tty::Shell::new(p, profile.args.clone())),
        working_directory: profile.cwd.clone(),
        drain_on_exit: true,
        env,
        #[cfg(target_os = "windows")]
        escape_args: true,
    };
    let pty = tty::new(&options, window_size(size), NEXT_ID.fetch_add(1, Ordering::Relaxed))?;
    #[cfg(unix)]
    let pid = Some(pty.child().id());
    #[cfg(windows)]
    let pid = pty.child_watcher().pid().map(std::num::NonZeroU32::get);
    #[cfg(unix)]
    let probe = foreground::probe(std::os::fd::AsRawFd::as_raw_fd(pty.file()), pid);
    #[cfg(windows)]
    let probe = foreground::probe(pid);

    let history_clear = Arc::new(AtomicU8::new(TermOptions::default().history_clear.to_u8()));
    let probe = Arc::new(Mutex::new(probe));
    let redrawing = filtered::Redrawing::default();
    let erased = Arc::new(AtomicU64::new(0));
    let pty = filtered::Filtered::new(
        pty,
        Arc::clone(&history_clear),
        Arc::clone(&probe),
        Arc::clone(&redrawing),
        Arc::clone(&erased),
    );
    let shared = Arc::new(Shared {
        wakes: wakes::Wakes::new(waker),
        dirty: AtomicBool::new(true),
        title: Mutex::new(None),
        exit: Mutex::new(None),
        sender: OnceLock::new(),
        history_clear,
        colors: Mutex::default(),
    });
    let listener = Listener(Arc::clone(&shared));
    let term = Arc::new(FairMutex::new(Term::new(
        term_config(TermOptions::default()),
        &grid_size(size),
        listener.clone(),
    )));
    let woken = Arc::downgrade(&shared);
    let parking = Arc::new(parking::Parking::new(
        &term,
        erased,
        TermOptions::default().scrollback,
        Box::new(move || {
            if let Some(shared) = woken.upgrade() {
                shared.mark_dirty();
            }
        }),
    ));
    parking::watch(&parking);
    let event_loop = EventLoop::new(Arc::clone(&term), listener, pty, true, false)?;
    let sender = event_loop.channel();
    let _ = shared.sender.set(sender.clone());
    event_loop.spawn();
    tracing::info!(cwd = ?profile.cwd, "terminal started");
    Ok(PtySession {
        term,
        sender,
        shared,
        pid,
        probe,
        redrawing,
        resizes: OnceLock::new(),
        memory: AtomicU64::new(0),
        parking,
    })
}

impl PtySession {
    /// The program that redraws over its whole screen is still the one in the foreground
    /// (the same start, within a second): it draws its screen again after a resize.
    fn redrawing_now(&self) -> bool {
        let Some(started) = *self.redrawing.lock().unwrap_or_else(PoisonError::into_inner) else {
            return false;
        };
        let foreground = self.probe.lock().unwrap_or_else(PoisonError::into_inner).foreground();
        let same = match foreground {
            terminal_session::history::Foreground::Program(age) => {
                Instant::now().checked_sub(age).is_some_and(|now| {
                    let (a, b) = if now > started { (now, started) } else { (started, now) };
                    a.duration_since(b) < Duration::from_secs(1)
                })
            }
            _ => false,
        };
        if !same {
            *self.redrawing.lock().unwrap_or_else(PoisonError::into_inner) = None;
        }
        same
    }
}

/// `term` resized to `size`. With a program redrawing over its whole screen (`redrawn`: it
/// draws it again after the resize), the history stays as it was: its screen emptied first
/// (nothing of it pushed up into the history by a shrink or a reflow), and what a taller
/// screen pulled down from the history put back (its redraws erase the screen without
/// pushing it: those lines would be lost). Otherwise alacritty's resize as it is.
pub(crate) fn resize_keeping_history<L: alacritty_terminal::event::EventListener>(
    term: &mut Term<L>,
    size: GridSize,
    redrawn: bool,
) {
    use alacritty_terminal::grid::Dimensions as _;
    use alacritty_terminal::index::{Column, Line, Point};
    if !redrawn {
        term.resize(size);
        return;
    }
    let grid = term.grid_mut();
    let template = grid.cursor.template.clone();
    for line in 0..grid.screen_lines() {
        grid[Line(line as i32)].reset(&template);
    }
    grid.cursor.point = Point::new(Line(0), Column(0));
    grid.saved_cursor.point = Point::new(Line(0), Column(0));
    let before = grid.history_size();
    term.resize(size);
    let grid = term.grid_mut();
    let pulled = before.saturating_sub(grid.history_size());
    if pulled > 0 {
        let lines = grid.screen_lines() as i32;
        grid.scroll_up(&(Line(0)..Line(lines)), pulled);
        grid.cursor.point.line = Line(0);
        grid.saved_cursor.point.line = Line(0);
    }
}

/// The thread telling the PTY its size once a run of resizes stops (one per session that is
/// resized, asleep between; it ends with the session: its sender dropped).
#[allow(clippy::disallowed_methods)] // A session's own, for its lifetime.
fn resize_thread(sender: EventLoopSender) -> std::sync::mpsc::Sender<WindowSize> {
    let (tx, rx) = std::sync::mpsc::channel::<WindowSize>();
    let spawned = std::thread::Builder::new().name("pty-resize".into()).spawn(move || {
        let mut debounce = ResizeDebounce::default();
        loop {
            let asked = match debounce.due() {
                Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                None => rx.recv().map_err(|_| std::sync::mpsc::RecvTimeoutError::Disconnected),
            };
            match asked {
                Ok(size) => debounce.ask(size, Instant::now()),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    // The last size still told (the session going: harmless).
                    if let Some((size, _)) = debounce.pending.take() {
                        let _ = sender.send(Msg::Resize(size));
                    }
                    return;
                }
            }
            if let Some(size) = debounce.take(Instant::now()) {
                let _ = sender.send(Msg::Resize(size));
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "can't start the terminal's resize thread");
    }
    tx
}

impl Drop for PtySession {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn color(c: Color) -> (TermColor, u8) {
    match c {
        Color::Spec(rgb) => (TermColor::Rgb(rgb.r, rgb.g, rgb.b), 0),
        Color::Indexed(i) => (TermColor::Indexed(i), 0),
        Color::Named(named) => {
            let index = named as usize;
            match index {
                0..=15 => (TermColor::Indexed(index as u8), 0),
                _ => match named {
                    NamedColor::DimBlack => (TermColor::Indexed(0), flags::DIM),
                    NamedColor::DimRed => (TermColor::Indexed(1), flags::DIM),
                    NamedColor::DimGreen => (TermColor::Indexed(2), flags::DIM),
                    NamedColor::DimYellow => (TermColor::Indexed(3), flags::DIM),
                    NamedColor::DimBlue => (TermColor::Indexed(4), flags::DIM),
                    NamedColor::DimMagenta => (TermColor::Indexed(5), flags::DIM),
                    NamedColor::DimCyan => (TermColor::Indexed(6), flags::DIM),
                    NamedColor::DimWhite => (TermColor::Indexed(7), flags::DIM),
                    NamedColor::DimForeground => (TermColor::Default, flags::DIM),
                    _ => (TermColor::Default, 0),
                },
            }
        }
    }
}

/// A session's [`terminal_session::SessionWatch`]: what it shares with the PTY's reader, held
/// weakly (gone with the session).
struct PtyWatch {
    probe: std::sync::Weak<Mutex<foreground::Probe>>,
    shared: std::sync::Weak<Shared>,
}

impl terminal_session::SessionWatch for PtyWatch {
    fn foreground(&self) -> terminal_session::history::Foreground {
        self.probe.upgrade().map_or(terminal_session::history::Foreground::Unknown, |p| {
            p.lock().unwrap_or_else(PoisonError::into_inner).foreground()
        })
    }

    fn program(&self) -> Option<String> {
        self.probe.upgrade()?.lock().unwrap_or_else(PoisonError::into_inner).program()
    }

    fn title(&self) -> Option<String> {
        self.shared.upgrade()?.title.lock().ok().and_then(|t| t.clone())
    }
}

impl TerminalSession for PtySession {
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn watch(&self) -> Option<Arc<dyn terminal_session::SessionWatch>> {
        Some(Arc::new(PtyWatch {
            probe: Arc::downgrade(&self.probe),
            shared: Arc::downgrade(&self.shared),
        }))
    }

    /// Asked from the UI thread (the Task Manager): never waits for the reader, never puts
    /// a parked history back (its compact form counted as it is).
    fn memory(&self) -> u64 {
        use alacritty_terminal::grid::Dimensions as _;
        let Some(term) = self.term.try_lock_unfair() else {
            return self.memory.load(Ordering::Relaxed);
        };
        let grid = term.grid();
        // The rows held, with the ones the grid keeps made ahead (up to 1000) and each row's
        // own vector (measured: the cells alone came ~10–20% short).
        let rows = grid.total_lines() + ROWS_AHEAD.min(grid.total_lines());
        let row = grid.columns() * std::mem::size_of::<alacritty_terminal::term::cell::Cell>()
            + std::mem::size_of::<Vec<alacritty_terminal::term::cell::Cell>>()
            + 16;
        let bytes = (rows * row + self.parking.bytes()) as u64;
        self.memory.store(bytes, Ordering::Relaxed);
        bytes
    }

    fn set_options(&mut self, options: TermOptions) {
        self.shared.history_clear.store(options.history_clear.to_u8(), Ordering::Relaxed);
        self.term.lock().set_options(term_config(options));
        self.parking.set_limit(options.scrollback);
    }

    fn write(&self, bytes: &[u8]) {
        if !bytes.is_empty() {
            let _ = self.sender.send(Msg::Input(Cow::Owned(bytes.to_vec())));
        }
    }

    fn resize(&mut self, size: TermSize) {
        {
            let term = self.term.lock();
            if term.columns() == size.cols as usize && term.screen_lines() == size.rows as usize {
                return;
            }
        }
        // Reflowed with the rest of the history.
        self.parking.restore(true);
        // Asked before the terminal is locked (the reader asks it with the terminal held).
        let redrawn = self.redrawing_now();
        let mut term = self.term.lock();
        resize_keeping_history(&mut term, grid_size(size), redrawn);
        drop(term);
        // The grid follows at once (what's shown); the program is told the size once the
        // drag stops.
        let resizes = self.resizes.get_or_init(|| resize_thread(self.sender.clone()));
        if resizes.send(window_size(size)).is_err() {
            // No thread to wait in: told at once.
            let _ = self.sender.send(Msg::Resize(window_size(size)));
        }
    }

    fn snapshot(&self, out: &mut GridSnapshot) {
        self.parking.shown(true);
        let term = self.term.lock();
        if self.parking.must_restore() {
            // Parked just now, scrolled back: what's shown is in the rows parked.
            drop(term);
            self.parking.restore(true);
            self.snapshot_parked(&self.term.lock(), out);
        } else {
            self.snapshot_parked(&term, out);
        }
        self.shared.redefined(&out.palette_overrides);
    }

    /// The reader holds the terminal while it parses (and, for a held `ESC[2J`, while it
    /// waits a moment for its `ESC[3J`: `filtered::HOLD`): a frame doesn't wait for it.
    fn try_snapshot(&self, out: &mut GridSnapshot) -> bool {
        if !self.parking.shown(false) {
            return false;
        }
        let Some(mut term) = self.term.try_lock_unfair() else { return false };
        if self.parking.must_restore() {
            drop(term);
            if !self.parking.restore(false) {
                return false;
            }
            let Some(again) = self.term.try_lock_unfair() else { return false };
            term = again;
        }
        self.snapshot_parked(&term, out);
        drop(term);
        self.shared.redefined(&out.palette_overrides);
        true
    }

    fn set_palette(&self, palette: TermPalette) {
        self.shared.colors().palette = Some(palette);
    }

    fn try_modes(&self) -> Option<InputModes> {
        let term = self.term.try_lock_unfair()?;
        let mode = term.mode();
        Some(InputModes {
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        })
    }

    fn try_scroll(&mut self, lines: i32) -> bool {
        if !self.parking.restore(false) {
            return false;
        }
        match self.term.try_lock_unfair() {
            Some(mut term) => {
                term.scroll_display(Scroll::Delta(lines));
                true
            }
            None => false,
        }
    }

    fn scroll(&mut self, lines: i32) {
        self.parking.restore(true);
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    fn scroll_to_bottom(&mut self) {
        self.parking.restore(true);
        let mut term = self.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    fn clear_history(&mut self) {
        self.parking.forget();
        clear_history_of(&mut self.term.lock());
        self.shared.mark_dirty();
    }

    fn title(&self) -> Option<String> {
        self.shared.title.lock().ok().and_then(|t| t.clone())
    }

    fn exit_status(&self) -> Option<i32> {
        self.shared.exit.lock().ok().and_then(|e| *e)
    }

    fn take_dirty(&self) -> bool {
        self.shared.dirty.swap(false, Ordering::AcqRel)
    }
}

impl PtySession {
    /// `term` copied into `out`, the history's length with the rows parked (on its own
    /// screen, a full-screen program has none).
    fn snapshot_parked(&self, term: &Term<Listener>, out: &mut GridSnapshot) {
        snapshot_of(term, out);
        if !out.alt_screen {
            out.history = self.parking.history(out.history);
        }
    }
}

/// What `term` shows now, copied into `out` (its allocations reused): the visible cells, the
/// cursor, the modes that change what keys and the wheel send, the history's length.
/// One of alacritty's cells as the view's.
fn term_cell(cell: &alacritty_terminal::term::cell::Cell) -> TermCell {
    let (fg, fg_flags) = color(cell.fg);
    let (bg, _) = color(cell.bg);
    let mut f = fg_flags;
    let src = cell.flags;
    for (from, to) in [
        (Flags::BOLD, flags::BOLD),
        (Flags::ITALIC, flags::ITALIC),
        (Flags::INVERSE, flags::INVERSE),
        (Flags::DIM, flags::DIM),
        (Flags::STRIKEOUT, flags::STRIKE),
        (Flags::HIDDEN, flags::HIDDEN),
        (Flags::WIDE_CHAR, flags::WIDE),
    ] {
        if src.contains(from) {
            f |= to;
        }
    }
    if src.intersects(Flags::ALL_UNDERLINES) {
        f |= flags::UNDERLINE;
    }
    let spacer = src.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    TermCell { ch: if spacer { ' ' } else { cell.c }, fg, bg, flags: f }
}

fn snapshot_of<L: EventListener>(term: &Term<L>, out: &mut GridSnapshot) {
    let content = term.renderable_content();
    let (cols, rows) = (term.columns(), term.screen_lines());
    out.cols = cols as u16;
    out.rows = rows as u16;
    out.cells.clear();
    out.cells.resize(cols * rows, TermCell::default());
    out.display_offset = content.display_offset;
    out.app_cursor = content.mode.contains(TermMode::APP_CURSOR);
    out.bracketed_paste = content.mode.contains(TermMode::BRACKETED_PASTE);
    out.history = term.grid().history_size();
    out.alt_screen = content.mode.contains(TermMode::ALT_SCREEN);
    out.alternate_scroll = content.mode.contains(TermMode::ALTERNATE_SCROLL);
    out.mouse = if !content.mode.intersects(TermMode::MOUSE_MODE) {
        MouseReport::Off
    } else if content.mode.contains(TermMode::SGR_MOUSE) {
        MouseReport::Sgr
    } else {
        MouseReport::Normal
    };
    let offset = content.display_offset as i32;
    for indexed in content.display_iter {
        let row = indexed.point.line.0 + offset;
        let col = indexed.point.column.0;
        if row < 0 || row as usize >= rows || col >= cols {
            continue;
        }
        out.cells[row as usize * cols + col] = term_cell(indexed.cell);
    }
    // The history line just above the screen, when the view asked (scrolled by part of a
    // line): drawn in the gap.
    out.above.clear();
    let offset_lines = content.display_offset;
    if out.want_above && offset_lines < term.grid().history_size() {
        use alacritty_terminal::index::{Column, Line};
        let row = &term.grid()[Line(-(offset_lines as i32) - 1)];
        out.above.extend((0..cols).map(|c| term_cell(&row[Column(c)])));
    }
    let cursor_row = content.cursor.point.line.0 + offset;
    out.cursor = (cursor_row >= 0 && (cursor_row as usize) < rows)
        .then_some((cursor_row as u16, content.cursor.point.column.0 as u16));
    out.cursor_shape = match content.cursor.shape {
        VtCursor::Block | VtCursor::HollowBlock => CursorShape::Block,
        VtCursor::Beam => CursorShape::Beam,
        VtCursor::Underline => CursorShape::Underline,
        VtCursor::Hidden => CursorShape::Hidden,
    };
    out.palette_overrides.clear();
    for index in [256usize, 257, 258].into_iter().chain(0..256) {
        if let Some(rgb) = content.colors[index] {
            out.palette_overrides.insert(index as u16, (rgb.r, rgb.g, rgb.b));
        }
    }
}

/// Clear Terminal: the history emptied, and the screen above the cursor's line, that line
/// (the prompt) moved to the top — as other terminals do; a full-screen program's own
/// screen is left to it (it has no history).
fn clear_history_of<L: EventListener>(term: &mut Term<L>) {
    use alacritty_terminal::vte::ansi::{ClearMode, Handler};
    term.scroll_display(Scroll::Bottom);
    if !term.mode().contains(TermMode::ALT_SCREEN) {
        let line = term.grid().cursor.point.line.0;
        if line > 0 {
            // Above the cursor into the history, then the history gone.
            term.scroll_up(line as usize);
            term.goto_line(0);
        }
    }
    term.clear_screen(ClearMode::Saved);
}

/// The view that scrolls reads the grid again and repaints at once: no extra wake-up.
/// The emulator's settings, as the user chose: the history kept (10k lines are ~50 MB for
/// a busy terminal at typical widths, so bounded by the settings) and the cursor's shape
/// when the program doesn't ask for one.
fn term_config(options: TermOptions) -> Config {
    let shape = match options.cursor {
        CursorShape::Beam => VtCursor::Beam,
        CursorShape::Underline => VtCursor::Underline,
        CursorShape::Block | CursorShape::Hidden => VtCursor::Block,
    };
    Config {
        scrolling_history: options.scrollback,
        default_cursor_style: alacritty_terminal::vte::ansi::CursorStyle { shape, blinking: false },
        ..Config::default()
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
