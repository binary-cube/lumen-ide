//! What modules ask of each other (see `docs/plan/69-comunicare-module.md`): each request is a
//! [`kernel::Request`] with the data it carries and the answer it gets, grouped by what it's
//! about. Nothing here knows who answers; whoever asks handles nobody answering.

pub mod agents;
pub mod decorations;
pub mod profiling;
pub mod scenes;
pub mod sleep;
pub mod terminal;
