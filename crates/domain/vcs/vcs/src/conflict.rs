//! Conflicts: a file a merge / rebase / cherry-pick left with conflict markers, read into
//! the parts both sides agree on and the hunks they don't (each hunk: our lines, their lines
//! and, with `diff3` / `zdiff3`, the base's), each hunk resolved by a choice; and what may be
//! done with the operation that stopped on them. Pure: no I/O.

use std::ops::Range;

/// One side of a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The version being merged into (`HEAD`; during a rebase: the branch rebased onto).
    Ours,
    /// The version being brought in (during a rebase: the commit being replayed).
    Theirs,
}

/// A version of a conflicted file kept by the repository while the conflict lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// The common ancestor (absent when both sides added the file).
    Base,
    Ours,
    Theirs,
}

impl From<Side> for Stage {
    fn from(side: Side) -> Self {
        match side {
            Side::Ours => Self::Ours,
            Side::Theirs => Self::Theirs,
        }
    }
}

/// How a conflict hunk is resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Ours,
    Theirs,
    /// Our lines, then theirs.
    OursThenTheirs,
    /// Their lines, then ours.
    TheirsThenOurs,
    /// The base's lines (neither change; only when the markers kept them).
    Base,
    /// Lines written by hand.
    Text(String),
}

/// A hunk the two sides changed differently. Its texts end with their line endings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub ours: String,
    /// The base's lines (`diff3` / `zdiff3` markers only).
    pub base: Option<String>,
    pub theirs: String,
    /// What follows `<<<<<<<` (e.g. `HEAD`).
    pub ours_label: String,
    /// What follows `>>>>>>>` (e.g. `275df80 (Cart fix)`).
    pub theirs_label: String,
    /// What follows `|||||||` (e.g. `merged common ancestors`).
    pub base_label: String,
}

impl Hunk {
    /// Its lines once resolved by `choice`.
    pub fn resolved(&self, choice: &Choice) -> String {
        match choice {
            Choice::Ours => self.ours.clone(),
            Choice::Theirs => self.theirs.clone(),
            Choice::OursThenTheirs => joined(&self.ours, &self.theirs),
            Choice::TheirsThenOurs => joined(&self.theirs, &self.ours),
            Choice::Base => self.base.clone().unwrap_or_default(),
            Choice::Text(text) => text.clone(),
        }
    }

    /// The hunk written back with its markers, as git wrote it.
    fn marked(&self, newline: &str) -> String {
        let mut text = marker(OPEN, &self.ours_label, newline);
        text.push_str(&ended(&self.ours, newline));
        if let Some(base) = &self.base {
            text.push_str(&marker(BASE, &self.base_label, newline));
            text.push_str(&ended(base, newline));
        }
        text.push_str(&marker(SEPARATOR, "", newline));
        text.push_str(&ended(&self.theirs, newline));
        text.push_str(&marker(CLOSE, &self.theirs_label, newline));
        text
    }
}

/// A part of a conflicted file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Lines both sides agree on (or a hunk already resolved).
    Common(String),
    Conflict(Hunk),
}

/// A file's text with its conflict markers read: what's agreed, and the hunks still to
/// resolve. Written back by [`Merged::text`] (unresolved hunks keep their markers).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Merged {
    pieces: Vec<Piece>,
    /// The file's line ending (`\r\n` on files written with it), for the markers written back.
    newline: &'static str,
}

const OPEN: &str = "<<<<<<<";
const BASE: &str = "|||||||";
const SEPARATOR: &str = "=======";
const CLOSE: &str = ">>>>>>>";

impl Merged {
    /// `text` read for its markers. Markers that don't make a whole hunk (a `<<<<<<<` never
    /// closed, a `=======` alone: text that only looks like one) stay plain text.
    pub fn parse(text: &str) -> Self {
        let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let mut pieces = Vec::new();
        let mut common = String::new();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut at = 0;
        while at < lines.len() {
            if let Some(label) = marker_label(lines[at], OPEN)
                && let Some((hunk, next)) = hunk_at(&lines, at + 1, label)
            {
                if !common.is_empty() {
                    pieces.push(Piece::Common(std::mem::take(&mut common)));
                }
                pieces.push(Piece::Conflict(hunk));
                at = next;
                continue;
            }
            common.push_str(lines[at]);
            at += 1;
        }
        if !common.is_empty() {
            pieces.push(Piece::Common(common));
        }
        Self { pieces, newline }
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    /// The hunks still to resolve, in order.
    pub fn hunks(&self) -> impl Iterator<Item = &Hunk> {
        self.pieces.iter().filter_map(|p| match p {
            Piece::Conflict(hunk) => Some(hunk),
            Piece::Common(_) => None,
        })
    }

    /// How many hunks are still to resolve.
    pub fn unresolved(&self) -> usize {
        self.hunks().count()
    }

    pub fn is_resolved(&self) -> bool {
        self.unresolved() == 0
    }

    /// The `index`th unresolved hunk resolved by `choice` (it becomes agreed text); `false`
    /// if there's no such hunk.
    pub fn resolve(&mut self, index: usize, choice: &Choice) -> bool {
        let Some(at) = self.position(index) else { return false };
        let Piece::Conflict(hunk) = &self.pieces[at] else { return false };
        self.pieces[at] = Piece::Common(hunk.resolved(choice));
        true
    }

    /// Every unresolved hunk resolved by `choice`.
    pub fn resolve_all(&mut self, choice: &Choice) {
        for piece in &mut self.pieces {
            if let Piece::Conflict(hunk) = piece {
                *piece = Piece::Common(hunk.resolved(choice));
            }
        }
    }

    /// The text: agreed parts as they are, unresolved hunks with their markers.
    pub fn text(&self) -> String {
        self.pieces
            .iter()
            .map(|piece| match piece {
                Piece::Common(text) => text.clone(),
                Piece::Conflict(hunk) => hunk.marked(self.newline),
            })
            .collect()
    }

    /// The whole file as one side has it (every unresolved hunk taken from `side`).
    pub fn side(&self, side: Side) -> String {
        let choice = match side {
            Side::Ours => Choice::Ours,
            Side::Theirs => Choice::Theirs,
        };
        let mut all = self.clone();
        all.resolve_all(&choice);
        all.text()
    }

    /// The lines (0-based, end exclusive) the `index`th unresolved hunk takes in
    /// [`Merged::text`], markers included.
    pub fn hunk_lines(&self, index: usize) -> Option<Range<usize>> {
        let at = self.position(index)?;
        let start: usize = self.pieces[..at].iter().map(|p| self.line_count(p)).sum();
        Some(start..start + self.line_count(&self.pieces[at]))
    }

    fn position(&self, index: usize) -> Option<usize> {
        self.pieces
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p, Piece::Conflict(_)))
            .nth(index)
            .map(|(at, _)| at)
    }

    fn line_count(&self, piece: &Piece) -> usize {
        match piece {
            Piece::Common(text) => text.split_inclusive('\n').count(),
            Piece::Conflict(hunk) => hunk.marked(self.newline).split_inclusive('\n').count(),
        }
    }
}

/// The text after `marker` when `line` is that marker (exactly seven characters, then a
/// space and a label, or the line's end).
fn marker_label<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(marker)?;
    let rest = rest.trim_end_matches(['\n', '\r']);
    if rest.is_empty() { Some("") } else { rest.strip_prefix(' ') }
}

/// The hunk whose lines start at `at` (after its `<<<<<<<`), and the line after its
/// `>>>>>>>`; `None` when the markers don't close it.
fn hunk_at(lines: &[&str], mut at: usize, ours_label: &str) -> Option<(Hunk, usize)> {
    #[derive(PartialEq)]
    enum Part {
        Ours,
        Base,
        Theirs,
    }
    let mut part = Part::Ours;
    let (mut ours, mut base, mut theirs) = (String::new(), None::<String>, String::new());
    let mut base_label = String::new();
    while at < lines.len() {
        let line = lines[at];
        at += 1;
        match part {
            Part::Ours | Part::Base => {
                if marker_label(line, OPEN).is_some() {
                    // Another hunk opens before this one closes: not a hunk.
                    return None;
                }
                if part == Part::Ours
                    && let Some(label) = marker_label(line, BASE)
                {
                    base_label = label.to_owned();
                    base = Some(String::new());
                    part = Part::Base;
                } else if marker_label(line, SEPARATOR) == Some("") {
                    part = Part::Theirs;
                } else if part == Part::Ours {
                    ours.push_str(line);
                } else if let Some(base) = &mut base {
                    base.push_str(line);
                }
            }
            Part::Theirs => {
                if let Some(label) = marker_label(line, CLOSE) {
                    let hunk = Hunk {
                        ours,
                        base,
                        theirs,
                        ours_label: ours_label.to_owned(),
                        theirs_label: label.to_owned(),
                        base_label,
                    };
                    return Some((hunk, at));
                }
                if marker_label(line, OPEN).is_some() {
                    return None;
                }
                theirs.push_str(line);
            }
        }
    }
    None
}

/// A marker line.
fn marker(marker: &str, label: &str, newline: &str) -> String {
    if label.is_empty() {
        format!("{marker}{newline}")
    } else {
        format!("{marker} {label}{newline}")
    }
}

/// `text` ending with a line ending (the last line of a file may have none, but a marker
/// must start a line).
fn ended(text: &str, newline: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_owned()
    } else {
        format!("{text}{newline}")
    }
}

/// `first` then `second`, on separate lines.
fn joined(first: &str, second: &str) -> String {
    if first.is_empty() || first.ends_with('\n') || second.is_empty() {
        format!("{first}{second}")
    } else {
        format!("{first}\n{second}")
    }
}

/// What may be done with an operation stopped in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationStep {
    /// Carry on once its conflicts are resolved (the merge committed, the next commit
    /// replayed).
    Continue,
    /// Leave out the commit it stopped on and carry on.
    Skip,
    /// Give it up: everything back as it was before it started.
    Abort,
}

impl crate::OperationKind {
    /// Whether `step` can be done with it (a merge has nothing to skip; a bisect is only
    /// ended).
    pub fn allows(self, step: OperationStep) -> bool {
        use crate::OperationKind::{ApplyMailbox, Bisect, CherryPick, Merge, Rebase, Revert};
        match (self, step) {
            (_, OperationStep::Abort) => true,
            (Rebase | CherryPick | Revert | ApplyMailbox, _) => true,
            (Merge, OperationStep::Continue) => true,
            (Merge, OperationStep::Skip) | (Bisect, _) => false,
        }
    }

    /// Whether it stops on conflicts (a bisect never does).
    pub fn conflicts(self) -> bool {
        self != crate::OperationKind::Bisect
    }
}

/// Where an operation got to after a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proceeded {
    /// It's over (done or given up).
    Finished,
    /// It stopped again (new conflicts, or a commit to edit): still in progress.
    Stopped,
}

#[cfg(test)]
#[path = "../tests/unit/conflict.rs"]
mod tests;
