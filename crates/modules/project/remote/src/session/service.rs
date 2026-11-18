//! The remote project followed: a project service starting its session, its events turned into
//! transfers, the questions asked (removals on the server, conflicts).

use super::*;

impl ProjectService for RemoteService {
    fn describe(&self, root: &Path) -> Option<ProjectKind> {
        let saved = config::find(root)?;
        let c = &saved.connection;
        let protocol = i18n::t(&format!("remote.protocol_{}", c.protocol.id().replace('-', "_")));
        let (icon, key) = match c.mode {
            remote::Mode::Mirror => ("refresh", "remote.kind_synced"),
            remote::Mode::OnDemand => ("globe", "remote.kind_remote"),
        };
        Some(ProjectKind {
            icon,
            label: i18n::tr!(key, protocol = protocol.as_str()),
            detail: Some(c.address()),
            edit: Some(workbench_model::ProjectEdit {
                label: i18n::t("remote.edit_current"),
                menu_label: i18n::t("remote.edit_row"),
                icon: crate::EDIT_ICON,
            }),
        })
    }

    fn edit(&self, host: &mut dyn WorkbenchHost, root: &Path) -> bool {
        crate::edit_connection(host, root)
    }

    fn listed(&self, dir: &Path) -> Option<Vec<workspace::DirEntry>> {
        let (state, relative) = within(dir)?;
        // Mirrored, everything is on disk: only the marks of what's there.
        if !state.on_demand() && !dir.is_dir() {
            return None;
        }
        Some(state.known(&relative, dir))
    }

    fn fetch(&self, path: &Path) -> Option<std::sync::mpsc::Receiver<Result<(), String>>> {
        let (state, relative) = on_demand(path)?;
        if relative.is_empty() || state.saved.connection.is_excluded(&relative) {
            return None;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        state.fetch(relative, tx);
        Some(rx)
    }

    fn attach(&self, project: &ProjectContext<'_>) -> Vec<Subscription> {
        // A local project: nothing runs, nothing is loaded.
        let Some(saved) = config::find(project.root) else { return Vec::new() };
        // A connection that came with the project (not made here): asked first.
        if !config::approved(&saved) {
            return wait_for_approval(project, saved);
        }
        let secret = config::secret_store(&saved).get(&saved.connection.id);
        let connector = connector(&saved, secret, None);
        start(project, saved, connector)
    }

    fn busy(&self, id: ProjectId) -> bool {
        session(id).is_some_and(|state| state.outgoing.busy())
    }

    fn poll(&self, host: &mut dyn WorkbenchHost) -> bool {
        let (Some(root), Some(id)) = (host.project_root(), host.project_id()) else {
            return false;
        };
        if ask_approval(id, host) {
            return true;
        }
        // Its connection edited (here or in another window): reconnected with it.
        if super::reconnect::poll(host, id, &root) {
            return true;
        }
        let Some(state) = session(id) else { return false };
        // Folders of the server listed meanwhile: the tree shows them.
        let relisted: Vec<String> =
            state.relisted.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default();
        for dir in &relisted {
            let path = if dir.is_empty() { root.clone() } else { root.join(dir) };
            host.relist(&path);
        }
        // The server took fewer connections at once than asked: said once.
        if let Some(count) = state.pool.as_ref().and_then(|p| p.take_lowered()) {
            host.notify(workbench_model::Notification::warning(i18n::tr!(
                "remote.connections_lowered",
                count = count
            )));
        }
        // The status bar: what runs and waits to upload (a click: the log).
        let now = state.now();
        let shown = (now.running, now.to_upload);
        let moved = state.shown.lock().is_ok_and(|mut s| s.replace(shown) != Some(shown));
        if moved {
            let progress = (shown != (0, 0)).then(|| workbench_model::Progress {
                label: i18n::tr!("remote.status", running = now.running, waiting = now.to_upload),
                icon: "server",
                // What waits: the list of it; else the log.
                stop: Some(if now.to_upload > 0 { crate::SHOW_PENDING } else { crate::SHOW_LOG }),
            });
            host.set_progress("remote", progress);
        }
        // The button's count: what the "To upload" tab lists.
        let badge = state.outgoing.summary();
        let recounted = badge.is_some()
            && state.badge.lock().is_ok_and(|mut b| std::mem::replace(&mut *b, badge) != badge);
        // Something failed: the error's color; a conflict: the warning's; else what waits,
        // the accent's.
        if let Some((count, kind)) = badge.filter(|_| recounted) {
            let label = i18n::tr!("remote.pending_count", count = count);
            let badge = workbench_model::ToolBadge { count, kind, label };
            host.set_tool_badge(crate::TOOL, Some(badge));
        }
        let notices = state.take_notices();
        let changed = moved || recounted || !notices.is_empty() || !relisted.is_empty();
        for notice in notices {
            match notice {
                Notice::Message(message) => host.notify(message),
                Notice::Conflict { path, relative, local, server } => {
                    ask_conflict(host, id, path, relative, local, server);
                }
                Notice::ConfirmRemove(paths) => ask_remove(host, id, paths),
                Notice::Compare { path, relative, local, server } => {
                    compare(host, path, &relative, local, server, false);
                }
                Notice::ConfirmDiscard(rows) => ask_discard(host, id, rows),
                Notice::Exclude(paths) => exclude(host, &root, paths),
                Notice::BulkConflicts { conflicts, rest } => {
                    ask_bulk_conflicts(host, id, conflicts, rest);
                }
            }
        }
        changed
    }
}

/// The session of a remote project (its connection `saved`, reached with `connector`):
/// its state, its thread, its subscriptions to the project's events (returned: they live as
/// long as the project).
/// A project's connection waiting for the user's approval: what starting it needs, and where
/// its subscriptions go once it starts (held by the project, through [`wait_for_approval`]'s
/// guard).
struct Waiting {
    id: ProjectId,
    root: PathBuf,
    storage: PathBuf,
    events: kernel::EventBus,
    wake: Arc<dyn Fn() + Send + Sync>,
    settings: Option<Arc<settings_model::SettingsService>>,
    language_of: workbench_model::LanguageOf,
    quality_tools: Vec<Arc<dyn code_quality::QualityTool>>,
    text_formatters: Vec<Arc<dyn code_quality::TextFormatter>>,
    notifier: Arc<dyn workbench_model::Notifier>,
    marks: workspace::marks::ProjectMarks,
    saved: Saved,
    started: Weak<Mutex<Vec<Subscription>>>,
    /// The question was asked (once per opening).
    asked: bool,
}

static WAITING: Mutex<Vec<Waiting>> = Mutex::new(Vec::new());

/// Nothing runs yet: the question is asked at the next poll. The returned guard lives as long
/// as the project; with it go the subscriptions of a session started later.
fn wait_for_approval(project: &ProjectContext<'_>, saved: Saved) -> Vec<Subscription> {
    let started: Arc<Mutex<Vec<Subscription>>> = Arc::default();
    if let Ok(mut waiting) = WAITING.lock() {
        waiting.retain(|w| w.started.strong_count() > 0);
        waiting.push(Waiting {
            id: project.id,
            root: project.root.to_path_buf(),
            storage: project.storage.to_path_buf(),
            events: project.events.clone(),
            wake: Arc::clone(&project.wake),
            settings: project.settings.clone(),
            language_of: Arc::clone(&project.language_of),
            quality_tools: project.quality_tools.clone(),
            text_formatters: project.text_formatters.clone(),
            notifier: Arc::clone(&project.notifier),
            marks: project.marks.clone(),
            saved,
            started: Arc::downgrade(&started),
            asked: false,
        });
    }
    (project.wake)();
    let guard = project.events.subscribe(move |_: &workspace::events::BulkChangeEnded| {
        let _held = &started;
    });
    // Its question goes with the project (and with it the project's bus, settings…).
    let id = project.id;
    let asked = Subscription::on_end(move || {
        if let Ok(mut waiting) = WAITING.lock() {
            waiting.retain(|w| w.id != id);
        }
    });
    vec![guard, asked]
}

/// Asks once whether project `id`'s connection may run; "yes" allows it (kept) and starts it.
/// `true` if it asked.
fn ask_approval(id: ProjectId, host: &mut dyn WorkbenchHost) -> bool {
    let saved = {
        let Ok(mut waiting) = WAITING.lock() else { return false };
        let Some(w) = waiting.iter_mut().find(|w| w.id == id && !w.asked) else { return false };
        w.asked = true;
        w.saved.clone()
    };
    let c = &saved.connection;
    let question = workbench_model::Question {
        title: i18n::t("dialog.remote_approve_title"),
        message: i18n::tr!(
            "dialog.remote_approve",
            address = c.address().as_str(),
            protocol = c.protocol.id()
        ),
        icon: "server",
        yes: i18n::t("dialog.remote_approve_yes"),
        danger: false,
        list: Vec::new(),
    };
    host.confirm(question, Box::new(move |_host| approve_and_start(id)))
}

/// Project `id` waiting for the user's approval: started now with `saved` (edited, which
/// approved it). `false`: it isn't waiting.
pub(super) fn start_waiting(id: ProjectId, saved: &Saved) -> bool {
    let found = WAITING.lock().is_ok_and(|mut waiting| {
        let Some(w) = waiting.iter_mut().find(|w| w.id == id) else { return false };
        w.saved = saved.clone();
        // Not asked any more: the edit was the answer.
        w.asked = true;
        true
    });
    if found {
        approve_and_start(id);
    }
    found
}

/// The project at `root` waiting for the user's approval: its identity and its waker.
pub(super) fn waiting_at(root: &Path) -> Option<(ProjectId, Arc<dyn Fn() + Send + Sync>)> {
    let waiting = WAITING.lock().ok()?;
    let w = waiting.iter().find(|w| w.root == root && w.started.strong_count() > 0)?;
    Some((w.id, Arc::clone(&w.wake)))
}

/// Project `id`'s connection is waiting for the user's approval.
pub(crate) fn is_waiting(id: ProjectId) -> bool {
    WAITING.lock().is_ok_and(|waiting| waiting.iter().any(|w| w.id == id))
}

fn approve_and_start(id: ProjectId) {
    let Some(w) = WAITING.lock().ok().and_then(|mut waiting| {
        let at = waiting.iter().position(|w| w.id == id)?;
        Some(waiting.remove(at))
    }) else {
        return;
    };
    let Some(started) = w.started.upgrade() else { return };
    config::approve(&w.saved);
    let project = ProjectContext {
        marks: w.marks.clone(),
        id: w.id,
        root: &w.root,
        storage: &w.storage,
        events: &w.events,
        wake: Arc::clone(&w.wake),
        settings: w.settings.clone(),
        language_of: Arc::clone(&w.language_of),
        quality_tools: w.quality_tools.clone(),
        text_formatters: w.text_formatters.clone(),
        notifier: Arc::clone(&w.notifier),
    };
    let secret = config::secret_store(&w.saved).get(&w.saved.connection.id);
    let connector = connector(&w.saved, secret, None);
    let subscriptions = start(&project, w.saved.clone(), connector);
    if let Ok(mut held) = started.lock() {
        held.extend(subscriptions);
    }
    (w.wake)();
}

/// Starts project `project`'s session (its connection `saved`, reached with `connector`),
/// restartable with other settings ([`super::reconnect`]): the subscriptions returned hold
/// it, as long as the project.
pub(super) fn start(
    project: &ProjectContext<'_>,
    saved: Saved,
    connector: Arc<dyn Connector>,
) -> Vec<Subscription> {
    let place = Place {
        id: project.id,
        root: project.root.to_path_buf(),
        events: project.events.clone(),
        wake: Arc::clone(&project.wake),
    };
    let held = run(&place, saved, connector);
    super::reconnect::hold(place, held)
}

/// What a session needs of its project: its identity, its root, its events, its waker.
#[derive(Clone)]
pub(super) struct Place {
    pub id: ProjectId,
    pub root: PathBuf,
    pub events: kernel::EventBus,
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

/// The session itself: its state, its thread, its queues, its subscriptions to the
/// project's events (returned: the session lives as long as they do).
// Its own thread: the session's jobs, one after another, for the session's lifetime.
#[allow(clippy::disallowed_methods)]
pub(super) fn run(
    project: &Place,
    saved: Saved,
    connector: Arc<dyn Connector>,
) -> Vec<Subscription> {
    // Every connection of the project from one pool: kept, shared, limited.
    let limits = saved.connection.limits(crate::settings::defaults().limits);
    let pool = pool_for(&saved, connector, limits.uploads + limits.downloads);
    let pooled: Arc<dyn Connector> = Arc::new(crate::pool::Pooled(Arc::clone(&pool)));
    // Files written take the owner and group asked for; a refusal said once (the state
    // made below: reached through `told`).
    let told: Arc<std::sync::OnceLock<Weak<State>>> = Arc::default();
    let refused = {
        let told = Arc::clone(&told);
        let said = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Arc::new(move |path: &str, e: &remote::RemoteError| {
            tracing::warn!("can't set who {path} belongs to: {e}");
            if said.swap(true, std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            if let Some(state) = told.get().and_then(Weak::upgrade) {
                state.tell(Notice::Message(workbench_model::Notification::warning(i18n::tr!(
                    "remote.write_as_refused",
                    reason = e.to_string().as_str()
                ))));
            }
        })
    };
    let connector =
        remote::ownership::OwnedWrites::wrap(pooled, saved.connection.write_as.clone(), refused);
    let state = Arc::new(State::pooled(
        project.id,
        saved,
        connector,
        Some(pool),
        Arc::clone(&project.wake),
        project.events.clone(),
    ));
    let _ = told.set(Arc::downgrade(&state));
    // In a git repository, the project's data (the manifest, the encrypted secrets) must
    // stay out of commits.
    if !data_ignored(&project.root) {
        state.tell(Notice::Message(workbench_model::Notification::warning(i18n::tr!(
            "remote.not_ignored",
            dir = kernel::brand::PROJECT_DIR
        ))));
    }
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.retain(|(.., s)| s.strong_count() > 0);
        sessions.push((project.id, project.root.clone(), Arc::downgrade(&state)));
    }
    let (tx, rx) = std::sync::mpsc::channel::<Job>();
    if let Ok(mut jobs) = state.jobs.lock() {
        *jobs = Some(tx.clone());
    }
    // Held weakly: the state goes with the project (its subscriptions, its views), its
    // senders with it — and the channel ending, the thread ends.
    let weak = Arc::downgrade(&state);
    let spawned = std::thread::Builder::new().name("remote-sync".into()).spawn(move || {
        let mut session = None;
        for job in rx {
            let Some(worker) = weak.upgrade() else { break };
            match job {
                Job::UploadMine(relative) => worker.decide(&relative, Action::Upload, &mut session),
                Job::TakeServer(relative) => {
                    worker.decide(&relative, Action::Download, &mut session);
                }
                Job::Compare(here, there) => worker.compare(&here, &there, &mut session),
                Job::Discard(rows) => worker.discard_now(&rows, &mut session),
                Job::List(dir) => worker.list(&dir, &mut session),
                Job::Rename(from, to) => {
                    worker.rename_on_server(&from, &to, &mut session);
                }
                Job::SendMove(here, there) => worker.send_move(&here, &there, &mut session),
                Job::Remove(paths) => worker.remove_on_server(&paths, &mut session),
            }
            // Back to the pool between jobs (kept open there for the next).
            session = None;
        }
    });
    if let Err(e) = crate::transfers::start(&state) {
        tracing::error!("the remote transfers didn't start: {e}");
        return Vec::new();
    }
    if let Err(e) = spawned {
        tracing::error!("the remote session didn't start: {e}");
        return Vec::new();
    }
    drop(tx);
    let root = project.root.clone();
    let held = Arc::clone(&state);
    // A save: to upload (saved again before it went: once) — or, uploads waiting for the
    // user, listed (the list reads it again).
    let saved = project.events.subscribe(move |e: &document::events::FileSaved| {
        let Some(relative) = version_relative(&root, &e.path) else { return };
        if held.saved.connection.is_excluded(&relative)
            || relative.starts_with(kernel::brand::PROJECT_DIR)
        {
            return;
        }
        // Marked as changed here until it's up.
        held.relist_around(std::iter::once(relative.as_str()));
        if held.auto_upload() {
            held.uploads.push(relative);
        }
    });
    // Moves and deletions made here (when they follow): what the server got of them.
    let (root_moved, root_deleted) = (project.root.clone(), project.root.clone());
    let (on_moved, on_deleted) = (Arc::clone(&state), Arc::clone(&state));
    // Uploads waiting for the user: kept in the outbox, sent when the user says (whatever
    // the moves' option: nothing goes by itself).
    let moved = project.events.subscribe(move |e: &workspace::events::FileRenamed| {
        let state = &on_moved;
        let (Some(from), Some(to)) =
            (version_relative(&root_moved, &e.from), version_relative(&root_moved, &e.to))
        else {
            return;
        };
        if !state.auto_upload() {
            // Into an excluded folder: deleted, for the server; out of one: new.
            match (state.travels(&from), state.travels(&to)) {
                (true, true) => state.note_moved(&from, &to),
                (true, false) => state.note_removed(&[from]),
                _ => {}
            }
            return;
        }
        if state.saved.connection.moves
            && state.travels(&from)
            && state.travels(&to)
            && !state.sent_under(&from).is_empty()
        {
            state.send_job(Job::Rename(from, to));
        }
    });
    let deleted = project.events.subscribe(move |e: &workspace::events::EntriesDeleted| {
        let state = &on_deleted;
        let paths: Vec<String> = e
            .paths
            .iter()
            .filter_map(|p| version_relative(&root_deleted, p))
            .filter(|p| state.travels(p))
            .collect();
        if !state.auto_upload() {
            state.note_removed(&paths);
            return;
        }
        if !state.saved.connection.moves {
            return;
        }
        let there: Vec<String> =
            paths.into_iter().filter(|p| !state.sent_under(p).is_empty()).collect();
        if !there.is_empty() {
            // Hard to take back on the server: asked first.
            state.tell(Notice::ConfirmRemove(there));
        }
    });
    // Many entries changing at once (a paste): the uploads wait for its end, then go as one.
    let (started, ended) = (state.uploads.clone(), state.uploads.clone());
    crate::outgoing::refresh(&state);
    vec![
        saved,
        moved,
        deleted,
        recount::<remote::events::TransfersChanged>(&state, &project.events),
        recount::<document::events::FileSaved>(&state, &project.events),
        recount::<workspace::events::FilesWritten>(&state, &project.events),
        recount::<workspace::events::EntriesChanged>(&state, &project.events),
        recount::<workspace::events::EntriesDeleted>(&state, &project.events),
        recount::<workspace::events::FileRenamed>(&state, &project.events),
        recount::<workspace::events::BulkChangeEnded>(&state, &project.events),
        project
            .events
            .subscribe(move |e: &workspace::events::BulkChangeStarted| started.hold(e.id)),
        project.events.subscribe(move |e: &workspace::events::BulkChangeEnded| ended.release(e.id)),
    ]
}

/// What's to upload read again when `E` happens (what runs and waits, a save, the disk).
pub(super) fn recount<E: kernel::DomainEvent>(
    state: &Arc<State>,
    events: &kernel::EventBus,
) -> Subscription {
    let state = Arc::downgrade(state);
    events.subscribe(move |_: &E| {
        if let Some(state) = state.upgrade() {
            crate::outgoing::refresh(&state);
        }
    })
}

/// The session `path` is in, and `path` relative to its root (`""`: the root).
pub(super) fn within(path: &Path) -> Option<(Arc<State>, String)> {
    let sessions = SESSIONS.lock().ok()?;
    // Which project a file is in: a question about the disk, so by path.
    let (root, state) = sessions.iter().find_map(|(_, root, s)| {
        let state = s.upgrade()?;
        path.starts_with(root).then(|| (root.clone(), state))
    })?;
    drop(sessions);
    let relative = if path == root { String::new() } else { version_relative(&root, path)? };
    Some((state, relative))
}

/// The on-demand session `path` is in, and `path` relative to its root (`""`: the root).
pub(super) fn on_demand(path: &Path) -> Option<(Arc<State>, String)> {
    within(path).filter(|(state, _)| state.on_demand())
}

/// The folder holding `path` (relative; `""`: the root).
pub(super) fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// The server's `entry` known (in its folder's listing, if that was listed), with the
/// folders above it.
pub(super) fn learn(listings: &mut HashMap<String, Vec<RemoteEntry>>, entry: RemoteEntry) {
    let mut entry = entry;
    loop {
        let dir = parent(&entry.path).to_owned();
        if let Some(entries) = listings.get_mut(&dir) {
            match entries.iter_mut().find(|e| e.path == entry.path) {
                Some(known) if known.is_dir => {}
                Some(known) => *known = entry,
                None => entries.push(entry),
            }
        }
        if dir.is_empty() {
            break;
        }
        entry = RemoteEntry { path: dir, is_dir: true, size: 0, modified: 0 };
    }
}

/// Asks before deleting on the server what was deleted here (`paths`, relative): how many,
/// and which (one named in the message; many listed under it, every one of them).
pub(super) fn ask_remove(host: &mut dyn WorkbenchHost, id: ProjectId, paths: Vec<String>) {
    let (message, list) = match paths.as_slice() {
        [one] => (i18n::tr!("remote.remove_one", name = format!("**{one}**")), Vec::new()),
        many => (i18n::tr!("remote.remove_many", count = many.len()), many.to_vec()),
    };
    let question = workbench_model::Question {
        title: i18n::t("remote.remove_title"),
        message,
        icon: "trash",
        yes: i18n::t("remote.remove_yes"),
        danger: true,
        list,
    };
    host.confirm(
        question,
        Box::new(move |_| {
            if let Some(state) = session(id) {
                state.send_job(Job::Remove(paths));
            }
        }),
    );
}

/// A save that wasn't uploaded (the server's copy changed since the last transfer): what
/// now — compare them, upload ours anyway, or take the server's.
pub(super) fn ask_conflict(
    host: &mut dyn WorkbenchHost,
    id: ProjectId,
    path: PathBuf,
    relative: String,
    local: String,
    server: String,
) {
    use workbench_model::{ChoiceItem, ChoiceList};
    let item = |icon, label: &str, detail: &str| ChoiceItem {
        icon,
        label: i18n::t(label),
        detail: i18n::t(detail),
    };
    let list = ChoiceList {
        title: i18n::t("remote.conflict_title"),
        subject: relative.clone(),
        icon: "server",
        action: i18n::t("remote.conflict_action"),
        items: vec![
            item("compare", "remote.conflict_compare", "remote.conflict_compare_detail"),
            item("upload", "remote.conflict_mine", "remote.conflict_mine_detail"),
            item("download", "remote.conflict_server", "remote.conflict_server_detail"),
        ],
    };
    host.choose(
        list,
        Box::new(move |chosen, host| {
            let Some(state) = session(id) else { return };
            match chosen {
                0 => {
                    if let Ok(mut resolving) = state.resolving.lock() {
                        resolving.insert(relative.clone());
                    }
                    compare(host, path, &relative, local, server, true);
                }
                1 => state.send_job(Job::UploadMine(relative)),
                _ => state.send_job(Job::TakeServer(relative)),
            }
        }),
    );
}

/// An upload of several stopped before anything went: `conflicts` changed on the server
/// too. Resolve them first (the first one's question now, the others from the list), or
/// upload the `rest` now (the conflicts stay listed, decided one by one).
fn ask_bulk_conflicts(
    host: &mut dyn WorkbenchHost,
    id: ProjectId,
    conflicts: Vec<String>,
    rest: Vec<(String, crate::outgoing::Why)>,
) {
    use workbench_model::{ChoiceItem, ChoiceList};
    let mut items = vec![ChoiceItem {
        icon: "compare",
        label: i18n::t("remote.bulk_resolve"),
        detail: i18n::t("remote.bulk_resolve_detail"),
    }];
    if !rest.is_empty() {
        items.push(ChoiceItem {
            icon: "upload",
            label: i18n::tr!("remote.bulk_rest", count = rest.len()),
            detail: i18n::t("remote.bulk_rest_detail"),
        });
    }
    let list = ChoiceList {
        title: i18n::tr!("remote.bulk_conflicts_title", count = conflicts.len()),
        subject: conflicts.join(", "),
        icon: "server",
        action: i18n::t("remote.conflict_action"),
        items,
    };
    host.choose(
        list,
        Box::new(move |chosen, _host| {
            let Some(state) = session(id) else { return };
            match chosen {
                // The first one's question (upload alone: asked what to do with it).
                0 => {
                    if let Some(first) = conflicts.first() {
                        state.send_now(&[(first.clone(), crate::outgoing::Why::Conflict)]);
                    }
                }
                _ => state.send_now(&rest),
            }
        }),
    );
}

/// The server's copy of `relative` (`path` here) beside ours (editable): `resolving` a
/// conflict, the user is told their next save is the resolution.
fn compare(
    host: &mut dyn WorkbenchHost,
    path: PathBuf,
    relative: &str,
    local: String,
    server: String,
    resolving: bool,
) {
    let input = diff::CompareInput {
        left: (path.clone(), server),
        right: (path, local),
        labels: [Some(i18n::t("remote.server_copy")), Some(i18n::t("remote.local_copy"))],
        read_only: [true, false],
    };
    host.compare(input, &i18n::tr!("remote.compare_tab", path = relative));
    if resolving {
        host.notify(workbench_model::Notification::warning(i18n::t(
            "remote.conflict_after_compare",
        )));
    }
}

/// Asks before giving up the changes here of `rows` (how many, which): yes takes the
/// server's copies back (what was made here goes to the trash).
fn ask_discard(
    host: &mut dyn WorkbenchHost,
    id: ProjectId,
    rows: Vec<(String, crate::outgoing::Why)>,
) {
    let paths: Vec<String> = rows.iter().map(|(p, _)| p.clone()).collect();
    let (message, list) = match paths.as_slice() {
        [one] => (i18n::tr!("remote.discard_one", name = format!("**{one}**")), Vec::new()),
        many => (i18n::tr!("remote.discard_many", count = many.len()), paths.clone()),
    };
    let question = workbench_model::Question {
        title: i18n::t("remote.discard_title"),
        message,
        icon: "undo",
        yes: i18n::t("remote.discard_yes"),
        danger: true,
        list,
    };
    host.confirm(
        question,
        Box::new(move |_| {
            if let Some(state) = session(id) {
                state.send_job(Job::Discard(rows));
            }
        }),
    );
}

/// `paths` (files, folders, `*.ext`) added to the connection of the project at `root`, kept;
/// the session starts again with them (what runs asked about first).
fn exclude(host: &mut dyn WorkbenchHost, root: &Path, paths: Vec<String>) {
    let Some(mut saved) = config::find(root) else { return };
    let excluded = &mut saved.connection.excluded;
    for path in paths {
        if !excluded.contains(&path) {
            excluded.push(path);
        }
    }
    match config::save(&saved) {
        Ok(()) => {
            super::reconnect_later(saved, None);
        }
        Err(e) => host.notify(workbench_model::Notification::error(i18n::tr!(
            "remote.failed",
            reason = e.to_string()
        ))),
    }
}

/// `file` relative to `root`, with `/`.
pub(super) fn version_relative(root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(root).ok()?;
    let parts: Vec<String> =
        rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}
