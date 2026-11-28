//! The sessions' panel: the project's sessions of every agent on the machine, the most
//! recently active first — each its agent's icon, its title, how long ago it was active; a
//! click takes it up again in a terminal tab of the editor (the terminal asked, never named),
//! its tab showing its agent's icon. "+" starts a new session of the agent chosen (✓ in the
//! chooser beside it, which lists the agents not hosted yet too, greyed out). A search above
//! the list narrows it by the titles (every word, any case); while searching, all of the
//! project's sessions are looked through, not only those listed.
//!
//! A row's "⋮" (on the row under the mouse) and its right click open its menu: open it,
//! pin it (on top of the list, above the favorites, a pin on its row), mark it done (dimmed, after the others) or unread (a dot, gone when it's opened), give it
//! a color label (its row tinted), add it to the favorites, rename it in place (in its
//! agent's files), copy what it is, move it into a folder.
//!
//! The user's folders ([`crate::folders`]) group the sessions as a tree: a folder's row
//! opens and closes it, a session dragged onto one goes into it (dragged elsewhere in the
//! list, out of it), its menu renames or deletes it (its sessions back in the list).
//!
//! The agents are read on a worker (`agent-sessions`): when the panel is first shown, when
//! asked ("refresh"), and again when it's shown after a while (their stamps kept by each
//! agent: a list asked again reads only what changed). Hidden, it reads nothing.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use agents::{AgentProvider, AgentSession};
use contracts::agents::OpenAgentChat;
use contracts::terminal::{RunInTerminal, TerminalPlace};

use crate::favorites::Mark;
use crate::folders::Folder;

mod filter;
mod frame;
mod menus;
mod paint;
mod parts;
mod rename;
mod tool_panel;
use i18n::t;
use kernel::{Point, Rect};
use ui_components::control::{Control, Controls, Thumb};
use ui_components::picker::PickerEvent;
use ui_components::search_field::SearchField;
use ui_components::{Asked, EdgeScrollbar, ToolPanelAction, ViewerAction};
use ui_kit::TextInput;

/// The sessions listed at first, how many more "Show more…" adds, how many a search looks
/// through.
const FIRST: usize = 20;
const MORE: usize = 30;
const SEARCHED: usize = 1_000;
/// The agents' filter's width at most (logical pixels), beside the search.
const FILTER_W: f32 = 120.0;
/// The search field's height and its margins (logical pixels), as the project panel's.
const FIELD_H: f32 = 34.0;
const FIELD_MARGIN: f32 = 16.0;
/// Shown again after this long: read again (what changed meanwhile).
const STALE: Duration = Duration::from_secs(15);
/// A row's height (logical pixels).
const ROW: f32 = 30.0;

/// The header's buttons.
const NEW: &str = "new";
const REFRESH: &str = "refresh";
/// The header's filter: only the favorites listed.
const ONLY_FAVORITES: &str = "only-favorites";
/// The width kept for a row's age (`now`, `12m`, `3d`): its star stands in one column left
/// of it, whatever the age says.
const AGE_COLUMN: f32 = 34.0;
/// The new session's menu rows: `new:<agent>`.
const NEW_WITH: &str = "new:";
/// A session's menu rows.
const OPEN_CHAT: &str = "open-chat";
const OPEN_TERMINAL: &str = "open-terminal";
/// A session's terminal put to sleep by hand (asked of the sleeping: `PutToSleep`).
const SLEEP: &str = "sleep";
/// A session's sleeping terminal woken (asked of the sleeping: `WakeUp`).
const WAKE: &str = "wake";
/// A session added to the favorites, or taken off them.
const FAVORITE: &str = "favorite";
/// The header's agent chooser: which agent "+" starts.
const CHOOSE: &str = "choose-agent";
/// A session's marks, its rename, what's copied of it (`copy:<what>`), its color label
/// (`label:<key>`, `label:` none).
const DONE: &str = "done";
const PIN: &str = "pin";
const UNREAD: &str = "unread";
const RENAME: &str = "rename";
const LABEL: &str = "label:";
const COPY: &str = "copy:";
/// What can be copied of a session.
const COPY_WHAT: [(&str, &str); 4] = [
    ("title", "agents.copy_title"),
    ("id", "agents.copy_id"),
    ("command", "agents.copy_command"),
    ("file", "agents.copy_file"),
];

/// The agent "+" starts: the last one chosen (none yet: the first installed).
static CHOSEN: Mutex<Option<&'static str>> = Mutex::new(None);

/// What's renamed in its row: a session (its agent, its id), or a folder (its id).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Renamed {
    Session(&'static str, String),
    Folder(String),
}

/// Something renamed in its row: what, what's typed, where its field is.
struct Renaming {
    target: Renamed,
    input: TextInput,
    field: Rect,
}

/// A row of the list, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A favorite whose session is gone (its place among them).
    Missing(usize),
    /// A folder (its place among the project's).
    Folder(usize),
    /// A session (its index), under a folder or not.
    Session { i: usize, nested: bool },
}

/// Where a session dragged would go: into a folder, or out of any.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DropOn {
    Folder(String),
    Loose,
}

/// How far the mouse moves, pressed on a session, before it's a drag (logical pixels).
const DRAG_FROM: f32 = 4.0;

/// The places of its parts among its controls (see [`SessionsPanel::with_controls`]; the
/// list's scrollbar between the first two).
const AGENTS_LIST: usize = 0;
const RENAME_FIELD: usize = 2;
const SEARCH_FIELD: usize = 3;
const PRESSED_ROW: usize = 4;
/// The header's "new folder", and the menus' rows for the folders.
const NEW_FOLDER: &str = "new-folder";
const MOVE: &str = "move:";
const MOVE_NEW: &str = "move-new";
const FOLDER_RENAME: &str = "folder-rename";
const FOLDER_DELETE: &str = "folder-delete";

/// What a read found: the sessions (newest first), and whether there were more.
struct Found {
    sessions: Vec<AgentSession>,
    more: bool,
    /// Favorites newly found missing (their sessions gone): the user told once.
    told: Vec<crate::favorites::Favorite>,
}

/// A read going on: for how many, and its answer.
struct Reading {
    limit: usize,
    rx: Receiver<Found>,
}

pub struct SessionsPanel {
    root: Option<PathBuf>,
    sessions: Vec<AgentSession>,
    more: bool,
    limit: usize,
    /// When the list was last read (`None`: never).
    read_at: Option<Instant>,
    /// The follower's changes as last seen ([`crate::follow::changes`]): more, read again.
    followed: u64,
    reading: Option<Reading>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    list: Rect,
    scroll: f32,
    max_scroll: f32,
    bar: EdgeScrollbar,
    /// Its controls' state of use (see [`SessionsPanel::with_controls`]).
    controls: Controls,
    row_h: f32,
    hovered: Option<usize>,
    /// Only the favorites listed (the header's star).
    only_favorites: bool,
    /// Only one agent's sessions listed (the dropdown beside the search; `None`: every one's),
    /// where its field was drawn (`None`: not shown, one agent's sessions only), and whether
    /// the mouse is on it.
    agent_filter: Option<&'static str>,
    filter_field: Option<Rect>,
    hover_filter: bool,
    /// The agents' filter's list, open under it (the shared picker), and the agent of each
    /// of its rows (`None`: every one).
    filter_list: ui_components::picker::Picker,
    filter_rows: Vec<Option<&'static str>>,
    /// What the agents' list did at a press or a key (the panel follows it right after).
    list_event: Option<PickerEvent>,
    /// Each row's star as drawn (a click on it adds the session to the favorites or takes
    /// it off), and the one under the mouse.
    stars: Vec<(Rect, Star)>,
    /// Where each row's terminal says what it holds (its memory, or asleep), from the last
    /// paint: its tooltip explains.
    holds: Vec<(Rect, contracts::sleep::SleepState)>,
    /// The memory labels shaped, by their value and font size (see `paint::held_label`).
    held_labels: Vec<((u64, u32), std::sync::Arc<text_layout::ShapedLine>)>,
    hover_star: Option<Star>,
    /// "Show more…" (the row after the last).
    more_rect: Option<Rect>,
    action: Option<ToolPanelAction>,
    /// The agents asked for (tests: their own; else every one registered).
    agents: Option<Vec<Arc<dyn AgentProvider>>>,
    /// Whether conversations are shown (tests: said; else asked of who answers).
    chats: Option<bool>,
    /// The session a right click was on (its agent and its id: the list may be read again
    /// while the menu is open).
    menu_on: Option<(&'static str, String)>,
    /// What's searched for in the titles (the app's search field: it has the keys while
    /// it's the focused part), what its last key, text or × asked, and where it's laid out.
    query: SearchField,
    search_asked: Option<parts::FieldAsk>,
    field: Rect,
    /// The sessions shown (indices: the search's), for the read and the search they were
    /// made for.
    shown: Vec<usize>,
    shown_for: Option<(u64, u64, String, Option<&'static str>)>,
    /// Counts the reads taken in.
    generation: u64,
    /// The agents' icons, by their ids (kept from the last read: nothing copied per frame).
    icons: Vec<(&'static str, &'static str)>,
    /// The project's identity (its favorites are its).
    project: Option<kernel::ProjectId>,
    /// Which of the sessions read are favorites (by index), and the favorites missing.
    favorite: Vec<bool>,
    missing: Vec<crate::favorites::Favorite>,
    /// The favorite a menu was opened on (missing ones too: by agent and id).
    menu_favorite: Option<crate::favorites::Favorite>,
    /// Each session's marks (by index), as the project's were when the list was made.
    marked: Vec<Option<Mark>>,
    /// The "⋮" of the row under the mouse (where, whose), and whether the mouse is on it.
    more_button: Option<(Rect, usize)>,
    hover_more: bool,
    /// A session being renamed in its row (its field has the keys meanwhile), and what its
    /// last key asked.
    renaming: Option<Renaming>,
    rename_asked: Option<parts::FieldAsk>,
    /// Titles given here not yet read back from the agents' files (agent, id, title).
    retitled: Vec<(&'static str, String, String)>,
    /// The rows as listed (made with the sessions shown), the project's folders and how
    /// many of the sessions shown each holds.
    rows: Vec<Row>,
    folders: Vec<Folder>,
    folder_counts: Vec<usize>,
    hovered_folder: Option<usize>,
    /// The folder a menu was opened on (its id).
    menu_folder: Option<String>,
    /// A session pressed (its index, where): opened on letting go, or dragged.
    press: Option<(usize, Point)>,
    /// The session dragged, and where it would go.
    dragging: Option<usize>,
    drop_on: Option<DropOn>,
}

impl Default for SessionsPanel {
    fn default() -> Self {
        Self {
            root: None,
            sessions: Vec::new(),
            more: false,
            limit: FIRST,
            read_at: None,
            followed: 0,
            reading: None,
            waker: None,
            list: Rect::default(),
            scroll: 0.0,
            max_scroll: 0.0,
            // Hidden at rest, as the project's tree's: shown while it scrolls or is hovered.
            bar: EdgeScrollbar::auto_hiding(),
            controls: Controls::default(),
            row_h: ROW,
            hovered: None,
            only_favorites: false,
            agent_filter: None,
            filter_field: None,
            hover_filter: false,
            filter_list: ui_components::picker::Picker::default(),
            filter_rows: Vec::new(),
            list_event: None,
            stars: Vec::new(),
            holds: Vec::new(),
            held_labels: Vec::new(),
            hover_star: None,
            more_rect: None,
            action: None,
            agents: None,
            chats: None,
            menu_on: None,
            query: SearchField::default(),
            search_asked: None,
            field: Rect::default(),
            shown: Vec::new(),
            shown_for: None,
            generation: 0,
            icons: Vec::new(),
            project: None,
            favorite: Vec::new(),
            missing: Vec::new(),
            menu_favorite: None,
            marked: Vec::new(),
            more_button: None,
            hover_more: false,
            renaming: None,
            rename_asked: None,
            retitled: Vec::new(),
            rows: Vec::new(),
            folders: Vec::new(),
            folder_counts: Vec::new(),
            hovered_folder: None,
            menu_folder: None,
            press: None,
            dragging: None,
            drop_on: None,
        }
    }
}

impl std::fmt::Debug for SessionsPanel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionsPanel")
            .field("sessions", &self.sessions.len())
            .finish_non_exhaustive()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Every agent's sessions in `root`, the most recently active first, at most `limit` (and
/// whether there were more).
fn gather(
    agents: &[Arc<dyn AgentProvider>],
    root: &Path,
    limit: usize,
    project: Option<kernel::ProjectId>,
) -> Found {
    // Each agent looked for again first (installed meanwhile; its launches ready for a
    // click, which reads nothing).
    for agent in agents {
        let _ = agent.detect();
    }
    // The project's favorites: each found among all of its sessions (shown though older
    // than the list), or marked missing — one read for both, as many as either needs.
    let favorites = project.map_or_else(Vec::new, |project| {
        crate::favorites::load(project);
        crate::favorites::list(project).1
    });
    let want = if favorites.is_empty() { limit.saturating_add(1) } else { SEARCHED.max(limit + 1) };
    let mut all: Vec<AgentSession> = agents.iter().flat_map(|a| a.sessions(root, want)).collect();
    all.sort_by_key(|s| std::cmp::Reverse(s.active));
    let more = all.len() > limit;
    let mut told = Vec::new();
    if let Some(project) = project
        && !favorites.is_empty()
    {
        let found: Vec<(&str, &str)> = all.iter().map(|s| (s.agent, s.id.as_str())).collect();
        told = crate::favorites::seen(project, &found);
    }
    let older = all.split_off(limit.min(all.len()));
    let mut sessions = all;
    sessions.extend(
        older.into_iter().filter(|s| favorites.iter().any(|f| f.agent == s.agent && f.id == s.id)),
    );
    Found { sessions, more, told }
}

impl SessionsPanel {
    /// With these agents instead of the registered ones (tests).
    pub fn with_agents(agents: Vec<Arc<dyn AgentProvider>>) -> Self {
        Self { agents: Some(agents), ..Self::default() }
    }

    fn agents(&self) -> Vec<Arc<dyn AgentProvider>> {
        self.agents.clone().unwrap_or_else(agents::providers)
    }

    /// The sessions read again on a worker (unless a read for as many goes on).
    fn read(&mut self) {
        let Some(root) = self.root.clone() else { return };
        if self.reading.as_ref().is_some_and(|r| r.limit == self.limit) {
            return;
        }
        let (agents, limit, waker) = (self.agents(), self.limit, self.waker.clone());
        let project = self.project;
        let (tx, rx) = std::sync::mpsc::channel();
        kernel::work::run("agent-sessions", move || {
            let _ = tx.send(gather(&agents, &root, limit, project));
            if let Some(wake) = waker {
                wake();
            }
        });
        self.reading = Some(Reading { limit, rx });
    }

    /// What a read found, taken in: `true` if it came.
    fn take_read(&mut self) -> bool {
        let Some(reading) = &self.reading else { return false };
        let Ok(found) = reading.rx.try_recv() else { return false };
        self.sessions = found.sessions;
        // A title given here shows until the agent's files say it.
        self.retitled.retain(|(agent, id, title)| {
            let Some(s) = self.sessions.iter_mut().find(|s| s.agent == *agent && s.id == *id)
            else {
                return true;
            };
            if s.title == *title {
                return false;
            }
            s.title.clone_from(title);
            true
        });
        self.more = found.more;
        if !found.told.is_empty() {
            self.tell_missing(found.told);
        }
        self.generation += 1;
        self.icons = self.agents().iter().map(|a| (a.id(), a.icon())).collect();
        self.read_at = Some(Instant::now());
        self.reading = None;
        true
    }

    /// The row under `p` (its place in the list).
    fn row_index(&self, p: Point) -> Option<usize> {
        if !self.list.contains(p) || self.row_h <= 0.0 {
            return None;
        }
        let n = ((p.y - self.list.min_y() + self.scroll.round()) / self.row_h) as usize;
        (n < self.rows.len()).then_some(n)
    }

    /// Where row `n` starts (its top).
    fn row_top(&self, n: usize) -> f32 {
        self.list.min_y() + n as f32 * self.row_h - self.scroll.round()
    }

    /// The session under `p`, by its index.
    fn row_at(&self, p: Point) -> Option<usize> {
        match self.rows.get(self.row_index(p)?)? {
            Row::Session { i, .. } => Some(*i),
            _ => None,
        }
    }

    /// The missing favorite under `p` (its row, dimmed, first in the list).
    fn missing_at(&self, p: Point) -> Option<usize> {
        match self.rows.get(self.row_index(p)?)? {
            Row::Missing(m) => Some(*m),
            _ => None,
        }
    }

    /// The folder under `p` (its place among the project's).
    fn folder_at(&self, p: Point) -> Option<usize> {
        match self.rows.get(self.row_index(p)?)? {
            Row::Folder(f) => Some(*f),
            _ => None,
        }
    }

    /// Favorites newly found missing, told once, with a way to take them off the list.
    fn tell_missing(&mut self, told: Vec<crate::favorites::Favorite>) {
        if let Some(project) = self.project {
            crate::favorites::told(project, &told);
        }
        let names: Vec<&str> = told.iter().map(|f| f.title.as_str()).collect();
        let mut note = workbench_model::Notification::warning(i18n::tr!(
            "agents.favorite_missing",
            names = names.join(", ").as_str()
        ));
        note.actions.push(workbench_model::NotificationAction {
            label: t("agents.forget_missing"),
            command: crate::FORGET_MISSING,
        });
        let job = ui_components::WithHost::once(move |host| host.notify(note));
        self.action = Some(ToolPanelAction::View(ViewerAction::WithHost(job)));
    }

    /// Run `launch` in a terminal tab of the editor known by `key`, called `title` with the
    /// sessions' icon (`icon`).
    fn run_in_terminal(
        &mut self,
        launch: agents::Launch,
        (key, title): (String, String),
        icon: &'static str,
    ) {
        let place = TerminalPlace::Editor;
        let run = RunInTerminal { launch, key, title, icon: Some(icon), place };

        self.action = Some(ToolPanelAction::View(ViewerAction::Ask(Asked::new(run))));
    }

    /// A conversation asked for (`session`: `None` a new one), when someone shows them.
    fn open_chat(&mut self, agent: &'static str, session: Option<String>, title: String) {
        let Some(folder) = self.root.clone() else { return };
        let open = OpenAgentChat { agent, session, folder, title };
        self.action = Some(ToolPanelAction::View(ViewerAction::Ask(Asked::new(open))));
    }

    /// Conversations are shown (some module answers for them).
    fn chats(&self) -> bool {
        self.chats.unwrap_or_else(workbench_model::presence::request::<OpenAgentChat>)
    }

    /// Session `i` taken up again — as a conversation when they're shown, else (or `terminal`)
    /// in a terminal — its tab brought to the front if it's open.
    fn resume(&mut self, i: usize, terminal: bool) {
        let Some(session) = self.sessions.get(i).cloned() else { return };
        let Some(agent) = self.agents().into_iter().find(|a| a.id() == session.agent) else {
            return;
        };
        // Opened: read.
        if let Some(project) = self.project
            && self.marked.get(i).and_then(Option::as_ref).is_some_and(|m| m.unread)
        {
            crate::favorites::mark(project, session.agent, &session.id, |m| m.unread = false);
        }
        if !terminal && self.chats() {
            self.open_chat(agent.id(), Some(session.id.clone()), session.title);
            return;
        }
        // Known by its agent's and its own ids — or by the tab a new session that took this
        // id has: the same session, the same tab.
        let key = crate::follow::tab_of(&session)
            .unwrap_or_else(|| format!("{}-{}", agent.id(), session.id));
        let folder = session.folder.clone();
        crate::follow::follow(
            key.clone(),
            agent.id(),
            Some(session.id.clone()),
            folder,
            session.title.clone(),
        );
        self.run_in_terminal(agent.resume(&session), (key, session.title.clone()), agent.icon());
    }

    /// A new session of `agent` in the project, in a terminal tab.
    fn start(&mut self, agent: &Arc<dyn AgentProvider>) {
        let Some(root) = self.root.clone() else { return };
        let title = crate::next_title(&agent.name());
        // A new session has no id yet: one of its tab's own (its id taken when its file
        // appears).
        let key = format!("{}-new-{:x}", agent.id(), kernel::id::new_uuid());
        crate::follow::follow(key.clone(), agent.id(), None, root.clone(), title.clone());
        self.run_in_terminal(agent.new_session(&root), (key, title), agent.icon());
        *lock(&CHOSEN) = Some(agent.id());
        // Its file appears once it's written: read again a little later, when shown.
        self.read_at = None;
    }
}

/// A session's file, if its agent keeps one.
fn session_file(session: Option<&AgentSession>) -> Option<&Path> {
    session?.file.as_deref()
}

/// `r` grown by `by` on every side (a small target easier to hit).
fn around(r: Rect, by: f32) -> Rect {
    Rect::new(r.min_x() - by, r.min_y() - by, r.size.width + 2.0 * by, r.size.height + 2.0 * by)
}

/// A row's star: a session's (by its index), or a missing favorite's (by its place).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Star {
    Session(usize),
    Missing(usize),
}

impl SessionsPanel {
    fn star_at(&self, p: Point) -> Option<Star> {
        if !self.list.contains(p) {
            return None;
        }
        self.stars.iter().find(|(r, _)| r.contains(p)).map(|(_, s)| *s)
    }

    /// The star clicked: the session added to the favorites, or taken off them.
    fn toggle_star(&mut self, star: Star) -> bool {
        let Some(project) = self.project else { return false };
        let (agent, id, title) = match star {
            Star::Session(i) => {
                let Some(s) = self.sessions.get(i) else { return false };
                (s.agent.to_owned(), s.id.clone(), s.title.clone())
            }
            Star::Missing(n) => {
                let Some(f) = self.missing.get(n) else { return false };
                (f.agent.clone(), f.id.clone(), f.title.clone())
            }
        };
        crate::favorites::toggle(project, &agent, &id, &title);
        true
    }

    /// The "⋮" under `p`: its session.
    fn more_at(&self, p: Point) -> Option<usize> {
        self.more_button.filter(|(r, _)| self.list.contains(p) && r.contains(p)).map(|(_, i)| i)
    }

    /// The agent "+" starts: the one chosen last, if it's installed; else the first that is.
    fn chosen(&self) -> Option<Arc<dyn AgentProvider>> {
        let agents = self.agents();
        let chosen = *lock(&CHOSEN);
        let installed = |a: &&Arc<dyn AgentProvider>| a.installed();
        chosen
            .and_then(|id| agents.iter().filter(installed).find(|a| a.id() == id))
            .or_else(|| agents.iter().find(installed))
            .cloned()
    }

    /// The scale the list was last drawn at (its rows' height says it).
    fn scale(&self) -> f32 {
        if self.row_h > 0.0 { self.row_h / ROW } else { 1.0 }
    }

    /// Whether a star's session is a favorite now.
    fn starred(&self, star: Star) -> bool {
        match star {
            Star::Session(i) => self.favorite.get(i).copied().unwrap_or(false),
            Star::Missing(_) => true,
        }
    }
}

impl SessionsPanel {
    /// Its controls, the topmost first: the agents' list (over everything while open), the
    /// list's scrollbar with the scroll it moves, the renamed row's field, the search, a
    /// session pressed (held from the panel's own press, to be dragged).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        let from = DRAG_FROM * self.scale();
        let mut agents =
            parts::AgentsList { list: &mut self.filter_list, event: &mut self.list_event };
        let mut list = Thumb { bar: &mut self.bar, offset: &mut self.scroll };
        let mut rename = self.renaming.as_mut().map(|r| parts::FieldPart {
            input: &mut r.input,
            area: r.field,
            asked: &mut self.rename_asked,
        });
        let mut search = parts::SearchPart {
            field: &mut self.query,
            area: self.field,
            asked: &mut self.search_asked,
        };
        let mut rows = parts::RowDrag { pressed: self.press, dragging: &mut self.dragging, from };
        let mut parts: [&mut dyn Control; 5] =
            [&mut agents, &mut list, &mut rename, &mut search, &mut rows];
        f(&mut self.controls, &mut parts)
    }

    /// The search has the keys (clicked into).
    fn typing(&self) -> bool {
        self.controls.focused() == Some(SEARCH_FIELD)
    }

    /// The keys to the search (`true`), or taken from it.
    fn keys_to_search(&mut self, on: bool) {
        let to = if on { Some(SEARCH_FIELD) } else { None };
        if on || self.typing() {
            self.with_controls(|c, parts| c.focus(parts, to));
        }
    }

    /// The renamed row's field gone: the keys it had go nowhere.
    fn rename_ended(&mut self) {
        if self.controls.focused() == Some(RENAME_FIELD) {
            self.with_controls(|c, parts| c.focus(parts, None));
        }
    }

    /// What part `part` asked at a key, a text or a command: the agents' list's row chosen
    /// (or it closed), the name given or left, the first session found taken up again, the
    /// search let go or changed.
    fn follow(&mut self, part: usize) {
        match part {
            AGENTS_LIST => {
                let event = self.list_event.take();
                self.filter_list_event(event);
            }
            RENAME_FIELD => match self.rename_asked.take() {
                Some(parts::FieldAsk::Enter) => self.finish_rename(),
                Some(parts::FieldAsk::Escape) => {
                    self.renaming = None;
                    self.rename_ended();
                }
                Some(parts::FieldAsk::Searched) | None => {}
            },
            SEARCH_FIELD => match self.search_asked.take() {
                Some(parts::FieldAsk::Enter) => {
                    if let Some(&i) = self.shown.first() {
                        self.resume(i, true);
                    }
                }
                Some(parts::FieldAsk::Escape) => self.keys_to_search(false),
                Some(parts::FieldAsk::Searched) => self.searching(),
                None => {}
            },
            _ => {}
        }
    }
}

/// The poison-proof lock (kept here for the title counter).
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether session `agent` / `id`'s "Put to Sleep" row shows, and greyed out (the tests).
#[cfg(test)]
pub(crate) fn sleep_row_for_tests(agent: &str, id: &str) -> Option<bool> {
    menus::sleep_row(agent, id).map(|row| match row {
        ui_components::ViewMenuEntry::Item { look, .. } => look.disabled,
        _ => false,
    })
}

/// Whether session `agent` / `id`'s row offers "Wake Up" (the tests).
#[cfg(test)]
pub(crate) fn wake_row_for_tests(agent: &str, id: &str) -> Option<bool> {
    menus::sleep_row(agent, id).map(|row| match row {
        ui_components::ViewMenuEntry::Item { id, .. } => id == WAKE,
        _ => false,
    })
}

#[cfg(test)]
#[path = "../../tests/unit/panel.rs"]
mod tests;
