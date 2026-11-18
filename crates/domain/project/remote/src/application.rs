//! The use cases: what changed since the last transfer ([`plan`]) and doing it
//! ([`apply`], in parallel: each worker on its own session), over three ports — the server
//! ([`RemoteFs`], opened by a [`Connector`]) and the local copy ([`LocalFiles`]).

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::manifest::{Manifest, Synced};
use crate::properties::{
    Accounts, Algo, Capabilities, Properties, PropertiesChange, SetProperties,
};
use crate::{Connection, LocalEntry, RemoteEntry, RemoteError, content_hash};

/// A session with the server (paths relative to the project's root, with `/`).
pub trait RemoteFs: Send {
    /// A folder's direct entries (`""`: the root).
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError>;
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError>;
    /// Writes the file (its folders created), safely where the protocol allows (a
    /// temporary file renamed); returns it as the server now has it.
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError>;
    fn remove(&mut self, path: &str) -> Result<(), RemoteError>;
    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError>;
    /// `None`: not there.
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError>;
    /// [`read`](Self::read), telling `moved` the bytes as they come; stopped
    /// ([`RemoteError::Cancelled`]) as soon as `moved` answers `false`. By default, the whole
    /// file at once.
    fn read_moving(&mut self, path: &str, moved: Moved<'_>) -> Result<Vec<u8>, RemoteError> {
        let data = self.read(path)?;
        if moved(data.len() as u64) { Ok(data) } else { Err(RemoteError::Cancelled) }
    }
    /// [`write`](Self::write), telling `moved` the bytes as they go; stopped
    /// ([`RemoteError::Cancelled`], the file left as it was) as soon as `moved` answers
    /// `false`. By default, the whole file at once.
    fn write_moving(
        &mut self,
        path: &str,
        data: &[u8],
        moved: Moved<'_>,
    ) -> Result<RemoteEntry, RemoteError> {
        if !moved(0) {
            return Err(RemoteError::Cancelled);
        }
        let written = self.write(path, data)?;
        moved(data.len() as u64);
        Ok(written)
    }
    /// Where the user lands on the server (an absolute path), to browse from.
    fn home(&mut self) -> Result<String, RemoteError> {
        Ok("/".into())
    }
    /// What the server says of each of `paths` (`None`: not there): the permissions, the
    /// owner and the group where the protocol tells them. By default, only what a listing
    /// says.
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        paths.iter().map(|p| Ok(self.stat(p)?.as_ref().map(Properties::of))).collect()
    }
    /// Folder `dir`'s entries with what the server says of them (permissions, owners), in
    /// one listing. By default, only what [`list`](Self::list) says.
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        Ok(self.list(dir)?.iter().map(Properties::of).collect())
    }
    /// The server's users and groups, to choose an owner or a group from (by default: none
    /// known — only those the files have are shown).
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        Ok(Accounts::default())
    }
    /// What the session can change and compute (by default: nothing).
    fn capabilities(&mut self) -> Capabilities {
        Capabilities::default()
    }
    /// `path`'s digest computed by the server (lower-case hex); `None`: it can't (then it's
    /// downloaded and computed here).
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        let _ = (path, algo);
        Ok(None)
    }
    /// Sets a file's mode, owner or group (by default: refused — the protocol can't).
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        let _ = set;
        Err(RemoteError::Server(format!("{path}: can't change its properties here")))
    }
}

/// What's under a folder on the server, counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Measured {
    pub bytes: u64,
    pub files: u64,
    pub folders: u64,
}

/// What applying a [`PropertiesChange`] did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Applied {
    /// Files and folders changed (those already so are left alone, not counted).
    pub changed: u64,
    /// Those looked at (changed or not).
    pub seen: u64,
    /// What the server refused: the path and why.
    pub failed: Vec<(String, String)>,
    /// Stopped by the user part way (what's done stays).
    pub stopped: bool,
}

/// Applies `change` to `targets` (as the server described them) and, when it's recursive,
/// to everything in the folders among them, folder by folder; `progress` told the files
/// looked at so far. A file the server refuses is noted and the rest goes on; a lost
/// connection stops it (`Err`).
pub fn apply_properties(
    fs: &mut dyn RemoteFs,
    targets: &[Properties],
    change: &PropertiesChange,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(u64),
) -> Result<Applied, RemoteError> {
    let mut applied = Applied::default();
    let one = |fs: &mut dyn RemoteFs, path: &str, is_dir, mode, applied: &mut Applied| {
        applied.seen += 1;
        let set = change.for_file(is_dir, mode);
        if set.is_empty() {
            return Ok(());
        }
        match fs.set_properties(path, &set) {
            Ok(()) => applied.changed += 1,
            Err(e) if e.is_transient() => return Err(e),
            Err(e) => applied.failed.push((path.to_owned(), e.to_string())),
        }
        Ok(())
    };
    let mut dirs = Vec::new();
    for target in targets {
        if cancelled() {
            applied.stopped = true;
            return Ok(applied);
        }
        one(fs, &target.path, target.is_dir, target.mode, &mut applied)?;
        if change.recursive && target.is_dir {
            dirs.push(target.path.clone());
        }
        progress(applied.seen);
    }
    while let Some(dir) = dirs.pop() {
        if cancelled() {
            applied.stopped = true;
            return Ok(applied);
        }
        let entries = match fs.list(&dir) {
            Ok(entries) => entries,
            Err(e) if e.is_transient() => return Err(e),
            Err(e) => {
                applied.failed.push((dir, e.to_string()));
                continue;
            }
        };
        // Each keeps its mixed bits: their modes read (one request per folder's batch).
        let modes: Vec<Option<crate::properties::Mode>> = if change.needs_modes() {
            let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
            fs.properties(&paths)?.into_iter().map(|p| p.and_then(|p| p.mode)).collect()
        } else {
            vec![None; entries.len()]
        };
        for (entry, mode) in entries.into_iter().zip(modes) {
            one(fs, &entry.path, entry.is_dir, mode, &mut applied)?;
            if entry.is_dir {
                dirs.push(entry.path);
            }
        }
        progress(applied.seen);
    }
    Ok(applied)
}

/// Everything under folder `dir` (relative to the root) counted, folder by folder — the
/// exclusions too (it's the server's size); `progress` told the count so far after each
/// folder. `cancelled` stops it ([`RemoteError::Cancelled`]).
pub fn measure(
    fs: &mut dyn RemoteFs,
    dir: &str,
    cancelled: &dyn Fn() -> bool,
    progress: &mut dyn FnMut(Measured),
) -> Result<Measured, RemoteError> {
    let mut measured = Measured::default();
    let mut dirs = vec![dir.to_owned()];
    while let Some(dir) = dirs.pop() {
        if cancelled() {
            return Err(RemoteError::Cancelled);
        }
        for entry in fs.list(&dir).map_err(|e| e.during(&format!("listing /{dir}")))? {
            if entry.is_dir {
                measured.folders += 1;
                dirs.push(entry.path);
            } else {
                measured.files += 1;
                measured.bytes += entry.size;
            }
        }
        progress(measured);
    }
    Ok(measured)
}

/// Bytes moved by a transfer, told as they pass: `false` back stops it.
pub type Moved<'a> = &'a mut dyn FnMut(u64) -> bool;

/// Opens sessions with the server: one per worker, so transfers run in parallel.
pub trait Connector: Send + Sync {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError>;
}

/// The project's local copy (paths relative to its root, with `/`).
pub trait LocalFiles: Send + Sync {
    /// Every file.
    fn list(&self) -> Vec<LocalEntry>;
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>>;
    /// Writes the file (its folders created); returns it as it is now.
    fn write(&self, path: &str, data: &[u8]) -> std::io::Result<LocalEntry>;
    fn remove(&self, path: &str) -> std::io::Result<()>;
    fn stat(&self, path: &str) -> Option<LocalEntry>;
}

/// Every file on the server under the root (but the exclusions), folder by folder.
/// `cancelled` stops early.
pub fn list_remote(
    connection: &Connection,
    fs: &mut dyn RemoteFs,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<RemoteEntry>, RemoteError> {
    list_remote_under(connection, fs, "", cancelled)
}

/// The files on the server under `scope` (a folder or a file, relative to the root; `""`:
/// everything), but the exclusions. A scope that isn't there has nothing under it.
pub fn list_remote_under(
    connection: &Connection,
    fs: &mut dyn RemoteFs,
    scope: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<RemoteEntry>, RemoteError> {
    if !scope.is_empty() {
        match fs.stat(scope) {
            Ok(Some(file)) if !file.is_dir => {
                let excluded = connection.is_excluded(scope);
                return Ok(if excluded { Vec::new() } else { vec![file] });
            }
            Ok(_) | Err(RemoteError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    let mut files = Vec::new();
    let mut dirs = vec![scope.to_owned()];
    while let Some(dir) = dirs.pop() {
        if cancelled() {
            break;
        }
        let listed = match fs.list(&dir) {
            Ok(listed) => listed,
            // The scope itself is gone from the server: nothing there.
            Err(RemoteError::NotFound(_)) if dir == scope => Vec::new(),
            Err(e) => return Err(e.during(&format!("listing /{dir}"))),
        };
        for entry in listed {
            if connection.is_excluded(&entry.path) {
                continue;
            }
            if entry.is_dir {
                dirs.push(entry.path);
            } else {
                files.push(entry);
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// `path` is `scope` or under it (`""`: everything).
pub fn in_scope(path: &str, scope: &str) -> bool {
    scope.is_empty()
        || path == scope
        || path.strip_prefix(scope).is_some_and(|rest| rest.starts_with('/'))
}

/// Which way changes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// The server's changes come down.
    Pull,
    /// The local changes go up.
    Push,
    /// Both.
    Sync,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Download,
    Upload,
    /// Gone from the server: removed locally too.
    DeleteLocal,
    /// Gone locally: removed from the server too.
    DeleteRemote,
}

/// A file to transfer (or remove), as the plan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub action: Action,
    /// Changed on both sides since the last transfer: the user decides (keep theirs, take
    /// the server's, skip).
    pub conflict: bool,
    /// Done when the plan is applied (conflicts and removals wait for the user).
    pub chosen: bool,
    pub remote: Option<RemoteEntry>,
    pub local: Option<LocalEntry>,
}

impl Change {
    /// A conflict (or a removal) decided: the server's copy wins (downloaded, or removed
    /// here when it's gone there) or ours does (uploaded, or removed there).
    pub fn take(&mut self, server: bool) {
        self.action = match (server, self.remote.is_some(), self.local.is_some()) {
            (true, true, _) => Action::Download,
            (true, false, _) => Action::DeleteLocal,
            (false, _, true) => Action::Upload,
            (false, _, false) => Action::DeleteRemote,
        };
        self.chosen = true;
    }
}

/// The server's copy moved since the last transfer (appeared, went, or changed).
fn remote_changed(r: Option<&RemoteEntry>, m: Option<&Synced>) -> bool {
    match (r, m) {
        (Some(r), Some(m)) => r.size != m.remote_size || r.modified != m.remote_modified,
        (None, None) => false,
        _ => true,
    }
}

/// The local copy moved since the last transfer; its content is hashed only when its size
/// or date moved.
fn local_changed(
    path: &str,
    l: Option<&LocalEntry>,
    m: Option<&Synced>,
    local_hash: &dyn Fn(&str) -> Option<u64>,
) -> bool {
    match (l, m) {
        (Some(l), Some(m)) => {
            (l.size != m.local_size || l.modified != m.local_modified)
                && local_hash(path).is_none_or(|h| h != m.local_hash)
        }
        (None, None) => false,
        _ => true,
    }
}

/// Where a local file stands against its last transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// As it was transferred (nothing to do).
    Same,
    /// Never transferred: only here.
    OnlyHere,
    /// Changed here: to send.
    Ahead,
    /// Changed (or gone) on the server: to bring.
    Behind,
    /// Changed on both sides.
    Diverged,
}

/// Where local file `local` stands, from the manifest's record of it and the server's copy
/// — `server`: `None` when the server's side isn't known (only ours is judged then),
/// `Some(None)` when the server doesn't have it. `local_hash` as for [`plan`].
pub fn standing(
    local: &LocalEntry,
    manifest: &Manifest,
    server: Option<Option<&RemoteEntry>>,
    local_hash: &dyn Fn(&str) -> Option<u64>,
) -> Standing {
    let synced = manifest.get(&local.path);
    let Some(server) = server else {
        return match synced {
            None => Standing::OnlyHere,
            Some(_) if local_changed(&local.path, Some(local), synced, local_hash) => {
                Standing::Ahead
            }
            Some(_) => Standing::Same,
        };
    };
    if server.is_none() && synced.is_none() {
        return Standing::OnlyHere;
    }
    let ahead = local_changed(&local.path, Some(local), synced, local_hash);
    match (ahead, remote_changed(server, synced)) {
        (false, false) => Standing::Same,
        (true, false) => Standing::Ahead,
        (false, true) => Standing::Behind,
        (true, true) => Standing::Diverged,
    }
}

/// What changed since the last transfer, for `direction`. `local_hash` hashes a local file
/// (only called when its size or date moved). Excluded paths are left out.
pub fn plan(
    connection: &Connection,
    manifest: &Manifest,
    local: &[LocalEntry],
    remote: &[RemoteEntry],
    direction: Direction,
    local_hash: &dyn Fn(&str) -> Option<u64>,
) -> Vec<Change> {
    use std::collections::{BTreeMap, BTreeSet};
    let locals: BTreeMap<&str, &LocalEntry> = local.iter().map(|l| (l.path.as_str(), l)).collect();
    let remotes: BTreeMap<&str, &RemoteEntry> =
        remote.iter().filter(|r| !r.is_dir).map(|r| (r.path.as_str(), r)).collect();
    let paths: BTreeSet<&str> =
        locals.keys().chain(remotes.keys()).copied().chain(manifest.paths()).collect();
    let down = direction != Direction::Push;
    let up = direction != Direction::Pull;
    let mut changes = Vec::new();
    for path in paths {
        if connection.is_excluded(path) {
            continue;
        }
        let (l, r, m) = (locals.get(path).copied(), remotes.get(path).copied(), manifest.get(path));
        let remote_changed = remote_changed(r, m);
        let local_changed = local_changed(path, l, m, local_hash);
        let change = |action, conflict| Change {
            path: path.to_owned(),
            action,
            conflict,
            // Conflicts and removals wait for the user.
            chosen: !conflict && matches!(action, Action::Download | Action::Upload),
            remote: r.cloned(),
            local: l.cloned(),
        };
        let action = match (remote_changed, local_changed) {
            (true, false) if down => {
                Some((if r.is_some() { Action::Download } else { Action::DeleteLocal }, false))
            }
            (false, true) if up => {
                Some((if l.is_some() { Action::Upload } else { Action::DeleteRemote }, false))
            }
            // Both sides changed (or both appeared without a past transfer).
            (true, true) => {
                let action = match (direction, l.is_some(), r.is_some()) {
                    (Direction::Push, true, _) => Action::Upload,
                    (Direction::Push, false, _) => Action::DeleteRemote,
                    (_, _, true) => Action::Download,
                    (_, _, false) => Action::DeleteLocal,
                };
                Some((action, true))
            }
            _ => None,
        };
        if let Some((action, conflict)) = action {
            changes.push(change(action, conflict));
        }
    }
    changes
}

/// A transfer done (or failed), for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub path: String,
    pub action: Action,
    pub bytes: u64,
    /// When it ended (seconds since the Unix epoch).
    pub at: i64,
    pub error: Option<String>,
}

/// What a worker did to the manifest after a transfer.
enum Record {
    Set(String, Synced),
    Remove(String),
}

/// How a run of transfers goes: `workers` sessions in parallel, each transfer reported as
/// it ends (`on_transfer`, for the log), `cancelled` stopping it after the transfers in
/// progress.
pub struct Run<'a> {
    pub workers: usize,
    /// A transfer failing on the network (or the server) is tried again, on a new session.
    pub retry: crate::Retry,
    /// Seconds since the Unix epoch.
    pub now: &'a (dyn Fn() -> i64 + Sync),
    pub on_transfer: &'a (dyn Fn(&Transfer) + Sync),
    /// Bytes moved, as they pass, and which way (each worker's; a retried transfer
    /// counts again).
    pub on_bytes: &'a (dyn Fn(Action, u64) + Sync),
    /// Checked between transfers and between a transfer's chunks: one going on stops,
    /// leaving nothing half-written in place.
    pub cancelled: &'a (dyn Fn() -> bool + Sync),
    /// Runs a worker on `n` threads at once, returning when every one is done (the
    /// platform's threads; a worker takes the next transfer left, so running them in turn
    /// does the same work).
    pub parallel: &'a (dyn Fn(usize, &(dyn Fn() + Sync)) + Sync),
    /// Waits before a transfer is tried again.
    pub pause: &'a (dyn Fn(std::time::Duration) + Sync),
}

impl std::fmt::Debug for Run<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Run").field("workers", &self.workers).finish_non_exhaustive()
    }
}

/// Does the chosen changes (in parallel, see [`Run`]), keeping the manifest up to date.
/// Returns every transfer.
pub fn apply(
    changes: &[Change],
    connector: &dyn Connector,
    local: &dyn LocalFiles,
    manifest: &mut Manifest,
    run: &Run<'_>,
) -> Vec<Transfer> {
    let Run { workers, retry, now, on_transfer, on_bytes, cancelled, parallel, pause } = *run;
    let todo: Vec<&Change> = changes.iter().filter(|c| c.chosen).collect();
    let next = AtomicUsize::new(0);
    let done: Mutex<Vec<(Transfer, Option<Record>)>> = Mutex::new(Vec::new());
    let workers = workers.clamp(1, todo.len().max(1));
    // A worker: transfers until none is left (or it's stopped).
    let work = || {
        let mut session = None;
        loop {
            if cancelled() {
                break;
            }
            let i = next.fetch_add(1, Ordering::Relaxed);
            let Some(change) = todo.get(i) else { break };
            let mut outcome = None;
            for attempt in 1..=retry.attempts.max(1) {
                let wait = retry.wait_before(attempt);
                if !wait.is_zero() {
                    if cancelled() {
                        break;
                    }
                    pause(wait);
                }
                // A session per worker, opened when it has something to do (again,
                // after one dropped).
                if session.is_none() {
                    match connector.connect() {
                        Ok(fs) => session = Some(fs),
                        Err(e) => {
                            let again = e.is_transient();
                            outcome = Some((failed(change, now(), &e.to_string()), None));
                            if again {
                                continue;
                            }
                            break;
                        }
                    }
                }
                let Some(fs) = session.as_mut() else { break };
                let mut moved = |n: u64| {
                    on_bytes(change.action, n);
                    !cancelled()
                };
                match transfer(change, fs.as_mut(), local, now(), &mut moved) {
                    Err((t, true)) => {
                        // The session is suspect: a new one for the next attempt.
                        session = None;
                        outcome = Some((t, None));
                    }
                    Err((t, false)) => {
                        outcome = Some((t, None));
                        break;
                    }
                    Ok(done) => {
                        outcome = Some(done);
                        break;
                    }
                }
            }
            let Some((transfer, record)) = outcome else { continue };
            on_transfer(&transfer);
            done.lock().unwrap_or_else(|p| p.into_inner()).push((transfer, record));
        }
    };
    parallel(workers, &work);
    let done = done.into_inner().unwrap_or_else(|p| p.into_inner());
    let mut transfers = Vec::with_capacity(done.len());
    for (transfer, record) in done {
        match record {
            Some(Record::Set(path, synced)) => manifest.set(&path, synced),
            Some(Record::Remove(path)) => manifest.remove(&path),
            None => {}
        }
        transfers.push(transfer);
    }
    transfers
}

fn failed(change: &Change, at: i64, why: &str) -> Transfer {
    Transfer {
        path: change.path.clone(),
        action: change.action,
        bytes: 0,
        at,
        error: Some(why.to_owned()),
    }
}

/// Why a transfer failed, and whether it's worth trying again.
type Failure = (String, bool);

fn remote_failure(e: RemoteError) -> Failure {
    let again = e.is_transient();
    (e.to_string(), again)
}

fn local_failure(e: std::io::Error) -> Failure {
    (e.to_string(), false)
}

/// Why `change` can't be done as planned: the side it writes over (or removes) changed since
/// the plan was made (a review can stay open a long while): a newer save here is never
/// lost, nor a file the server changed before it's removed there. Uploads ask nothing of the
/// server (on FTP a file's state costs listing its folder: one per file would be too slow).
fn changed_since_planned(
    change: &Change,
    fs: &mut dyn RemoteFs,
    local: &dyn LocalFiles,
) -> Option<String> {
    let path = change.path.as_str();
    let same_local = |now: Option<LocalEntry>| match (&change.local, now) {
        (None, None) => true,
        (Some(a), Some(b)) => (a.size, a.modified) == (b.size, b.modified),
        _ => false,
    };
    let local_changed = match change.action {
        Action::Download | Action::DeleteLocal => !same_local(local.stat(path)),
        Action::Upload | Action::DeleteRemote => false,
    };
    let remote_changed = match (change.action, &change.remote) {
        // As the plan saw it (a removal asked for without a plan has nothing to stand on).
        (Action::DeleteRemote, Some(planned)) => match fs.stat(path) {
            Ok(Some(now)) => (planned.size, planned.modified) != (now.size, now.modified),
            // Gone already, or not known: the removal itself says so.
            Ok(None) | Err(_) => false,
        },
        _ => false,
    };
    if local_changed {
        Some(format!("{path} changed here since the plan was made"))
    } else if remote_changed {
        Some(format!("{path} changed on the server since the plan was made"))
    } else {
        None
    }
}

/// One change, on one session: done, or failed (and whether to try again).
fn transfer(
    change: &Change,
    fs: &mut dyn RemoteFs,
    local: &dyn LocalFiles,
    at: i64,
    moved: Moved<'_>,
) -> Result<(Transfer, Option<Record>), (Transfer, bool)> {
    let path = change.path.as_str();
    if let Some(why) = changed_since_planned(change, fs, local) {
        return Err((failed(change, at, &why), false));
    }
    let outcome: Result<(u64, Record), Failure> = match change.action {
        Action::Download => fs.read_moving(path, moved).map_err(remote_failure).and_then(|data| {
            let written = local.write(path, &data).map_err(local_failure)?;
            let remote = change.remote.clone().unwrap_or(RemoteEntry {
                path: path.to_owned(),
                is_dir: false,
                size: data.len() as u64,
                modified: 0,
            });
            Ok((data.len() as u64, Record::Set(path.to_owned(), synced(&remote, &written, &data))))
        }),
        Action::Upload => local.read(path).map_err(local_failure).and_then(|data| {
            let remote = fs.write_moving(path, &data, moved).map_err(remote_failure)?;
            let now = local.stat(path).ok_or_else(|| (format!("{path} disappeared"), false))?;
            Ok((data.len() as u64, Record::Set(path.to_owned(), synced(&remote, &now, &data))))
        }),
        Action::DeleteLocal => {
            local.remove(path).map(|()| (0, Record::Remove(path.to_owned()))).map_err(local_failure)
        }
        Action::DeleteRemote => {
            fs.remove(path).map(|()| (0, Record::Remove(path.to_owned()))).map_err(remote_failure)
        }
    };
    match outcome {
        Ok((bytes, record)) => Ok((
            Transfer { path: path.to_owned(), action: change.action, bytes, at, error: None },
            Some(record),
        )),
        Err((why, again)) => Err((failed(change, at, &why), again)),
    }
}

fn synced(remote: &RemoteEntry, local: &LocalEntry, data: &[u8]) -> Synced {
    Synced {
        remote_size: remote.size,
        remote_modified: remote.modified,
        local_size: local.size,
        local_modified: local.modified,
        local_hash: content_hash(data),
    }
}

#[cfg(test)]
#[path = "../tests/unit/application.rs"]
mod tests;
