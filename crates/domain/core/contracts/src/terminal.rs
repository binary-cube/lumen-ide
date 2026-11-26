//! Programs run in a terminal by modules that don't know the terminal (an agent's session).

use kernel::{DomainEvent, Request};

/// Where a program's terminal opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalPlace {
    /// A tab of the editor (an agent's session, read beside the code).
    Editor,
    /// A tab of the terminal's window at the bottom.
    Bottom,
}

/// Run `launch` in a terminal of its own, known by `key` (what it is: an agent's session, by
/// its agent's and its own ids — never a name read from somewhere), its tab called `title`
/// with `icon` (one the asking module contributes; `None`: the terminal's). Asked again with
/// the same key, the tab already open comes to the front instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunInTerminal {
    pub launch: agents::Launch,
    pub key: String,
    pub title: String,
    pub icon: Option<&'static str>,
    pub place: TerminalPlace,
}

impl Request for RunInTerminal {
    /// Whether its tab opened (or came to the front).
    type Answer = bool;
}

/// The terminal known by `key` ([`RunInTerminal::key`]) called `title` now (its session
/// renamed: by the user in it, or elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameTerminal {
    pub key: String,
    pub title: String,
}

impl Request for RenameTerminal {
    /// `false`: no such terminal (its tab closed).
    type Answer = bool;
}

/// Whether the program of the terminal known by `key` still runs (an agent's session: what
/// it keeps in memory it may write over its files while it does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalRunning {
    pub key: String,
}

impl Request for TerminalRunning {
    /// `false`: it ended, or there's no such terminal (its tab closed).
    type Answer = bool;
}

/// How the program of the terminal known by `key` is started again where it left off (its
/// terminal put to sleep: the program ended, its tab kept): whoever started it answers, its
/// own way (an agent's session resumed by its id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeLaunch {
    pub key: String,
    /// How long it must have done nothing for it to be stopped: a program whose own record
    /// changed sooner (a session's file written during a turn) is answered `None`.
    pub idle: std::time::Duration,
}

impl Request for ResumeLaunch {
    /// `None`: nobody knows how (or it's at work): it isn't put to sleep.
    type Answer = Option<agents::Launch>;
}

/// What the terminal known by `key` holds now, as last measured (read from memory only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalUse {
    pub key: String,
}

/// A terminal's program as last measured: its memory with everything it started (`None`:
/// not measured yet, or asleep), whether it runs now (started, not ended, not asleep),
/// whether it sleeps (stopped, resumed when woken), and whether it can be put to sleep by
/// hand now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalState {
    pub memory: Option<u64>,
    pub running: bool,
    pub asleep: bool,
    pub can_sleep: CanSleep,
}

/// The terminal known by `key` stands otherwise now (asleep, on its way to sleep or back,
/// able to sleep or not, its memory measured again), or is gone (`None`: its tab closed):
/// published on its project's bus (a terminal of no project: on every open project's), so
/// what shows it follows at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalStateChanged {
    pub key: String,
    pub state: Option<TerminalState>,
}

impl DomainEvent for TerminalStateChanged {}

/// Whether a terminal can be put to sleep by hand now ([`SleepTerminal`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CanSleep {
    /// Never: nobody knows how to resume it, it's asleep already, its program ended.
    #[default]
    No,
    /// Not now: something typed in it isn't sent yet (it would be lost). Its program writing
    /// (a spinner, a status line) doesn't stop the user: they chose.
    Typed,
    /// Not yet: its session just started, how it resumes isn't known yet (its owner learns
    /// it within a second or two).
    Starting,
    /// Not now: it's being put to sleep (the order given, not yet taken).
    Sleeping,
    /// Not now: it's waking up (asked to, or its program starting again).
    Waking,
    Yes,
}

/// Put the terminal known by `key` to sleep now (by hand: whatever the idle time, shown or
/// not): its program stopped, its tab kept, resumed when the user acts on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SleepTerminal {
    pub key: String,
}

impl Request for SleepTerminal {
    /// `false`: no such terminal, or it can't sleep now ([`CanSleep`]).
    type Answer = bool;
}

/// Wake the terminal known by `key` (asleep: its program started again the way it resumes),
/// its tab brought to the front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WakeTerminal {
    pub key: String,
}

impl Request for WakeTerminal {
    /// `false`: no such terminal, or it isn't asleep.
    type Answer = bool;
}

impl Request for TerminalUse {
    /// `None`: no such terminal (its tab closed).
    type Answer = Option<TerminalState>;
}

/// `text` as a tab's key: letters, digits, `-` and `_` kept, the rest `-`, at most 80
/// characters — never a path's separator, a drive or `..` (it names a tab, read from
/// anywhere).
pub fn tab_key(text: &str) -> String {
    let key: String = text
        .chars()
        .take(80)
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' })
        .collect();
    if key.is_empty() { "-".to_owned() } else { key }
}

#[cfg(test)]
#[path = "../tests/unit/terminal.rs"]
mod tests;
