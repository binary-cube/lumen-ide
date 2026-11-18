//! Whether git may run in a repository. Its own settings (`.git/config`) come with it — from
//! an archive, a clone — and some run programs on everyday commands (`status`, `blame`): a
//! filter, a diff driver, a hooks folder… A repository without any runs as always; one with
//! them only once the user trusted it as it is (the lines kept with the user's settings,
//! never in the project: changed ones ask again).

use std::path::{Path, PathBuf};

/// The keys that run a program, by section (`filter.<x>.clean`, `diff.<x>.textconv`…).
/// `core.fsmonitor` and `protocol.ext` are turned off on every run anyway.
fn runs_a_program(section: &str, key: &str) -> bool {
    let (name, sub) = section.split_once(' ').unwrap_or((section, ""));
    match name {
        "filter" => matches!(key, "clean" | "smudge" | "process"),
        "core" => {
            matches!(key, "hookspath" | "sshcommand" | "pager" | "editor" | "askpass" | "gitproxy")
        }
        "diff" => {
            matches!(key, "external") || (!sub.is_empty() && matches!(key, "command" | "textconv"))
        }
        "merge" => !sub.is_empty() && key == "driver",
        "credential" => key == "helper",
        "gpg" => key == "program",
        "pager" | "include" | "includeif" => true,
        "sequence" => key == "editor",
        _ => false,
    }
}

/// The repository's own config file (`.git/config`, or the one a `.git` file points to: a
/// worktree, a submodule), for any folder in it.
fn config_file(dir: &Path) -> Option<PathBuf> {
    let root = repository_root(dir)?;
    let dot_git = root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git.join("config"));
    }
    let pointer = std::fs::read_to_string(&dot_git).ok()?;
    let dir = pointer.lines().find_map(|l| l.strip_prefix("gitdir:"))?.trim();
    let dir = if Path::new(dir).is_absolute() { PathBuf::from(dir) } else { root.join(dir) };
    // A worktree's own folder: the repository's config is in the common one.
    let common = std::fs::read_to_string(dir.join("commondir"))
        .ok()
        .map(|c| dir.join(c.trim()))
        .unwrap_or(dir);
    Some(common.join("config"))
}

/// The repository `dir` is in (the folder holding its `.git`).
fn repository_root(dir: &Path) -> Option<&Path> {
    dir.ancestors().find(|d| d.join(".git").exists())
}

/// The lines of `root`'s own config that run programs (none: git may run there freely).
pub fn risky(root: &Path) -> Vec<String> {
    let Some(text) = config_file(root).and_then(|f| std::fs::read_to_string(f).ok()) else {
        return Vec::new();
    };
    let mut section = String::new();
    let mut found = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(['#', ';']) {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            let header = header.split(']').next().unwrap_or("");
            section = header.replace('"', "").trim().to_lowercase();
            // `[section.sub]`, the old way of writing `[section "sub"]`.
            if !section.contains(' ')
                && let Some((name, sub)) = section.split_once('.')
            {
                section = format!("{name} {sub}");
            }
            continue;
        }
        let key = line.split(['=', ' ', '\t']).next().unwrap_or("").to_lowercase();
        if runs_a_program(&section, &key) {
            found.push(format!("[{section}] {line}"));
        }
    }
    found
}

/// Where the trusted repositories are kept (with the user's settings).
fn trusted_file() -> Option<PathBuf> {
    // Tests never touch the user's settings.
    if cfg!(test) {
        let thread = format!("{:?}", std::thread::current().id());
        let name = format!(
            "trusted-repositories-{}-{}",
            std::process::id(),
            thread.replace(['(', ')'], "")
        );
        return Some(std::env::temp_dir().join(name));
    }
    files::places::config_file("trusted-repositories")
}

/// `root` trusted with `lines`: its line in the list.
fn entry(root: &Path, lines: &[String]) -> String {
    let hash = lines
        .join("\n")
        .bytes()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{}\t{hash:016x}", root.display().to_string().escape_default())
}

/// Whether git may run in `root`: nothing in its own config runs a program, or the user
/// trusted it as it is now. `Err`: the lines asking for trust.
pub fn check(dir: &Path) -> Result<(), Vec<String>> {
    let lines = risky(dir);
    if lines.is_empty() {
        return Ok(());
    }
    let root = repository_root(dir).unwrap_or(dir);
    let wanted = entry(root, &lines);
    let trusted = trusted_file()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .is_some_and(|text| text.lines().any(|l| l == wanted));
    if trusted { Ok(()) } else { Err(lines) }
}

/// Trusts `root` as its config is now (a changed one asks again).
pub fn trust(dir: &Path) -> std::io::Result<()> {
    let Some(file) = trusted_file() else { return Ok(()) };
    let root = repository_root(dir).unwrap_or(dir);
    let line = entry(root, &risky(root));
    let prefix = format!("{}\t", root.display().to_string().escape_default());
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let mut lines: Vec<&str> = text.lines().filter(|l| !l.starts_with(&prefix)).collect();
    lines.push(&line);
    files::write_private(&file, (lines.join("\n") + "\n").as_bytes())
}

#[cfg(test)]
#[path = "../tests/unit/trust.rs"]
mod tests;
