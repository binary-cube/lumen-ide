//! An AI agent's session as a graphical conversation, as a module: a tab of the editor that
//! talks with the agent through its Agent Client Protocol adapter (found by its own module,
//! `agents::AgentProvider::acp`; this one names no agent). Opened when asked
//! ([`OpenAgentChat`]: the sessions' panel asks, never naming this module); the agent's process
//! lives as long as the tab.

mod field;
mod layout;
mod view;

use std::path::Path;
use std::sync::Arc;

use contracts::agents::OpenAgentChat;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::{Viewer, ViewerKind, ViewerProvider};

#[cfg(feature = "snapshot")]
pub use view::snapshot;
pub use view::{ChatStart, ChatView};

/// Its tab's virtual extension.
pub const EXTENSION: &str = "view-agent-chat";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("agents.chat"),
    name: "Agent Chat",
    description: "An AI agent's session as a graphical conversation: its tools, its changes compared, its permissions answered",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "comment", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct ChatModule;

impl Module for ChatModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn ViewerProvider>>(Arc::new(Chats));
        // A session asked for as a conversation: its tab (brought to the front if open).
        ctx.contribute(workbench_model::Answers::of(|open: OpenAgentChat, host| {
            let start = ChatStart {
                agent: open.agent,
                session: open.session,
                folder: open.folder,
                title: open.title.clone(),
            };
            host.open_view(&open.title, EXTENSION, &start)
        }));
        Ok(())
    }
}

#[derive(Debug)]
struct Chats;

impl ViewerProvider for Chats {
    fn id(&self) -> &'static str {
        "agents.chat"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(ChatView::default()))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
