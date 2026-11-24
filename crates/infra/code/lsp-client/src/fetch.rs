//! What the language servers' downloads come through: [`pinned_downloads::fetch`], from the
//! hosts their pinned sources are on ([`HOSTS`]; every redirect checked too) and nowhere
//! else.

use std::path::Path;

#[cfg(any(test, feature = "fake"))]
pub use pinned_downloads::fetch::Memory;
pub use pinned_downloads::fetch::{Body, Fetch, FetchError, Going, MAX_REDIRECTS, Pin, Wanted};

/// The only hosts anything is downloaded from: Node's releases, npm's registry, GitHub's
/// releases (and where they redirect: its storage of release files) and its raw files at a
/// pinned commit or tag, static-php-cli's builds (and where they redirect: its project's
/// storage bucket).
pub const HOSTS: [&str; 8] = [
    "nodejs.org",
    "registry.npmjs.org",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "raw.githubusercontent.com",
    "dl.static-php.dev",
    "static-php-cli.fra1.digitaloceanspaces.com",
];

/// Whether `url` may be downloaded from: HTTPS, one of [`HOSTS`], no credentials, no port.
pub fn allowed(url: &str) -> bool {
    pinned_downloads::fetch::allowed(url, &HOSTS)
}

/// The network, from [`HOSTS`] only.
#[derive(Debug, Clone)]
pub struct Http(pinned_downloads::fetch::Http);

impl Default for Http {
    fn default() -> Self {
        Self(pinned_downloads::fetch::Http::new(&HOSTS))
    }
}

impl Fetch for Http {
    fn open(&self, url: &str, from: u64) -> Result<Body, FetchError> {
        self.0.open(url, from)
    }
}

/// [`pinned_downloads::fetch::fetch_bytes`] from [`HOSTS`].
pub fn fetch_bytes(
    going: Going<'_>,
    (url, pin, limit): (&str, &Pin, u64),
) -> Result<Vec<u8>, FetchError> {
    pinned_downloads::fetch::fetch_bytes(going, Wanted { url, hosts: &HOSTS, pin, limit })
}

/// [`pinned_downloads::fetch::fetch_file`] from [`HOSTS`].
pub fn fetch_file(
    going: Going<'_>,
    (url, pin, limit): (&str, &Pin, u64),
    part: &Path,
) -> Result<(), FetchError> {
    pinned_downloads::fetch::fetch_file(going, Wanted { url, hosts: &HOSTS, pin, limit }, part)
}

#[cfg(test)]
#[path = "../tests/unit/fetch.rs"]
mod tests;
