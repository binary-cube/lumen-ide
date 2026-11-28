//! What the terminal follows from its settings (Settings → Terminal), handed over by
//! [`crate::settings`] at startup and after each change: the shell new tabs start, the
//! colors, the cursor, the text, the history. Read by every shell's view.

use std::sync::{Arc, RwLock};

/// The terminal's colors: the theme's ANSI palette, the classic xterm one, or the theme's
/// with more contrast — the terminal's only, never the rest of the app's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Colors {
    #[default]
    Theme,
    Classic,
    HighContrast,
}

/// The cursor's shape, unless the program asks for its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cursor {
    #[default]
    Block,
    Bar,
    Underline,
}

/// When a tab is named after what its program says it is (OSC 0 / 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabTitle {
    Never,
    /// While a command runs; its own name at the shell's prompt.
    #[default]
    WhileRunning,
    Always,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalConfig {
    /// The shell new tabs start: one found on the system (by its id, `""`: the default) or
    /// `custom`'s command.
    pub shell: String,
    /// "Other…": the program and its arguments (`/usr/local/bin/nu -l`, a quoted path with
    /// spaces first).
    pub custom: String,
    /// The prompt in the theme's colors (user@host, the directory, the branch), given to
    /// the shell at start; off: the shell's own.
    pub themed_prompt: bool,
    /// Each project's shells keep their own command history (off: the user's usual one).
    pub project_history: bool,
    pub colors: Colors,
    pub cursor: Cursor,
    pub blink: bool,
    /// The font size (logical points; `None`: the editor's).
    pub font_size: Option<f32>,
    /// The line height, a multiple of the font size.
    pub line_height: f32,
    /// Bold text in the bright colors (as older terminals).
    pub bold_bright: bool,
    /// The lines kept above the screen.
    pub scrollback: usize,
    /// What a program's request to erase the history gets.
    pub history_clear: terminal_session::history::HistoryClear,
    pub tab_title: TabTitle,
    /// A tab closes when its shell ends.
    pub close_on_exit: bool,
    /// Programs that ask for the mouse get its wheel.
    pub mouse_reporting: bool,
    /// A selection made with the mouse is copied at once.
    pub copy_on_select: bool,
    /// Links and paths underlined under the pointer, opened with a click.
    pub highlight_links: bool,
    /// A program another module runs (an agent's session) stopped after this long unseen
    /// and doing nothing, resumed once woken (`None`: never; see `crate::sleep`).
    pub sleep_after: Option<std::time::Duration>,
}

/// The id of "Other…" among the shells.
pub const CUSTOM: &str = "custom";

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: String::new(),
            custom: String::new(),
            themed_prompt: false,
            project_history: true,
            colors: Colors::Theme,
            cursor: Cursor::Block,
            blink: true,
            font_size: None,
            line_height: 1.35,
            bold_bright: false,
            scrollback: 5_000,
            history_clear: Default::default(),
            tab_title: TabTitle::WhileRunning,
            close_on_exit: true,
            mouse_reporting: true,
            copy_on_select: false,
            highlight_links: true,
            sleep_after: None,
        }
    }
}

fn current_slot() -> &'static RwLock<Arc<TerminalConfig>> {
    static CURRENT: std::sync::OnceLock<RwLock<Arc<TerminalConfig>>> = std::sync::OnceLock::new();
    CURRENT.get_or_init(RwLock::default)
}

/// The settings, as saved now.
pub fn current() -> Arc<TerminalConfig> {
    current_slot().read().map(|c| Arc::clone(&c)).unwrap_or_default()
}

/// The settings changed (or were read at startup).
pub fn configure(config: TerminalConfig) {
    // The sessions' idle time told to the sleeping (turned on: they're looked at again).
    ui_components::sleep::configure(&crate::sleep::SESSIONS, config.sleep_after);
    if let Ok(mut current) = current_slot().write() {
        *current = Arc::new(config);
    }
}

/// How a new tab starts its shell in `cwd`: the one chosen, or "Other…"'s command; the
/// history of the project at `project` its own (when chosen); with the themed prompt when
/// chosen (a shell it knows).
pub fn profile(
    config: &TerminalConfig,
    cwd: Option<std::path::PathBuf>,
    project: Option<&std::path::Path>,
) -> terminal_session::ShellProfile {
    let profile = match split_command(&config.custom).filter(|_| config.shell == CUSTOM) {
        Some((program, args)) => terminal_session::ShellProfile {
            name: i18n::t("terminal.local"),
            program: Some(program),
            args,
            cwd,
            env: Default::default(),
        },
        None => pty::profile(&config.shell, cwd),
    };
    let profile = match project.filter(|_| config.project_history) {
        Some(root) => pty::project_history(&profile, root),
        None => profile,
    };
    if config.themed_prompt { pty::themed_prompt(&profile) } else { profile }
}

/// How a program a module asked for starts (an agent's session) in `cwd` (its own folder
/// first): run directly, never through a shell's quoting, its environment added to ours.
pub fn launched(
    launch: &agents::Launch,
    cwd: Option<std::path::PathBuf>,
) -> terminal_session::ShellProfile {
    terminal_session::ShellProfile {
        name: launch.program.clone(),
        program: Some(launch.program.clone()),
        args: launch.args.clone(),
        cwd: launch.cwd.clone().or(cwd),
        env: launch.env.iter().cloned().collect(),
    }
}

/// A command as written: the program (quoted if its path has spaces) and its arguments.
/// `None` when empty.
pub fn split_command(command: &str) -> Option<(String, Vec<String>)> {
    let command = command.trim();
    let (program, rest) = match command.strip_prefix('"') {
        Some(quoted) => {
            let end = quoted.find('"')?;
            (&quoted[..end], &quoted[end + 1..])
        }
        None => command.split_once(char::is_whitespace).unwrap_or((command, "")),
    };
    if program.is_empty() {
        return None;
    }
    Some((program.to_owned(), rest.split_whitespace().map(str::to_owned).collect()))
}

#[cfg(test)]
#[path = "../tests/unit/config.rs"]
mod tests;
