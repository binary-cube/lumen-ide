//! Switching branches and stashes, through the installed `git`: what differs on a target
//! (for the check before a switch, in batches of paths), the switch itself, and the stashes
//! made, applied, dropped and turned into branches — a stash found by its commit, never by
//! a position that moves.

use std::sync::OnceLock;

use version_control::{StashOptions, SwitchTarget, VcsError};

use crate::GitRepository;

/// Paths per `git` call (a command line stays well under every system's limit).
const BATCH: usize = 200;

/// A revision or a branch name that could be read as an option.
fn checked(name: &str) -> Result<&str, VcsError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(VcsError::Failed(format!("not a revision: {name}")));
    }
    Ok(name)
}

/// `-z` output's names.
fn names(out: &str) -> impl Iterator<Item = String> + '_ {
    out.split('\0').filter(|s| !s.is_empty()).map(str::to_owned)
}

impl GitRepository {
    /// Of `paths`, those whose committed content differs between `HEAD` and `revision`.
    pub(crate) fn changed_between(
        &self,
        revision: &str,
        paths: &[String],
    ) -> Result<Vec<String>, VcsError> {
        let revision = checked(revision)?;
        if version_control::Repository::head_id(self).is_none() {
            // Nothing committed yet: what the target has differs.
            return self.present_in(revision, paths);
        }
        let mut found = Vec::new();
        for batch in paths.chunks(BATCH) {
            let mut args =
                vec!["--literal-pathspecs", "diff", "--name-only", "-z", "HEAD", revision, "--"];
            args.extend(batch.iter().map(String::as_str));
            found.extend(names(&self.git(&args)?));
        }
        Ok(found)
    }

    /// Of `paths`, those that exist in `revision`.
    pub(crate) fn present_in(
        &self,
        revision: &str,
        paths: &[String],
    ) -> Result<Vec<String>, VcsError> {
        let revision = checked(revision)?;
        let mut found = Vec::new();
        for batch in paths.chunks(BATCH) {
            let mut args =
                vec!["--literal-pathspecs", "ls-tree", "-r", "-z", "--name-only", revision, "--"];
            args.extend(batch.iter().map(String::as_str));
            found.extend(names(&self.git(&args)?));
        }
        Ok(found)
    }

    /// Switched to `target`; `discard`: the changes in the way thrown away.
    pub(crate) fn switch_to(&self, target: &SwitchTarget, discard: bool) -> Result<(), VcsError> {
        let mut args = vec!["switch"];
        if discard {
            args.push("--discard-changes");
        }
        match target {
            SwitchTarget::Branch(name) => args.push(checked(name)?),
            SwitchTarget::NewBranch { name, start, track } => {
                args.extend(["-c", checked(name)?]);
                args.push(if *track { "--track" } else { "--no-track" });
                args.push(checked(start)?);
            }
            SwitchTarget::Detached(revision) => args.extend(["--detach", checked(revision)?]),
        }
        self.git(&args).map(|_| ())
    }

    /// A stash made as `options` say.
    pub(crate) fn stash_push(&self, options: &StashOptions) -> Result<(), VcsError> {
        options.check().map_err(|c| VcsError::Failed(format!("stash options: {c:?}")))?;
        // Paths taken as they are only when there are any (git's own `:/` must stay magic).
        let mut args = if options.paths.is_empty() { vec![] } else { vec!["--literal-pathspecs"] };
        args.extend(["stash", "push"]);
        let message = options.message.as_deref().map(str::trim).filter(|m| !m.is_empty());
        if let Some(message) = message {
            args.extend(["-m", message]);
        }
        if options.keep_index {
            args.push("--keep-index");
        }
        if options.untracked {
            args.push("--include-untracked");
        }
        if options.staged_only {
            args.push("--staged");
        }
        if !options.paths.is_empty() {
            args.push("--");
            args.extend(options.paths.iter().map(String::as_str));
        }
        self.git(&args).map(|_| ())
    }

    /// The stash with commit `id`'s name now.
    fn stash_ref(&self, id: &str) -> Result<String, VcsError> {
        let stashes = version_control::Repository::stashes(self);
        version_control::stash_named(&stashes, id)
            .map(|s| s.name.clone())
            .ok_or_else(|| VcsError::Failed("the stash is no longer there".into()))
    }

    /// Stash `id` applied (what was staged staged again when it can be); dropped after when
    /// `pop` (git keeps it if it conflicted).
    pub(crate) fn stash_apply(&self, id: &str, pop: bool) -> Result<(), VcsError> {
        let name = self.stash_ref(id)?;
        let command = if pop { "pop" } else { "apply" };
        match self.git(&["stash", command, "--index", &name]) {
            Ok(_) => Ok(()),
            // The index couldn't be put back as it was: the changes alone.
            Err(VcsError::Failed(message)) if message.contains("--index") => {
                self.git(&["stash", command, &name]).map(|_| ())
            }
            Err(e) => Err(e),
        }
    }

    /// Stash `id` thrown away.
    pub(crate) fn stash_drop(&self, id: &str) -> Result<(), VcsError> {
        let name = self.stash_ref(id)?;
        self.git(&["stash", "drop", &name]).map(|_| ())
    }

    /// Branch `branch` made where stash `id` was made, checked out, the stash applied there
    /// and dropped.
    pub(crate) fn stash_branch(&self, id: &str, branch: &str) -> Result<(), VcsError> {
        let name = self.stash_ref(id)?;
        self.git(&["stash", "branch", checked(branch)?, &name]).map(|_| ())
    }

    /// Whether this `git` stashes only what's staged (2.35 and newer).
    pub(crate) fn stashes_staged(&self) -> bool {
        static AT_LEAST: OnceLock<bool> = OnceLock::new();
        *AT_LEAST.get_or_init(|| {
            let out = self.git(&["--version"]).unwrap_or_default();
            version_at_least(&out, (2, 35))
        })
    }
}

/// `git version 2.39.3 (Apple Git-146)` at least `min` (major, minor).
pub(crate) fn version_at_least(out: &str, min: (u32, u32)) -> bool {
    let Some(version) =
        out.split_whitespace().find(|w| w.starts_with(|c: char| c.is_ascii_digit()))
    else {
        return false;
    };
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    (major, minor) >= min
}
