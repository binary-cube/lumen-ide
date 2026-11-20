//! Tool windows: what a module (or the app itself) puts on the window's sides — a button
//! with its icon and tooltip, and what showing it does — registered once at `register`
//! (a [`ToolWindowProvider`]) and discovered by the app, which lays them out: one dock per
//! side, the same buttons, tooltips and headers for all. Registering costs only this
//! description: nothing is made before it's first shown.
//!
//! A tool window's right-click menu is a `MenuContribution` with the same id.

use std::fmt::Debug;

use crate::tool_panel::ToolPanel;

/// Where a tool window's button sits, and where it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dock {
    /// The top bar's right side (search, settings): a command, no window.
    Top,
    /// The left strip's top; shown beside the editor, on the left (the project).
    Left,
    /// The left strip's bottom; shown in the bottom area (the terminal, Git, Remote).
    Bottom,
    /// The right strip; shown beside the editor, on the right (Structure, Task Manager).
    Right,
}

/// What showing a tool window does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolContent {
    /// A panel, made the first time it's shown ([`ToolWindowProvider::create`]).
    Panel,
    /// Views the module opens in the window itself, as its tabs
    /// (`WorkbenchHost::show_in_panel`). With none open, the button runs `command` (Git: its
    /// log) — and shows only while that command is enabled (Git: in a repository). `new`:
    /// the "+" after the tabs runs it (the terminal: another shell).
    Views { command: &'static str, new: Option<&'static str> },
    /// No window: the button runs `command` (the canvas opens as a tab), lit while the
    /// active tab is a `tab` view (its extension).
    Command { command: &'static str, tab: Option<&'static str> },
}

/// A tool window, as registered: its button and what showing it does.
pub trait ToolWindowProvider: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    /// The icon's name (the app's set, or one the same module contributes).
    fn icon(&self) -> &'static str;
    /// Its name (translated): the button's tooltip and the window's title.
    fn title(&self) -> String;
    fn dock(&self) -> Dock;
    /// Its place among its side's buttons (smaller first).
    fn order(&self) -> u16 {
        100
    }
    /// Its button in its strip's bottom group (the right strip's, at the window's bottom
    /// right corner; the left strip's bottom group is [`Dock::Bottom`]'s). Where its window
    /// shows doesn't change.
    fn strip_end(&self) -> bool {
        false
    }
    /// The command showing / hiding it, if it has one (its shortcut in the tooltip).
    fn toggle_command(&self) -> Option<&'static str> {
        None
    }
    /// Its group among its strip's buttons (what it's about: `code`, `services`,
    /// `documents`): between neighbors of different groups the strip draws a short line.
    /// Empty (the default): no group of its own — no line beside another without one.
    fn group(&self) -> &'static str {
        ""
    }
    fn content(&self) -> ToolContent;
    /// The color its button is lit in while [`Self::active`] (a recording's red); `None`
    /// (the default): the accent, as every other.
    fn active_tone(&self) -> Option<crate::ds::Tone> {
        None
    }
    /// A command's button lit: what it turns on is on (a mode: Focus mode). Asked as the
    /// strip is drawn: cheap. A window's button is lit while its window shows, whatever
    /// this says.
    fn active(&self) -> bool {
        false
    }
    /// Its button is offered now (asked as the strip is drawn: cheap). A window whose
    /// partner may be missing (the agents' sessions, without any agent) says so here.
    fn available(&self) -> bool {
        true
    }
    /// The panel ([`ToolContent::Panel`]), made when it's first shown.
    fn create(&self) -> Option<Box<dyn ToolPanel>> {
        None
    }
}

/// A tool window that makes no panel of its own (its views or a command), described by
/// data alone.
#[derive(Debug, Clone, Copy)]
pub struct ToolWindow {
    pub id: &'static str,
    pub icon: &'static str,
    /// Its title's translation key.
    pub title_key: &'static str,
    pub dock: Dock,
    pub order: u16,
    /// See [`ToolWindowProvider::strip_end`].
    pub strip_end: bool,
    /// See [`ToolWindowProvider::group`].
    pub group: &'static str,
    pub toggle: Option<&'static str>,
    pub content: ToolContent,
}

impl ToolWindowProvider for ToolWindow {
    fn id(&self) -> &'static str {
        self.id
    }

    fn icon(&self) -> &'static str {
        self.icon
    }

    fn title(&self) -> String {
        i18n::t(self.title_key)
    }

    fn dock(&self) -> Dock {
        self.dock
    }

    fn order(&self) -> u16 {
        self.order
    }

    fn strip_end(&self) -> bool {
        self.strip_end
    }

    fn toggle_command(&self) -> Option<&'static str> {
        self.toggle
    }

    fn group(&self) -> &'static str {
        self.group
    }

    fn content(&self) -> ToolContent {
        self.content
    }
}

#[cfg(test)]
#[path = "../tests/unit/tool_window.rs"]
mod tests;
