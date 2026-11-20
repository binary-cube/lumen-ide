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

use crate::favorites::{Label, Mark};
use crate::folders::Folder;

mod paint;
use i18n::t;
use kernel::{Point, Rect};
use ui_components::ds::{self, Ctx};
use ui_components::picker::{PickerEvent, PickerItem};
use ui_components::{
    Asked, EdgeScrollbar, KeyReach, PanelButton, PanelContext, ToolPanel, ToolPanelAction,
    ViewMenuEntry, ViewerAction, ViewerCursor, ViewerKey, ViewerPaint,
};
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
    /// Each row's star as drawn (a click on it adds the session to the favorites or takes
    /// it off), and the one under the mouse.
    stars: Vec<(Rect, Star)>,
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
    /// What's searched for in the titles, and whether it has the keys (clicked into).
    query: TextInput,
    typing: bool,
    field: Rect,
    /// The search's × (while there's text).
    clear: Option<Rect>,
    hover_clear: bool,
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
    /// A session being renamed in its row.
    renaming: Option<Renaming>,
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
            row_h: ROW,
            hovered: None,
            only_favorites: false,
            agent_filter: None,
            filter_field: None,
            hover_filter: false,
            filter_list: ui_components::picker::Picker::default(),
            filter_rows: Vec::new(),
            stars: Vec::new(),
            hover_star: None,
            more_rect: None,
            action: None,
            agents: None,
            chats: None,
            menu_on: None,
            query: TextInput::default(),
            typing: false,
            field: Rect::default(),
            clear: None,
            hover_clear: false,
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

    /// Something is searched for (the folders holding what's found shown open).
    fn searched(&self) -> bool {
        !self.query.text().trim().is_empty()
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

    /// The sessions shown made again when the read or the search changed: those whose title
    /// has every word searched (any case), in the read's order.
    fn filter(&mut self) {
        // Compared first without taking anything (every paint): the list only on a change.
        let at = self.project.map_or(0, crate::favorites::generation);
        let query = self.query.text();
        let agent = self.agent_filter;
        if self.shown_for.as_ref().is_some_and(|(g, f, q, a)| {
            *g == self.generation && *f == at && q == query && *a == agent
        }) {
            return;
        }
        let key = (self.generation, at, query.to_owned(), agent);
        let (_, favorites, missing) = self.project.map(crate::favorites::list).unwrap_or_default();
        self.favorite = self
            .sessions
            .iter()
            .map(|s| favorites.iter().any(|f| f.agent == s.agent && f.id == s.id))
            .collect();
        let marks = self.project.map(crate::favorites::marks).unwrap_or_default();
        self.marked = self
            .sessions
            .iter()
            .map(|s| marks.iter().find(|m| m.agent == s.agent && m.id == s.id).cloned())
            .collect();
        let words: Vec<String> = key.2.split_whitespace().map(str::to_lowercase).collect();
        let matches = |title: &str| {
            let title = title.to_lowercase();
            words.iter().all(|w| title.contains(w.as_str()))
        };
        self.missing = favorites
            .into_iter()
            .filter(|f| missing.contains(&(f.agent.clone(), f.id.clone())) && matches(&f.title))
            .filter(|f| agent.is_none_or(|a| f.agent == a))
            .collect();
        self.shown = self
            .sessions
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                matches(&s.title)
                    && (!self.only_favorites || self.favorite[*i])
                    && agent.is_none_or(|a| s.agent == a)
            })
            .map(|(i, _)| i)
            .collect();
        // The pinned first, then the favorites (in the read's order), then the rest; in each,
        // those done last.
        let (favorite, marked) = (&self.favorite, &self.marked);
        self.shown.sort_by_key(|&i| {
            let mark = marked.get(i).and_then(Option::as_ref);
            let (pinned, done) = mark.map_or((false, false), |m| (m.pinned, m.done));
            (!pinned, !favorite.get(i).copied().unwrap_or(false), done)
        });
        self.shown_for = Some(key);
        self.arrange();
    }

    /// The rows from the sessions shown: the missing favorites, each folder with its
    /// sessions under it (open, or searched; a folder holding none of what's searched or
    /// filtered left out), then the sessions in none.
    fn arrange(&mut self) {
        self.folders = self.project.map(crate::folders::list).unwrap_or_default();
        let narrowed = self.searched() || self.only_favorites || self.agent_filter.is_some();
        let folder_of = |i: usize| -> Option<&str> {
            let id = self.marked.get(i)?.as_ref()?.folder.as_deref()?;
            self.folders.iter().any(|f| f.id == id).then_some(id)
        };
        let mut rows: Vec<Row> = (0..self.missing.len()).map(Row::Missing).collect();
        let mut counts = Vec::with_capacity(self.folders.len());
        for (f, folder) in self.folders.iter().enumerate() {
            let held: Vec<usize> =
                self.shown.iter().copied().filter(|&i| folder_of(i) == Some(&folder.id)).collect();
            counts.push(held.len());
            if narrowed && held.is_empty() {
                continue;
            }
            rows.push(Row::Folder(f));
            if folder.open || self.searched() {
                rows.extend(held.into_iter().map(|i| Row::Session { i, nested: true }));
            }
        }
        let loose: Vec<Row> = self
            .shown
            .iter()
            .copied()
            .filter(|&i| folder_of(i).is_none())
            .map(|i| Row::Session { i, nested: false })
            .collect();
        rows.extend(loose);
        self.rows = rows;
        self.folder_counts = counts;
    }

    /// Searching: all of the project's sessions looked through (read again for as many).
    fn searching(&mut self) {
        self.scroll = 0.0;
        self.hovered = None;
        if !self.query.text().trim().is_empty() && self.limit < SEARCHED {
            self.limit = SEARCHED;
            self.read();
        }
    }

    /// The search field: the app's pill, what's typed, its ×.
    fn paint_search(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        let mut pen = ui_components::LayerPainter::new(layer, ctx.text, ctx.icons, *ds);
        ds::search_input(&mut pen, self.field, &self.query, self.typing, &t("agents.search"));
        self.clear = None;
        if !self.query.text().is_empty() {
            let mid = self.field.min_y() + self.field.size.height / 2.0;
            let x = self.field.max_x() - ds.px(18.0);
            let close = ds::clear_button(pen.layer, ctx.icons, ds, x, mid, self.hover_clear);
            self.clear = Some(close);
        }
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
        let key = crate::follow::key_of(agent.id(), &session.id)
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

    /// The agents whose sessions were read (in the agents' order), for the filter.
    fn agents_read(&self) -> Vec<&'static str> {
        let mut read: Vec<&'static str> = self.icons.iter().map(|(id, _)| *id).collect();
        read.retain(|id| self.sessions.iter().any(|s| s.agent == *id));
        read
    }

    /// The filter is offered while the list mixes agents (or one is chosen: to undo it).
    fn filters_agents(&self) -> bool {
        self.agent_filter.is_some() || self.agents_read().len() > 1
    }

    /// The agents' filter's list opened under it: every agent, then each one with sessions,
    /// the chosen one checked.
    fn open_agent_filter(&mut self) {
        let mut ids = self.agents_read();
        if let Some(chosen) = self.agent_filter.filter(|a| !ids.contains(a)) {
            ids.push(chosen);
        }
        let mut rows: Vec<Option<&'static str>> = vec![None];
        rows.extend(ids.into_iter().map(Some));
        let items = rows
            .iter()
            .map(|row| match row {
                None => PickerItem { label: t("agents.all_agents"), ..PickerItem::default() },
                Some(id) => PickerItem {
                    label: self.agent_name(id),
                    icon: Some(
                        self.icons.iter().find(|(a, _)| a == id).map_or(crate::ICON, |(_, i)| *i),
                    ),
                    ..PickerItem::default()
                },
            })
            .collect();
        let chosen = rows.iter().position(|r| *r == self.agent_filter);
        self.filter_list.open(items, chosen);
        self.filter_rows = rows;
    }

    /// What the agents' list answered: a row chosen filters by its agent.
    fn filter_list_event(&mut self, event: Option<PickerEvent>) -> bool {
        match event {
            Some(PickerEvent::Chosen(i)) => {
                match self.filter_rows.get(i).copied() {
                    Some(Some(id)) => self.filter_by_agent(id),
                    Some(None) => self.filter_by_agent(""),
                    None => {}
                }
                true
            }
            Some(_) => true,
            None => false,
        }
    }

    /// The panel: its search, its agents' field, its rows (the agents' list over them is
    /// painted after).
    fn paint_panel(&mut self, ctx: &mut ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.take_read();
        if self.read_at.is_none_or(|at| at.elapsed() >= STALE) {
            self.read();
        }
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let b = ctx.bounds;
        let margin = ds.px(FIELD_MARGIN);
        self.field = Rect::new(
            b.min_x() + margin,
            b.min_y() + ds.px(4.0),
            (b.size.width - margin * 2.0).max(0.0),
            ds.px(FIELD_H),
        );
        // The agents' filter on the search's right, while there's a choice to make.
        self.filter_field = None;
        if self.filters_agents() {
            let width = ds.px(FILTER_W).min(self.field.size.width * 0.45).round();
            let gap = ds.px(6.0);
            self.field.size.width = (self.field.size.width - width - gap).max(0.0);
            self.filter_field = Some(Rect::new(
                self.field.max_x() + gap,
                self.field.min_y(),
                width,
                self.field.size.height,
            ));
        }
        let top = self.field.max_y() + ds.px(8.0);
        self.list = Rect::new(b.min_x(), top, b.size.width, (b.max_y() - top).max(0.0));
        self.filter();
        if self.root.is_some() {
            self.paint_search(ctx, &ds);
            self.paint_agent_filter(ctx, &ds);
        }
        let b = self.list;
        let layer = ctx.scene.push_layer(Some(b));
        if self.root.is_none() {
            ds::empty_state(layer, ctx.text, &ds, b, &t("agents.no_project"), None);
            return;
        }
        if self.read_at.is_none() {
            let label = t("agents.reading");
            let mid = b.min_y() + ds.px(24.0);
            ds::loader(
                layer,
                ctx.text,
                &ds,
                b.min_x() + ds.px(16.0),
                mid,
                &ds::Loader::with_text(&label),
            );
            return;
        }
        if self.shown.is_empty() && self.missing.is_empty() {
            if self.only_favorites && self.query.text().trim().is_empty() {
                let detail = t("agents.no_favorites_detail");
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.no_favorites"), Some(&detail));
            } else if self.sessions.is_empty() {
                let detail = t("agents.none_detail");
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.none"), Some(&detail));
            } else {
                ds::empty_state(layer, ctx.text, &ds, b, &t("agents.not_found"), None);
            }
            return;
        }
        self.paint_rows(ctx, &ds);
    }

    /// Agent `id`'s name (its id when it's no longer offered).
    fn agent_name(&self, id: &str) -> String {
        self.agents().iter().find(|a| a.id() == id).map_or_else(|| id.to_owned(), |a| a.name())
    }

    /// Only agent `id`'s sessions listed (empty: every agent's).
    fn filter_by_agent(&mut self, id: &str) {
        let chosen = self.agents().iter().map(|a| a.id()).find(|a| *a == id);
        if chosen == self.agent_filter {
            return;
        }
        self.agent_filter = chosen;
        self.scroll = 0.0;
        self.hovered = None;
    }

    /// The agents' filter: our dropdown, the agent chosen (or every one) in it.
    fn paint_agent_filter(&mut self, ctx: &mut ViewerPaint<'_>, ds: &Ctx<'_>) {
        let Some(r) = self.filter_field else { return };
        let value = match self.agent_filter {
            Some(id) => self.agent_name(id),
            None => t("agents.all_agents"),
        };
        let layer = ctx.scene.push_layer(Some(ctx.bounds));
        let spec = ds::DropdownSpec {
            value: &value,
            open: false,
            hovered: self.hover_filter,
            enabled: true,
            note: None,
            placeholder: None,
        };
        ds::dropdown(layer, ctx.text, ctx.icons, ds, r, spec);
    }

    /// The agents to choose from: those offered (greyed out while not installed), then those
    /// not hosted yet (greyed out); the one "+" starts checked.
    fn agents_menu(&self) -> Vec<ViewMenuEntry> {
        let chosen = self.chosen().map(|a| a.id());
        let mut rows: Vec<ViewMenuEntry> = self
            .agents()
            .iter()
            .map(|a| {
                ViewMenuEntry::item(format!("{NEW_WITH}{}", a.id()), a.name())
                    .with_icon(a.icon())
                    .disabled_if(!a.installed())
                    .checked_if(chosen == Some(a.id()))
            })
            .collect();
        // Tests list their own agents only.
        if self.agents.is_none() {
            rows.extend(agents::planned().into_iter().map(|a| {
                ViewMenuEntry::item(format!("{NEW_WITH}{}", a.id), a.name)
                    .with_icon(a.icon)
                    .disabled_if(true)
            }));
        }
        rows
    }

    /// Session `i`'s menu (its right click's, its "⋮"'s), the session kept for the row chosen.
    fn session_menu(&mut self, i: usize) -> Option<Vec<ViewMenuEntry>> {
        let session = self.sessions.get(i)?;
        self.menu_on = Some((session.agent, session.id.clone()));
        self.menu_favorite = Some(crate::favorites::Favorite {
            agent: session.agent.to_owned(),
            id: session.id.clone(),
            title: session.title.clone(),
        });
        let mark = self.marked.get(i).cloned().flatten().unwrap_or_default();
        let mut rows = vec![
            ViewMenuEntry::item(OPEN_TERMINAL, t("agents.open_terminal")).with_icon("terminal"),
        ];
        if self.chats() {
            rows.push(ViewMenuEntry::item(OPEN_CHAT, t("agents.open_chat")).with_icon(crate::ICON));
        }
        rows.push(ViewMenuEntry::Separator);
        let pin = if mark.pinned { "agents.unpin" } else { "agents.pin" };
        rows.push(ViewMenuEntry::item(PIN, t(pin)).with_icon("pin"));
        let (done, done_icon) =
            if mark.done { ("agents.not_done", "rotate-ccw") } else { ("agents.done", "check") };
        rows.push(ViewMenuEntry::item(DONE, t(done)).with_icon(done_icon));
        let unread = if mark.unread { "agents.mark_read" } else { "agents.mark_unread" };
        rows.push(ViewMenuEntry::item(UNREAD, t(unread)).with_icon(crate::UNREAD_ICON));
        let mut labels: Vec<ViewMenuEntry> = Label::ALL
            .into_iter()
            .map(|l| {
                let name = t(&format!("agents.color_{}", l.key()));
                ViewMenuEntry::item(format!("{LABEL}{}", l.key()), name)
                    .with_swatch(l.rgb())
                    .checked_if(mark.label == Some(l))
            })
            .collect();
        labels.push(ViewMenuEntry::Separator);
        labels.push(
            ViewMenuEntry::item(LABEL, t("agents.color_default")).checked_if(mark.label.is_none()),
        );
        rows.push(ViewMenuEntry::submenu(t("agents.color_label"), labels).with_icon("tag"));
        rows.push(ViewMenuEntry::Separator);
        let key = if self.favorite.get(i).copied().unwrap_or(false) {
            "agents.unfavorite"
        } else {
            "agents.favorite"
        };
        rows.push(ViewMenuEntry::item(FAVORITE, t(key)).with_icon(crate::FAVORITE_ICON));
        let renamable = self.renamable(i);
        rows.push(
            ViewMenuEntry::item(RENAME, t("agents.rename"))
                .with_icon("text-cursor")
                .disabled_if(!renamable),
        );
        rows.push(self.move_menu(mark.folder.as_deref()));
        rows.push(ViewMenuEntry::Separator);
        let copies = COPY_WHAT
            .iter()
            .filter(|(what, _)| *what != "file" || session_file(self.sessions.get(i)).is_some())
            .map(|(what, key)| ViewMenuEntry::item(format!("{COPY}{what}"), t(key)))
            .collect();
        rows.push(ViewMenuEntry::submenu(t("agents.copy_info"), copies).with_icon("copy"));
        Some(rows)
    }

    /// "Move to Folder ›": the project's folders (the one it's in checked), a new one, out
    /// of the one it's in.
    fn move_menu(&self, now: Option<&str>) -> ViewMenuEntry {
        let mut entries: Vec<ViewMenuEntry> = self
            .folders
            .iter()
            .map(|f| {
                ViewMenuEntry::item(format!("{MOVE}{}", f.id), f.name.clone())
                    .with_icon("folder")
                    .checked_if(now == Some(f.id.as_str()))
            })
            .collect();
        entries.push(ViewMenuEntry::Separator);
        entries.push(
            ViewMenuEntry::item(MOVE_NEW, t("agents.new_folder_with")).with_icon("folder-plus"),
        );
        if now.is_some() {
            entries.push(
                ViewMenuEntry::item(MOVE, t("agents.remove_from_folder")).with_icon("folder-x"),
            );
        }
        ViewMenuEntry::submenu(t("agents.move_to_folder"), entries).with_icon("folder-input")
    }

    /// A row of the folders' menus chosen: `true` if it was one.
    fn folder_command(&mut self, id: &str) -> bool {
        let Some(project) = self.project else { return false };
        match id {
            NEW_FOLDER => self.new_folder(|_, _| {}),
            FOLDER_RENAME => {
                let Some(folder) = self.menu_folder.take() else { return true };
                let name = self.folders.iter().find(|f| f.id == folder).map(|f| f.name.clone());
                self.rename_in_row(Renamed::Folder(folder), &name.unwrap_or_default());
            }
            FOLDER_DELETE => {
                if let Some(folder) = self.menu_folder.take() {
                    crate::folders::remove(project, &folder);
                }
            }
            // A new folder, the session the menu was on moved into it.
            MOVE_NEW => {
                let on = self.menu_on.take();
                self.new_folder(|_, folder| {
                    if let Some((agent, session)) = on {
                        crate::folders::move_to(project, agent, &session, Some(folder));
                    }
                });
            }
            _ => {
                let Some(folder) = id.strip_prefix(MOVE) else { return false };
                if let Some((agent, session)) = self.menu_on.take() {
                    let folder = (!folder.is_empty()).then_some(folder);
                    crate::folders::move_to(project, agent, &session, folder);
                }
            }
        }
        true
    }

    /// The scale the list was last drawn at (its rows' height says it).
    fn scale(&self) -> f32 {
        if self.row_h > 0.0 { self.row_h / ROW } else { 1.0 }
    }

    /// Whether session `i`'s agent can rename it in its files.
    fn renamable(&self, i: usize) -> bool {
        let Some(session) = self.sessions.get(i) else { return false };
        self.agents().iter().any(|a| a.id() == session.agent && a.can_rename())
    }

    /// The session the menu was opened on, by its index now.
    fn menu_session(&self) -> Option<usize> {
        let (agent, id) = self.menu_on.as_ref()?;
        self.sessions.iter().position(|s| s.agent == *agent && s.id == *id)
    }

    /// Session `i` renamed in its row: a field with its title, all of it selected.
    fn start_rename(&mut self, i: usize) {
        let Some(session) = self.sessions.get(i) else { return };
        let target = Renamed::Session(session.agent, session.id.clone());
        let title = session.title.clone();
        self.rename_in_row(target, &title);
    }

    /// `target` renamed in its row, from `now`, all of it selected.
    fn rename_in_row(&mut self, target: Renamed, now: &str) {
        let mut input = TextInput::new(now);
        input.select_all();
        self.renaming = Some(Renaming { target, input, field: Rect::default() });
        self.typing = false;
    }

    /// A folder made (named "New Folder"), `then` done with its id, and named in its row.
    fn new_folder(&mut self, then: impl FnOnce(&mut Self, &str)) {
        let Some(project) = self.project else { return };
        let Some(id) = crate::folders::create(project, "") else { return };
        then(self, &id);
        self.shown_for = None;
        self.filter();
        let name = self.folders.iter().find(|f| f.id == id).map(|f| f.name.clone());
        self.rename_in_row(Renamed::Folder(id), &name.unwrap_or_default());
    }

    /// The name typed, given: in the agent's files — at once if its tab isn't open, else
    /// once its agent ends (and its tab named so now) — and shown here meanwhile.
    fn finish_rename(&mut self) {
        let Some(renaming) = self.renaming.take() else { return };
        let title = crate::one_line(renaming.input.text());
        let (agent, id) = match renaming.target {
            Renamed::Folder(id) => {
                if let Some(project) = self.project {
                    crate::folders::rename(project, &id, &title);
                }
                return;
            }
            Renamed::Session(agent, id) => (agent, id),
        };
        let Some(i) = self.sessions.iter().position(|s| s.agent == agent && s.id == id) else {
            return;
        };
        if title.is_empty() || title == self.sessions[i].title {
            return;
        }
        title.clone_into(&mut self.sessions[i].title);
        let folder = self.sessions[i].folder.clone();
        self.retitled.retain(|(a, r, _)| !(*a == agent && *r == id));
        self.retitled.push((agent, id.clone(), title.clone()));
        self.shown_for = None;
        if let Some(project) = self.project
            && self.favorite.get(i).copied().unwrap_or(false)
        {
            // A favorite keeps the name it's known by when gone.
            crate::favorites::retitle(project, agent, &id, &title);
        }
        // Its tab open: its name there now, in the agent's files once it ends.
        if let Some(key) = crate::follow::key_of(agent, &id) {
            crate::follow::renamed(&key, &title);
            crate::follow::rename_tab(key, title);
            return;
        }
        kernel::work::run("agent-sessions", move || {
            let renamed = agents::provider(agent)
                .map_or(Err("no such agent".into()), |a| a.rename_session(&folder, &id, &title));
            if let Err(e) = renamed {
                tracing::warn!(error = %e, "a session wasn't renamed in its agent");
            }
        });
    }

    /// What's copied of the session the menu was on (`what`: a [`COPY_WHAT`]'s).
    fn copy(&mut self, what: &str) {
        let Some(i) = self.menu_session() else { return };
        let session = &self.sessions[i];
        let text = match what {
            "title" => session.title.clone(),
            "id" => session.id.clone(),
            "command" => {
                let Some(agent) = self.agents().into_iter().find(|a| a.id() == session.agent)
                else {
                    return;
                };
                agent.resume(session).display()
            }
            "file" => match session_file(Some(session)) {
                Some(file) => file.display().to_string(),
                None => return,
            },
            _ => return,
        };
        let job = ui_components::WithHost::once(move |host| host.set_clipboard_text(&text));
        self.action = Some(ToolPanelAction::View(ViewerAction::WithHost(job)));
    }

    /// Whether a star's session is a favorite now.
    fn starred(&self, star: Star) -> bool {
        match star {
            Star::Session(i) => self.favorite.get(i).copied().unwrap_or(false),
            Star::Missing(_) => true,
        }
    }
}

impl ToolPanel for SessionsPanel {
    fn update(&mut self, _ctx: &PanelContext<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(view) = data.downcast_ref::<workbench_model::ProjectView>() else { return };
        self.project = view.project;
        let root = view.queries.as_ref().map(|q| q.root());
        if root != self.root {
            self.root = root;
            self.sessions.clear();
            self.more = false;
            self.read_at = None;
            self.reading = None;
        }
    }

    /// A read come in; a session the follower saw change (found, renamed): read again now.
    fn poll(&mut self) -> bool {
        let changes = crate::follow::changes();
        if changes != self.followed {
            self.followed = changes;
            self.read();
        }
        self.take_read()
    }

    fn is_loading(&self) -> bool {
        self.reading.is_some() && self.read_at.is_none()
    }

    /// Hidden: the read going on given up (asked again when shown).
    fn parked(&mut self) {
        self.reading = None;
    }

    fn paint(&mut self, mut ctx: ViewerPaint<'_>) {
        self.paint_panel(&mut ctx);
        // The agents' list, over everything, under its field.
        if let Some(anchor) = self.filter_field
            && self.filter_list.is_open()
        {
            let ds = Ctx::new(ctx.palette, ctx.scale);
            let bounds = ctx.bounds;
            self.filter_list.paint(ctx.scene, ctx.text, ctx.icons, &ds, (anchor, bounds), "");
        } else {
            self.filter_list.close();
        }
    }

    fn scroll(&mut self, dy: f32) -> bool {
        if self.filter_list.is_open() {
            return self.filter_list.scroll(dy);
        }
        let before = self.scroll;
        self.scroll = (self.scroll + dy).clamp(0.0, self.max_scroll);
        self.scroll != before
    }

    fn click(&mut self, p: Point) -> Option<ToolPanelAction> {
        // The agents' list open: it takes the click (a row, or closed by one outside).
        if self.filter_list.is_open() {
            let event = self.filter_list.press(p);
            self.filter_list_event(event);
            return None;
        }
        if let Some(offset) = self.bar.press(p) {
            self.scroll = offset;
            return None;
        }
        // Renaming: a click in its field moves the caret; elsewhere, the name's given.
        if let Some(renaming) = &mut self.renaming {
            if renaming.field.contains(p) {
                renaming.input.press(p.x, 1);
                return None;
            }
            self.finish_rename();
        }
        // The agents' filter: its list, under it.
        if let Some(anchor) = self.filter_field.filter(|r| r.contains(p)) {
            self.typing = false;
            if anchor.size.width > 0.0 {
                self.open_agent_filter();
            }
            return None;
        }
        // The search: its ×, or the keys to it (a click elsewhere takes them back).
        if self.clear.is_some_and(|r| r.contains(p)) {
            self.query = TextInput::default();
            self.typing = true;
            self.searching();
            return None;
        }
        self.typing = self.field.contains(p);
        if self.typing {
            self.query.press(p.x, 1);
            return None;
        }
        if self.more_rect.is_some_and(|r| r.contains(p)) {
            self.limit = self.limit.saturating_add(MORE);
            self.read();
            return None;
        }
        // A row's star: added to the favorites or taken off (the session isn't opened).
        if let Some(star) = self.star_at(p) {
            self.toggle_star(star);
            return None;
        }
        // A row's "⋮": its menu, under it.
        if let Some(i) = self.more_at(p) {
            let anchor = self.more_button.map(|(r, _)| r).unwrap_or_default();
            let entries = self.session_menu(i)?;
            return Some(ToolPanelAction::MenuUnder { anchor, entries });
        }
        // A folder: opened or closed.
        if let Some(f) = self.folder_at(p) {
            if let (Some(project), Some(folder)) = (self.project, self.folders.get(f)) {
                crate::folders::toggle_open(project, &folder.id);
            }
            return None;
        }
        // A session: opened on letting go (unless it's dragged into a folder meanwhile).
        let i = self.row_at(p)?;
        self.press = Some((i, p));
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        if let Some(offset) = self.bar.drag(p) {
            let changed = offset != self.scroll;
            self.scroll = offset;
            return changed;
        }
        let Some((i, from)) = self.press else { return false };
        let far = (p.x - from.x).hypot(p.y - from.y) >= DRAG_FROM * self.scale();
        if self.dragging.is_none() && !far {
            return false;
        }
        self.dragging = Some(i);
        // Onto a folder or one of its sessions: into it; elsewhere in the list: out of any.
        let into = |f: &Folder| DropOn::Folder(f.id.clone());
        let drop = match self.row_index(p).and_then(|n| self.rows.get(n)) {
            Some(Row::Folder(f)) => self.folders.get(*f).map(into),
            Some(Row::Session { i, nested: true }) => {
                let folder = self.marked.get(*i).cloned().flatten().and_then(|m| m.folder);
                self.folders.iter().find(|f| Some(&f.id) == folder.as_ref()).map(into)
            }
            _ => self.list.contains(p).then_some(DropOn::Loose),
        };
        let changed = drop != self.drop_on;
        self.drop_on = drop;
        changed || self.dragging.is_some()
    }

    /// Let go: a session dragged goes where it was dropped; one pressed and not moved is
    /// opened (its terminal asked for, as a click's would be).
    fn release(&mut self) -> bool {
        if self.bar.release() {
            return true;
        }
        let Some((i, _)) = self.press.take() else { return false };
        let dropped = self.drop_on.take();
        if self.dragging.take().is_some() {
            if let (Some(project), Some(session), Some(drop)) =
                (self.project, self.sessions.get(i), dropped)
            {
                let folder = match &drop {
                    DropOn::Folder(id) => Some(id.as_str()),
                    DropOn::Loose => None,
                };
                crate::folders::move_to(project, session.agent, &session.id, folder);
            }
            return true;
        }
        self.resume(i, true);
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        if self.filter_list.is_open() {
            return self.filter_list.hover(p);
        }
        let mut changed = self.bar.hover(p);
        let folder = self.folder_at(p);
        changed |= folder != self.hovered_folder;
        self.hovered_folder = folder;
        let row = self.row_at(p);
        let clear = self.clear.is_some_and(|r| r.contains(p));
        let star = self.star_at(p);
        let more = self.more_at(p).is_some();
        let filter = self.filter_field.is_some_and(|r| r.contains(p));
        changed |= filter != self.hover_filter;
        self.hover_filter = filter;
        changed |= row != self.hovered
            || clear != self.hover_clear
            || star != self.hover_star
            || more != self.hover_more;
        (self.hovered, self.hover_clear, self.hover_star, self.hover_more) =
            (row, clear, star, more);
        changed
    }

    /// Clicked into (or a session renamed), it takes the keys (its rows stay the mouse's).
    fn takes_keys(&self) -> bool {
        self.typing || self.renaming.is_some() || self.filter_list.is_open()
    }

    fn wants_text(&self) -> bool {
        self.typing || self.renaming.is_some() || self.filter_list.searches()
    }

    fn text_input(&mut self, typed: &str) -> bool {
        let typed: String = typed.chars().filter(|c| !c.is_control()).collect();
        if let Some(renaming) = &mut self.renaming {
            renaming.input.insert(&typed);
            return true;
        }
        if !self.typing {
            return false;
        }
        self.query.insert(&typed);
        self.searching();
        true
    }

    fn selected_text(&self) -> Option<String> {
        if let Some(renaming) = &self.renaming {
            let selected = renaming.input.selected_text();
            return (!selected.is_empty()).then(|| selected.to_owned());
        }
        let selected = self.query.selected_text();
        (self.typing && !selected.is_empty()).then(|| selected.to_owned())
    }

    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        // The agents' list open: its keys (↑ ↓ ↵, Esc closes it).
        if self.filter_list.is_open() {
            let event = self.filter_list.key(key, reach);
            return self.filter_list_event(event) || true;
        }
        // Renaming: ↵ gives the name, Esc leaves it as it was.
        if let Some(renaming) = &mut self.renaming {
            match key {
                ViewerKey::Enter => self.finish_rename(),
                ViewerKey::Escape => self.renaming = None,
                _ => return reach.edit(&mut renaming.input, key),
            }
            return true;
        }
        if !self.typing {
            return false;
        }
        let before = self.query.text().to_owned();
        let used = match key {
            // Esc: what's typed goes, then the keys.
            ViewerKey::Escape if before.is_empty() => {
                self.typing = false;
                true
            }
            ViewerKey::Escape => {
                self.query = TextInput::default();
                true
            }
            // ↵: the first session found, taken up again.
            ViewerKey::Enter => {
                if let Some(&i) = self.shown.first() {
                    self.resume(i, true);
                }
                true
            }
            _ => reach.edit(&mut self.query, key),
        };
        if self.query.text() != before {
            self.searching();
        }
        used
    }

    fn focus_lost(&mut self) {
        self.typing = false;
        self.finish_rename();
    }

    fn clickable(&self, p: Point) -> bool {
        self.row_at(p).is_some()
            || self.more_at(p).is_some()
            || self.star_at(p).is_some()
            || self.more_rect.is_some_and(|r| r.contains(p))
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        if self.clear.is_some_and(|r| r.contains(p)) {
            return Some(ViewerCursor::Pointer);
        }
        if self.field.contains(p) || self.renaming.as_ref().is_some_and(|n| n.field.contains(p)) {
            return Some(ViewerCursor::Text);
        }
        self.clickable(p).then_some(ViewerCursor::Pointer)
    }

    fn tooltip_at(&self, p: Point) -> Option<ui_components::tooltip::Tip> {
        if let Some((rect, _)) = self.more_button.filter(|(r, _)| r.contains(p)) {
            return Some(ui_components::tooltip::Tip::new(rect, t("agents.more_actions")));
        }
        // A star says what a click does.
        if let Some((rect, star)) = self.stars.iter().find(|(r, _)| r.contains(p)) {
            let key = if self.starred(*star) { "agents.unfavorite" } else { "agents.favorite" };
            return Some(ui_components::tooltip::Tip::new(*rect, t(key)));
        }
        let n = self.row_index(p)?;
        let label = match self.rows.get(n)? {
            Row::Session { i, .. } => self.sessions.get(*i)?.title.clone(),
            Row::Folder(f) => self.folders.get(*f)?.name.clone(),
            Row::Missing(_) => return None,
        };
        let rect = Rect::new(self.list.min_x(), self.row_top(n), self.list.size.width, self.row_h);
        Some(ui_components::tooltip::Tip::new(rect, label))
    }

    fn header(&self) -> Option<Vec<PanelButton>> {
        let button = |id, icon, key: &str| PanelButton { id, icon, tooltip: t(key), group: 0 };
        let mut buttons = vec![button(REFRESH, "refresh", "agents.refresh")];
        if self.project.is_some() {
            buttons.insert(0, button(NEW_FOLDER, "folder-plus", "agents.new_folder"));
        }
        // The filter: a full star while only the favorites are listed.
        let (icon, key) = if self.only_favorites {
            (crate::FAVORITE_ON_ICON, "agents.all_sessions")
        } else {
            (crate::FAVORITE_ICON, "agents.only_favorites")
        };
        buttons.insert(0, button(ONLY_FAVORITES, icon, key));
        let offered = match &self.agents {
            Some(agents) => !agents.is_empty(),
            None => agents::any(|_| true),
        };
        if self.root.is_some() && offered {
            buttons.insert(0, button(CHOOSE, "chevron-down", "agents.choose"));
            buttons.insert(0, button(NEW, "plus", "agents.new"));
        }
        Some(buttons)
    }

    fn header_action(&mut self, id: &str) -> Option<ToolPanelAction> {
        match id {
            ONLY_FAVORITES => {
                self.only_favorites = !self.only_favorites;
                self.shown_for = None;
                self.scroll = 0.0;
                None
            }
            REFRESH => {
                self.read_at = None;
                self.read();
                None
            }
            // The agent chosen starts; none installed: which one, from the list.
            NEW => match self.chosen() {
                Some(agent) => {
                    self.start(&agent);
                    self.action.take()
                }
                None => Some(ToolPanelAction::Menu(self.agents_menu())),
            },
            CHOOSE => Some(ToolPanelAction::Menu(self.agents_menu())),
            NEW_FOLDER => {
                self.new_folder(|_, _| {});
                None
            }
            _ => None,
        }
    }

    /// A session's own menu: open it as a conversation (when they're shown) or in a terminal.
    fn menu_at(&mut self, p: Point) -> Option<Vec<ViewMenuEntry>> {
        // A missing favorite's: taken off the list.
        if let Some(n) = self.missing_at(p) {
            self.menu_favorite = Some(self.missing[n].clone());
            self.menu_on = None;
            let off = ViewMenuEntry::item(FAVORITE, t("agents.unfavorite")).with_icon("x");
            return Some(vec![off]);
        }
        // A folder's: renamed, deleted; a new one beside it.
        if let Some(f) = self.folder_at(p) {
            self.menu_folder = self.folders.get(f).map(|f| f.id.clone());
            self.menu_on = None;
            return Some(vec![
                ViewMenuEntry::item(FOLDER_RENAME, t("agents.rename_folder"))
                    .with_icon("text-cursor"),
                ViewMenuEntry::item(FOLDER_DELETE, t("agents.delete_folder")).with_icon("trash"),
                ViewMenuEntry::Separator,
                ViewMenuEntry::item(NEW_FOLDER, t("agents.new_folder")).with_icon("folder-plus"),
            ]);
        }
        // Below the rows: a new folder.
        let Some(i) = self.row_at(p) else {
            return (self.project.is_some() && self.list.contains(p)).then(|| {
                vec![
                    ViewMenuEntry::item(NEW_FOLDER, t("agents.new_folder"))
                        .with_icon("folder-plus"),
                ]
            });
        };
        self.session_menu(i)
    }

    /// An edit command for its search while it's typed in (⌘A, ⌘X, ⌘Z, ⇧⌘Z).
    fn command(&mut self, id: &str) -> bool {
        if let Some(renaming) = &mut self.renaming {
            match id {
                "select_all" => renaming.input.select_all(),
                "cut" => renaming.input.insert(""),
                "undo" => {
                    renaming.input.undo();
                }
                "redo" => {
                    renaming.input.redo();
                }
                _ => return false,
            }
            return true;
        }
        if !self.typing {
            return false;
        }
        let edited = match id {
            "select_all" => {
                self.query.select_all();
                return true;
            }
            "cut" => {
                self.query.insert("");
                true
            }
            "undo" => self.query.undo(),
            "redo" => self.query.redo(),
            _ => return false,
        };
        if edited {
            self.searching();
        }
        true
    }

    fn menu_command(&mut self, id: &str) -> bool {
        if self.folder_command(id) {
            return true;
        }
        // A session's marks, its rename, what's copied of it.
        if let Some(what) = id.strip_prefix(COPY) {
            self.copy(what);
            return true;
        }
        if id == RENAME {
            if let Some(i) = self.menu_session() {
                self.start_rename(i);
            }
            return true;
        }
        if id == PIN || id == DONE || id == UNREAD || id.starts_with(LABEL) {
            let (Some(project), Some((agent, session))) = (self.project, self.menu_on.clone())
            else {
                return false;
            };
            let label = id.strip_prefix(LABEL).map(Label::from_key);
            crate::favorites::mark(project, agent, &session, |m| match label {
                Some(label) => m.label = label,
                None if id == PIN => m.pinned = !m.pinned,
                None if id == DONE => m.done = !m.done,
                None => m.unread = !m.unread,
            });
            return true;
        }
        // Added to the favorites, or taken off them.
        if id == FAVORITE {
            let (Some(project), Some(f)) = (self.project, self.menu_favorite.take()) else {
                return false;
            };
            crate::favorites::toggle(project, &f.agent, &f.id, &f.title);
            return true;
        }
        if let Some((agent, session)) =
            self.menu_on.take().filter(|_| id == OPEN_CHAT || id == OPEN_TERMINAL)
        {
            let at = self.sessions.iter().position(|s| s.agent == agent && s.id == session);
            if let Some(i) = at {
                self.resume(i, id == OPEN_TERMINAL);
            }
            return true;
        }
        let Some(agent) = id.strip_prefix(NEW_WITH) else { return false };
        let Some(agent) = self.agents().into_iter().find(|a| a.id() == agent && a.installed())
        else {
            return false;
        };
        self.start(&agent);
        true
    }

    fn take_action(&mut self) -> Option<ToolPanelAction> {
        self.action.take()
    }

    /// Its scrollbar fading: a frame when it next changes.
    fn frame_deadline(&self) -> Option<Instant> {
        self.bar.next_frame()
    }

    fn purge(&mut self) {
        self.read_at = None;
    }
}

/// The poison-proof lock (kept here for the title counter).
pub(crate) fn lock<T>(m: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
#[path = "../../tests/unit/panel.rs"]
mod tests;
