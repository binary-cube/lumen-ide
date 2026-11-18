//! Identifiers shared between contexts.

use std::borrow::Cow;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier of an open document, valid for the lifetime of the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(u64);

impl DocumentId {
    /// Generates a new ID, unique within the process.
    pub fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc#{}", self.0)
    }
}

/// A new UUID v4, for an identity that must never repeat (a project's opening, an
/// operation…).
pub fn new_uuid() -> u128 {
    uuid::Uuid::new_v4().as_u128()
}

/// The identity of `name` inside `parent` (a server's database inside its data source):
/// the same pair always gives the same UUID — it's found again after a restart, never
/// saved — and two pairs practically never one. A 128-bit FNV-1a hash of both, stamped as
/// RFC 9562's version 8 (custom: derived, never random), so it never equals a v4 identity.
pub fn derived_uuid(parent: u128, name: &str) -> u128 {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET;
    for byte in parent.to_be_bytes().into_iter().chain([0xff]).chain(name.bytes()) {
        hash ^= u128::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    // Version 8 (bits 48–51 of the text's order), variant 10 (bits 64–65).
    let version = 0x8u128 << 76;
    let variant = 0x2u128 << 62;
    (hash & !(0xfu128 << 76) & !(0x3u128 << 62)) | version | variant
}

/// A UUID's text (`8-4-4-4-12` lowercase hex digits), as it's saved.
pub fn uuid_text(id: u128) -> String {
    uuid::Uuid::from_u128(id).to_string()
}

/// A UUID from its text (hyphenated or not), `None` if it isn't one.
pub fn parse_uuid(text: &str) -> Option<u128> {
    uuid::Uuid::parse_str(text.trim()).ok().map(|u| u.as_u128())
}

/// An open project's identity: a UUID v4 given at each opening, never derived from its
/// name or its path (two projects may share a name; a folder may be renamed, or closed and
/// opened again — another project, another state). What modules keep per project they
/// key by it; the path only says where the project is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectId(u128);

impl ProjectId {
    /// A new identity (UUID v4).
    pub fn new() -> Self {
        Self(new_uuid())
    }

    pub const fn get(self) -> u128 {
        self.0
    }
}

impl Default for ProjectId {
    /// A new identity: a project made without saying which is another project.
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", uuid::Uuid::from_u128(self.0))
    }
}

/// Defines a textual identifier, constructible in `const`/`static` contexts too.
macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            pub const fn new_static(id: &'static str) -> Self {
                Self(Cow::Borrowed(id))
            }

            pub fn new(id: impl Into<String>) -> Self {
                Self(Cow::Owned(id.into()))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_id! {
    /// A module identifier, e.g. `lang.php`, `terminal`.
    ModuleId
}

string_id! {
    /// A language identifier, e.g. `php`, `html`, `javascript`.
    LanguageId
}

impl LanguageId {
    pub const PLAIN_TEXT: Self = Self::new_static("plaintext");
}

#[cfg(test)]
#[path = "../tests/unit/id.rs"]
mod tests;
