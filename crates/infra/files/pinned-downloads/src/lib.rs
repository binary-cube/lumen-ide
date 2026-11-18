//! Downloads nothing uses before they're checked, shared by whoever fetches a pinned file
//! once (the language servers, a web app installed on demand): [`fetch`] brings a file over
//! HTTPS only from the hosts its caller allows (every redirect checked too), checked against
//! its pinned SHA-256 or npm's SHA-512, a large one resumed from its `.part`; [`archive`]
//! unpacks it by our own code, nothing written outside its folder; [`digest`] writes the
//! sums as the pins are written; [`paths`] says which paths a pattern names. [`samples`]
//! makes archives in memory for tests.

pub mod archive;
pub mod digest;
pub mod fetch;
pub mod paths;
#[cfg(any(test, feature = "fake"))]
pub mod samples;
