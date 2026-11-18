//! Where the rows of a run go: a result kept, a file written, the first value.

use super::*;

/// What the rows read go into: a new result set for each set the statement brings, or the
/// rows of a result read again / further.
pub(super) enum Destination {
    New,
    /// Appended to this result (its next page).
    Append(ServiceId),
    /// This result read again from its start (its rows replaced, its columns too).
    Replace(ServiceId),
}

/// Keeps a statement's results as they come (see [`crate::results`]), and when the first
/// row came (the execution's end, the fetching's start).
pub(super) struct Collecting<'a> {
    pub(super) inner: &'a Inner,
    pub(super) into: Destination,
    /// The first set's title (a query of one table), its statement, its place.
    pub(super) title: Option<String>,
    pub(super) statement: &'a str,
    pub(super) range: Range<usize>,
    pub(super) rerunnable: bool,
    /// What the first set's query is made of (for editing its rows).
    pub(super) shape: Option<Result<sql::QueryShape, database::edits::NotEditable>>,
    /// The sets made, in order; the one filled now.
    pub(super) sets: Vec<ServiceId>,
    pub(super) current: Option<ServiceId>,
    pub(super) rows: u64,
    pub(super) first: Option<Instant>,
    pub(super) replaced: bool,
    pub(super) memory: usize,
}

impl<'a> Collecting<'a> {
    pub(super) fn new(
        inner: &'a Inner,
        into: Destination,
        statement: &'a str,
        range: Range<usize>,
    ) -> Self {
        let memory = inner.state().settings.result_memory;
        Self {
            inner,
            into,
            title: None,
            statement,
            range,
            rerunnable: false,
            shape: None,
            sets: Vec::new(),
            current: None,
            rows: 0,
            first: None,
            replaced: false,
            memory,
        }
    }

    /// The rows being filled sealed (seen by the views), the console kept within its memory.
    pub(super) fn flush(&mut self) {
        let Some(id) = self.current else { return };
        let mut results = self.inner.results();
        if let Some(set) = results.get_mut(id) {
            set.rows.seal();
            set.touch();
        }
        results.trim(self.memory, Some(id));
    }
}

impl RowSink for Collecting<'_> {
    fn columns(&mut self, columns: &[Column]) {
        // A statement without a result (an `UPDATE`) may say none.
        if columns.is_empty() {
            return;
        }
        self.flush();
        self.first.get_or_insert_with(|| self.inner.runtime.now());
        self.rows = 0;
        let mut results = self.inner.results();
        let id = match self.into {
            Destination::New => {
                results.next += 1;
                let number = results.next;
                let title = match self.title.take() {
                    Some(title) => title,
                    None => i18n::tr!("console.result_title", number = number),
                };
                let mut set =
                    ResultSet::new(number, title, self.statement.to_owned(), self.range.clone());
                set.rerunnable = self.rerunnable && self.sets.is_empty();
                if self.sets.is_empty()
                    && let Some(shape) = self.shape.take()
                {
                    set.shape = shape;
                }
                set.columns = columns.to_vec();
                set.loading = true;
                let id = set.id;
                results.sets.push(set);
                results.generation += 1;
                id
            }
            Destination::Append(id) | Destination::Replace(id) => {
                let replace = matches!(self.into, Destination::Replace(_)) && !self.replaced;
                self.replaced |= replace;
                if let Some(set) = results.get_mut(id) {
                    if replace || set.columns.is_empty() {
                        // Other columns: what was known of editing them is asked again.
                        if set.columns != columns {
                            set.editing = Editability::Unknown;
                        }
                        set.columns = columns.to_vec();
                    }
                    if replace {
                        set.rows.clear();
                        set.dropped = false;
                    }
                    set.touch();
                }
                id
            }
        };
        self.sets.push(id);
        self.current = Some(id);
        drop(results);
        self.inner.changed();
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        let Some(id) = self.current else { return Flow::Continue };
        self.rows += 1;
        let sealed = {
            let mut results = self.inner.results();
            let Some(set) = results.get_mut(id) else { return Flow::Stop };
            set.rows.push(values);
            set.rows.len() % crate::results::CHUNK_ROWS as u64 == 0
        };
        if sealed {
            self.flush();
            self.inner.changed();
        }
        Flow::Continue
    }
}

/// An export's rows written as they come, a block at a time, in the columns asked for.
pub(super) struct Writing<'a> {
    pub(super) stream: ExportWriter,
    pub(super) sink: &'a mut ExportSink,
    pub(super) wanted: Vec<String>,
    /// Where each wanted column is among the result's.
    pub(super) at: Vec<Option<usize>>,
    pub(super) block: Vec<Vec<Value>>,
    pub(super) error: Option<String>,
    pub(super) rows: &'a AtomicU64,
    pub(super) wake: Arc<Inner>,
}

impl Writing<'_> {
    pub(super) fn write_block(&mut self) {
        if self.block.is_empty() || self.error.is_some() {
            self.block.clear();
            return;
        }
        let rows: Vec<Vec<_>> = self
            .block
            .iter()
            .map(|row| {
                self.at
                    .iter()
                    .map(|i| {
                        i.and_then(|i| row.get(i))
                            .map_or(ui_components::data_grid::CellRef::Null, cell_ref)
                    })
                    .collect()
            })
            .collect();
        match self.stream.rows(&rows) {
            Ok(bytes) => {
                if let Err(e) = self.sink.write_bytes(&bytes) {
                    self.error = Some(e.to_string());
                }
            }
            Err(why) => self.error = Some(why),
        }
        self.rows.fetch_add(self.block.len() as u64, Ordering::Relaxed);
        self.block.clear();
        self.wake.wake();
    }
}

impl RowSink for Writing<'_> {
    fn columns(&mut self, columns: &[Column]) {
        if columns.is_empty() {
            return;
        }
        self.write_block();
        self.at = self.wanted.iter().map(|w| columns.iter().position(|c| c.name == *w)).collect();
    }

    fn row(&mut self, values: Vec<Value>) -> Flow {
        if self.error.is_some() {
            return Flow::Stop;
        }
        self.block.push(values);
        if self.block.len() >= EXPORT_BLOCK {
            self.write_block();
        }
        Flow::Continue
    }
}

/// The first value of the first row (a big cell read whole).
#[derive(Default)]
pub(super) struct FirstValue {
    pub(super) value: Option<Value>,
}

impl RowSink for FirstValue {
    fn columns(&mut self, _columns: &[Column]) {}

    fn row(&mut self, values: Vec<Value>) -> Flow {
        self.value = values.into_iter().next();
        Flow::Stop
    }
}
