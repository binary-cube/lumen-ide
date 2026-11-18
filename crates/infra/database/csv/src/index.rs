//! Reading a CSV file a record at a time from any record's start, and the index of where
//! its records start: built a chunk at a time, only as far as asked (a page further down,
//! or the end for the total), kept between requests, so a page anywhere is read by a
//! `seek` to the nearest mark — never the whole file in memory.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;

use database::csv::{RecordScanner, is_blank};
use database::{Cancel, DbError};

/// A file as it was on disk: changed by another program when this changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
}

impl Stamp {
    pub fn of(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        if !meta.is_file() {
            return Err(std::io::Error::other("not a file"));
        }
        Ok(Self { len: meta.len(), modified: meta.modified().ok() })
    }
}

/// FNV-1a, continued over `bytes`.
pub fn hash(mut state: u64, bytes: &[u8]) -> u64 {
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    for &b in bytes {
        state ^= u64::from(b);
        state = state.wrapping_mul(PRIME);
    }
    state
}

/// FNV-1a's start.
pub const HASH_START: u64 = 0xcbf2_9ce4_8422_2325;

/// A file's records, read one at a time from a record's start (blank lines too: they're
/// copied as they are when the file is written back).
#[derive(Debug)]
pub struct RecordReader {
    inner: BufReader<File>,
    scanner: RecordScanner,
    /// Where the next record starts.
    offset: u64,
}

impl RecordReader {
    /// Reads `file` from `offset` (a record's start), `buffer` bytes at a time.
    pub fn at(
        mut file: File,
        offset: u64,
        delimiter: u8,
        quote: u8,
        buffer: usize,
    ) -> std::io::Result<Self> {
        file.seek(SeekFrom::Start(offset))?;
        Ok(Self {
            inner: BufReader::with_capacity(buffer, file),
            scanner: RecordScanner::new(delimiter, quote),
            offset,
        })
    }

    /// Where the next record starts.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// The next record's bytes into `out` (its line ending included; the last one may have
    /// none), and where it starts; `None` at the end.
    pub fn next_into(&mut self, out: &mut Vec<u8>) -> std::io::Result<Option<u64>> {
        out.clear();
        let start = self.offset;
        loop {
            let buf = self.inner.fill_buf()?;
            if buf.is_empty() {
                self.scanner.reset();
                return Ok((!out.is_empty()).then_some(start));
            }
            match self.scanner.end(buf) {
                Some(n) => {
                    out.extend_from_slice(&buf[..=n]);
                    self.inner.consume(n + 1);
                    self.offset += n as u64 + 1;
                    return Ok(Some(start));
                }
                None => {
                    let len = buf.len();
                    out.extend_from_slice(buf);
                    self.inner.consume(len);
                    self.offset += len as u64;
                }
            }
        }
    }

    /// The next record that isn't a blank line.
    pub fn next_record(&mut self, out: &mut Vec<u8>) -> std::io::Result<Option<u64>> {
        loop {
            match self.next_into(out)? {
                Some(_) if is_blank(out) => continue,
                other => return Ok(other),
            }
        }
    }
}

/// How an index is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    /// A mark every this many records (a page is read from the nearest one before it).
    pub every: u64,
    /// Bytes scanned between two looks at the cancel.
    pub chunk: u64,
    /// The read buffer's size.
    pub buffer: usize,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { every: 1024, chunk: 4 << 20, buffer: 64 << 10 }
    }
}

/// Where a file's data records start (the header not counted), every [`Tuning::every`]th
/// of them kept: 8 bytes per thousand records. Built as far as asked, a chunk at a time,
/// stopping at a record's end when cancelled (what was found kept: the next request goes
/// on from there). What it scanned is hashed: a commit checks the file still holds it.
#[derive(Debug, Clone)]
pub struct OffsetIndex {
    pub stamp: Stamp,
    tuning: Tuning,
    delimiter: u8,
    quote: u8,
    /// The first record is the header (not a data record).
    header: bool,
    header_seen: bool,
    /// Where the scan began (after the byte order mark).
    start: u64,
    marks: Vec<u64>,
    records: u64,
    /// Scanned up to here (a record's end).
    pos: u64,
    hash: u64,
    done: bool,
}

impl OffsetIndex {
    pub fn new(
        stamp: Stamp,
        tuning: Tuning,
        delimiter: u8,
        quote: u8,
        header: bool,
        start: u64,
    ) -> Self {
        Self {
            stamp,
            tuning,
            delimiter,
            quote,
            header,
            header_seen: false,
            start,
            marks: Vec::new(),
            records: 0,
            pos: start,
            hash: HASH_START,
            done: false,
        }
    }

    /// Every record found: the total is known.
    pub fn done(&self) -> bool {
        self.done
    }

    /// The data records found so far.
    pub fn records(&self) -> u64 {
        self.records
    }

    /// Scanned up to here, and the hash of what was (from the start).
    pub fn scanned(&self) -> (u64, u64) {
        (self.pos, self.hash)
    }

    /// Where the scan began (the byte order mark's end).
    pub fn start(&self) -> u64 {
        self.start
    }

    /// The marks kept (memory: 8 bytes each).
    pub fn marks(&self) -> usize {
        self.marks.len()
    }

    /// The nearest mark at or before data record `ordinal`: its offset and its record's
    /// ordinal (`None`: not found yet).
    pub fn mark_for(&self, ordinal: u64) -> Option<(u64, u64)> {
        let at = ordinal / self.tuning.every;
        let offset = *self.marks.get(usize::try_from(at).ok()?)?;
        Some((offset, at * self.tuning.every))
    }

    /// Scans on until record `ordinal` has a mark, or the end.
    pub fn reach(&mut self, path: &Path, ordinal: u64, cancel: &Cancel) -> Result<(), DbError> {
        let every = self.tuning.every;
        self.extend(path, cancel, &|index| index.marks.len() as u64 > ordinal / every)
    }

    /// Scans on until `count` records are found, or the end: what a page shows is scanned
    /// (and hashed), so a commit finds it changed.
    pub fn cover(&mut self, path: &Path, count: u64, cancel: &Cancel) -> Result<(), DbError> {
        self.extend(path, cancel, &|index| index.records >= count)
    }

    /// Scans to the end (the total).
    pub fn complete(&mut self, path: &Path, cancel: &Cancel) -> Result<u64, DbError> {
        self.extend(path, cancel, &|_| false)?;
        Ok(self.records)
    }

    /// Scans on, a chunk at a time, until `enough` or the end; cancelled between chunks.
    fn extend(
        &mut self,
        path: &Path,
        cancel: &Cancel,
        enough: &dyn Fn(&Self) -> bool,
    ) -> Result<(), DbError> {
        if self.done || enough(self) {
            return Ok(());
        }
        let file = File::open(path).map_err(|e| crate::io_error(&e, "reading the file"))?;
        let mut reader =
            RecordReader::at(file, self.pos, self.delimiter, self.quote, self.tuning.buffer)
                .map_err(|e| crate::io_error(&e, "reading the file"))?;
        let mut record = Vec::new();
        let mut chunk = 0u64;
        loop {
            let Some(start) = reader
                .next_into(&mut record)
                .map_err(|e| crate::io_error(&e, "reading the file"))?
            else {
                self.done = true;
                return Ok(());
            };
            self.hash = hash(self.hash, &record);
            if !is_blank(&record) {
                if self.header && !self.header_seen {
                    self.header_seen = true;
                } else {
                    if self.records.is_multiple_of(self.tuning.every) {
                        self.marks.push(start);
                    }
                    self.records += 1;
                }
            }
            self.pos = reader.offset();
            if enough(self) {
                return Ok(());
            }
            chunk += record.len() as u64;
            if chunk >= self.tuning.chunk {
                chunk = 0;
                if cancel.is_cancelled() {
                    return Err(DbError::Cancelled);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/index.rs"]
mod tests;
