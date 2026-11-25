//! The web host's parts, shared by its two programs (the host, and on macOS its helper for
//! CEF's subprocesses): see `main.rs` for what the host does.

pub mod access;
pub mod app;
pub mod engine;
pub mod handlers;
pub mod ipc;
#[cfg(target_os = "macos")]
pub mod mac_app;
pub mod options;
pub mod pixels;
pub mod pump;
pub mod sandbox;
pub mod scheme;
pub mod transfer;

/// A string CEF hands back (a URL, a selection, a message's name), as ours.
pub fn text_of(value: &cef::CefStringUserfree) -> String {
    cef::CefString::from(value).to_string()
}
