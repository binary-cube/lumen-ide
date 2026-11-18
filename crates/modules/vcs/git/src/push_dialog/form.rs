//! What the Push… dialog has chosen so far, apart from how it's drawn.

use i18n::t;
use ui_components::ds::Check;
use ui_kit::TextInput;
use version_control::{PushPlan, PushedBranch, RefNameError, Remote, SyncState, check_ref_name};

use super::PushRequest;

/// A row of the branches' table.
#[derive(Debug)]
pub(crate) struct BranchLine {
    pub local: String,
    /// What it tracks now (`origin/main`).
    upstream: Option<String>,
    pub push: bool,
    pub remote_name: TextInput,
    pub track: bool,
    /// The name there, as it would be without the user's typing.
    default_name: String,
}

impl BranchLine {
    /// The name there on `remote` and whether it tracks it, unless told otherwise: what it
    /// tracks there, else its own name (a branch tracking another remote keeps doing so).
    fn defaults(local: &str, upstream: Option<&str>, remote: &str) -> (String, bool) {
        match upstream {
            None => (local.to_owned(), true),
            Some(upstream) => match upstream.strip_prefix(&format!("{remote}/")) {
                Some(name) => (name.to_owned(), true),
                None => (local.to_owned(), false),
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct PushForm {
    pub remotes: Vec<Remote>,
    pub remote: usize,
    pub lines: Vec<BranchLine>,
    pub tags: bool,
    pub force: bool,
}

impl PushForm {
    pub(crate) fn new(request: &PushRequest) -> Self {
        let upstream_of = |branch: &str| {
            let state: Option<&SyncState> = request.states.iter().find(|s| s.branch == branch);
            state.and_then(|s| s.upstream.clone())
        };
        let remote = Self::first_remote(request, &upstream_of);
        let name = request.remotes.get(remote).map_or("", |r| r.name.as_str());
        let lines = request
            .branches
            .iter()
            .map(|local| {
                let upstream = upstream_of(local);
                let (default_name, track) = BranchLine::defaults(local, upstream.as_deref(), name);
                BranchLine {
                    local: local.clone(),
                    upstream,
                    push: request.branch.as_deref() == Some(local),
                    remote_name: TextInput::new(&default_name),
                    track,
                    default_name,
                }
            })
            .collect();
        Self { remotes: request.remotes.clone(), remote, lines, tags: false, force: false }
    }

    /// The remote asked for, else the one the branch tracks, else `origin`, else the first.
    fn first_remote(request: &PushRequest, upstream_of: &dyn Fn(&str) -> Option<String>) -> usize {
        let at = |name: &str| request.remotes.iter().position(|r| r.name == name);
        let tracked = || {
            let upstream = upstream_of(request.branch.as_deref()?)?;
            request.remotes.iter().position(|r| upstream.starts_with(&format!("{}/", r.name)))
        };
        request
            .remote
            .as_deref()
            .and_then(at)
            .or_else(tracked)
            .or_else(|| at("origin"))
            .unwrap_or(0)
    }

    pub(crate) fn remote_name(&self) -> &str {
        self.remotes.get(self.remote).map_or("", |r| r.name.as_str())
    }

    /// Remote `i` chosen: the names nobody typed follow it.
    pub(crate) fn choose_remote(&mut self, i: usize) {
        if i >= self.remotes.len() || i == self.remote {
            return;
        }
        self.remote = i;
        let remote = self.remotes[i].name.clone();
        for line in &mut self.lines {
            let (name, track) =
                BranchLine::defaults(&line.local, line.upstream.as_deref(), &remote);
            if line.remote_name.text() == line.default_name {
                line.remote_name = TextInput::new(&name);
            }
            line.default_name = name;
            line.track = track;
        }
    }

    /// The "Select All" box: ticked when all are, a dash when only some.
    pub(crate) fn all_state(&self) -> Check {
        let chosen = self.lines.iter().filter(|l| l.push).count();
        match chosen {
            0 => Check::Off,
            n if n == self.lines.len() => Check::On,
            _ => Check::Mixed,
        }
    }

    /// All ticked, or none if they all were.
    pub(crate) fn toggle_all(&mut self) {
        let on = self.all_state() != Check::On;
        for line in &mut self.lines {
            line.push = on;
        }
    }

    /// Why it can't go yet.
    pub(crate) fn problem(&self) -> Option<String> {
        let mut chosen = self.lines.iter().filter(|l| l.push).peekable();
        if chosen.peek().is_none() && !self.tags {
            return Some(t("git.push_nothing_chosen"));
        }
        chosen.find_map(|line| check_ref_name(line.remote_name.text().trim()).err()).map(|error| {
            t(match error {
                RefNameError::Empty => "git.ref_empty",
                RefNameError::BadCharacter => "git.ref_bad_character",
                RefNameError::BadShape | RefNameError::Exists => "git.ref_bad_shape",
            })
        })
    }

    /// What's sent.
    pub(crate) fn plan(&self) -> PushPlan {
        let branches = self
            .lines
            .iter()
            .filter(|l| l.push)
            .map(|l| PushedBranch {
                local: l.local.clone(),
                remote: l.remote_name.text().trim().to_owned(),
                track: l.track,
            })
            .collect();
        PushPlan {
            remote: self.remote_name().to_owned(),
            branches,
            tags: self.tags,
            force: self.force,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/push_dialog/form.rs"]
mod tests;
