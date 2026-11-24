//! The messages' shapes, as they cross the pipe (JSON; a manifest's description in TOML).
//! They mirror the domain's model, but they're the protocol's own: the domain changes
//! freely, these only with a new protocol version (a field added is optional, and ignored
//! by whoever doesn't know it). Names are `snake_case`; an enum is a string (`"null"`,
//! `"any"`) or an object of one key (`{"int": 5}`, `{"up_to": 1000}`).

use serde::{Deserialize, Serialize};

mod changing;
mod console;
mod handshake;
mod profile;
mod reading;

pub use changing::*;
pub use console::*;
pub use handshake::*;
pub use profile::*;
pub use reading::*;

/// A range of protocol versions (both included).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    pub min: u32,
    pub max: u32,
}

impl Versions {
    pub fn contains(self, version: u32) -> bool {
        (self.min..=self.max).contains(&version)
    }

    /// The newest version both speak.
    pub fn common(self, other: Self) -> Option<u32> {
        let (low, high) = (self.min.max(other.min), self.max.min(other.max));
        (low <= high).then_some(high)
    }

    pub fn pair(self) -> (u32, u32) {
        (self.min, self.max)
    }
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}
