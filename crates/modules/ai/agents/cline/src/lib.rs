//! Cline (its command-line program, `cline`), as an agent the editor hosts: an
//! [`AgentProvider`] the sessions' panel reaches without knowing it. It finds the program
//! where it's installed, lists a project's sessions from Cline's own files ([`sessions`]:
//! one folder per session under `~/.cline/data/sessions`, its facts in `<id>.json`), and
//! says how a session starts (`cline --tui`) or is taken up again (`cline --tui --id <id>`).
//! Nothing runs before it's asked; every disk read is on the asker's worker.

mod sessions;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use agents::{AgentInstall, AgentProvider, AgentSession, Launch};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// Its id among the agents (saved: never a name that could change).
pub const ID: &str = "cline";
/// Its icon (contributed, of our own drawing: a robot's face, its ears, its two eyes).
pub const ICON: &str = "agent-cline";
const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="5" y="6" width="14" height="13" rx="4"/><path d="M12 6V3"/><path d="M3 11v3"/><path d="M21 11v3"/><path d="M9.5 11v2"/><path d="M14.5 11v2"/></svg>"##;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("agents.cline"),
    name: "Cline",
    description: "Cline as an agent the editor hosts: its sessions listed, started and taken up again",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "sparkles", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct ClineModule;

impl Module for ClineModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(languages::IconSvg { name: ICON, svg: ICON_SVG });
        ctx.contribute::<Arc<dyn AgentProvider>>(Arc::new(Cline::default()));
        Ok(())
    }
}

/// Cline on this machine.
#[derive(Debug, Default)]
pub struct Cline {
    /// Its program, as last found (only ever looked for on a worker).
    program: Mutex<Option<PathBuf>>,
    sessions: sessions::Sessions,
    /// Where its data is (tests: a folder of their own).
    data: Option<PathBuf>,
}

impl Cline {
    /// Reading its sessions from `data` instead of the user's (tests).
    pub fn with_data(data: PathBuf) -> Self {
        Self { data: Some(data), ..Self::default() }
    }

    fn program(&self) -> Option<PathBuf> {
        self.program.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Where Cline keeps its data: `~/.cline/data` (its `--data-dir`'s default).
    fn data_dir(&self) -> Option<PathBuf> {
        self.data.clone().or_else(|| Some(dirs::home_dir()?.join(".cline").join("data")))
    }

    /// How it starts with `args` in `cwd`: its program found (its folder first on `PATH`,
    /// for an npm launcher's `node`), else its name.
    fn launch(&self, args: &[&str], cwd: &Path) -> Launch {
        match self.program() {
            Some(program) => {
                let mut launch = Launch::new(program.to_string_lossy(), args, Some(cwd));
                launch.env.push(agent_files::path_beside(&program));
                launch
            }
            None => Launch::new("cline", args, Some(cwd)),
        }
    }
}

impl AgentProvider for Cline {
    fn id(&self) -> &'static str {
        ID
    }

    fn name(&self) -> String {
        "Cline".to_owned()
    }

    fn icon(&self) -> &'static str {
        ICON
    }

    fn detect(&self) -> Option<AgentInstall> {
        let found = agent_files::find("cline", &[]);
        found.clone_into(&mut self.program.lock().unwrap_or_else(PoisonError::into_inner));
        Some(AgentInstall { program: found?, version: None })
    }

    fn installed(&self) -> bool {
        self.program.lock().unwrap_or_else(PoisonError::into_inner).is_some()
    }

    fn sessions(&self, root: &Path, limit: usize) -> Vec<AgentSession> {
        let Some(data) = self.data_dir() else { return Vec::new() };
        self.sessions.of(&data.join("sessions"), root, limit)
    }

    /// Its terminal interface, in the project.
    fn new_session(&self, root: &Path) -> Launch {
        self.launch(&["--tui"], root)
    }

    /// Its terminal interface, the session taken up again by its id.
    fn resume(&self, session: &AgentSession) -> Launch {
        self.launch(&["--tui", "--id", &session.id], &session.folder)
    }

    /// Its own ACP mode (`cline --acp`), once it's found.
    fn acp(&self, root: &Path) -> Option<Launch> {
        self.program()?;
        Some(self.launch(&["--acp"], root))
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
