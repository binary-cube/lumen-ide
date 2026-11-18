//! Calm scenes over the whole screen, for while nobody's at the computer.

use kernel::Request;

/// Show a calm scene over the whole screen until it's left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShowCalmScene;

impl Request for ShowCalmScene {
    /// Whether it opened.
    type Answer = bool;
}
