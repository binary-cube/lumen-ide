//! The releases the repository's host lists (GitHub's `releases` API, newest first): each
//! one's version (its tag), its notes, whether it's a pre-release, its files; and the
//! newest one worth offering over the version running.

use serde_json::Value;

use crate::Version;

/// A file of a release, where it's downloaded from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    pub tag: String,
    /// Its title (`Lumen 1.0.0-beta.2`), its tag when it has none.
    pub name: String,
    /// Its notes, as written (Markdown): what the user reads before choosing.
    pub notes: String,
    pub prerelease: bool,
    /// When it was published (`2026-11-18T10:00:00Z`), as the host says it.
    pub published: String,
    /// Its page on the host.
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// Its file named `name`.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The releases in the host's `releases` list (`list`: its JSON array). Drafts are left out
/// (only their authors see them), so is anything without a version for its tag.
pub fn parse(list: &Value) -> Vec<Release> {
    let text = |r: &Value, key: &str| r.get(key).and_then(Value::as_str).unwrap_or("").to_owned();
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|r| r.get("draft").and_then(Value::as_bool) != Some(true))
        .filter_map(|r| {
            let tag = text(r, "tag_name");
            let version = Version::parse(&tag)?;
            let assets = r
                .get("assets")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|a| {
                    Some(Asset {
                        name: a.get("name")?.as_str()?.to_owned(),
                        url: a.get("browser_download_url")?.as_str()?.to_owned(),
                        size: a.get("size").and_then(Value::as_u64).unwrap_or(0),
                    })
                })
                .collect();
            let name = text(r, "name");
            Some(Release {
                name: if name.trim().is_empty() { tag.clone() } else { name },
                tag,
                notes: text(r, "body"),
                prerelease: r.get("prerelease").and_then(Value::as_bool).unwrap_or(false)
                    || version.is_pre(),
                published: text(r, "published_at"),
                page: text(r, "html_url"),
                version,
                assets,
            })
        })
        .collect()
}

/// The newest of `releases` above `current` worth offering: a pre-release only when
/// `prereleases` (the user takes them — by default whoever runs one), and never `skipped`
/// (the version the user chose to skip; a newer one is offered again).
pub fn newest<'a>(
    releases: &'a [Release],
    current: &Version,
    prereleases: bool,
    skipped: Option<&Version>,
) -> Option<&'a Release> {
    releases
        .iter()
        .filter(|r| r.version > *current)
        .filter(|r| prereleases || !r.prerelease)
        .filter(|r| skipped != Some(&r.version))
        .max_by(|a, b| a.version.cmp(&b.version))
}

#[cfg(test)]
#[path = "../tests/unit/release.rs"]
mod tests;
