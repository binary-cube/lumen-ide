//! A remote project's transfers, from its queues: uploads (files saved, merged — saved five
//! times before it went: uploaded once; a batch of changes, a paste, held and sent as one)
//! and downloads (files opened on demand; asked twice: brought once, both told). Each queue
//! has its thread (`remote-uploads`, `remote-downloads`) handing a batch to as many workers
//! at once as the connection allows (`remote-upload`, `remote-download`; its own limits, else
//! the global settings), their sessions from the project's pool. What
//! couldn't go (offline, an error) stays to upload and is tried again, waiting longer each
//! time; the user is told once that the server is out of reach.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};

use kernel::{Batch, SharedQueue};
use remote::application::{Action, RemoteFs};

use crate::session::{Notice, State};

/// How an upload ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uploaded {
    Done,
    /// Changed on the server since: not uploaded, the user asked.
    Conflict,
    /// Not here any more (deleted before it went).
    Gone,
    /// The server out of reach: to upload when it's back.
    Offline,
    /// Refused (permissions, a full disk): to upload again; why.
    Failed(String),
}

/// Starts the project's queues' threads; they end with its state (the queues closed then).
/// Their own threads: each waits on its queue for the session's lifetime.
#[allow(clippy::disallowed_methods)]
pub fn start(state: &Arc<State>) -> std::io::Result<()> {
    let (uploads, downloads) = (state.uploads.clone(), state.downloads.clone());
    let (up, down) = (Arc::downgrade(state), Arc::downgrade(state));
    let limits = state.limits();
    std::thread::Builder::new()
        .name("remote-uploads".into())
        .spawn(move || run(&uploads, &up, limits.uploads, "remote-upload", upload))?;
    std::thread::Builder::new()
        .name("remote-downloads".into())
        .spawn(move || run(&downloads, &down, limits.downloads, "remote-download", download))?;
    Ok(())
}

type Work = fn(&Arc<State>, &str, &mut Option<Box<dyn RemoteFs>>);

/// A queue's thread: each batch to at most `workers` at once.
// Scoped threads, at most the connection's limit, gone with the batch (see `start`).
#[allow(clippy::disallowed_methods)]
fn run(queue: &SharedQueue<String>, state: &Weak<State>, workers: usize, name: &str, work: Work) {
    while let Some(batch) = queue.next() {
        let Some(state) = state.upgrade() else { break };
        let paths: Vec<String> = match batch {
            // Never asked for (each path is); what's waiting, to be safe.
            Batch::All => state.to_upload().into_keys().collect(),
            Batch::Keys(paths) => paths,
        };
        let next = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..workers.min(paths.len()) {
                let spawned = std::thread::Builder::new().name(name.into()).spawn_scoped(s, || {
                    let mut session = None;
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(path) = paths.get(i) else { break };
                        work(&state, path, &mut session);
                    }
                });
                if let Err(e) = spawned {
                    tracing::warn!("can't start a transfer: {e}");
                }
            }
        });
    }
}

fn upload(state: &Arc<State>, relative: &str, session: &mut Option<Box<dyn RemoteFs>>) {
    let started = state.sending_generation();
    let uploaded = state.upload(relative, session);
    // However it ended, what the user sent before it began is no longer going (its loader
    // stops; sent again meanwhile, it waits for that upload).
    state.end_sending_since(std::iter::once(relative), started);
    match uploaded {
        Uploaded::Done => {
            state.uploaded(relative);
        }
        Uploaded::Conflict | Uploaded::Gone => state.forget_pending(relative),
        Uploaded::Offline => {
            state.keep_pending(relative, &i18n::t("remote.offline"));
            if !state.offline.swap(true, Ordering::Relaxed) {
                state.tell(Notice::Message(workbench_model::Notification::warning(i18n::t(
                    "remote.offline",
                ))));
            }
            retry_later(state);
        }
        Uploaded::Failed(why) => {
            state.keep_pending(relative, &why);
            state.failed(&why);
            retry_later(state);
        }
    }
}

fn download(state: &Arc<State>, relative: &str, session: &mut Option<Box<dyn RemoteFs>>) {
    let outcome = state.transfer(relative, Action::Download, session);
    state.fetched(relative, outcome);
}

/// What's still to upload, tried again after a wait (longer each time, at most a minute);
/// one wait at a time, on its own thread (a pool's would sleep that long).
#[allow(clippy::disallowed_methods)]
fn retry_later(state: &Arc<State>) {
    if state.retrying.swap(true, Ordering::Relaxed) {
        return;
    }
    let round = state.failures.fetch_add(1, Ordering::Relaxed).min(6) as u32;
    let wait = (state.retry().delay * 2u32.pow(round)).min(std::time::Duration::from_secs(60));
    let (weak, queue) = (Arc::downgrade(state), state.uploads.clone());
    let spawned = std::thread::Builder::new().name("remote-retry".into()).spawn(move || {
        std::thread::sleep(wait);
        let Some(state) = weak.upgrade() else { return };
        state.retrying.store(false, Ordering::Relaxed);
        // Each one by its path: "everything" would swallow what's asked meanwhile.
        queue.push_many(state.to_upload().into_keys());
    });
    if let Err(e) = spawned {
        tracing::warn!("can't schedule a retry: {e}");
        state.retrying.store(false, Ordering::Relaxed);
    }
}
