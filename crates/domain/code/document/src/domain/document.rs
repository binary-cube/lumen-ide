//! The [`Document`] aggregate: text, cursors, history and state relative to disk.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use kernel::DocumentId;

use super::buffer::TextBuffer;
use super::edit::{TextChange, Transaction};
use super::encoding::{Decoded, Encoding, LineEnding, encode_chunks};
use super::history::{EditKind, History};
use super::selection::{Selection, Selections};

/// The document's indentation style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Spaces(u8),
    Tabs,
}

impl Indent {
    pub fn unit(&self) -> String {
        match self {
            Indent::Spaces(n) => " ".repeat(*n as usize),
            Indent::Tabs => "\t".into(),
        }
    }

    /// Detects the style from the first indented lines; 4 spaces by default.
    pub fn detect(buffer: &TextBuffer) -> Self {
        let mut line = String::new();
        let (mut tabs, mut spaces) = (0, 0);
        for i in 0..buffer.line_count().min(500) {
            buffer.write_line(i, &mut line);
            if line.starts_with('\t') {
                tabs += 1;
            } else if line.starts_with("  ") {
                spaces += 1;
            }
        }
        if tabs > spaces { Indent::Tabs } else { Indent::Spaces(4) }
    }
}

/// How many recent versions keep their changes (for lagging consumers:
/// the incremental parser, folding).
const CHANGE_LOG: usize = 256;

/// At most this many changes kept over those versions (Replace All of 100,000 matches is
/// that many in one: 256 of them would be gigabytes); the oldest versions leave first.
#[cfg(not(test))]
const CHANGE_LOG_CHANGES: usize = 1 << 20;
/// Small in the tests (they make that many).
#[cfg(test)]
const CHANGE_LOG_CHANGES: usize = 64;

/// What is cleaned up automatically on save (e.g. from `.editorconfig`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SaveCleanup {
    pub trim_trailing_whitespace: bool,
    /// `None`: not set for this file (the app's default applies, see [`Self::or_final_newline`]).
    pub insert_final_newline: Option<bool>,
}

impl SaveCleanup {
    /// With the final newline decided: the file's own setting, else `default`.
    pub fn or_final_newline(self, default: bool) -> Self {
        Self { insert_final_newline: Some(self.insert_final_newline.unwrap_or(default)), ..self }
    }
}

#[derive(Debug)]
pub struct Document {
    id: DocumentId,
    path: Option<PathBuf>,
    buffer: TextBuffer,
    selections: Selections,
    history: History,
    version: u64,
    saved_version: u64,
    /// The encoding and line terminator the file has on disk (as last read or saved): one
    /// chosen otherwise is a change to save, as a text change is.
    saved_format: (Encoding, LineEnding),
    encoding: Encoding,
    line_ending: LineEnding,
    indent: Indent,
    tab_width: u32,
    cleanup: SaveCleanup,
    /// (resulting version, the changes that produced it), oldest first.
    changes: VecDeque<(u64, Vec<TextChange>)>,
    /// Locked: every edit is refused (only the disk's changes come in, [`Self::reload`]).
    read_only: bool,
    /// An edit was refused since the last [`Self::take_refused`] (the user is told).
    refused: bool,
    /// Lines `[first, last]` that act as one (a region collapsed in the editor showing it):
    /// the commands working on whole lines take them all. Set by the editor around a
    /// command, sorted, non-overlapping.
    line_groups: Vec<(usize, usize)>,
}

impl Document {
    /// A new, unsaved document.
    pub fn new(text: &str) -> Self {
        Self::from_decoded(
            None,
            Decoded {
                text: text.to_owned(),
                encoding: Encoding::Utf8,
                line_ending: LineEnding::Lf,
            },
        )
    }

    pub fn from_decoded(path: Option<PathBuf>, decoded: Decoded) -> Self {
        let buffer = TextBuffer::new(&decoded.text);
        let indent = Indent::detect(&buffer);
        Self {
            id: DocumentId::next(),
            path,
            buffer,
            selections: Selections::single(Selection::cursor(0)),
            history: History::default(),
            version: 0,
            saved_version: 0,
            saved_format: (decoded.encoding, decoded.line_ending),
            encoding: decoded.encoding,
            line_ending: decoded.line_ending,
            indent,
            tab_width: 4,
            cleanup: SaveCleanup::default(),
            changes: VecDeque::new(),
            read_only: false,
            refused: false,
            line_groups: Vec::new(),
        }
    }

    /// The lines that act as one (see the field); empty: every line on its own.
    pub fn set_line_groups(&mut self, groups: Vec<(usize, usize)>) {
        self.line_groups = groups;
    }

    /// Lines `first..=last` grown over the groups they touch: a group is taken whole.
    pub fn grow_lines(&self, first: usize, last: usize) -> (usize, usize) {
        let (mut first, mut last) = (first, last);
        let at = self.line_groups.partition_point(|&(_, end)| end < first);
        for &(start, end) in &self.line_groups[at..] {
            if start > last {
                break;
            }
            (first, last) = (first.min(start), last.max(end));
        }
        (first, last)
    }

    pub fn id(&self) -> DocumentId {
        self.id
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn buffer(&self) -> &TextBuffer {
        &self.buffer
    }

    pub fn selections(&self) -> &Selections {
        &self.selections
    }

    /// Increases with each text change (including undo/redo).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Unsaved: its text changed, or the encoding or line terminator it's to be written in.
    pub fn is_dirty(&self) -> bool {
        self.version != self.saved_version || (self.encoding, self.line_ending) != self.saved_format
    }

    pub fn encoding(&self) -> Encoding {
        self.encoding
    }

    pub fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    pub fn indent(&self) -> Indent {
        self.indent
    }

    pub fn tab_width(&self) -> u32 {
        self.tab_width
    }

    pub fn save_cleanup(&self) -> SaveCleanup {
        self.cleanup
    }

    /// Spaces between 1 and 16 (an `.editorconfig`'s `indent_size = 0` would divide by zero).
    pub fn set_indent(&mut self, indent: Indent) {
        self.indent = match indent {
            Indent::Spaces(n) => Indent::Spaces(n.clamp(1, 16)),
            Indent::Tabs => Indent::Tabs,
        };
    }

    pub fn set_tab_width(&mut self, width: u32) {
        self.tab_width = width.clamp(1, 16);
    }

    /// The terminator used on the next save (the project's rule: not a change of its own).
    pub fn set_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
        self.saved_format.1 = line_ending;
    }

    /// The encoding used on the next save (the project's rule: not a change of its own).
    pub fn set_encoding(&mut self, encoding: Encoding) {
        self.encoding = encoding;
        self.saved_format.0 = encoding;
    }

    /// The user chose another terminator: written on the next save, unsaved until then
    /// (back to the file's own: saved again).
    pub fn change_line_ending(&mut self, line_ending: LineEnding) {
        self.line_ending = line_ending;
    }

    /// The user chose another encoding to write the file in: unsaved until it's saved.
    pub fn change_encoding(&mut self, encoding: Encoding) {
        self.encoding = encoding;
    }

    pub fn set_save_cleanup(&mut self, cleanup: SaveCleanup) {
        self.cleanup = cleanup;
    }

    /// Locked (read-only): every edit, undo and redo is refused; the cursors still move and
    /// the disk's changes still come in ([`Self::reload`], [`Self::apply_external`]).
    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
        self.history.seal();
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Whether an edit was refused (locked) since the last call: the user is told once.
    pub fn take_refused(&mut self) -> bool {
        std::mem::take(&mut self.refused)
    }

    /// Notes an edit refused elsewhere (a command checking first): the user is told.
    pub fn refuse(&mut self) {
        self.refused = true;
    }

    /// Applies a transaction and records it in history. Without `selections`, the existing
    /// cursors are mapped through the transaction. Locked, a change is refused (noted:
    /// [`Self::take_refused`]).
    pub fn apply(&mut self, tx: Transaction, selections: Option<Selections>, kind: EditKind) {
        if self.read_only && !tx.is_empty() {
            self.refused = true;
            return;
        }
        self.apply_external(tx, selections, kind);
    }

    /// [`Self::apply`] even when locked: a change that doesn't come from the user (the
    /// file's text on disk, the same file's other tab following it).
    pub fn apply_external(
        &mut self,
        tx: Transaction,
        selections: Option<Selections>,
        kind: EditKind,
    ) {
        if tx.is_empty() {
            if let Some(s) = selections {
                self.set_selections(s);
            }
            return;
        }
        let before = self.selections.clone();
        let (inverse, changes) = tx.apply(&mut self.buffer);
        let after = selections.unwrap_or_else(|| before.map(&tx)).clamp(self.buffer.len());
        self.history.record(tx, inverse, before, after.clone(), kind);
        self.selections = after;
        self.bump(changes);
    }

    /// The changes from `version` until now, in order. `None` if `version` is too
    /// old (the consumer must rebuild everything from scratch).
    pub fn changes_since(&self, version: u64) -> Option<Vec<TextChange>> {
        if version == self.version {
            return Some(Vec::new());
        }
        let oldest = self.changes.front().map(|(v, _)| *v)?;
        if version + 1 < oldest || version > self.version {
            return None;
        }
        Some(
            self.changes
                .iter()
                .filter(|(v, _)| *v > version)
                .flat_map(|(_, c)| c.iter().copied())
                .collect(),
        )
    }

    fn bump(&mut self, changes: Vec<TextChange>) {
        self.version += 1;
        self.changes.push_back((self.version, changes));
        let mut kept: usize = self.changes.iter().map(|(_, c)| c.len()).sum();
        while self.changes.len() > CHANGE_LOG
            || (kept > CHANGE_LOG_CHANGES && self.changes.len() > 1)
        {
            if let Some((_, gone)) = self.changes.pop_front() {
                kept -= gone.len();
            }
        }
    }

    /// Moves the cursors without modifying the text (closes the current undo group).
    pub fn set_selections(&mut self, selections: Selections) {
        self.selections = selections.clamp(self.buffer.len());
        self.history.seal();
    }

    /// What the document holds in memory: its text and its undo / redo history (bytes).
    pub fn memory(&self) -> (usize, usize) {
        (self.buffer.len(), self.history.bytes())
    }

    pub fn undo(&mut self) -> bool {
        if self.read_only {
            self.refused = true;
            return false;
        }
        match self.history.undo(&mut self.buffer) {
            Some((s, changes)) => {
                self.selections = s.clamp(self.buffer.len());
                self.bump(changes);
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        if self.read_only {
            self.refused = true;
            return false;
        }
        match self.history.redo(&mut self.buffer) {
            Some((s, changes)) => {
                self.selections = s.clamp(self.buffer.len());
                self.bump(changes);
                true
            }
            None => false,
        }
    }

    /// The content in the on-disk format (original encoding + line terminator).
    pub fn encoded(&self) -> Vec<u8> {
        encode_chunks(self.buffer.chunks(), self.buffer.len(), self.encoding, self.line_ending)
    }

    /// Marks the document as saved at `path`.
    /// Changes the document's path (e.g. the file was renamed), without touching the save
    /// state (unsaved changes remain).
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = Some(path);
    }

    /// The file changed on disk (another program, a download): the text becomes the one
    /// read (`decoded`), as one undo step replacing only what differs (the cursors stay
    /// where they were), and it counts as saved. `false` if it was the same text.
    pub fn reload(&mut self, decoded: Decoded) -> bool {
        self.encoding = decoded.encoding;
        self.line_ending = decoded.line_ending;
        self.saved_format = (decoded.encoding, decoded.line_ending);
        let current = self.buffer.to_string();
        let new = decoded.text;
        if current == new {
            self.saved_version = self.version;
            return false;
        }
        // The common start and end (on character boundaries): only the middle changes.
        let mut start = current.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
        while !current.is_char_boundary(start) || !new.is_char_boundary(start) {
            start -= 1;
        }
        let room = current.len().min(new.len()) - start;
        let mut end = current
            .bytes()
            .rev()
            .zip(new.bytes().rev())
            .take(room)
            .take_while(|(a, b)| a == b)
            .count();
        while !current.is_char_boundary(current.len() - end)
            || !new.is_char_boundary(new.len() - end)
        {
            end -= 1;
        }
        let tx = Transaction::single(start..current.len() - end, &new[start..new.len() - end]);
        self.history.seal();
        // Locked or not: the file's text as it is now.
        self.apply_external(tx, None, EditKind::Command);
        self.history.seal();
        self.saved_version = self.version;
        true
    }

    pub fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_version = self.version;
        self.saved_format = (self.encoding, self.line_ending);
        self.history.seal();
    }

    /// Version `version` in `format` was written to its file (a save finished elsewhere,
    /// meanwhile typed on: still unsaved then, against what was written). Its path stays.
    pub fn mark_saved_version(&mut self, version: u64, format: (Encoding, LineEnding)) {
        if version > self.version {
            return;
        }
        self.saved_version = version;
        self.saved_format = format;
        // The written version is the one in the editor: the next typing starts a new step.
        if version == self.version {
            self.history.seal();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/document.rs"]
mod tests;
