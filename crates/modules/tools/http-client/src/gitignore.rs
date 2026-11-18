//! The user's own environment files kept out of the repository: their names added to the
//! project's `.gitignore` (a pattern without a slash matches them in every folder), unless
//! it ignores them already.

use std::path::Path;

/// What's ignored: the private and the local environment files.
pub const IGNORED: &[&str] = &[http_requests::env::PRIVATE, http_requests::env::LOCAL];

/// `gitignore`'s text with the lines of [`IGNORED`] it lacks added (`None`: none lacked).
pub fn with_ignored(gitignore: &str) -> Option<String> {
    let lacking: Vec<&str> = IGNORED
        .iter()
        .copied()
        .filter(|name| !gitignore.lines().any(|l| l.trim().trim_start_matches(['/', '*']) == *name))
        .collect();
    if lacking.is_empty() {
        return None;
    }
    let mut out = gitignore.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("# The HTTP client's private environments (the user's own)\n");
    for name in lacking {
        out.push_str(name);
        out.push('\n');
    }
    Some(out)
}

/// `root`'s `.gitignore` made to ignore them, when `root` is a repository. Whether it was
/// changed.
pub fn ensure(root: &Path) -> bool {
    if !root.join(".git").exists() {
        return false;
    }
    let path = root.join(".gitignore");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    with_ignored(&text).is_some_and(|text| files::write_atomic(&path, text.as_bytes()).is_ok())
}

#[cfg(test)]
#[path = "../tests/unit/gitignore.rs"]
mod tests;
