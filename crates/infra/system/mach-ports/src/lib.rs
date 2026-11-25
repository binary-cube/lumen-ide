//! macOS: a Mach port passed from one process to another — how the web engine hands its
//! app a frame's `IOSurface` (plan 99: the system's safe way, no copy, nothing global).
//!
//! The receiving process checks in a service under a name of its own ([`Inbox::check_in`]:
//! its process and a random part, told to the sender through their own channel); the
//! sender looks it up ([`Outbox::look_up`]) and sends a send right with two numbers (a page
//! and its frame). Every message received carries the system's audit trailer: one not sent
//! by the expected process ([`Peer`]: its id, and its id's version when known, so a later
//! process given a dead one's id is refused too) is dropped, its right let go. A receive
//! waits as long as it's asked (or for good), and another thread of the process can end it
//! ([`Waker`]).
//!
//! Elsewhere this crate is empty.

#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::{Delivery, Inbox, Outbox, Peer, Right, Waker};

#[cfg(all(test, target_os = "macos"))]
#[path = "../tests/unit/lib.rs"]
mod tests;
