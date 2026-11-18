//! What was moved or deleted here and not yet on the server, while uploads wait for the user
//! (automatic uploads off): kept per file the server has, by its name here and its name on
//! the server, so a file moved twice is one move, moved back is none, and a moved file
//! deleted is the server's file deleted. Files never sent aren't kept (they're simply new
//! here); edits are found by the manifest, not kept here.

use std::collections::{BTreeMap, BTreeSet};

/// The moves and deletions waiting (relative paths, `/`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outbox {
    /// By the name here: the name on the server.
    renamed: BTreeMap<String, String>,
    /// The server's names of files deleted here.
    deleted: BTreeSet<String>,
}

/// `path` is `dir` or under it.
fn under(path: &str, dir: &str) -> bool {
    path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/'))
}

/// `path` (under `from`) with `from` replaced by `to`.
fn moved_to(path: &str, from: &str, to: &str) -> String {
    format!("{to}{}", &path[from.len()..])
}

impl Outbox {
    /// Nothing waits.
    pub fn is_empty(&self) -> bool {
        self.renamed.is_empty() && self.deleted.is_empty()
    }

    /// The moves waiting: the name here, the name on the server.
    pub fn renamed(&self) -> impl Iterator<Item = (&str, &str)> {
        self.renamed.iter().map(|(here, there)| (here.as_str(), there.as_str()))
    }

    /// The server's names of the files deleted here.
    pub fn deleted(&self) -> impl Iterator<Item = &str> {
        self.deleted.iter().map(String::as_str)
    }

    /// The name on the server of the file named `here` here, if it was moved.
    pub fn server_name(&self, here: &str) -> Option<&str> {
        self.renamed.get(here).map(String::as_str)
    }

    /// The server's file `there` is one this list already speaks of (moved or deleted).
    fn claimed(&self, there: &str) -> bool {
        self.deleted.contains(there) || self.renamed.values().any(|t| t == there)
    }

    /// `from` (a file or a folder) moved here to `to`; `sent`: the server's files at or under
    /// `from` (the manifest's).
    pub fn moved(&mut self, from: &str, to: &str, sent: &[String]) {
        if from == to {
            return;
        }
        // Moved before: they keep their name on the server.
        let earlier: Vec<String> =
            self.renamed.keys().filter(|here| under(here, from)).cloned().collect();
        for here in earlier {
            if let Some(there) = self.renamed.remove(&here) {
                self.renamed.insert(moved_to(&here, from, to), there);
            }
        }
        // Never moved: where they are here is their name on the server.
        for there in sent.iter().filter(|s| under(s, from)) {
            if !self.claimed(there) {
                self.renamed.insert(moved_to(there, from, to), there.clone());
            }
        }
        // Moved back where they were: nothing to do on the server.
        self.renamed.retain(|here, there| here != there);
    }

    /// `path` (a file or a folder) deleted here; `sent`: the server's files at or under it.
    pub fn removed(&mut self, path: &str, sent: &[String]) {
        let moved: Vec<String> =
            self.renamed.keys().filter(|here| under(here, path)).cloned().collect();
        for here in moved {
            if let Some(there) = self.renamed.remove(&here) {
                self.deleted.insert(there);
            }
        }
        for there in sent.iter().filter(|s| under(s, path)) {
            if !self.claimed(there) {
                self.deleted.insert(there.clone());
            }
        }
    }

    /// A file is here again at `path` (made anew where one was deleted): it's an edit of the
    /// server's, no longer a deletion. `true` if that changed the list.
    pub fn revived(&mut self, path: &str) -> bool {
        self.deleted.remove(path)
    }

    /// The move to `here` (a file, or the files under a folder) was made on the server.
    pub fn rename_done(&mut self, here: &str) {
        self.renamed.retain(|h, _| !under(h, here));
    }

    /// The server's `there` (a file, or the files under a folder) was deleted there.
    pub fn delete_done(&mut self, there: &str) {
        self.deleted.retain(|d| !under(d, there));
    }

    /// One line per entry: `R\there\tthere`, `D\tthere`.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# remote-outbox 1\n");
        for (here, there) in &self.renamed {
            out.push_str(&format!("R\t{here}\t{there}\n"));
        }
        for there in &self.deleted {
            out.push_str(&format!("D\t{there}\n"));
        }
        out
    }

    /// Read back from [`to_text`](Self::to_text); broken lines are skipped.
    pub fn from_text(text: &str) -> Self {
        let mut outbox = Self::default();
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let mut f = line.split('\t');
            match (f.next(), f.next(), f.next()) {
                (Some("R"), Some(here), Some(there)) if !here.is_empty() && !there.is_empty() => {
                    outbox.renamed.insert(here.to_owned(), there.to_owned());
                }
                (Some("D"), Some(there), None) if !there.is_empty() => {
                    outbox.deleted.insert(there.to_owned());
                }
                _ => {}
            }
        }
        outbox
    }
}

#[cfg(test)]
#[path = "../tests/unit/outbox.rs"]
mod tests;
