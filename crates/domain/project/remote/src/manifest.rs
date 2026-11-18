//! What each file was at its last transfer, on both sides: the server's size and date, the
//! local copy's size, date and hash. A side changed when it no longer matches; the local
//! hash is only computed when its size or date moved (files touched but unchanged aren't
//! sent again).

use std::collections::BTreeMap;

/// A file as it was when it was last transferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Synced {
    pub remote_size: u64,
    pub remote_modified: i64,
    pub local_size: u64,
    pub local_modified: i64,
    pub local_hash: u64,
}

/// The transferred files (relative paths, `/`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    files: BTreeMap<String, Synced>,
}

impl Manifest {
    pub fn get(&self, path: &str) -> Option<&Synced> {
        self.files.get(path)
    }

    pub fn set(&mut self, path: &str, synced: Synced) {
        self.files.insert(path.to_owned(), synced);
    }

    pub fn remove(&mut self, path: &str) {
        self.files.remove(path);
    }

    /// A file under folder `dir` was transferred.
    pub fn has_under(&self, dir: &str) -> bool {
        let prefix = format!("{dir}/");
        self.files.range(prefix.clone()..).next().is_some_and(|(path, _)| path.starts_with(&prefix))
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// One line per file: `path\tremote size\tremote date\tlocal size\tlocal date\thash`.
    pub fn to_text(&self) -> String {
        let mut out = String::from("# remote-manifest 1\n");
        for (path, s) in &self.files {
            out.push_str(&format!(
                "{path}\t{}\t{}\t{}\t{}\t{:016x}\n",
                s.remote_size, s.remote_modified, s.local_size, s.local_modified, s.local_hash
            ));
        }
        out
    }

    /// Read back from [`to_text`](Self::to_text); broken lines are skipped (the file is
    /// then simply transferred again).
    pub fn from_text(text: &str) -> Self {
        let files = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .filter_map(|line| {
                let mut f = line.split('\t');
                let path = f.next()?.to_owned();
                let synced = Synced {
                    remote_size: f.next()?.parse().ok()?,
                    remote_modified: f.next()?.parse().ok()?,
                    local_size: f.next()?.parse().ok()?,
                    local_modified: f.next()?.parse().ok()?,
                    local_hash: u64::from_str_radix(f.next()?, 16).ok()?,
                };
                Some((path, synced))
            })
            .collect();
        Self { files }
    }
}

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
