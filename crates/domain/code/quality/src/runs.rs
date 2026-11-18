//! The runs in progress: two runs of the same kind on the same target never overlap (a
//! formatter rewriting a file while another rewrites it would lose one's work). One asked
//! for while another runs waits — and asked for again meanwhile, it runs once.

use std::collections::{HashMap, HashSet};

use crate::{Kind, Target};

/// Whether a run starts now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Start it.
    Now,
    /// Another runs on it: this one runs after it (once, however many times asked).
    Queued,
}

/// The runs in progress and those waiting, by target and kind.
#[derive(Debug, Default)]
pub struct Runs {
    running: HashSet<(Target, Kind)>,
    waiting: HashMap<(Target, Kind), u32>,
}

impl Runs {
    /// A run of `kind` on `target` is asked for.
    pub fn ask(&mut self, target: &Target, kind: Kind) -> Start {
        let key = (target.clone(), kind);
        if self.running.contains(&key) {
            *self.waiting.entry(key).or_default() += 1;
            return Start::Queued;
        }
        // The whole project's run covers every file's (and the other way round): one at a
        // time.
        if self.overlaps(target, kind) {
            *self.waiting.entry(key).or_default() += 1;
            return Start::Queued;
        }
        self.running.insert(key);
        Start::Now
    }

    fn overlaps(&self, target: &Target, kind: Kind) -> bool {
        self.running.iter().any(|(t, k)| {
            *k == kind && (matches!(t, Target::Project) || matches!(target, Target::Project))
        })
    }

    /// The run of `kind` on `target` ended: the runs to start now (those waiting that no
    /// longer overlap; each once, however many times they were asked for).
    pub fn finished(&mut self, target: &Target, kind: Kind) -> Vec<Target> {
        self.running.remove(&(target.clone(), kind));
        let ready: Vec<Target> =
            self.waiting.keys().filter(|(_, k)| *k == kind).map(|(t, _)| t.clone()).collect();
        let mut started = Vec::new();
        for target in ready {
            if self.running.contains(&(target.clone(), kind)) || self.overlaps(&target, kind) {
                continue;
            }
            self.waiting.remove(&(target.clone(), kind));
            self.running.insert((target.clone(), kind));
            started.push(target);
        }
        started
    }

    /// Something runs (for the status bar).
    pub fn busy(&self) -> bool {
        !self.running.is_empty()
    }
}

#[cfg(test)]
#[path = "../tests/unit/runs.rs"]
mod tests;
