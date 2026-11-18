//! A release's manifest (`latest.json`, made by `cargo xtask release-manifest`, signed with
//! the project's key): its version and, for each platform, the file's name, size and
//! SHA-256. Its signature is checked before it's read (by the infrastructure: this only
//! reads what was vouched for).

use serde_json::Value;

use crate::Version;

/// A platform's file in the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestFile {
    pub platform: String,
    pub name: String,
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// What a release carries, as signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub date: String,
    pub files: Vec<ManifestFile>,
}

impl Manifest {
    /// `text` read (the manifest's JSON); `None` if it isn't one. A file whose SHA-256 isn't
    /// 64 hex digits, or whose name has a folder in it, is left out (never downloaded).
    pub fn parse(text: &str) -> Option<Self> {
        let doc: Value = serde_json::from_str(text).ok()?;
        let version = Version::parse(doc.get("version")?.as_str()?)?;
        let files = doc
            .get("files")?
            .as_array()?
            .iter()
            .filter_map(|f| {
                let field = |k: &str| f.get(k).and_then(Value::as_str).map(str::to_owned);
                let file = ManifestFile {
                    platform: field("platform")?,
                    name: field("name")?,
                    size: f.get("size")?.as_u64()?,
                    sha256: field("sha256")?.to_ascii_lowercase(),
                };
                let hex =
                    file.sha256.len() == 64 && file.sha256.bytes().all(|b| b.is_ascii_hexdigit());
                let plain = !file.name.is_empty()
                    && !file.name.contains(['/', '\\', ':'])
                    && file.name != "."
                    && file.name != "..";
                (hex && plain).then_some(file)
            })
            .collect();
        Some(Self { version, date: doc.get("date").and_then(Value::as_str)?.to_owned(), files })
    }

    /// The file for this system: the first of `platforms` (this system's, best first:
    /// [`crate::platform::preferred`]) the manifest has.
    pub fn file_for(&self, platforms: &[&str]) -> Option<&ManifestFile> {
        platforms.iter().find_map(|p| self.files.iter().find(|f| f.platform == *p))
    }
}

#[cfg(test)]
#[path = "../tests/unit/manifest.rs"]
mod tests;
