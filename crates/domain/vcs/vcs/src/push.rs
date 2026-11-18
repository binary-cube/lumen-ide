//! A push asked for in detail: to which remote, which branches under which names there, and
//! whether tags go too or the remote's history may be overwritten.

/// A remote of the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    /// Where pushes go, fit to show (never a password).
    pub url: String,
}

/// A local branch sent to a remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushedBranch {
    pub local: String,
    /// Its name on the remote.
    pub remote: String,
    /// The local branch tracks it from now on.
    pub track: bool,
}

/// What a detailed push sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushPlan {
    pub remote: String,
    pub branches: Vec<PushedBranch>,
    pub tags: bool,
    /// The remote's branches may be overwritten, unless they moved since they were last read.
    pub force: bool,
}

impl PushPlan {
    /// There's something to send.
    pub fn has_work(&self) -> bool {
        self.tags || !self.branches.is_empty()
    }
}

/// `url` without the password it may carry (`https://me:secret@host/x` → `https://me@host/x`).
pub fn url_without_password(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else { return url.to_owned() };
    let end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(end);
    let Some((credentials, host)) = authority.rsplit_once('@') else { return url.to_owned() };
    let user = credentials.split(':').next().unwrap_or_default();
    format!("{scheme}://{user}@{host}{path}")
}

#[cfg(test)]
#[path = "../tests/unit/push.rs"]
mod tests;
