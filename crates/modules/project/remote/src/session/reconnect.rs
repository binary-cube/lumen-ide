//! A remote project's session started again with other settings (its connection edited):
//! the old one ends — its subscriptions, its queues, its pooled connections closed — and the
//! new one starts on the same project (its identity, its events, its root) without the
//! project opening again. What of the old one still runs is asked about first, in the
//! running-work question ([`WorkbenchHost::ask_running`]).

use super::*;

/// A session's place, and where the project holds its subscriptions ([`hold`]'s guard):
/// restarting replaces them.
struct Restartable {
    place: Place,
    held: Weak<Mutex<Vec<Subscription>>>,
}

static RESTARTABLE: Mutex<Vec<Restartable>> = Mutex::new(Vec::new());

/// Edited connections waiting for their project's window to reconnect them: the connection,
/// and its new secret (`None`: the kept one).
static PENDING: Mutex<Vec<(Saved, Option<remote::Secret>)>> = Mutex::new(Vec::new());

/// The session at `place` (its `subscriptions`) held for the project: the guard returned
/// lives as long as the project; the session can be restarted meanwhile ([`restart`]).
pub(super) fn hold(place: Place, subscriptions: Vec<Subscription>) -> Vec<Subscription> {
    let id = place.id;
    let held = Arc::new(Mutex::new(subscriptions));
    if let Ok(mut all) = RESTARTABLE.lock() {
        all.retain(|r| r.held.strong_count() > 0);
        all.push(Restartable { place, held: Arc::downgrade(&held) });
    }
    vec![Subscription::on_end(move || {
        if let Ok(mut all) = RESTARTABLE.lock() {
            all.retain(|r| r.place.id != id);
        }
        drop(held);
    })]
}

/// Project `id`'s session started again: with `saved`, reached with `connector`. `false`:
/// no session of that project (closed meanwhile).
pub(crate) fn restart(id: ProjectId, saved: Saved, connector: Arc<dyn Connector>) -> bool {
    let found = RESTARTABLE.lock().ok().and_then(|all| {
        let r = all.iter().find(|r| r.place.id == id)?;
        Some((r.place.clone(), r.held.upgrade()?))
    });
    let Some((place, held)) = found else { return false };
    // The old session ends first: its subscriptions (with them its state, its threads, its
    // pooled connections), its entry (the project's identity finds the new one).
    let old = held.lock().map(|mut h| std::mem::take(&mut *h)).unwrap_or_default();
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.retain(|(i, ..)| *i != id);
    }
    drop(old);
    tracing::info!("the remote session starts again with its edited connection");
    let new = run(&place, saved, connector);
    if let Ok(mut h) = held.lock() {
        h.extend(new);
    }
    (place.wake)();
    true
}

/// Project `id` reconnected with `saved` (its new secret, `None`: the kept one); one waiting
/// for the user's approval starts now (saving it approved it). `false`: not open any more.
pub(crate) fn reconnect(id: ProjectId, saved: Saved, secret: Option<remote::Secret>) -> bool {
    if start_waiting(id, &saved) {
        return true;
    }
    let secret = secret.or_else(|| config::secret_store(&saved).get(&saved.connection.id));
    let connector = connector(&saved, secret, None);
    restart(id, saved, connector)
}

/// The project open at `root` (a session, or one waiting for approval): its identity and
/// its waker.
fn open_at(root: &Path) -> Option<(ProjectId, Arc<dyn Fn() + Send + Sync>)> {
    let restartable = RESTARTABLE.lock().ok().and_then(|all| {
        all.iter()
            .find(|r| r.place.root == root && r.held.strong_count() > 0)
            .map(|r| (r.place.id, Arc::clone(&r.place.wake)))
    });
    restartable.or_else(|| waiting_at(root))
}

/// After `saved` was edited and kept: its project, if it's open (in any window), reconnects
/// with it — once that window looks ([`poll`]). `false`: not open (nothing to reconnect).
pub(crate) fn reconnect_later(saved: Saved, secret: Option<remote::Secret>) -> bool {
    let Some((_, wake)) = open_at(&saved.local) else { return false };
    if let Ok(mut pending) = PENDING.lock() {
        // The latest edit wins.
        pending.retain(|(s, _)| s.local != saved.local);
        pending.push((saved, secret));
    }
    wake();
    true
}

/// The window of the project at `root` (`id`) reconnects it if its connection was edited:
/// its transfers still running asked about first (Stop / Wait / Cancel). `true` if it did
/// something.
pub(super) fn poll(host: &mut dyn WorkbenchHost, id: ProjectId, root: &Path) -> bool {
    let Some(edited) = PENDING.lock().ok().and_then(|mut pending| {
        let at = pending.iter().position(|(s, _)| s.local == root)?;
        Some(pending.remove(at))
    }) else {
        return false;
    };
    // Run once: by the question's answer, or here when the host can't ask.
    let job = Arc::new(Mutex::new(Some(edited)));
    let answered = Arc::clone(&job);
    let then: workbench_model::OnYes = Box::new(move |host| {
        if let Some((saved, secret)) = answered.lock().ok().and_then(|mut j| j.take()) {
            finish(host, id, saved, secret);
        }
    });
    if !host.ask_running(id, crate::MODULE_ID, then)
        && let Some((saved, secret)) = job.lock().ok().and_then(|mut j| j.take())
    {
        finish(host, id, saved, secret);
    }
    true
}

/// Reconnected: the tree's marks read again, the user told.
fn finish(
    host: &mut dyn WorkbenchHost,
    id: ProjectId,
    saved: Saved,
    secret: Option<remote::Secret>,
) {
    let root = saved.local.clone();
    let address = saved.connection.address();
    if reconnect(id, saved, secret) {
        host.relist(&root);
        host.notify(workbench_model::Notification::info(i18n::tr!(
            "remote.reconnected",
            address = address.as_str()
        )));
    }
}
