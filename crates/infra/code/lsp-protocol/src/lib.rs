//! What both ends of LSP share, with no I/O of its own: JSON-RPC as LSP frames it
//! ([`jsonrpc`]), paths as `file://` URIs ([`uri`]), a document's lines and its positions
//! counted in the negotiated units ([`text`]).

pub mod jsonrpc;
pub mod text;
pub mod uri;
