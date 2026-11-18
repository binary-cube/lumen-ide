//! A project's session: the open editor tabs per group (with the active, pinned and locked ones),
//! how groups are laid out (the split tree), the current group, the bottom windows' views
//! that are reopened (the terminal's shells, with their names and pins) and the one shown,
//! the editor zoom, the project-only theme and the sizes the user dragged. Saved on every change and restored when the
//! project reopens. The use cases: what's captured ([`Session::capture`]), when it's saved
//! ([`SessionKeeper`]), what's reopened ([`Session::reopen`]).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// A saved editor tab.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedTab {
    /// Relative to the project root (absolute when outside it).
    pub path: PathBuf,
    pub pinned: bool,
    /// Its preview's Split mode: the share of the width its text has (`None`: never
    /// dragged, the default applies).
    pub split: Option<f32>,
    /// Locked (read-only) by the user.
    pub locked: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SavedGroup {
    pub tabs: Vec<SavedTab>,
    pub active: usize,
}

/// A view of a bottom window, saved to be made again (a terminal's shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedView {
    /// Its tab's title (as the user may have renamed it).
    pub title: String,
    /// Its viewer's virtual extension (`view-terminal`).
    pub extension: String,
    pub pinned: bool,
    /// The group (split) of its window it's in (0: the first, the only one unsplit).
    pub group: usize,
    /// The name its module opens it by, when the user renamed its tab (it finds it again).
    pub key: Option<String>,
    /// What the view needs to be made again, as it says ([`Viewer::memento`]: a file's
    /// history's path).
    pub memento: Option<String>,
}

/// A bottom window's saved views and the active one; split, how its groups are laid out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedToolWindow {
    pub id: String,
    pub views: Vec<SavedView>,
    /// The active view of the current group (its index among that group's views).
    pub active: usize,
    /// Its groups' split tree in its textual form (`R(0.5000 L0,0.5000 L1)`); empty: one
    /// group.
    pub split: String,
    /// The current group, and each group's active view (among its own views).
    pub group: usize,
    pub actives: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Session {
    pub groups: Vec<SavedGroup>,
    pub group: usize,
    /// The split tree, in its textual form (`L0`, `R(0.5000 L0,0.5000 L1)`).
    pub split: String,
    /// The bottom windows' reopened views, and the window shown there (its id).
    pub tool_windows: Vec<SavedToolWindow>,
    pub bottom: Option<String>,
    /// Editor text size (1 = 100%).
    pub zoom: f32,
    /// Theme chosen for this project only (otherwise the global one).
    pub theme: Option<String>,
    /// What the panels keep of themselves (their id, their own text: a splitter's place),
    /// sorted by id.
    pub panels: Vec<(String, String)>,
    /// The sizes the user dragged (the docks', the views' splitters: [`crate::LayoutSizes`]),
    /// sorted by key.
    pub sizes: Vec<(String, f32)>,
}

/// Where sessions are kept (port; the TOML adapter lives in `storage`).
pub trait SessionStore: std::fmt::Debug {
    /// The session saved for the project at `root`, if any (and readable).
    fn load(&self, root: &Path) -> Option<Session>;
    fn save(&self, root: &Path, session: &Session) -> std::io::Result<()>;
}

/// An editor group as it is now: its tabs in order (`None`: not a file on disk — a view,
/// a new file never saved), the active one.
#[derive(Debug, Clone, Default)]
pub struct OpenGroup<'a> {
    pub tabs: Vec<Option<OpenTab<'a>>>,
    pub active: usize,
}

/// A file's tab, as it is now.
#[derive(Debug, Clone, Copy)]
pub struct OpenTab<'a> {
    pub path: &'a Path,
    pub pinned: bool,
    /// Its preview's Split ratio, when the user dragged it.
    pub split: Option<f32>,
    /// Locked (read-only) by the user (not because its file is read-only on disk).
    pub locked: bool,
}

/// The workbench as it is now, what a session is captured from.
#[derive(Debug, Clone, Default)]
pub struct Current<'a> {
    pub groups: Vec<OpenGroup<'a>>,
    pub group: usize,
    pub split: String,
    pub tool_windows: Vec<SavedToolWindow>,
    pub bottom: Option<String>,
    pub zoom: f32,
    pub theme: Option<String>,
    pub panels: Vec<(String, String)>,
    pub sizes: Vec<(String, f32)>,
}

/// A tab to reopen: its file (absolute), pinned, the group's active one, its preview's
/// Split ratio, locked.
#[derive(Debug, Clone, PartialEq)]
pub struct Reopen {
    pub path: PathBuf,
    pub pinned: bool,
    pub active: bool,
    pub split: Option<f32>,
    pub locked: bool,
}

impl Session {
    /// The session of `now`, in the project at `root`: the files' tabs only (relative to
    /// the root when inside it), each group's active one among them. A group left without
    /// a file can't have its layout restored: then the tabs are kept, in a single group.
    pub fn capture(root: &Path, now: Current<'_>) -> Self {
        let groups: Vec<SavedGroup> = now
            .groups
            .iter()
            .map(|group| {
                let mut saved = SavedGroup::default();
                for (i, tab) in group.tabs.iter().enumerate() {
                    let Some(tab) = tab else { continue };
                    if i == group.active {
                        saved.active = saved.tabs.len();
                    }
                    let path = tab.path.strip_prefix(root).unwrap_or(tab.path).to_path_buf();
                    saved.tabs.push(SavedTab {
                        path,
                        pinned: tab.pinned,
                        split: tab.split,
                        locked: tab.locked,
                    });
                }
                saved
            })
            .collect();
        let (groups, group, split) = if groups.iter().any(|g| g.tabs.is_empty()) && groups.len() > 1
        {
            let tabs: Vec<SavedTab> = groups.into_iter().flat_map(|g| g.tabs).collect();
            (vec![SavedGroup { tabs, active: 0 }], 0, "L0".to_owned())
        } else {
            (groups, now.group, now.split)
        };
        Self {
            groups,
            group,
            split,
            tool_windows: now.tool_windows,
            bottom: now.bottom,
            zoom: now.zoom,
            theme: now.theme,
            panels: now.panels,
            sizes: now.sizes,
        }
    }

    /// The tabs to reopen, per group, in the project at `root`: the files that still
    /// exist (`exists`), the group's active one marked.
    pub fn reopen(&self, root: &Path, exists: &dyn Fn(&Path) -> bool) -> Vec<Vec<Reopen>> {
        let resolve = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { root.join(p) };
        self.groups
            .iter()
            .map(|group| {
                group
                    .tabs
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| (i, resolve(&tab.path), tab))
                    .filter(|(_, path, _)| exists(path))
                    .map(|(i, path, tab)| Reopen {
                        path,
                        pinned: tab.pinned,
                        active: i == group.active,
                        split: tab.split,
                        locked: tab.locked,
                    })
                    .collect()
            })
            .collect()
    }
}

/// When the session is saved: compared with the last one saved at most every half second
/// (not on every frame), written only if it changed, and not at all once frozen (the
/// window / project closing: its tabs closing mustn't be saved).
#[derive(Debug, Default)]
pub struct SessionKeeper {
    saved: Option<Session>,
    frozen: bool,
    checked: Option<Instant>,
}

impl SessionKeeper {
    /// How often it's compared at most.
    pub const EVERY: Duration = Duration::from_millis(500);

    /// Another project: nothing saved for it yet, not frozen.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Whether it's time to compare again (`now`): at most every [`Self::EVERY`].
    pub fn due(&mut self, now: Instant) -> bool {
        if self.checked.is_some_and(|t| now.duration_since(t) < Self::EVERY) {
            return false;
        }
        self.checked = Some(now);
        true
    }

    /// Saves `current` (in `store`, for `root`) if it changed since the last one; not once
    /// frozen. A failed write is returned (and tried again at the next change).
    pub fn save_if_changed(
        &mut self,
        store: &dyn SessionStore,
        root: &Path,
        current: Option<Session>,
    ) -> std::io::Result<()> {
        if self.frozen || current == self.saved {
            return Ok(());
        }
        let written = current.as_ref().map_or(Ok(()), |session| store.save(root, session));
        self.saved = current;
        written
    }

    /// The window / project is closing: no more saves.
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// The closing was cancelled (a tab opened again): saved again on changes.
    pub fn thaw(&mut self) {
        self.frozen = false;
    }

    /// What was just reopened is the saved session (not written again right away).
    pub fn remember(&mut self, session: Option<Session>) {
        self.saved = session;
    }
}

#[cfg(test)]
#[path = "../tests/unit/session.rs"]
mod tests;
