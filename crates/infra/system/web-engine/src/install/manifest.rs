//! The engine's packages as published (`web-engine.json`, signed like the app's releases:
//! [`web_engine_protocol::PackageList`]): read only once its signature is the project's,
//! every package's fields checked. Which one is worth installing is the domain's rule
//! ([`web_engine_versions::choose`]); fetched elsewhere.

use web_engine_protocol::{PackageEntry, PackageList};
use web_engine_versions::{Choice, Listed, Offer, Version, Wanted};

/// One package of the list: an engine for one system.
pub type Package = PackageEntry;

/// The list as published, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Below this, a version installed is replaced even if it works (a serious flaw fixed),
    /// and nothing older is installed.
    pub minimum: Option<String>,
    /// Every package published, each checked well formed.
    pub packages: Vec<Package>,
}

/// Why a list wasn't taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    /// Its signature isn't the project's.
    Unsigned,
    /// Not the list's form.
    Malformed(String),
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsigned => f.write_str("the engine's list isn't signed by the project"),
            Self::Malformed(why) => write!(f, "the engine's list can't be read: {why}"),
        }
    }
}

/// What the list says to do for this install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick<'a> {
    /// This package (newer than the one installed, or none is).
    Install(&'a Package),
    /// The one installed stays (`below_minimum`: it's older than the list's safe minimum
    /// and nothing replaces it).
    Current { below_minimum: bool },
    /// Nothing fits, nothing is installed.
    Nothing,
}

impl Manifest {
    /// The list in `bytes`, trusted only when `signature` (hex) is `public`'s over them; a
    /// package that isn't well formed (its version, SHA-256, size, archive name) or a
    /// minimum that isn't a version refuses the whole list.
    pub fn read(bytes: &[u8], signature: &str, public: &str) -> Result<Self, ManifestError> {
        if !pinned_downloads::signature::verify(bytes, signature, public) {
            return Err(ManifestError::Unsigned);
        }
        let list: PackageList =
            serde_json::from_slice(bytes).map_err(|e| ManifestError::Malformed(e.to_string()))?;
        if list.minimum.as_deref().is_some_and(|m| Version::parse(m).is_none()) {
            return Err(ManifestError::Malformed("its minimum".into()));
        }
        let bad = list.packages.iter().position(|p| {
            !web_engine_versions::well_formed(&p.version, &p.sha256, p.size, &p.url)
                || p.platform.is_empty()
        });
        match bad {
            Some(index) => Err(ManifestError::Malformed(format!("package {index}"))),
            None => Ok(Self { minimum: list.minimum, packages: list.packages }),
        }
    }

    /// What to do for `wanted` (the system, the protocol, the versions refused here, the
    /// one installed).
    pub fn pick(&self, wanted: Wanted<'_>) -> Pick<'_> {
        let offers: Vec<Offer<'_>> = self
            .packages
            .iter()
            .map(|p| Offer { version: &p.version, protocol: p.protocol, platform: &p.platform })
            .collect();
        let listed = Listed { offers: &offers, minimum: self.minimum.as_deref() };
        match web_engine_versions::choose(listed, wanted) {
            Choice::Take(index) => self.packages.get(index).map_or(Pick::Nothing, Pick::Install),
            Choice::Current { below_minimum } => Pick::Current { below_minimum },
            Choice::Nothing => Pick::Nothing,
        }
    }
}

/// This system's name in the list (`None`: no engine is made for it).
pub fn platform() -> Option<&'static str> {
    web_engine_versions::platform(std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
#[path = "../../tests/unit/install/manifest.rs"]
mod tests;
