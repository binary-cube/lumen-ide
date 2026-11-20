//! The grid's rows (`GridSource`), from the reader's thread: what's shown is asked, a block
//! at a time, and kept (at most [`KEPT`] blocks, the least lately shown let go first);
//! nothing here reads the disk. Paused, the count shown stays where it was (the rows don't
//! move) while the reader goes on; resumed, it jumps to what's there now.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use ui_components::data_grid::source::{
    CellClass, CellRef, GridCaps, GridColumn, GridQuery, GridSource, RowState, SourceStatus, Total,
};

use crate::entry::{COLUMNS, Entry, LEVEL};
use crate::reader::{self, Answer, Ask, BLOCK};

/// The blocks kept at most (the rows shown and around them).
pub const KEPT: usize = 32;

/// A view's rows.
pub struct LogSource {
    columns: Vec<GridColumn>,
    asks: Sender<Ask>,
    answers: Receiver<Answer>,
    /// The count the reader told last.
    latest: u64,
    /// The count shown (held while paused).
    shown: u64,
    scanning: bool,
    paused: bool,
    generation: u64,
    query: GridQuery,
    blocks: HashMap<u64, Vec<Entry>>,
    /// When each block was last wanted (a counter), to let the oldest go.
    used: HashMap<u64, u64>,
    clock: u64,
    asked: HashSet<u64>,
    /// The rows the grid asked for last: asked again by the source itself when what it held
    /// of them went (new lines shown, a block read for an older count) — the grid asks only
    /// when its rows in view change.
    wanted: Range<u64>,
    failed: Option<String>,
    /// The log files before and after this one (told by the reader).
    neighbors: (Option<PathBuf>, Option<PathBuf>),
}

impl std::fmt::Debug for LogSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogSource")
            .field("shown", &self.shown)
            .field("paused", &self.paused)
            .finish_non_exhaustive()
    }
}

impl LogSource {
    /// The rows of `path`, read by a thread of its own that wakes the view with `wake`.
    /// `None` if the thread couldn't start.
    pub fn open(path: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        let (asks, asked) = mpsc::channel();
        let (answer, answers) = mpsc::channel();
        readers().spawn(kernel::work::Priority::Interactive, None, move || {
            reader::run(&path, &asked, &answer, &wake);
        });
        Some(Self::with(asks, answers))
    }

    pub(crate) fn with(asks: Sender<Ask>, answers: Receiver<Answer>) -> Self {
        let label = |c: &str| i18n::t(&format!("log_viewer.column_{c}"));
        let columns = COLUMNS
            .iter()
            .map(|c| {
                let mut column = GridColumn::new(*c, label(c), CellClass::Text);
                column.editable = false;
                column
            })
            .collect();
        Self {
            columns,
            asks,
            answers,
            latest: 0,
            shown: 0,
            scanning: true,
            paused: false,
            generation: 0,
            query: GridQuery::default(),
            blocks: HashMap::new(),
            used: HashMap::new(),
            clock: 0,
            asked: HashSet::new(),
            wanted: 0..0,
            failed: None,
            neighbors: (None, None),
        }
    }

    /// The new lines aren't shown (the rows stay still); the reader goes on.
    pub fn pause(&mut self, paused: bool) {
        self.paused = paused;
        if !paused {
            self.show_latest();
        }
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The log file before this one and the one after (`None`: none, or not known yet).
    pub fn neighbors(&self) -> &(Option<PathBuf>, Option<PathBuf>) {
        &self.neighbors
    }

    /// The lines shown, and those there are now (more while paused).
    pub fn counts(&self) -> (u64, u64) {
        (self.shown, self.latest)
    }

    /// What's shown moves to what's there now: the rows are other lines, read again.
    fn show_latest(&mut self) {
        if self.shown != self.latest {
            self.shown = self.latest;
            self.blocks.clear();
            self.used.clear();
            self.asked.clear();
            self.ask_wanted();
        }
    }

    /// The blocks of the rows wanted that aren't here or asked yet, asked.
    fn ask_wanted(&mut self) {
        if self.wanted.is_empty() || self.shown == 0 {
            return;
        }
        self.clock += 1;
        let (first, _) = Self::block_of(self.wanted.start);
        let (last, _) = Self::block_of(self.wanted.end.saturating_sub(1).min(self.shown - 1));
        for block in first..=last {
            self.used.insert(block, self.clock);
            if self.blocks.contains_key(&block) || !self.asked.insert(block) {
                continue;
            }
            let ask = Ask::Block { block, count: self.shown, generation: self.generation };
            let _ = self.asks.send(ask);
        }
    }

    /// The block row `row` is in, and its place there.
    fn block_of(row: u64) -> (u64, usize) {
        (row / BLOCK, usize::try_from(row % BLOCK).unwrap_or(0))
    }

    /// Row `row`'s line, when its block is here.
    pub fn entry_at(&self, row: u64) -> Option<&Entry> {
        self.entry(row)
    }

    fn entry(&self, row: u64) -> Option<&Entry> {
        let (block, at) = Self::block_of(row);
        self.blocks.get(&block)?.get(at)
    }

    /// The oldest blocks let go past [`KEPT`].
    fn trim(&mut self) {
        while self.blocks.len() > KEPT {
            let Some((&oldest, _)) = self
                .used
                .iter()
                .filter(|(b, _)| self.blocks.contains_key(b))
                .min_by_key(|(_, at)| **at)
            else {
                break;
            };
            self.blocks.remove(&oldest);
            self.used.remove(&oldest);
        }
    }
}

/// A level's icon and its tone (`None`: a level the log doesn't write).
pub fn level_icon(level: &str) -> Option<(&'static str, ui_components::ds::Tone)> {
    use ui_components::ds::Tone;
    match level {
        "ERROR" => Some(("octagon-alert", Tone::Danger)),
        "WARN" => Some(("alert", Tone::Warning)),
        "INFO" => Some(("info", Tone::Accent)),
        "DEBUG" | "TRACE" => Some(("wrench", Tone::Neutral)),
        _ => None,
    }
}

/// The readers' threads.
pub const THREAD: &str = "log-reader";

/// The views open at once with a reader each (a reader holds its worker while its view is
/// open; one more waits for one to close).
const READERS: usize = 16;

/// The readers' pool (a worker ends a while after its view let go of its reader).
fn readers() -> &'static kernel::work::Pool {
    static POOL: std::sync::OnceLock<kernel::work::Pool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        kernel::work::Pool::new(THREAD, READERS, std::time::Duration::from_secs(30))
    })
}

impl GridSource for LogSource {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }

    fn total(&self) -> Total {
        Total::Known(self.shown)
    }

    fn row_state(&self, row: u64) -> RowState {
        if row >= self.shown {
            return RowState::Missing;
        }
        if self.entry(row).is_some() {
            RowState::Ready
        } else if self.failed.is_some() {
            RowState::Failed
        } else {
            RowState::Pending
        }
    }

    fn cell(&self, row: u64, column: usize) -> CellRef<'_> {
        self.entry(row).map_or(CellRef::Null, |e| CellRef::Text(e.cell(column)))
    }

    fn request(&mut self, query: &GridQuery, rows: Range<u64>) {
        if query.filter != self.query.filter {
            self.query = query.clone();
            self.generation += 1;
            self.blocks.clear();
            self.used.clear();
            self.asked.clear();
            self.latest = 0;
            self.shown = 0;
            self.scanning = true;
            let _ = self
                .asks
                .send(Ask::Filter { filter: query.filter.clone(), generation: self.generation });
        }
        self.wanted = rows;
        self.ask_wanted();
    }

    fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(answer) = self.answers.try_recv() {
            match answer {
                Answer::Count { count, generation, scanning } if generation == self.generation => {
                    self.latest = count;
                    self.scanning = scanning;
                    self.failed = None;
                    if !self.paused {
                        self.show_latest();
                    }
                    changed = true;
                }
                Answer::Block { block, count, entries, generation }
                    if generation == self.generation =>
                {
                    self.asked.remove(&block);
                    // Read for a count no longer shown (new lines came in between): other rows
                    // now, asked again when wanted.
                    if count == self.shown {
                        self.blocks.insert(block, entries);
                        self.trim();
                    } else {
                        self.ask_wanted();
                    }
                    changed = true;
                }
                Answer::Neighbors { older, newer } => {
                    self.neighbors = (older, newer);
                    changed = true;
                }
                Answer::Failed(why) => {
                    self.failed = Some(why);
                    changed = true;
                }
                Answer::Count { .. } | Answer::Block { .. } => {}
            }
        }
        changed
    }

    fn status(&self) -> SourceStatus {
        match &self.failed {
            Some(why) => SourceStatus::Failed(why.clone()),
            None if self.scanning || !self.asked.is_empty() => SourceStatus::Loading,
            None => SourceStatus::Ready,
        }
    }

    fn capabilities(&self) -> GridCaps {
        GridCaps {
            sort: false,
            filter: true,
            limit: Some(i18n::t("log_viewer.newest_first")),
            cancel: false,
        }
    }

    fn name(&self) -> String {
        "log".to_owned()
    }

    /// A line's level with its icon, in its tone: errors, warnings, info, the quieter ones.
    fn cell_icon(
        &self,
        row: u64,
        column: usize,
    ) -> Option<(&'static str, ui_components::ds::Tone)> {
        if column != LEVEL {
            return None;
        }
        level_icon(self.entry(row)?.cell(LEVEL))
    }

    fn filter_hint(&self) -> Option<String> {
        Some(i18n::t("log_viewer.where_hint"))
    }

    fn refresh(&mut self) {
        self.show_latest();
    }
}

#[cfg(test)]
#[path = "../tests/unit/source.rs"]
mod tests;
