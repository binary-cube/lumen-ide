//! What the app and its web engine's process (`web-host`, plan 99) say to each other.
//!
//! One `web-host` serves every window, project and instance of the app for a user: it
//! listens on a local channel named after the protocol's version ([`channel_name`]); an app
//! connects, says [`Hello`], is told [`Welcome`], then opens its pages and drives them with
//! [`Command`]s; the host answers with [`Event`]s. A page's picture never crosses as
//! pixels: each [`Frame`] carries a handle to a GPU texture (passed by the system's own way
//! between processes, beside the message), the app copies it on the GPU and says
//! [`Command::FrameDone`].
//!
//! Messages are framed by their length ([`framing`]) and encoded with `postcard` (compact,
//! no schema to parse). A version that changes them changes [`PROTOCOL`]: an app and a host
//! of other versions never talk (each checks the other's in the greeting), the app starts
//! the host of its own.
//!
//! The files they and the packaging share (the engine's `engine.json`, the packages' list,
//! the pages' pixels' names) are in [`files`].
//!
//! Pure: no I/O here. Everything read is hostile until checked (a length past
//! [`framing::MAX_MESSAGE`] is refused, never allocated).

pub mod files;
pub mod framing;
mod messages;

pub use files::{
    ENGINE_INFO, EngineInfo, PackageDescription, PackageEntry, PackageList, is_pixels_name,
    pixels_name,
};
pub use messages::*;

/// The longest message a page may send its app (bytes): a longer one is dropped where it
/// comes from, as the system's engines' pages' are (`platform_api::MAX_WEB_MESSAGE`).
pub const MAX_PAGE_MESSAGE: usize = 64 << 20;

/// The protocol's version: raised whenever a message changes.
pub const PROTOCOL: u32 = 3;

/// The local channel's name a host of this protocol listens on (one per user: the channel
/// itself is the user's — a named pipe of theirs on Windows, a socket in their private
/// folder elsewhere).
pub fn channel_name() -> String {
    format!("editor-web-host-{PROTOCOL}")
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
