//! The window's decorations (the effects a theme draws), as whoever edits them changes them.

use kernel::Request;

/// The user's own decorations, one encoded layer a line (empty: none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetOwnDecorations {
    pub layers: String,
}

impl Request for SetOwnDecorations {
    type Answer = ();
}

/// What's chosen of how the decorations run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationPace {
    Mode,
    Fps,
    Idle,
    FollowSystem,
}

/// One of the decorations' choices made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetDecorationPace {
    pub what: DecorationPace,
    pub value: i64,
}

impl Request for SetDecorationPace {
    type Answer = ();
}
