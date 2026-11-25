//! The app's web engine's packages, as rules (plan 99g), nothing read or written here: an
//! engine's [`Version`] compared part by part, the package worth installing chosen from the
//! signed list ([`choose`]), what a listed package must be to be considered at all
//! ([`well_formed`], [`archive_name`]), the systems an engine is made for ([`PLATFORMS`],
//! [`platform`]) and where its program is in an unpacked engine ([`program`]).
//!
//! The list is fetched and its signature checked by the infrastructure (`web-engine`); this
//! only decides from what was vouched for.

mod choice;
mod version;

use std::path::{Path, PathBuf};

pub use choice::{Choice, Listed, Offer, Wanted, archive_name, choose, well_formed};
pub use version::Version;

/// A system an engine is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    /// Its name in the list and the packages' (`macos-arm64`).
    pub name: &'static str,
    /// Its system, as `std::env::consts::OS` names it (`macos`).
    pub os: &'static str,
    /// Its architecture, as `std::env::consts::ARCH` names it (`aarch64`).
    pub arch: &'static str,
}

impl Platform {
    /// The one named `name` in the list (`None`: no engine is made for it).
    pub fn named(name: &str) -> Option<&'static Self> {
        PLATFORMS.iter().find(|p| p.name == name)
    }
}

/// Every system an engine is made for (the packaging makes one for each).
pub const PLATFORMS: [Platform; 4] = [
    Platform { name: "linux-x64", os: "linux", arch: "x86_64" },
    Platform { name: "macos-arm64", os: "macos", arch: "aarch64" },
    Platform { name: "macos-x64", os: "macos", arch: "x86_64" },
    Platform { name: "windows-x64", os: "windows", arch: "x86_64" },
];

/// This system's name in the list for `os` and `arch` (`std::env::consts`'s; `None`: no
/// engine is made for it).
pub fn platform(os: &str, arch: &str) -> Option<&'static str> {
    PLATFORMS.iter().find(|p| p.os == os && p.arch == arch).map(|p| p.name)
}

/// The host's program in an engine unpacked at `dir` for the system `os` (as
/// `std::env::consts::OS` names it, or a [`Platform`]'s `os`), as each system's package lays
/// it out: an app bundle on macOS, an `.exe` on Windows, a plain program elsewhere.
pub fn program(dir: &Path, os: &str) -> PathBuf {
    match os {
        "macos" => dir.join("web-host.app/Contents/MacOS/web-host"),
        "windows" => dir.join("web-host.exe"),
        _ => dir.join("web-host"),
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
