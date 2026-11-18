//! A file's identity, as sources and opened files are matched by: two ways to one file
//! (a link in the path, `/var` and `/private/var`; on Windows another case, a verbatim
//! `\\?\` prefix, a short 8.3 name) give the same [`FileKey`]. The links are resolved by
//! the caller through the file system's port; what's here is pure: the lexical rules each
//! platform compares paths by ([`normalize`]).

use std::path::{Component, Path, PathBuf};

/// How a platform writes and compares paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathRules {
    /// `/` separators, case kept.
    Unix,
    /// `\` (or `/`) separators, a drive or a share first, case ignored, `\\?\` prefixes.
    Windows,
}

impl PathRules {
    /// The rules of the platform the app runs on.
    pub const HOST: Self = if cfg!(windows) { Self::Windows } else { Self::Unix };
}

/// `text` as `rules` compare it: `.` and `..` resolved, separators single; on Windows also
/// `/` as `\`, the verbatim prefixes taken off (`\\?\C:\a` → `C:\a`, `\\?\UNC\srv\share` →
/// `\\srv\share`) and lowercase. The root (`/`, a drive, a share) is kept; `..` never goes
/// above it.
pub fn normalize(text: &str, rules: PathRules) -> String {
    match rules {
        PathRules::Unix => {
            let absolute = text.starts_with('/');
            let parts = resolve(text.split('/'), absolute);
            let joined = parts.join("/");
            if absolute { format!("/{joined}") } else { joined }
        }
        PathRules::Windows => {
            let text = text.replace('/', "\\");
            let text = if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
                format!(r"\\{rest}")
            } else if let Some(rest) = text.strip_prefix(r"\\?\") {
                rest.to_owned()
            } else {
                text
            };
            // The root: a share (`\\server\share`), a drive (`C:`, with `\` when absolute)
            // or a leading `\`.
            let (root, rest) = if let Some(share) = text.strip_prefix(r"\\") {
                let mut parts = share.splitn(3, '\\');
                let server = parts.next().unwrap_or_default();
                let name = parts.next().unwrap_or_default();
                (format!(r"\\{server}\{name}\"), parts.next().unwrap_or_default().to_owned())
            } else if text.as_bytes().get(1) == Some(&b':') {
                let (drive, rest) = text.split_at(2);
                match rest.strip_prefix('\\') {
                    Some(rest) => (format!("{drive}\\"), rest.to_owned()),
                    None => (drive.to_owned(), rest.to_owned()),
                }
            } else if let Some(rest) = text.strip_prefix('\\') {
                ("\\".to_owned(), rest.to_owned())
            } else {
                (String::new(), text)
            };
            let parts = resolve(rest.split('\\'), !root.is_empty());
            format!("{root}{}", parts.join("\\")).to_lowercase()
        }
    }
}

/// The parts left once `.`, empty ones and `..` (never above a root) are resolved.
fn resolve<'a>(parts: impl Iterator<Item = &'a str>, rooted: bool) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for part in parts {
        match part {
            "" | "." => {}
            ".." => match out.last() {
                Some(&last) if last != ".." => {
                    out.pop();
                }
                _ if rooted => {}
                _ => out.push(".."),
            },
            part => out.push(part),
        }
    }
    out
}

/// A file's identity: equal for two paths to one file once their links are resolved
/// ([`Self::lexical`] of a canonical path), or as written when it isn't there.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileKey(PathBuf);

impl FileKey {
    /// `path`'s key by the host's rules, its links not followed.
    pub fn lexical(path: &Path) -> Self {
        Self::with_rules(path, PathRules::HOST)
    }

    /// `path`'s key by `rules` (on Unix lossless: a name that isn't UTF-8 kept as it is).
    pub fn with_rules(path: &Path, rules: PathRules) -> Self {
        match rules {
            PathRules::Windows => Self(PathBuf::from(normalize(&path.to_string_lossy(), rules))),
            PathRules::Unix => {
                let mut out = PathBuf::new();
                for component in path.components() {
                    match component {
                        Component::CurDir => {}
                        Component::ParentDir => {
                            let rooted = out.has_root();
                            let last = out.components().next_back();
                            match last {
                                Some(Component::Normal(_)) => {
                                    out.pop();
                                }
                                _ if rooted => {}
                                _ => out.push(".."),
                            }
                        }
                        other => out.push(other.as_os_str()),
                    }
                }
                Self(out)
            }
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
#[path = "../tests/unit/identity.rs"]
mod tests;
