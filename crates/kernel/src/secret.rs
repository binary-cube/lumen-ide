//! A secret (a password, a key's passphrase): its text is wiped from memory when dropped,
//! never printed, and only read through [`Secret::expose`]. Where secrets are kept is the
//! [`SecretStore`] port, shared by every context that signs in somewhere (remote projects,
//! data sources).

use zeroize::Zeroize;

/// A password or passphrase, kept only as long as it's needed.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text, for the one call that needs it (signing in); never kept elsewhere.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<String> for Secret {
    fn from(text: String) -> Self {
        Self(text)
    }
}

impl From<&str> for Secret {
    fn from(text: &str) -> Self {
        Self(text.to_owned())
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// The secrets' port: by an id the caller chooses (a connection's, a data source's), never
/// written in plain text (the keychain, a project vault encrypted with a key kept there).
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Option<Secret>;
    fn set(&self, id: &str, secret: &str) -> Result<(), String>;
    fn remove(&self, id: &str) -> Result<(), String>;
}

/// Overwrites `text` with zeros and empties it (a password field's text, a decrypted key).
pub fn wipe(text: &mut String) {
    text.zeroize();
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "Secret(empty)" } else { "Secret(…)" })
    }
}

#[cfg(test)]
#[path = "../tests/unit/secret.rs"]
mod tests;
