//! The app's own updates, as rules (plan 84c–84f), nothing read or written here: a
//! [`Version`] compared as SemVer does (a pre-release before its release), the releases'
//! list the repository's host gives ([`release`]) and the newest one worth offering, the
//! signed manifest of a release ([`manifest`]) and the file it has for this system
//! ([`platform`]), the release's notes as lines a window shows ([`notes`]).
//!
//! Whether an update is installed is never decided here: the user is told, and chooses.

pub mod manifest;
pub mod notes;
pub mod platform;
pub mod release;
mod version;

pub use manifest::{Manifest, ManifestFile};
pub use release::{Asset, Release};
pub use version::Version;
