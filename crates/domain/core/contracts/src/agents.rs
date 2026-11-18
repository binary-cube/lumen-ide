//! An agent's session opened as a graphical conversation, by a module that doesn't know who
//! shows it (the sessions' panel).

use std::path::PathBuf;

use kernel::Request;

/// Open agent `agent`'s session `session` (`None`: a new one) in `folder` as a conversation,
/// its tab called `title`. Asked again with the same title, that tab comes to the front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAgentChat {
    pub agent: &'static str,
    pub session: Option<String>,
    pub folder: PathBuf,
    pub title: String,
}

impl Request for OpenAgentChat {
    /// Whether its tab opened (or came to the front).
    type Answer = bool;
}

/// The session whose tab is known by `key` (its terminal's) renamed `title` by the user, in
/// the agent too: whoever follows the sessions answers, the agent's own way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameSession {
    pub key: String,
    pub title: String,
}

impl Request for RenameSession {
    /// `false`: no session known for that tab, or its agent can't be renamed so.
    type Answer = bool;
}
