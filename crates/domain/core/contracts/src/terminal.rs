//! Programs run in a terminal by modules that don't know the terminal (an agent's session).

use kernel::Request;

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
