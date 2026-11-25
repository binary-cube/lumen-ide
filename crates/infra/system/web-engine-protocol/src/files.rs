//! The files the app, the host and the packaging agree on: what an unpacked engine says of
//! itself ([`EngineInfo`], `engine.json`), what the packaging writes beside each archive
//! ([`PackageDescription`]), the signed list of packages the app reads ([`PackageList`]),
//! and the names of a page's pixels' files ([`pixels_name`]).
//!
//! Plain data: written with `serde` by whoever makes them, checked by whoever reads them
//! (the list is hostile until its signature and its fields are checked, by the app).

use serde::{Deserialize, Serialize};

/// The file an unpacked engine says what it is in, at its folder's top.
pub const ENGINE_INFO: &str = "engine.json";

/// What an unpacked engine says of itself ([`ENGINE_INFO`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineInfo {
    /// `<Chromium's version>-<ours>` (`156.0.8078.100-2`).
    pub version: String,
    /// The protocol its host speaks ([`crate::PROTOCOL`]).
    pub protocol: u32,
}

/// What the packaging writes beside an archive (`<archive>.json`): the list is made from
/// these, each archive's address added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageDescription {
    /// `<Chromium's version>-<ours>` (`156.0.8078.100-2`).
    pub version: String,
    /// The protocol its host speaks.
    pub protocol: u32,
    /// The system and architecture (`macos-arm64`, `windows-x64`, `linux-x64`…).
    pub platform: String,
    /// The archive's file name (its format known by its extension).
    pub name: String,
    /// The archive's size (bytes).
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

impl PackageDescription {
    /// Its entry in the list, the archive published under `base_url` (a trailing `/` or not).
    pub fn listed(&self, base_url: &str) -> PackageEntry {
        PackageEntry {
            version: self.version.clone(),
            protocol: self.protocol,
            platform: self.platform.clone(),
            url: format!("{}/{}", base_url.trim_end_matches('/'), self.name),
            size: self.size,
            sha256: self.sha256.clone(),
        }
    }
}

/// The list of packages as published (`web-engine.json`, signed as written).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PackageList {
    /// The oldest version still safe to run: below it, an installed engine is replaced even
    /// if it works (a serious flaw fixed), and nothing older is installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minimum: Option<String>,
    /// Every package published, one per system (and per protocol while two are served).
    pub packages: Vec<PackageEntry>,
}

/// One package of the list: an engine for one system.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageEntry {
    /// `<Chromium's version>-<ours>` (`156.0.8078.100-2`).
    pub version: String,
    /// The protocol its host speaks.
    pub protocol: u32,
    /// The system and architecture (`macos-arm64`…).
    pub platform: String,
    /// Where its archive is (its last part the archive's file name).
    pub url: String,
    /// The archive's size (bytes): the download is pinned to it, never more read.
    pub size: u64,
    /// Lowercase hex.
    pub sha256: String,
}

/// The start of a page's pixels' file name.
const PIXELS_PREFIX: &str = "frames-";
/// The end of a page's pixels' file name (a popup's: `.popup.bgra`, [`crate::popup_pixels`]).
const PIXELS_SUFFIX: &str = ".bgra";

/// The name of a page's pixels' file, made by the app in the channel's folder: `frames-<id>.bgra`
/// (`id`: a UUID, so no two pages share one).
pub fn pixels_name(id: impl std::fmt::Display) -> String {
    format!("{PIXELS_PREFIX}{id}{PIXELS_SUFFIX}")
}

/// Whether `name` is a page's pixels' file name ([`pixels_name`], or its popup's): a plain
/// name, never a path.
pub fn is_pixels_name(name: &str) -> bool {
    let plain = !name.contains(['/', '\\', ':', '\0']) && !name.contains("..");
    plain
        && name
            .strip_prefix(PIXELS_PREFIX)
            .and_then(|rest| rest.strip_suffix(PIXELS_SUFFIX))
            .is_some_and(|id| !id.is_empty())
}

#[cfg(test)]
#[path = "../tests/unit/files.rs"]
mod tests;
