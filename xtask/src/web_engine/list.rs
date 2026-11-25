//! The packages' list (`web-engine.json`): made from the new packages' descriptions merged
//! into the published list, signed; and checked before it's published.
//!
//! The merge keeps the newest package of each system for each protocol: after a protocol's
//! bump, an app that still speaks the older one keeps finding its engine in the list.

use std::collections::BTreeMap;
use std::path::Path;

use super::dto::{Description, List, Package, Version};

/// The names of the systems an engine is made for (`web_engine_versions::PLATFORMS`): a
/// release lists each of them.
pub fn platforms() -> Vec<&'static str> {
    web_engine_versions::PLATFORMS.iter().map(|p| p.name).collect()
}

/// The list's file, and its signature beside it.
pub const LIST: &str = "web-engine.json";
pub const SIGNATURE: &str = "web-engine.json.sig";

/// A description as the list's package, its archive at `base`; refused when the app would
/// pass it over (`web_engine_versions::well_formed`: a version, a SHA-256, a size, a plain
/// archive name), or its system isn't one an engine is made for ([`platforms`]).
pub fn listed(description: &Description, base: &str) -> Result<Package, String> {
    let package = description.listed(base);
    let name = &description.name;
    let plain = web_engine_versions::archive_name(&package.url) == Some(name.as_str());
    if !plain
        || !web_engine_versions::well_formed(
            &package.version,
            &package.sha256,
            package.size,
            &package.url,
        )
    {
        return Err(format!(
            "{name}: a package the app would refuse (version, SHA-256, size, name)"
        ));
    }
    if web_engine_versions::Platform::named(&package.platform).is_none() {
        return Err(format!("{name}: {} isn't a system an engine is made for", package.platform));
    }
    Ok(package)
}

/// `previous`'s packages with `new` ones: the newest of each (system, protocol), a new one
/// winning over a previous one of the same version. Sorted by system, then protocol.
pub fn merged(previous: &[Package], new: &[Package]) -> Vec<Package> {
    let mut newest: BTreeMap<(String, u32), Package> = BTreeMap::new();
    for package in previous.iter().chain(new) {
        let key = (package.platform.clone(), package.protocol);
        let newer = newest
            .get(&key)
            .is_none_or(|kept| Version::parse(&package.version) >= Version::parse(&kept.version));
        if newer {
            newest.insert(key, package.clone());
        }
    }
    newest.into_values().collect()
}

/// The list's text (pretty JSON, a newline at the end: signed as written).
pub fn text(list: &List) -> Result<String, String> {
    let mut text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    text.push('\n');
    Ok(text)
}

/// A signed list read from `path` (its signature at `<path>.sig`), trusted only when the
/// signature is `public`'s.
pub fn read_signed(path: &Path, public: &str) -> Result<List, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut sig_path = path.as_os_str().to_owned();
    sig_path.push(".sig");
    let signature = std::fs::read_to_string(&sig_path)
        .map_err(|e| format!("{}: {e}", Path::new(&sig_path).display()))?;
    if !pinned_downloads::signature::verify(&bytes, &signature, public) {
        return Err(format!("{}: its signature isn't the project's", path.display()));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// What a published list must hold for `version`: a package of it for every system in
/// `platforms`, each found in `dir` with the size and the SHA-256 the list says.
pub fn check_release(
    list: &List,
    dir: &Path,
    version: &str,
    platforms: &[&str],
) -> Result<Vec<String>, String> {
    let mut checked = Vec::new();
    for platform in platforms {
        let found = list.packages.iter().any(|p| p.platform == *platform && p.version == version);
        if !found {
            return Err(format!("no {platform} package of {version} in the list"));
        }
    }
    for package in list.packages.iter().filter(|p| p.version == version) {
        let Some(name) = web_engine_versions::archive_name(&package.url) else {
            return Err(format!("{}: not an archive's address", package.url));
        };
        let path = dir.join(name);
        let bytes = std::fs::read(&path).map_err(|e| format!("{name}: {e}"))?;
        if bytes.len() as u64 != package.size {
            return Err(format!("{name}: {} bytes, the list says {}", bytes.len(), package.size));
        }
        let sha256 = pinned_downloads::digest::sha256_hex(&bytes);
        if sha256 != package.sha256 {
            return Err(format!("{name}: its SHA-256 isn't the list's"));
        }
        checked.push(format!("{}  {name}", package.platform));
    }
    Ok(checked)
}

#[cfg(test)]
#[path = "../../tests/unit/web_engine/list.rs"]
mod tests;
