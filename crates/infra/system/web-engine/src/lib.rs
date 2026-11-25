//! The app's side of its web engine (plan 99): web pages drawn by Chromium in its own
//! process (`web-host`), shared by every window, project and instance of the app.
//!
//! - [`Connection`]: one per app process ([`shared`]), made on the first page: it connects
//!   to the host that runs, or starts one ([`Launch`]) and waits for it to listen. Commands
//!   go out through a writer thread, events come in on a reader thread and go to their
//!   page's sink. The host gone (it ended, it crashed): every page is told
//!   ([`web_engine_protocol::Event::Crashed`]) and the next page opened connects anew. A
//!   host started that fails by itself is told apart ([`EngineFailed`]) from everything else
//!   that keeps a connection from being made: only that counts against its version.
//! - [`WebPage`]: a page opened through it, driven by its methods, closed when dropped.
//! - [`install`]: the engine itself, downloaded once from the project's signed list and
//!   kept up to date under the app's data, its old versions cleaned once nothing runs them.
//!
//! Nothing here runs on the app's UI thread but sending (a channel's push): connecting and
//! starting the host happen on the caller's thread, which the app keeps off its UI thread.

mod connection;
mod frames;
pub mod install;
mod launch;
mod page;
#[cfg(target_os = "macos")]
mod peer_mac;
#[cfg(windows)]
mod peer_windows;

pub use connection::{Connection, EngineFailed, PageEvent, Sink, engine_failed, shared};
pub use frames::{ArrivedFrame, Native};
pub use launch::{Ended, Launch, Started, channel};
pub use page::{PageSpec, WebPage};
pub use web_engine_protocol as protocol;

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
