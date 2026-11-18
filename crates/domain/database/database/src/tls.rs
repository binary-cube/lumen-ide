//! How a server connection is encrypted, in libpq's words (`sslmode`), which every SQL
//! client knows: from plain to the server's certificate and name checked. Read from a
//! target's `sslmode` option (any case), or from its [`Tls`] when none is given — and
//! `prefer` by default: encrypted when the server can, plain otherwise.

use crate::{Target, Tls};

/// The `sslmode` option's name.
pub const SSLMODE: &str = "sslmode";
/// The options naming the certificate authority's file, the client's certificate and its
/// key (PEM files).
pub const SSLROOTCERT: &str = "sslrootcert";
pub const SSLCERT: &str = "sslcert";
pub const SSLKEY: &str = "sslkey";

/// How a connection is encrypted (libpq's `sslmode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SslMode {
    /// Plain only.
    Disable,
    /// Plain first; encrypted if the server wants only that.
    Allow,
    /// Encrypted when the server can, plain otherwise (the certificate not checked).
    #[default]
    Prefer,
    /// Encrypted, or no connection (the certificate not checked — unless a certificate
    /// authority is given, as libpq does: then as `verify-ca`).
    Require,
    /// Encrypted, the certificate signed by a trusted authority (its name not checked).
    VerifyCa,
    /// Encrypted, the certificate trusted and naming the host.
    VerifyFull,
}

impl SslMode {
    pub const ALL: [Self; 6] =
        [Self::Disable, Self::Allow, Self::Prefer, Self::Require, Self::VerifyCa, Self::VerifyFull];

    /// From its name (`verify-full`, any case; `verify_full` too); `None` if it's none.
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "disable" => Self::Disable,
            "allow" => Self::Allow,
            "prefer" => Self::Prefer,
            "require" => Self::Require,
            "verify-ca" => Self::VerifyCa,
            "verify-full" => Self::VerifyFull,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Allow => "allow",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }

    /// `target`'s: its `sslmode` option, else its [`Tls`] (verify: `verify-full`, trust:
    /// `require`), else `prefer`. An option that isn't a mode is an error (never a silently
    /// weaker connection).
    pub fn of(target: &Target) -> Result<Self, String> {
        if let Some(text) = target.options.get(SSLMODE) {
            return Self::parse(text).ok_or_else(|| format!("`{text}` isn't an sslmode"));
        }
        Ok(match target.tls {
            Tls::Verify => Self::VerifyFull,
            Tls::Trust => Self::Require,
            Tls::Off => Self::Prefer,
        })
    }

    /// TLS may be used.
    pub fn may_encrypt(self) -> bool {
        self != Self::Disable
    }

    /// No plain connection is ever made.
    pub fn requires_tls(self) -> bool {
        matches!(self, Self::Require | Self::VerifyCa | Self::VerifyFull)
    }

    /// A plain connection is tried first (TLS only if the server refuses it).
    pub fn plain_first(self) -> bool {
        matches!(self, Self::Disable | Self::Allow)
    }

    /// How the server's certificate is checked, given whether a certificate authority's
    /// file is named (`require` with one checks the chain, as libpq).
    pub fn verification(self, has_root: bool) -> Verification {
        match self {
            Self::VerifyFull => Verification::Full,
            Self::VerifyCa => Verification::Chain,
            Self::Require if has_root => Verification::Chain,
            _ => Verification::None,
        }
    }
}

/// How far a server's certificate is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verification {
    /// Not at all (the connection is encrypted, its server not proven).
    None,
    /// Signed by a trusted authority; any name.
    Chain,
    /// Signed by a trusted authority, naming the host.
    Full,
}

#[cfg(test)]
#[path = "../tests/unit/tls.rs"]
mod tests;
