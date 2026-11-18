//! Running the installed `git` (through the shared `process` runner: no console window on
//! Windows, stopped after a timeout): never waiting for a prompt nobody sees (passwords, SSH
//! passphrases, an editor for a merge message), its errors sorted out (missing tool,
//! authentication, no remote).

use std::path::Path;
use std::time::Duration;

use process::{Run, RunError, Running};

/// The `git`s running now (the Task Manager measures them).
pub static RUNNING: std::sync::LazyLock<std::sync::Arc<Running>> =
    std::sync::LazyLock::new(std::sync::Arc::default);
use version_control::VcsError;

/// What a finished `git` printed.
#[derive(Debug, Clone, Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// Its last meaningful line, for the user (git reports on stderr too).
    pub fn summary(&self) -> String {
        let last = |s: &str| s.lines().map(str::trim).rfind(|l| !l.is_empty()).map(str::to_owned);
        last(&self.stdout).or_else(|| last(&self.stderr)).unwrap_or_default()
    }
}

/// Runs `git args` in `dir`; stopped after `timeout`. `batch_ssh`: SSH fails instead of
/// asking for a passphrase (when the user didn't configure an SSH command).
pub fn run(
    dir: &Path,
    args: &[&str],
    timeout: Duration,
    batch_ssh: bool,
) -> Result<Output, VcsError> {
    // Programs its own config runs (a filter, a diff driver): only once it's trusted.
    if let Err(lines) = crate::trust::check(dir) {
        return Err(VcsError::Untrusted(lines));
    }
    // A repository's own config (`.git/config`) can come with a project from an archive:
    // nothing in it runs a program on its own (`core.fsmonitor` runs on every `status`), and
    // no remote URL runs one (`ext::`).
    let safe = ["-c", "core.fsmonitor=false", "-c", "protocol.ext.allow=never"];
    let mut spec = Run::new("git", safe.into_iter().chain(args.iter().copied()))
        .owner("git")
        .dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        // A status refreshes the index only when it may (never taking `index.lock` while the
        // user's own git, in a terminal, wants it).
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_EDITOR", "true")
        .timeout(timeout)
        .listed_in(std::sync::Arc::clone(&RUNNING));
    if batch_ssh && std::env::var_os("GIT_SSH_COMMAND").is_none() {
        spec = spec.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let finished = match process::run(&spec) {
        Ok(finished) => finished,
        Err(RunError::NotFound(_)) => return Err(VcsError::ToolMissing("git".into())),
        Err(RunError::TimedOut | RunError::Stopped) => return Err(VcsError::TimedOut),
        Err(RunError::Failed(why)) => return Err(VcsError::Failed(why)),
    };
    let success = finished.success();
    let output = Output { stdout: finished.stdout, stderr: finished.stderr };
    if success {
        Ok(output)
    } else {
        Err(classify(args.first().copied().unwrap_or("git"), &output.stderr))
    }
}

/// A failed `git`'s error, from what it said.
pub fn classify(command: &str, stderr: &str) -> VcsError {
    let message = stderr.trim();
    let lower = message.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| lower.contains(n));
    if has(&[
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "permission denied (publickey",
        "host key verification failed",
    ]) {
        VcsError::Authentication(last_line(message))
    } else if has(&[
        "no configured push destination",
        "does not appear to be a git repository",
        "no remote repository specified",
        "there is no tracking information",
    ]) {
        VcsError::NoRemote
    } else if message.is_empty() {
        VcsError::Failed(format!("git {command} failed"))
    } else {
        VcsError::Failed(message.to_owned())
    }
}

fn last_line(message: &str) -> String {
    message.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default().to_owned()
}

#[cfg(test)]
#[path = "../tests/unit/process.rs"]
mod tests;
