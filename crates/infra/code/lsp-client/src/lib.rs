//! Generic LSP client: an adapter for the Intelligence ports. Any server speaking LSP on
//! stdio, described as data ([`spec`]), started from the app's bundled `lsp/` folder
//! ([`bundle`]) or anywhere, driven by one thread of its own ([`client`]): JSON-RPC
//! ([`jsonrpc`]), the documents kept in step with incremental changes counted in the
//! server's units ([`text`]), its answers turned into the Intelligence ports' types
//! ([`convert`], [`snippet`], [`markdown`], [`uri`]), its figures kept for the Task Manager
//! ([`status`]). The servers that aren't bundled are downloaded once into a folder shared by
//! every project and version of the app ([`downloads`]: the manifest and each component's
//! install; [`store`]: the folder; [`fetch`]: HTTPS from the pinned sources only, checked;
//! [`archive`]: unpacked by our code; [`assemble`]: the steps shared with the bundler);
//! [`install`] installs those that can't be redistributed (the user's click). [`fake`] is a
//! server for tests.

pub mod archive;
pub mod assemble;
pub mod bundle;
pub mod client;
pub mod convert;
pub mod downloads;
#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod fetch;
pub mod install;
pub mod launch;
pub mod markdown;
pub mod orphans;
pub mod snippet;
pub mod spec;
pub mod status;
pub mod store;

pub use lsp_protocol::{jsonrpc, text, uri};

#[cfg(test)]
#[path = "../tests/unit/real_servers.rs"]
mod real_servers;
