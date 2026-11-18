//! Is there an update: the repository's releases asked of GitHub (no account: nothing about
//! the user sent), the newest one worth offering chosen ([`app_updates::release::newest`]),
//! its manifest and signature fetched and checked, this system's file found in it.

use std::io::Read;

use app_updates::{Manifest, ManifestFile, Release, Version};
use pinned_downloads::fetch::{Fetch, FetchError};

/// What's offered: the release (its notes), its manifest as signed, this system's file in
/// it and where it's downloaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub release: Release,
    pub manifest: Manifest,
    pub file: ManifestFile,
    pub url: String,
}

/// Why it couldn't be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckError {
    /// No network, or GitHub unreachable: asked again later, nothing said.
    Offline(String),
    /// Anything else (a release without a signed manifest, one that doesn't check out, no
    /// file for this system): said when the user asked.
    Failed(String),
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Offline(why) | Self::Failed(why) => f.write_str(why),
        }
    }
}

impl From<FetchError> for CheckError {
    fn from(e: FetchError) -> Self {
        if e.offline() { Self::Offline(e.to_string()) } else { Self::Failed(e.to_string()) }
    }
}

/// The most a list, a manifest or a signature may weigh (a server sending on forever stops).
const SMALL: u64 = 4 * 1024 * 1024;

/// What's asked for an update.
#[derive(Debug, Clone, Copy)]
pub struct Ask<'a> {
    /// `owner/name` on GitHub.
    pub repository: &'a str,
    pub current: &'a Version,
    /// Pre-releases offered too.
    pub prereleases: bool,
    /// The version the user chose to skip.
    pub skipped: Option<&'a Version>,
    /// This system's platforms, best first ([`app_updates::platform::preferred`]).
    pub platforms: &'a [&'a str],
}

/// The update worth offering over `ask.current`, if any (`Ok(None)`: up to date), its
/// manifest checked against the project's key.
pub fn check(fetch: &dyn Fetch, ask: Ask<'_>) -> Result<Option<Available>, CheckError> {
    check_signed_by(fetch, ask, crate::PUBLIC_KEY)
}

/// [`check`] with the manifest checked against `public` (hex): a test's own key.
pub fn check_signed_by(
    fetch: &dyn Fetch,
    ask: Ask<'_>,
    public: &str,
) -> Result<Option<Available>, CheckError> {
    let list = format!("https://api.github.com/repos/{}/releases?per_page=30", ask.repository);
    let body = small(fetch, &list)?;
    let json: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|e| CheckError::Failed(format!("the releases' list isn't JSON: {e}")))?;
    let releases = app_updates::release::parse(&json);
    let Some(release) =
        app_updates::release::newest(&releases, ask.current, ask.prereleases, ask.skipped)
    else {
        return Ok(None);
    };
    let asset = |name: &str| {
        release.asset(name).ok_or_else(|| {
            CheckError::Failed(format!("{}: no signed manifest ({name})", release.tag))
        })
    };
    let manifest_bytes = small(fetch, &asset("latest.json")?.url)?;
    let signature = small(fetch, &asset("latest.json.sig")?.url)?;
    let signature = String::from_utf8_lossy(&signature);
    if !crate::signature::verify(&manifest_bytes, &signature, public) {
        return Err(CheckError::Failed(format!(
            "{}: its manifest isn't signed by the project's key",
            release.tag
        )));
    }
    let text = String::from_utf8_lossy(&manifest_bytes);
    let manifest = Manifest::parse(&text).ok_or_else(|| {
        CheckError::Failed(format!("{}: its manifest can't be read", release.tag))
    })?;
    if manifest.version != release.version {
        return Err(CheckError::Failed(format!(
            "{}: its manifest is for {}",
            release.tag, manifest.version
        )));
    }
    let file = manifest
        .file_for(ask.platforms)
        .cloned()
        .ok_or_else(|| CheckError::Failed(format!("{}: no build for this system", release.tag)))?;
    let url = release.asset(&file.name).map(|a| a.url.clone()).ok_or_else(|| {
        CheckError::Failed(format!("{}: {} isn't among its files", release.tag, file.name))
    })?;
    Ok(Some(Available { release: release.clone(), manifest, file, url }))
}

/// A small file (a list, a manifest, a signature), at most [`SMALL`] bytes.
fn small(fetch: &dyn Fetch, url: &str) -> Result<Vec<u8>, CheckError> {
    if !pinned_downloads::fetch::allowed(url, crate::HOSTS) {
        return Err(CheckError::Failed(format!("{url}: not a source updates come from")));
    }
    let body = fetch.open(url, 0)?;
    let mut out = Vec::new();
    body.reader
        .take(SMALL + 1)
        .read_to_end(&mut out)
        .map_err(|e| CheckError::Offline(format!("{url}: {e}")))?;
    if out.len() as u64 > SMALL {
        return Err(CheckError::Failed(format!("{url}: larger than expected")));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "../tests/unit/check.rs"]
mod tests;
