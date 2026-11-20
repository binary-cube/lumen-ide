//! The thread that reads a log file (`log-reader`, one per open view): the UI thread only
//! asks it and takes what it answers, never touches the disk.
//!
//! - **The file is never in memory whole.** It's walked in pieces of [`PIECE`] bytes and
//!   only where each line starts is kept (8 bytes a line); a line not ended yet (being
//!   written) waits for its end.
//! - **Rows are read when asked**, a block of [`BLOCK`] at a time, each line by its place
//!   (a seek, then its bytes), parsed into its cells here.
//! - **The filter runs here**, over the lines in pieces too: what passes is kept as the
//!   lines' numbers.
//! - **Live:** every [`LOOK_EVERY`] the file's size is looked at; what grew is walked (and
//!   filtered) and the new count told. A file that shrank or was replaced is read anew.
//!
//! The newest line is the view's first row: a row is a place counted from the end of the
//! lines the view was told of, so a block stays the same lines whatever came after.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ui_components::data_grid::filter::GridFilter;

use crate::entry::{self, Entry};

/// The bytes read at a time.
pub const PIECE: usize = 64 * 1024;
/// The rows of a block (asked and kept together).
pub const BLOCK: u64 = 200;
/// How often the file's growth is looked at.
pub const LOOK_EVERY: Duration = Duration::from_millis(500);
/// A line longer than this is read only this far (the log's own are at most 64 KB).
const MAX_LINE: u64 = 256 * 1024;

/// What the view asks.
#[derive(Debug)]
pub enum Ask {
    /// Only the lines that pass (`None`: all of them), from now; `generation` tells its
    /// answers apart from an older filter's.
    Filter { filter: Option<GridFilter>, generation: u64 },
    /// The block `block` of the lines that pass, counted from the start of the first
    /// `count` of them (what the view shows), for `generation`.
    Block { block: u64, count: u64, generation: u64 },
}

/// What the reader answers.
#[derive(Debug)]
pub enum Answer {
    /// The lines that pass now, for `generation`; `scanning`: still being walked.
    Count { count: u64, generation: u64, scanning: bool },
    /// Block `block`'s entries (its rows in order: the newest first) of the first `count`
    /// lines, for `generation`.
    Block { block: u64, count: u64, entries: Vec<Entry>, generation: u64 },
    /// The log files beside it in its folder: the one before it and the one after (its
    /// day's other parts, or the days around it).
    Neighbors { older: Option<PathBuf>, newer: Option<PathBuf> },
    /// The file can't be read (why, for the user).
    Failed(String),
}

/// The log files in `path`'s folder named as the log names them, in their order (by day,
/// then part): the one before `path` and the one after.
pub fn neighbors(path: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(dir) = path.parent() else { return (None, None) };
    let Ok(entries) = std::fs::read_dir(dir) else { return (None, None) };
    let mut files: Vec<(String, u32, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let (day, part) = crate::log_name(&name)?;
            Some((day.to_owned(), part, e.path()))
        })
        .collect();
    files.sort();
    let at = files.iter().position(|(_, _, p)| p == path);
    let Some(at) = at else { return (None, None) };
    let older = at.checked_sub(1).and_then(|i| files.get(i)).map(|f| f.2.clone());
    let newer = files.get(at + 1).map(|f| f.2.clone());
    (older, newer)
}

/// Reads `path` until the view lets go of `asks`; answers on `answers`, then `wake`s the
/// view.
pub fn run(
    path: &Path,
    asks: &Receiver<Ask>,
    answers: &Sender<Answer>,
    wake: &Arc<dyn Fn() + Send + Sync>,
) {
    let mut reader = Reader::new(path.to_path_buf());
    let tell = |answer: Answer| {
        let sent = answers.send(answer).is_ok();
        wake();
        sent
    };
    if let Err(why) = reader.grow() {
        tell(Answer::Failed(why));
    }
    if !tell(reader.count_answer(false)) {
        return;
    }
    let (older, newer) = neighbors(path);
    if !tell(Answer::Neighbors { older, newer }) {
        return;
    }
    let mut looked = Instant::now();
    loop {
        match asks.recv_timeout(LOOK_EVERY.saturating_sub(looked.elapsed())) {
            Ok(Ask::Filter { filter, generation }) => {
                reader.generation = generation;
                reader.filter = filter;
                reader.passing = if reader.filter.is_some() { Some(Vec::new()) } else { None };
                reader.filtered_upto = 0;
                // Filtered in pieces: the count told as it goes, the asks heard between.
                loop {
                    let done = reader.filter_some(20_000);
                    if !tell(reader.count_answer(!done)) {
                        return;
                    }
                    if done {
                        break;
                    }
                }
            }
            Ok(Ask::Block { block, count, generation }) => {
                if generation != reader.generation {
                    continue;
                }
                let entries = reader.block(block, count);
                if !tell(Answer::Block { block, count, entries, generation }) {
                    return;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                looked = Instant::now();
                let before = reader.count();
                match reader.grow() {
                    Ok(()) => {
                        reader.filter_some(usize::MAX);
                        if reader.count() != before && !tell(reader.count_answer(false)) {
                            return;
                        }
                    }
                    Err(why) => {
                        if !tell(Answer::Failed(why)) {
                            return;
                        }
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The file and what's known of it.
pub(crate) struct Reader {
    path: PathBuf,
    file: Option<File>,
    /// Where each whole line starts.
    pub(crate) starts: Vec<u64>,
    /// Where the walk got to: the end of the last whole line.
    pub(crate) walked: u64,
    filter: Option<GridFilter>,
    /// The lines that pass the filter (`None`: no filter, every one does).
    pub(crate) passing: Option<Vec<u64>>,
    /// The lines filtered so far.
    filtered_upto: usize,
    generation: u64,
}

impl Reader {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            starts: Vec::new(),
            walked: 0,
            filter: None,
            passing: None,
            filtered_upto: 0,
            generation: 0,
        }
    }

    /// The lines that pass now.
    pub(crate) fn count(&self) -> u64 {
        match &self.passing {
            Some(p) => p.len() as u64,
            None => self.starts.len() as u64,
        }
    }

    fn count_answer(&self, scanning: bool) -> Answer {
        Answer::Count { count: self.count(), generation: self.generation, scanning }
    }

    /// What was added to the file walked (its whole lines' starts kept); a file that shrank
    /// or isn't the same any more read from its start.
    pub(crate) fn grow(&mut self) -> Result<(), String> {
        let size = std::fs::metadata(&self.path).map_err(|e| e.to_string())?.len();
        if size < self.walked {
            // Truncated or replaced: read anew.
            self.file = None;
            self.starts.clear();
            self.walked = 0;
            self.filtered_upto = 0;
            if let Some(p) = &mut self.passing {
                p.clear();
            }
        }
        if size == self.walked {
            return Ok(());
        }
        if self.file.is_none() {
            self.file = Some(File::open(&self.path).map_err(|e| e.to_string())?);
        }
        let Some(file) = self.file.as_mut() else { return Ok(()) };
        file.seek(SeekFrom::Start(self.walked)).map_err(|e| e.to_string())?;
        let mut piece = vec![0u8; PIECE];
        let mut at = self.walked;
        // The start of the line being walked (a line may cross pieces).
        let mut line_start = self.walked;
        loop {
            let read = file.read(&mut piece).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            for (i, byte) in piece[..read].iter().enumerate() {
                if *byte == b'\n' {
                    self.starts.push(line_start);
                    line_start = at + i as u64 + 1;
                }
            }
            at += read as u64;
        }
        // Only whole lines: the one still being written waits.
        self.walked = line_start;
        Ok(())
    }

    /// The line `n`'s bytes (at most [`MAX_LINE`]), as text.
    fn line(&mut self, n: usize) -> Option<String> {
        let start = *self.starts.get(n)?;
        let end = self.starts.get(n + 1).copied().unwrap_or(self.walked);
        let length = end.saturating_sub(start).min(MAX_LINE);
        let file = self.file.as_mut()?;
        file.seek(SeekFrom::Start(start)).ok()?;
        let mut bytes = vec![0u8; usize::try_from(length).ok()?];
        file.read_exact(&mut bytes).ok()?;
        while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
            bytes.pop();
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Filters at most `most` more lines: `true` once every line known is.
    pub(crate) fn filter_some(&mut self, most: usize) -> bool {
        if self.passing.is_none() {
            self.filtered_upto = self.starts.len();
            return true;
        }
        let end = self.starts.len().min(self.filtered_upto.saturating_add(most));
        for n in self.filtered_upto..end {
            let passes = self
                .line(n)
                .is_some_and(|l| entry::passes(&entry::parse(&l), self.filter.as_ref()));
            if passes && let Some(p) = &mut self.passing {
                p.push(n as u64);
            }
        }
        self.filtered_upto = end;
        end == self.starts.len()
    }

    /// Block `block` of the first `count` lines that pass, the newest first: rows
    /// `block * BLOCK ..`, each row `r` the line `count - 1 - r` of those passing.
    pub(crate) fn block(&mut self, block: u64, count: u64) -> Vec<Entry> {
        let first = block.saturating_mul(BLOCK);
        let last = first.saturating_add(BLOCK).min(count);
        (first..last)
            .filter_map(|row| {
                let place = count.checked_sub(row + 1)?;
                let line = match &self.passing {
                    Some(p) => *p.get(usize::try_from(place).ok()?)?,
                    None => place,
                };
                let text = self.line(usize::try_from(line).ok()?)?;
                // The grid's cells cut short; the line whole for its details.
                Some(entry::parse(&text).shortened())
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "../tests/unit/reader.rs"]
mod tests;
