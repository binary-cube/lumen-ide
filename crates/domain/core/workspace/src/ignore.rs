//! `.gitignore` rules (the subset projects use): `#` comments, `!` negation, a trailing
//! `/` for directories only, patterns anchored to their file's directory when they
//! contain a `/`, and the wildcards `*`, `?`, `**` and `[…]`.

/// The rules collected while walking a project (each with the directory it came from).
#[derive(Debug, Clone, Default)]
pub struct IgnoreRules {
    rules: Vec<Rule>,
}

#[derive(Debug, Clone)]
struct Rule {
    /// The directory of the `.gitignore` (relative, `/`-separated; empty for the root).
    base: String,
    pattern: String,
    negate: bool,
    dir_only: bool,
    /// Matched against the path under `base`, else against the last component.
    anchored: bool,
}

impl IgnoreRules {
    /// Adds the rules of the `.gitignore` found in directory `base`.
    pub fn add(&mut self, base: &str, content: &str) {
        for line in content.lines() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (negate, line) = match line.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, line.strip_prefix('\\').unwrap_or(line)),
            };
            let (dir_only, line) = match line.strip_suffix('/') {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let anchored = line.contains('/');
            let pattern = line.trim_start_matches('/');
            if pattern.is_empty() {
                continue;
            }
            self.rules.push(Rule {
                base: base.to_owned(),
                pattern: pattern.to_owned(),
                negate,
                dir_only,
                anchored,
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// `path` (relative to the project, `/`-separated) is ignored. The last matching rule
    /// wins, as in git.
    pub fn is_ignored(&self, path: &str, is_dir: bool) -> bool {
        let name = path.rsplit('/').next().unwrap_or(path);
        let mut ignored = false;
        for rule in &self.rules {
            if rule.dir_only && !is_dir {
                continue;
            }
            let under = if rule.base.is_empty() {
                path
            } else {
                match path.strip_prefix(rule.base.as_str()).and_then(|p| p.strip_prefix('/')) {
                    Some(under) => under,
                    None => continue,
                }
            };
            let subject = if rule.anchored { under } else { name };
            if kernel::paths::glob_match(&rule.pattern, subject) {
                ignored = !rule.negate;
            }
        }
        ignored
    }
}

#[cfg(test)]
#[path = "../tests/unit/ignore.rs"]
mod tests;
