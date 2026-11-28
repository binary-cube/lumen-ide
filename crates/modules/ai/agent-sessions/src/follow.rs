//! The sessions with a terminal tab open, followed so their tabs keep their names: renamed in
//! the agent (`/rename`, a title it gives), the tab says so; a new session — no id yet when
//! started — takes the id of the file that appears for it, so taking it up again from the
//! list brings its tab to the front instead of starting it twice. Every agent alike: their
//! sessions are read through `agents::AgentProvider`, the tabs renamed through the terminal
//! (`RenameTerminal`), neither named here.
//!
//! A thread of its own (`agent-follow`) runs while some session is followed, reading the
//! agents' lists every few seconds (each agent keeps them by their files' stamps: a read
//! that finds nothing changed costs a folder's listing); what changed waits for the windows'
//! poll, which renames the tabs ([`take_renames`]). A tab gone is found by a rename refused:
//! then it's let go.
//!
//! A tab the user renames renames its session too, in the agent's own files — but only once
//! the agent no longer runs (it keeps its title in memory and writes it over the file as it
//! goes): until then the name waits ([`renamed`]); its program ended (or its tab closed), a
//! moment later it's written on a worker ([`due`]).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often the agents' lists are read while something is followed (each read costs a
/// folder's listing: the agents keep their sessions by their files' stamps).
const EVERY: Duration = Duration::from_secs(2);
/// Every so many reads, each followed tab is named again (unchanged): one gone is found.
const CHECK_EVERY: u32 = 10;
/// The most sessions followed (the oldest let go first).
const MOST: usize = 64;
/// A new session's file appears within this of its start (seconds).
const NEW_WITHIN: i64 = 10;

/// A session with a tab open.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Followed {
    /// Its tab's key.
    key: String,
    agent: &'static str,
    /// Its id (`None`: a new one, its file not seen yet).
    id: Option<String>,
    folder: PathBuf,
    /// When it was started (seconds since the epoch).
    since: i64,
    /// The name its tab has.
    title: String,
    /// The title the agent gave it at the last read: the tab renamed only when that changes
    /// (a name the user gave the tab is kept until the agent says another).
    said: Option<String>,
    /// When its agent last wrote to it, at the last read (seconds since the epoch; 0: not
    /// read yet).
    active: i64,
    /// What its terminal said at the last look ([`set_state`]): its memory, asleep.
    state: Option<contracts::sleep::SleepState>,
}

static FOLLOWED: Mutex<Vec<Followed>> = Mutex::new(Vec::new());
static RENAMES: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Counts what the follower saw change (a session given its id, a title the agent gave):
/// the panel reads its list again at once instead of at its next refresh.
static CHANGES: AtomicU64 = AtomicU64::new(0);

/// What the follower saw change so far (a panel compares it with the last it saw).
pub(crate) fn changes() -> u64 {
    CHANGES.load(Ordering::Acquire)
}

/// A session's new name, waiting for its agent to end.
#[derive(Debug, Clone)]
struct Pending {
    key: String,
    agent: &'static str,
    folder: PathBuf,
    id: Option<String>,
    title: String,
    /// When its program was found ended.
    ended: Option<std::time::Instant>,
}

static PENDING: Mutex<Vec<Pending>> = Mutex::new(Vec::new());
/// How long after its program ended a name is written (what it writes as it ends, written).
const GRACE: Duration = Duration::from_secs(2);
/// What wakes each open project's window (the newest asked; a closed one's let go).
type Wake = Arc<dyn Fn() + Send + Sync>;
static WAKES: Mutex<Vec<(kernel::ProjectId, Wake)>> = Mutex::new(Vec::new());

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// What wakes the app when a tab is to be renamed: project `project`'s window (as it attaches;
/// until [`unset_wake`], when it closes).
pub(crate) fn set_wake(project: kernel::ProjectId, wake: Wake) {
    let mut wakes = lock(&WAKES);
    wakes.retain(|(p, _)| *p != project);
    wakes.push((project, wake));
}

/// Project `project`'s window closed: not woken any more.
pub(crate) fn unset_wake(project: kernel::ProjectId) {
    lock(&WAKES).retain(|(p, _)| *p != project);
}

/// The tab of agent `agent`'s session `id`, if one is followed (a new session that took
/// that id keeps its own tab's key).
pub(crate) fn key_of(agent: &str, id: &str) -> Option<String> {
    lock(&FOLLOWED)
        .iter()
        .find(|f| f.agent == agent && f.id.as_deref() == Some(id))
        .map(|f| f.key.clone())
}

/// The tab of `session`, if one is followed: by its id, or — its file seen before the
/// follower's next read — the one new tab of its agent in its folder started just before
/// it, which takes its id now (taking it up again from the list brings that tab to the
/// front, never a second one). Several new tabs that could be its: none chosen here (the
/// follower sorts them in order).
pub(crate) fn tab_of(session: &agents::AgentSession) -> Option<String> {
    if let Some(key) = key_of(session.agent, &session.id) {
        return Some(key);
    }
    let started = session.started?;
    let mut followed = lock(&FOLLOWED);
    let mut new = followed.iter_mut().filter(|f| {
        f.id.is_none()
            && f.agent == session.agent
            && f.folder == session.folder
            && started >= f.since.saturating_sub(NEW_WITHIN)
    });
    let only = new.next()?;
    if new.next().is_some() {
        return None;
    }
    only.id = Some(session.id.clone());
    Some(only.key.clone())
}

/// A session's tab followed from now on (`id`: `None` for a new one); one followed already
/// keeps what its terminal told of it.
pub(crate) fn follow(
    key: String,
    agent: &'static str,
    id: Option<String>,
    folder: PathBuf,
    title: String,
) {
    {
        let mut followed = lock(&FOLLOWED);
        // Followed already (its session taken up again from the list): what its terminal
        // told stays (asleep, open) — only its terminal's event changes it.
        let state = followed.iter().find(|f| f.key == key).and_then(|f| f.state);
        followed.retain(|f| f.key != key);
        if followed.len() >= MOST {
            followed.remove(0);
        }
        let said = id.is_some().then(|| title.clone());
        followed.push(Followed {
            key,
            agent,
            id,
            folder,
            since: now(),
            title,
            said,
            active: 0,
            state,
        });
    }
    if !RUNNING.swap(true, Ordering::AcqRel) {
        // A thread of its own: it waits between reads for as long as something is followed
        // (a pool's would be held all that while), and ends when nothing is.
        #[allow(clippy::disallowed_methods)]
        let spawned = std::thread::Builder::new().name("agent-follow".into()).spawn(run);
        if spawned.is_err() {
            RUNNING.store(false, Ordering::Release);
        }
    }
}

/// The agent of the session whose tab is `key`, its name there set to `title` (renamed by
/// the user: kept until the agent gives the session another title).
pub(crate) fn renamed(key: &str, title: &str) -> Option<&'static str> {
    let pending = {
        let mut followed = lock(&FOLLOWED);
        let f = followed.iter_mut().find(|f| f.key == key)?;
        title.clone_into(&mut f.title);
        Pending {
            key: key.to_owned(),
            agent: f.agent,
            folder: f.folder.clone(),
            id: f.id.clone(),
            title: title.to_owned(),
            ended: None,
        }
    };
    let agent = pending.agent;
    let mut waiting = lock(&PENDING);
    waiting.retain(|p| p.key != key);
    if waiting.len() >= MOST {
        waiting.remove(0);
    }
    waiting.push(pending);
    Some(agent)
}

/// Tab `key` named `title` at the windows' next poll (a session renamed in the panel), the
/// windows woken for it.
pub(crate) fn rename_tab(key: String, title: String) {
    lock(&RENAMES).push((key, title));
    let wakes: Vec<Wake> = lock(&WAKES).iter().map(|(_, w)| Arc::clone(w)).collect();
    for wake in wakes {
        wake();
    }
}

/// Some session's name waits for its agent to end.
pub(crate) fn any_pending() -> bool {
    !lock(&PENDING).is_empty()
}

/// The names to write now (agent, folder, id, title): those whose program ended (`runs`
/// asked of each tab by its key) a moment ago, their ids as known by now. One whose session
/// never got an id (it never wrote a file) is let go.
pub(crate) fn due(
    runs: &mut dyn FnMut(&str) -> bool,
) -> Vec<(&'static str, PathBuf, String, String)> {
    let keys: Vec<(String, bool)> =
        lock(&PENDING).iter().map(|p| (p.key.clone(), p.ended.is_none())).collect();
    // Asked outside the lock (it reaches the terminal).
    let ended: Vec<String> =
        keys.into_iter().filter(|(k, open)| *open && !runs(k)).map(|(k, _)| k).collect();
    let now = std::time::Instant::now();
    let ids: Vec<(String, Option<String>)> =
        lock(&FOLLOWED).iter().map(|f| (f.key.clone(), f.id.clone())).collect();
    let mut waiting = lock(&PENDING);
    let mut out = Vec::new();
    waiting.retain_mut(|p| {
        if ended.contains(&p.key) {
            p.ended = Some(now);
        }
        if p.id.is_none() {
            p.id = ids.iter().find(|(k, _)| *k == p.key).and_then(|(_, id)| id.clone());
        }
        let Some(at) = p.ended else { return true };
        if now.duration_since(at) < GRACE {
            return true;
        }
        if let Some(id) = p.id.take() {
            out.push((p.agent, std::mem::take(&mut p.folder), id, std::mem::take(&mut p.title)));
        }
        false
    });
    out
}

/// How the session whose tab is `key` resumes, for its terminal to sleep: `None` when it
/// isn't followed, has no id yet (it never wrote a file), its agent is gone, or its file was
/// written within `idle` (a turn may run) or never read.
pub(crate) fn resume_launch(key: &str, idle: Duration) -> Option<agents::Launch> {
    let f = lock(&FOLLOWED).iter().find(|f| f.key == key)?.clone();
    let id = f.id?;
    let quiet = now().saturating_sub(f.active);
    let idle = i64::try_from(idle.as_secs()).unwrap_or(i64::MAX);
    if f.active <= 0 || quiet < idle {
        return None;
    }
    let session = agents::AgentSession {
        agent: f.agent,
        id,
        title: f.title,
        folder: f.folder,
        started: None,
        active: f.active,
        file: None,
    };
    Some(agents::provider(f.agent)?.resume(&session))
}

/// When the sessions' panel was last drawn (what their terminals hold is asked only while
/// it's shown), and for how long after that it still is.
static PANEL_DRAWN: Mutex<Option<std::time::Instant>> = Mutex::new(None);
const PANEL_SHOWN_FOR: Duration = Duration::from_secs(30);

/// The sessions' panel drawn just now.
pub(crate) fn panel_drawn() {
    *lock(&PANEL_DRAWN) = Some(std::time::Instant::now());
}

/// Whether the sessions' panel was drawn lately (its rows show what the terminals hold).
pub(crate) fn panel_shown() -> bool {
    lock(&PANEL_DRAWN).is_some_and(|at| at.elapsed() < PANEL_SHOWN_FOR)
}

/// The keys of the sessions' tabs followed now.
pub(crate) fn keys() -> Vec<String> {
    lock(&FOLLOWED).iter().map(|f| f.key.clone()).collect()
}

/// What the terminal of tab `key` said (its memory, asleep): kept for the panel's rows.
/// `true` if it changed.
pub(crate) fn set_state(key: &str, state: Option<contracts::sleep::SleepState>) -> bool {
    let mut followed = lock(&FOLLOWED);
    let Some(f) = followed.iter_mut().find(|f| f.key == key) else { return false };
    let changed = f.state != state;
    f.state = state;
    changed
}

/// The projects whose panel has a terminal's state to draw since it last did ([`told`]).
static TOLD: Mutex<Vec<kernel::ProjectId>> = Mutex::new(Vec::new());

/// What a terminal published on project `project`'s bus of how it stands
/// (`TerminalStateChanged`; `None`: gone), kept for its tab's row and menus; that project's
/// panel draws again ([`take_told`]).
pub(crate) fn told(
    project: kernel::ProjectId,
    key: &str,
    state: Option<contracts::sleep::SleepState>,
) {
    set_state(key, state);
    let mut told = lock(&TOLD);
    if !told.contains(&project) {
        told.push(project);
    }
}

/// Whether a terminal's state was told to project `project` since this was last asked
/// (taken: each project's panel its own).
pub(crate) fn take_told(project: Option<kernel::ProjectId>) -> bool {
    let Some(project) = project else { return false };
    let mut told = lock(&TOLD);
    let before = told.len();
    told.retain(|p| *p != project);
    told.len() != before
}

/// Tab `key` just asked to sleep or to wake (`to`: `Sleeping` or `Waking`): its menus say
/// so at once, before the sleeping tells how it went (its `SleepStateChanged`).
pub(crate) fn expect(key: &str, to: contracts::sleep::CanSleep) {
    let mut followed = lock(&FOLLOWED);
    let Some(f) = followed.iter_mut().find(|f| f.key == key) else { return };
    let state = f.state.get_or_insert_with(Default::default);
    state.can_sleep = to;
}

/// What the terminal of agent `agent`'s session `id` said last, if it has a tab followed.
pub(crate) fn state_of(agent: &str, id: &str) -> Option<contracts::sleep::SleepState> {
    lock(&FOLLOWED)
        .iter()
        .find(|f| f.agent == agent && f.id.as_deref() == Some(id))
        .and_then(|f| f.state)
}

/// Tab `key` gone: not followed any more.
pub(crate) fn forget(key: &str) {
    lock(&FOLLOWED).retain(|f| f.key != key);
}

/// The tabs to rename, taken (by a window's poll).
pub(crate) fn take_renames() -> Vec<(String, String)> {
    std::mem::take(&mut *lock(&RENAMES))
}

/// What one read of the agents' lists changes in `followed` (in the order they were
/// started): new sessions given their ids, renamed ones their titles; the tabs to rename (all
/// of them when `check`). Each agent's folder is listed once.
fn update(
    followed: &mut [Followed],
    sessions: &dyn Fn(&'static str, &std::path::Path) -> Vec<agents::AgentSession>,
    check: bool,
) -> Vec<(String, String)> {
    let mut renames = Vec::new();
    let taken: Vec<(&'static str, String)> =
        followed.iter().filter_map(|f| Some((f.agent, f.id.clone()?))).collect();
    let mut adopted: Vec<(&'static str, String)> = Vec::new();
    let mut lists: Vec<((&'static str, PathBuf), Vec<agents::AgentSession>)> = Vec::new();
    for f in followed.iter_mut() {
        let at = match lists.iter().position(|((a, d), _)| *a == f.agent && *d == f.folder) {
            Some(at) => at,
            None => {
                lists.push(((f.agent, f.folder.clone()), sessions(f.agent, &f.folder)));
                lists.len() - 1
            }
        };
        let listed = &lists[at].1;
        let found = match &f.id {
            Some(id) => listed.iter().find(|s| &s.id == id),
            // A new one: the first session started since it was, no other tab's (the tabs
            // started first take the sessions started first).
            None => listed
                .iter()
                .filter(|s| {
                    s.started.is_some_and(|at| at >= f.since.saturating_sub(NEW_WITHIN))
                        && !taken.contains(&(s.agent, s.id.clone()))
                        && !adopted.contains(&(s.agent, s.id.clone()))
                })
                .min_by_key(|s| s.started),
        };
        let Some(session) = found else {
            if check {
                renames.push((f.key.clone(), f.title.clone()));
            }
            continue;
        };
        if f.id.is_none() {
            f.id = Some(session.id.clone());
            adopted.push((session.agent, session.id.clone()));
        }
        f.active = session.active;
        if f.said.as_ref() != Some(&session.title) {
            f.said = Some(session.title.clone());
            if session.title != f.title {
                f.title.clone_from(&session.title);
                renames.push((f.key.clone(), f.title.clone()));
            }
        } else if check {
            renames.push((f.key.clone(), f.title.clone()));
        }
    }
    renames
}

/// The follower's loop: while something is followed.
fn run() {
    let mut reads = 0u32;
    loop {
        std::thread::sleep(EVERY);
        let before = lock(&FOLLOWED).clone();
        let mut followed = before.clone();
        // A name waiting for its agent to end: the window polled (it looks, and writes it).
        if any_pending() {
            let wake = lock(&WAKES).last().map(|(_, w)| Arc::clone(w));
            if let Some(wake) = wake {
                wake();
            }
        }
        if followed.is_empty() && !any_pending() {
            RUNNING.store(false, Ordering::Release);
            // Followed again meanwhile: go on (else a new follower is started by it).
            if (lock(&FOLLOWED).is_empty() && !any_pending())
                || RUNNING.swap(true, Ordering::AcqRel)
            {
                return;
            }
            continue;
        }
        reads = reads.wrapping_add(1);
        let sessions = |agent: &'static str, folder: &std::path::Path| {
            agents::provider(agent).map(|a| a.sessions(folder, 50)).unwrap_or_default()
        };
        let renames = update(&mut followed, &sessions, reads.is_multiple_of(CHECK_EVERY));
        // What it learned kept (a session forgotten meanwhile stays forgotten; a tab the user
        // renamed meanwhile keeps that name unless this read renamed it).
        {
            let mut kept = lock(&FOLLOWED);
            for f in kept.iter_mut() {
                let Some(read) = followed.iter().find(|r| r.key == f.key) else { continue };
                let was = before.iter().find(|b| b.key == f.key);
                f.id.clone_from(&read.id);
                f.said.clone_from(&read.said);
                f.active = read.active;
                if was.is_some_and(|b| b.title != read.title) {
                    f.title.clone_from(&read.title);
                }
            }
        }
        // A session found or renamed in its agent: the panel shows it now.
        let changed =
            followed.iter().zip(&before).any(|(now, was)| now.id != was.id || now.said != was.said);
        if changed {
            CHANGES.fetch_add(1, Ordering::AcqRel);
        }
        if renames.is_empty() && !changed {
            continue;
        }
        lock(&RENAMES).extend(renames);
        let wake = lock(&WAKES).last().map(|(_, w)| Arc::clone(w));
        if let Some(wake) = wake {
            wake();
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/follow.rs"]
mod tests;
