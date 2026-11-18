//! The "Services" window's contribution point: what a module shows at the bottom — a
//! tree of what it has running (a data source › its consoles' sessions › their
//! results; later a terminal's sessions, run configurations, Docker) and, for the node
//! chosen, its tabs, each a [`Viewer`] (the Output log, a result's grid).
//!
//! A module contributes a [`ServiceProvider`]; the app registers every one here at startup
//! ([`register`], as the databases' drivers are), and the window (another module, which
//! never knows who contributes) reads them ([`providers`]). Nothing of a contributor runs
//! before the window is first shown: then each one is asked for the project's services
//! ([`ServiceProvider::open`]), kept by the window for that project only (another project
//! opened in the window: they're dropped, the new one's asked for).
//!
//! Identities are UUID v4 ([`ServiceId`]): a node or a tab is itself whatever it's called.

use std::fmt::Debug;
use std::sync::{Arc, PoisonError, RwLock};

use kernel::{EventBus, ProjectId};

use crate::viewer::{ViewMenuEntry, Viewer};

/// A node's or a tab's identity: a UUID v4, never a name or a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServiceId(pub u128);

impl ServiceId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(kernel::id::new_uuid())
    }
}

/// A view's link to another tab of its node in the Services window (an Output's line naming
/// a profile): `service-tab:` and the tab's id ([`ServiceId::link`]), as a
/// [`crate::ViewerAction::OpenLink`].
pub const TAB_LINK: &str = "service-tab:";

impl ServiceId {
    /// The link showing this tab ([`TAB_LINK`]).
    pub fn link(self) -> String {
        format!("{TAB_LINK}{:032x}", self.0)
    }
}

impl Default for ServiceId {
    fn default() -> Self {
        Self::new()
    }
}

/// What a node is doing, shown after its name (a spinner, an error's mark, an open
/// transaction's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ServiceState {
    #[default]
    Idle,
    /// Work going on (a statement running): a spinner.
    Running,
    /// Its last work failed.
    Error,
    /// A transaction is open (nothing is permanent before it's committed).
    TxOpen,
}

/// A row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceNode {
    pub id: ServiceId,
    /// Its parent (`None`: under its contributor's group).
    pub parent: Option<ServiceId>,
    /// Its icon's name (the app's set, or one its module contributes).
    pub icon: &'static str,
    pub name: String,
    /// Dimmed after the name (`543 ms`).
    pub detail: String,
    pub state: ServiceState,
    /// A tag color's name (`red`, `green`… as [`crate::tag_colors::named`] reads it): its
    /// icon in it, and a mark at the row's start (a data source's color).
    pub color: Option<String>,
}

impl ServiceNode {
    /// A node named `name`, at the top of its group, idle.
    pub fn new(id: ServiceId, icon: &'static str, name: impl Into<String>) -> Self {
        Self {
            id,
            parent: None,
            icon,
            name: name.into(),
            detail: String::new(),
            state: ServiceState::Idle,
            color: None,
        }
    }

    /// The same node under `parent`.
    pub fn under(mut self, parent: ServiceId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// The same node with `detail` after its name.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }

    pub fn with_state(mut self, state: ServiceState) -> Self {
        self.state = state;
        self
    }

    pub fn with_color(mut self, color: impl Into<String>) -> Self {
        self.color = Some(color.into());
        self
    }
}

/// A node's tab: its identity, title, icon, and whether it's pinned (a pinned tab stays
/// when the next run would replace it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTab {
    pub id: ServiceId,
    pub title: String,
    pub icon: &'static str,
    pub pinned: bool,
    /// It can be closed (× and its menu); the Output of a session can't.
    pub closable: bool,
}

impl ServiceTab {
    pub fn new(id: ServiceId, icon: &'static str, title: impl Into<String>) -> Self {
        Self { id, title: title.into(), icon, pinned: false, closable: true }
    }

    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// The same tab, never closed by the user.
    pub fn fixed(mut self) -> Self {
        self.closable = false;
        self
    }
}

/// What a contributor gets when the window asks for a project's services.
#[derive(Clone)]
pub struct ServicesContext {
    /// The project's identity (`None`: no project open).
    pub project: Option<ProjectId>,
    /// The project's events (what it follows: its sessions, its runs).
    pub events: EventBus,
    /// Wakes the app from a background thread: the window then polls its services.
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl Debug for ServicesContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServicesContext").field("project", &self.project).finish_non_exhaustive()
    }
}

/// A module's contribution to the "Services" window.
pub trait ServiceProvider: Send + Sync + Debug {
    fn id(&self) -> &'static str;
    /// Its group's place in the tree (smaller first).
    fn order(&self) -> u16 {
        100
    }
    /// Its group's icon.
    fn icon(&self) -> &'static str;
    /// Its group's name (translated): `Database`.
    fn title(&self) -> String;
    /// The project's services (`None`: nothing for it, its group isn't shown). Asked when
    /// the window is first shown for a project; dropped with the project.
    fn open(&self, ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>>;
}

/// A contributor's services in one project: its nodes and their tabs.
pub trait ProjectServices: Send {
    /// The nodes, parents before their children (in their order among siblings).
    fn nodes(&self) -> &[ServiceNode];
    /// Changes whenever [`Self::nodes`] or a node's tabs change: the window lays its rows
    /// out again only then.
    fn generation(&self) -> u64;
    /// Back on the UI thread after a wake (or at a frame while shown): `true` if something
    /// changed.
    fn poll(&mut self) -> bool {
        false
    }
    /// Node `node`'s tabs, in their order.
    fn tabs(&self, node: ServiceId) -> Vec<ServiceTab>;
    /// Tab `tab`'s view, made when it's first shown (and again after it was let go).
    fn open_tab(&mut self, tab: ServiceId) -> Option<Box<dyn Viewer>>;
    /// The user closed tab `tab` (what it holds is let go).
    fn close_tab(&mut self, _tab: ServiceId) {}
    /// The user pinned / unpinned tab `tab`.
    fn pin_tab(&mut self, _tab: ServiceId, _pinned: bool) {}
    /// A node chosen with a double click or ↵ (a console: its editor shown). `true` if it
    /// did something.
    fn activate(&mut self, _node: ServiceId) -> bool {
        false
    }
    /// Node `node`'s right-click menu (empty: none).
    fn menu(&self, _node: ServiceId) -> Vec<ViewMenuEntry> {
        Vec::new()
    }
    /// A row of node `node`'s menu chosen. `true` if it used it.
    fn command(&mut self, _node: ServiceId, _id: &str) -> bool {
        false
    }
    /// What a row of a node's menu asks of the app (a command run with the host: a
    /// console renamed), taken once after [`Self::command`].
    fn take_action(&mut self) -> Option<crate::ViewerAction> {
        None
    }
    /// The node to show (its tab chosen): what the contributor wants seen now (a result
    /// just in), taken once.
    fn take_reveal(&mut self) -> Option<(ServiceId, Option<ServiceId>)> {
        None
    }
    /// What can be rebuilt is let go ("Clear and Rebuild Caches").
    fn purge(&mut self) {}
}

static PROVIDERS: RwLock<Vec<Arc<dyn ServiceProvider>>> = RwLock::new(Vec::new());

/// Registers a contributor (the app, at startup; one with the same id is replaced).
pub fn register(provider: Arc<dyn ServiceProvider>) {
    let mut providers = PROVIDERS.write().unwrap_or_else(PoisonError::into_inner);
    providers.retain(|p| p.id() != provider.id());
    providers.push(provider);
    providers.sort_by_key(|p| p.order());
}

/// Removes contributor `id` (tests, snapshots).
pub fn unregister(id: &str) {
    PROVIDERS.write().unwrap_or_else(PoisonError::into_inner).retain(|p| p.id() != id);
}

/// The contributors, by their order.
pub fn providers() -> Vec<Arc<dyn ServiceProvider>> {
    PROVIDERS.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The projects whose window is shown now (the window says it: drawn, hidden), so a
/// contributor knows whether what it finished is seen (a long run's end is told otherwise).
static SHOWN: RwLock<Vec<ProjectId>> = RwLock::new(Vec::new());

/// The window of `project` is shown (`true`) or hidden.
pub fn set_shown(project: ProjectId, shown: bool) {
    let mut all = SHOWN.write().unwrap_or_else(PoisonError::into_inner);
    let was = all.contains(&project);
    if shown && !was {
        all.push(project);
    } else if !shown && was {
        all.retain(|p| *p != project);
    }
}

/// Whether `project`'s window is shown now.
pub fn shown(project: ProjectId) -> bool {
    SHOWN.read().unwrap_or_else(PoisonError::into_inner).contains(&project)
}

#[cfg(test)]
#[path = "../tests/unit/services.rs"]
mod tests;
