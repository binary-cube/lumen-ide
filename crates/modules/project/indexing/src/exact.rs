//! Every occurrence of what's searched, as the field's options ask (`Cc`, `W`, `.*`): the
//! project's files (in the folder searched in) read in the background and matched line by
//! line, sent in batches as they're found, at most [`LIMIT`]. A newer search stops this one;
//! so does dropping it (the view closed).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError};

use document::{LineMatch, Matcher};
use workbench_model::ProjectQueries;
use workspace::{FileFilter, FileMask};

/// The occurrences listed at most (past them, the search says there are more).
pub const LIMIT: usize = 2000;
/// The lines found sent together (fewer wakes of the window).
const BATCH: usize = 64;

/// The files searched: under a folder (relative, without a trailing `/`; `None`: all of
/// them), those a mask keeps.
#[derive(Debug, Clone, Default)]
pub struct Files {
    pub under: Option<String>,
    pub mask: FileMask,
}

/// A line found: its file (relative, with `/`) and the line with its matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub path: String,
    pub line: LineMatch,
}

/// What the search sends: lines found, then its end (whether it stopped at [`LIMIT`]).
#[derive(Debug)]
enum Batch {
    Found(Vec<Found>),
    Done { capped: bool },
}

/// A search running in the background (see the module's doc).
#[derive(Debug)]
pub struct ExactSearch {
    rx: Receiver<Batch>,
    stop: Arc<AtomicBool>,
    done: bool,
    capped: bool,
}

/// What a poll brought: the lines found since, and whether anything changed.
pub type Polled = (Vec<Found>, bool);

impl ExactSearch {
    /// Starts matching `matcher` in the project's `files`; `wake` is called when lines
    /// arrive.
    pub fn start(
        queries: Arc<dyn ProjectQueries>,
        matcher: Matcher,
        files: Files,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = Arc::clone(&stop);
        kernel::work::run("find-in-files", move || {
            scan(&*queries, &matcher, files, &stopped, &tx, wake.as_deref());
        });
        Self { rx, stop, done: false, capped: false }
    }

    /// The lines found since the last poll.
    pub fn poll(&mut self) -> Polled {
        let mut found = Vec::new();
        let mut changed = false;
        loop {
            match self.rx.try_recv() {
                Ok(Batch::Found(lines)) => {
                    found.extend(lines);
                    changed = true;
                }
                Ok(Batch::Done { capped }) => {
                    self.done = true;
                    self.capped = capped;
                    changed = true;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    changed |= !self.done;
                    self.done = true;
                    break;
                }
            }
        }
        (found, changed)
    }

    /// Every file was read (or it stopped at [`LIMIT`]).
    pub fn done(&self) -> bool {
        self.done
    }

    /// It stopped at [`LIMIT`]: there are more.
    pub fn capped(&self) -> bool {
        self.capped
    }
}

impl Drop for ExactSearch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The places found as a refactoring's plan, in the files at `root`: each occurrence replaced
/// by `with` (a regular expression's `$1`, `${name}` filled in), checked again before it's
/// written (the text may have changed since), undone as any refactoring.
pub fn replace_plan(
    found: &[Found],
    matcher: &Matcher,
    with: &str,
    regex: bool,
    root: &Path,
) -> refactor::Plan {
    let mut plan = refactor::Plan::default();
    for f in found {
        let path = root.join(&f.path);
        if plan.files.last().is_none_or(|last| last.path != path) {
            plan.files.push(refactor::FilePlan { path, occurrences: Vec::new() });
        }
        let Some(file) = plan.files.last_mut() else { continue };
        let line = &f.line;
        for range in &line.ranges {
            let Some(original) = line.text.get(range.clone()) else { continue };
            file.occurrences.push(refactor::Occurrence {
                range: line.start + range.start..line.start + range.end,
                original: original.to_owned(),
                replacement: matcher.replacement(original, with, regex),
                certainty: refactor::Certainty::Certain,
                chosen: true,
                line: line.line,
                line_text: line.text.clone(),
                in_line: range.clone(),
            });
        }
    }
    plan
}

/// The search itself, on its thread: the files in order, a batch sent as it fills.
fn scan(
    queries: &dyn ProjectQueries,
    matcher: &Matcher,
    files: Files,
    stop: &AtomicBool,
    tx: &Sender<Batch>,
    wake: Option<&(dyn Fn() + Send + Sync)>,
) {
    let send = |batch: Batch| {
        let sent = tx.send(batch).is_ok();
        if let Some(wake) = wake {
            wake();
        }
        sent
    };
    let root = queries.root();
    let filter = FileFilter { extensions: &[], dependencies: false, under: files.under };
    let (mut batch, mut total) = (Vec::new(), 0);
    for path in queries.files(&filter) {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let Ok(relative) = path.strip_prefix(&root) else { continue };
        let name = relative_name(relative);
        if !files.mask.accepts(&name) {
            continue;
        }
        let Some(text) = queries.read_source(relative) else { continue };
        for line in matcher.lines_matching(&text, LIMIT - total) {
            total += line.ranges.len();
            batch.push(Found { path: name.clone(), line });
        }
        if batch.len() >= BATCH && !send(Batch::Found(std::mem::take(&mut batch))) {
            return;
        }
        if total >= LIMIT {
            break;
        }
    }
    if !batch.is_empty() && !send(Batch::Found(batch)) {
        return;
    }
    send(Batch::Done { capped: total >= LIMIT });
}

/// A path relative to the project, with `/` (on Windows too).
fn relative_name(relative: &Path) -> String {
    relative.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
#[path = "../tests/unit/exact.rs"]
mod tests;
