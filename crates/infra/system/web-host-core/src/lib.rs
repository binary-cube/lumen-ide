//! The web engine's process (`web-host`, plan 99) apart from the engine itself: what makes
//! it **one** for every app of the user, and what it does with them.
//!
//! - [`channel`]: where it's found. One local channel per user and protocol, and a lock
//!   beside it: only the process holding the lock is the host (a second one started at the
//!   same moment, or while one runs, ends at once); a socket left by a crashed host is
//!   taken over, a live one never.
//! - [`Server`]: its clients (every app connected: each window and project of an app share
//!   one connection), their pages, and when it ends. Each client has a reader and a writer
//!   thread of its own (nothing is written under a lock, a slow client never holds the
//!   others); what they read comes to the host's main thread, where the engine lives
//!   ([`Server::poll`]). A client gone closes its pages; with no page for
//!   [`Lifetime::idle`], the host ends — not before: a page opened again soon finds the
//!   engine running (tabs come and go without starting anything). Only the user's own
//!   programs are served, each the process it says it is (`peer`).
//! - [`Engine`]: the contract the engine (CEF, in the `web-host` program) answers. Tested
//!   here with a fake one.

mod acks;
pub mod channel;
mod engine;
mod peer;
mod server;

pub use acks::Acks;
pub use engine::{Engine, Kept, Outbox, PageKey, Undelivered};
pub use server::{ClientId, Lifetime, Server, State};
