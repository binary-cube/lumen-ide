//! What the session tells while it works: its limits, the operations under way (their
//! bytes, their files, stopped or finished), the transfers' log, the notices; the jobs
//! sent to its thread and the transfers they run.

use super::*;

/// Transfers kept in the log (the newest).
const LOG: usize = 500;

impl State {
    pub fn retry(&self) -> Retry {
        self.saved.connection.retries(crate::settings::defaults().retry)
    }

    /// Its transfers at once (its connection's, else the global settings).
    pub fn limits(&self) -> remote::Limits {
        self.saved.connection.limits(crate::settings::defaults().limits)
    }

    /// Wakes the app (a view's background work done).
    pub fn wake(&self) -> Arc<dyn Fn() + Send + Sync> {
        Arc::clone(&self.wake)
    }

    /// Bytes moved one way, as they pass: on the network, and on the disk here (an upload
    /// reads them, a download writes them).
    pub fn moved(&self, action: Action, bytes: u64) {
        count(&self.traffic, action, bytes);
        match action {
            Action::Upload => self.disk.read(bytes),
            Action::Download => self.disk.written(bytes),
            _ => {}
        }
    }

    /// `changes` as one operation (for the Task Manager): a single file counted in bytes,
    /// several in files. Follow it with [`Operation`] (from any thread: `apply`'s workers).
    pub(crate) fn begin(&self, changes: &[Change]) -> Operation {
        use workbench_model::{ActivityKind, ActivityUnit};
        let kind = |a: &Action| match a {
            Action::Upload => ActivityKind::Upload,
            Action::Download => ActivityKind::Download,
            Action::DeleteLocal | Action::DeleteRemote => ActivityKind::Delete,
        };
        let first = changes.first().map(|c| kind(&c.action)).unwrap_or(ActivityKind::Sync);
        let kind = if changes.iter().all(|c| kind(&c.action) == first) {
            first
        } else {
            ActivityKind::Sync
        };
        let (subject, unit, total) = match changes {
            [one] => {
                let size = match one.action {
                    Action::Upload => one.local.as_ref().map(|l| l.size),
                    _ => one.remote.as_ref().map(|r| r.size),
                };
                (one.path.clone(), ActivityUnit::Bytes, size)
            }
            many => (
                i18n::tr!("tasks.operation_files", count = many.len()),
                ActivityUnit::Files,
                Some(many.len() as u64),
            ),
        };
        let id = self.activities.start(Some(self.project), kind, subject, unit, total);
        Operation { id, unit }
    }

    /// `operation` stops once `flag` is set: the Task Manager can stop it.
    pub(crate) fn stoppable(&self, operation: Operation, flag: Arc<std::sync::atomic::AtomicBool>) {
        self.activities.stoppable(operation.id, flag);
    }

    /// Bytes of `operation` passed (it counts them when it's a single file's).
    pub(crate) fn bytes_of(&self, operation: Operation, bytes: u64) {
        if operation.unit == workbench_model::ActivityUnit::Bytes {
            self.activities.advance(operation.id, bytes);
        }
    }

    /// A file of `operation` done (it counts them when it's several files').
    pub(crate) fn file_of(&self, operation: Operation) {
        if operation.unit == workbench_model::ActivityUnit::Files {
            self.activities.advance(operation.id, 1);
        }
    }

    /// `operation` ended, with what `done` says of it.
    pub(crate) fn finish(&self, operation: Operation, done: &[Transfer], stopped: bool) {
        let outcome = match done.iter().find_map(|t| t.error.clone()) {
            _ if stopped => workbench_model::Outcome::Stopped,
            Some(why) => workbench_model::Outcome::Failed(why),
            None => workbench_model::Outcome::Done,
        };
        self.activities.end(operation.id, outcome);
    }

    pub fn record(&self, transfer: &Transfer) {
        // Uploaded: its permissions read again (and checked, the setting on).
        if transfer.action == Action::Upload && transfer.error.is_none() {
            self.uploaded_check(&transfer.path);
        }
        if let Ok(mut log) = self.log.lock() {
            log.push_back(transfer.clone());
            while log.len() > LOG {
                log.pop_front();
            }
        }
        self.events.publish(&remote::events::TransferFinished {
            path: transfer.path.clone(),
            action: transfer.action,
            bytes: transfer.bytes,
            error: transfer.error.clone(),
        });
        (self.wake)();
    }

    /// A transfer starts / ends (`started`): announced with what waits.
    pub fn running(&self, started: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        if started {
            self.busy.fetch_add(1, Relaxed);
        } else {
            self.busy.fetch_sub(1, Relaxed);
        }
        self.announce();
    }

    /// What runs and what waits, announced.
    pub(super) fn announce(&self) {
        self.events.publish(&self.now());
        (self.wake)();
    }

    /// What runs and what waits to upload now.
    pub fn now(&self) -> remote::events::TransfersChanged {
        remote::events::TransfersChanged {
            running: self.busy.load(std::sync::atomic::Ordering::Relaxed),
            to_upload: self.pending.lock().map_or(0, |p| p.len()),
        }
    }

    pub(crate) fn tell(&self, notice: Notice) {
        if let Ok(mut inbox) = self.inbox.lock() {
            inbox.push(notice);
        }
        (self.wake)();
    }

    pub(super) fn send_job(&self, job: Job) {
        if let Some(jobs) = self.jobs.lock().ok().and_then(|j| j.clone()) {
            let _ = jobs.send(job);
        }
    }

    /// Transfers `relative` one way (a conflict decided, a file brought on demand), the
    /// manifest updated. `Err`: why it failed.
    pub(crate) fn transfer(
        &self,
        relative: &str,
        action: Action,
        session: &mut Option<Box<dyn RemoteFs>>,
    ) -> Result<(), String> {
        let local = local_copy(&self.saved.local);
        if session.is_none() {
            *session = self.connector.connect().ok();
        }
        let server = session.as_mut().and_then(|fs| fs.stat(relative).ok().flatten());
        // Back to the pool before the transfer takes one (never two held at once).
        *session = None;
        let change = Change {
            path: relative.to_owned(),
            action,
            conflict: false,
            chosen: true,
            remote: server,
            local: local.stat(relative),
        };
        self.run(&change, &local)
    }

    /// Applies `change` (one transfer, logged), keeping the manifest. `Err`: why it failed.
    pub(super) fn run(
        &self,
        change: &Change,
        local: &fs_local::remote_copy::LocalCopy,
    ) -> Result<(), String> {
        let done = self.run_all(std::slice::from_ref(change), local);
        match done.first().and_then(|t| t.error.clone()) {
            Some(why) => Err(why),
            None => Ok(()),
        }
    }

    /// Applies `changes` one after the other (logged), keeping the manifest.
    pub(super) fn run_all(
        &self,
        changes: &[Change],
        local: &fs_local::remote_copy::LocalCopy,
    ) -> Vec<Transfer> {
        self.running(true);
        let operation = self.begin(changes);
        // Stopped from the Task Manager: what's done stays, the rest isn't transferred.
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.stoppable(operation, Arc::clone(&stop));
        let stopped = || stop.load(std::sync::atomic::Ordering::Relaxed);
        let run = Run {
            workers: 1,
            retry: self.retry(),
            now: &now,
            on_transfer: &|t| {
                self.record(t);
                self.file_of(operation);
            },
            on_bytes: &|action, n| {
                self.moved(action, n);
                self.bytes_of(operation, n);
            },
            cancelled: &stopped,
            parallel: &threads,
            pause: &std::thread::sleep,
        };
        let mut manifest = self.manifest.lock().map(|m| m.clone()).unwrap_or_default();
        let done = apply(changes, self.connector.as_ref(), local, &mut manifest, &run);
        self.finish(operation, &done, stopped());
        self.running(false);
        self.keep(manifest, &done);
        done
    }
}
