//! Claude Code, as an agent the editor hosts: an [`AgentProvider`] the sessions' panel, the
//! graphical face and the MCP server reach without knowing it. It finds the program where
//! it's installed, lists a project's sessions from Claude Code's own files (read from their
//! ends, kept by their stamps: [`files`]) and says how a session starts or is taken up
//! again. Nothing runs before it's asked; every disk read is on the asker's worker.
//!
//! Its folder is one of `modules/ai/agents/`: another agent is another folder beside it,
//! on the same contract, and nothing else changes.

mod files;
pub mod format;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use agents::{AgentInstall, AgentProvider, AgentSession, Launch};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// Its id among the agents (saved: never a name that could change).
pub const ID: &str = "claude";
/// Its icon (contributed): Claude's own mark (from Simple Icons, CC0), drawn inside 20 × 20
/// of its 24 × 24 box, as the editor's line icons are.
pub const ICON: &str = "agent-claude";
const ICON_SVG: &str = concat!(
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><g transform="translate(2 2) scale(0.8333)"><path fill="#000" d=""##,
    "m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z",
    r##""/></g></svg>"##
);

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("agents.claude"),
    name: "Claude Code",
    description: "Claude Code as an agent the editor hosts: its sessions listed, started and taken up again",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct ClaudeModule;

impl Module for ClaudeModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn AgentProvider>>(Arc::new(Claude::default()));
        Ok(())
    }
}

/// Claude Code on this machine.
#[derive(Debug, Default)]
pub struct Claude {
    /// Its program, as last found (only ever looked for on a worker: [`AgentProvider::detect`];
    /// not found, it's looked for again next time).
    program: Mutex<Option<PathBuf>>,
    sessions: files::Sessions,
    /// Where its files are (tests: a folder of their own).
    config: Option<PathBuf>,
}

impl Claude {
    /// Reading its files from `config` instead of the user's (tests).
    pub fn with_config(config: PathBuf) -> Self {
        Self { config: Some(config), ..Self::default() }
    }

    /// Its program as last found (nothing read here: a click may ask).
    fn program(&self) -> Option<PathBuf> {
        self.program.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// How it starts with `args` in `cwd`: its program found (its folder first on `PATH`, for
    /// an npm launcher's `node`), else its name.
    fn launch(&self, args: &[&str], cwd: &Path) -> Launch {
        match self.program() {
            Some(program) => {
                let mut launch = Launch::new(program.to_string_lossy(), args, Some(cwd));
                launch.env.push(agent_files::path_beside(&program));
                launch
            }
            None => Launch::new("claude", args, Some(cwd)),
        }
    }
}

impl AgentProvider for Claude {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> String {
        "Claude Code".to_owned()
    }

    fn icon(&self) -> &'static str {
        ICON
    }

    /// Looked for on disk (a worker): found, kept for the launches; not found, looked for
    /// again next time (installed meanwhile).
    fn detect(&self) -> Option<AgentInstall> {
        let found = files::find_program();
        found.clone_into(&mut self.program.lock().unwrap_or_else(PoisonError::into_inner));
        Some(AgentInstall { program: found?, version: None })
    }

    fn installed(&self) -> bool {
        self.program.lock().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    fn sessions(&self, root: &Path, limit: usize) -> Vec<AgentSession> {
        let Some(config) = self.config.clone().or_else(files::config_dir) else {
            return Vec::new();
        };
        self.sessions.of(&config.join("projects"), root, limit)
    }

    fn new_session(&self, root: &Path) -> Launch {
        self.launch(&[], root)
    }

    fn resume(&self, session: &AgentSession) -> Launch {
        self.launch(&["--resume", &session.id], &session.folder)
    }

    /// Its ACP adapter (`claude-agent-acp`, installed with npm), told which Claude Code to
    /// run (the one found) and to speak the protocol's first version.
    fn acp(&self, root: &Path) -> Option<Launch> {
        let adapter = files::find(ACP_ADAPTER)?;
        let mut launch = Launch::new(adapter.to_string_lossy(), &[], Some(root));
        if let Some(program) = self.program().or_else(files::find_program) {
            launch.env.push(("CLAUDE_CODE_EXECUTABLE".into(), program.to_string_lossy().into()));
        }
        launch.env.push(("CLAUDE_AGENT_ACP_PROTOCOL".into(), "v1".into()));
        launch.env.push(agent_files::path_beside(&adapter));
        Some(launch)
    }

    fn can_rename(&self) -> bool {
        true
    }

    /// The `custom-title` record its `/rename` writes, added to the session's file.
    fn rename_session(&self, folder: &Path, id: &str, title: &str) -> Result<(), String> {
        let config = self.config.clone().or_else(files::config_dir).ok_or("no Claude Code")?;
        files::rename(&config.join("projects"), folder, id, title)
    }

    /// `claude mcp add-json` of the editor's server for every project (`--scope user`), named
    /// `editor`, its headers from the editor's helper; an older entry of that name removed
    /// first.
    fn mcp_setup(&self, url: &str, headers: &str) -> Vec<Launch> {
        let home = std::env::temp_dir();
        // Its `headersHelper`: run by Claude Code as it connects, its output the headers.
        let server = serde_json::json!({"type": "http", "url": url, "headersHelper": headers});
        vec![
            self.launch(&["mcp", "remove", "--scope", "user", MCP_NAME], &home),
            self.launch(
                &["mcp", "add-json", "--scope", "user", MCP_NAME, &server.to_string()],
                &home,
            ),
        ]
    }

    /// `npm install -g` of the adapter (npm found where Node puts it).
    fn acp_install(&self) -> Option<Launch> {
        let npm = files::find("npm")?;
        let mut launch = Launch::new(npm.to_string_lossy(), &["install", "-g", ACP_PACKAGE], None);
        launch.env.push(agent_files::path_beside(&npm));
        Some(launch)
    }
}

/// The editor's MCP server's name in Claude Code's configuration (never the app's name).
pub const MCP_NAME: &str = "editor";

/// Its graphical face's adapter: the program, and the npm package it comes in (Claude
/// Code's ACP server, kept by the protocol's own project).
pub const ACP_ADAPTER: &str = "claude-agent-acp";
pub const ACP_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp";

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
