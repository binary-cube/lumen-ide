//! The Agent Client Protocol (ACP), as data: how the editor talks with an agent it hosts as a
//! graphical conversation — JSON-RPC 2.0 over the agent's standard input and output, one
//! message a line ([`wire`]) — and the conversation the agent's updates build
//! ([`conversation`]). Pure: the process and its pipes are infrastructure's.

pub mod conversation;
pub mod wire;

pub use conversation::{
    Author, Choice, Conversation, Diff, Item, Permission, Step, Tool, ToolStatus,
};
pub use wire::Inbound;
