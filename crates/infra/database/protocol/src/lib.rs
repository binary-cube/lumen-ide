//! The drivers' protocol (plan 37i): a driver installed apart — a program of any language,
//! a bridge to JDBC — working as one of the app's own `database` drivers.
//!
//! - [`dto`]: the messages, versioned ([`driver::VERSIONS`]; the handshake picks the
//!   newest version both sides speak, none in common is refused with why).
//! - [`wire`]: JSON-RPC 2.0 in `Content-Length` frames (as LSP), the methods and the
//!   errors' codes.
//! - [`convert`]: the messages to and from the domain's model.
//! - [`client`]: the editor's end of the pipe (ids, cancel, timeouts, the driver's error
//!   output kept short and scrubbed of the password, the program stopped with its session).
//! - [`driver`]: [`ProtocolDriver`], a `database::Driver` whose sessions are the driver's
//!   program; who writes the SQL is the driver's choice ([`dto::Sql`]): the editor in one
//!   of its dialects (the driver `execute`s it: what the preview shows is what runs), or
//!   nobody (the driver gets the changes as they are, and may plan its own statements for
//!   structure changes, shown and then run as shown).
//! - [`manifest`]: how drivers are installed (`drivers/<name>/driver.toml`), found and
//!   listed without starting anything.
//! - [`server`]: the driver's end, for a driver written in Rust.
//!
//! The password goes only in `connect`'s message, on the pipe: never in the program's
//! arguments or environment, never in a log (the frames holding it are wiped once written
//! or read).

pub mod client;
pub mod convert;
pub mod driver;
pub mod dto;
pub mod manifest;
pub mod server;
#[cfg(any(test, feature = "toy"))]
pub mod toy;
pub mod wire;

pub use client::{Connection, Transport};
pub use driver::{Launch, ProtocolDriver, ProtocolSession, VERSIONS};
pub use manifest::{Installed, Manifest, ProgramLaunch};
