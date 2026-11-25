//! The app's own updates, done (plan 84c–84f): [`check`] asks GitHub for the newest
//! release worth offering and reads its manifest, once its signature is checked with the
//! project's public key ([`PUBLIC_KEY`], the release workflow's); [`stage`] downloads this
//! system's file (its SHA-256 the manifest's) and unpacks it beside the app's data, never
//! over the app; [`install`] puts it in the app's place, by the app itself while it runs —
//! the old one set aside and given back if anything fails — and starts it again with its
//! windows.
//!
//! Nothing here decides to update: the app asks, the user chooses. Only GitHub's hosts are
//! talked to ([`HOSTS`]), over HTTPS, nothing about the user sent.

pub mod check;
pub mod install;
pub mod stage;

use std::path::PathBuf;

/// The hosts updates come from: GitHub's API, its release pages and where they redirect
/// the files to.
pub const HOSTS: &[&str] = &[
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// The project's public key (Ed25519, hex): a release's manifest is trusted only when
/// signed by its secret half (made once by `cargo xtask release-key`).
pub use pinned_downloads::signature::PUBLIC_KEY;

/// The folder updates are downloaded and staged in (the data folder's `updates`).
pub fn folder() -> Option<PathBuf> {
    files::places::data_dir().map(|d| d.join("updates"))
}
