//! What a look found, as the views get it: the whole app's values now and each part's row
//! ([`Frame`], shared by every view of a scope as one `Arc`), with the programs it started
//! ([`ProcessRow`]). The values over time are the hub's ([`super::history`]), not here: a
//! look carries only the present.

use kernel::ProjectId;
use workbench_model::OpenProject;

/// A row's key: its owner, its name and its project's identity (never the project's name:
/// two projects may share one).
pub fn key(owner: &str, name: &str, project: Option<ProjectId>) -> String {
    match project {
        Some(id) => format!("{owner}/{name}/{id}"),
        None => format!("{owner}/{name}"),
    }
}

/// A program a part started, with everything it started in turn (their use added).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ProcessRow {
    pub pid: u32,
    /// Its program's name (`git`, `zsh`), when the system tells.
    pub name: Option<String>,
    /// CPU, as a percentage of one core.
    pub cpu: f32,
    pub memory: u64,
    /// Disk: bytes a second (read and written).
    pub disk: f32,
    /// The name it was started by, when it isn't its program's (`claude` for a program
    /// named `2.1.325`): what the user typed.
    pub invoked: Option<String>,
    /// The programs in its tree (itself and what it started).
    pub processes: u32,
    /// How long it has been running.
    pub running: std::time::Duration,
    /// The project it works for, by name, when its part is every project's (a terminal's
    /// shell or agent).
    pub project: Option<String>,
    /// What it's called where the user sees it (its terminal tab's title), when its part
    /// says.
    pub title: Option<String>,
}

/// A part's row.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Its [`key`] (worked out once).
    pub key: String,
    pub name: String,
    pub owner: String,
    /// Its group ([`workbench_model::resources::APP`]…).
    pub group: &'static str,
    /// CPU, as a percentage of one core (it can pass 100 on several).
    pub cpu: f32,
    pub memory: Option<u64>,
    pub gpu: Option<u64>,
    /// Network: bytes a second (sent and received), when it uses it.
    pub net: Option<f32>,
    /// Disk: bytes a second (read and written), when it counts them.
    pub disk: Option<f32>,
    pub detail: String,
    /// The threads counted for it.
    pub threads: usize,
    /// Those threads by name, with their CPU (percent of a core) — the detail lists them.
    pub thread_cpu: Vec<(String, f32)>,
    /// The project it works for (`None`: the app's, every project's).
    pub project: Option<ProjectId>,
    /// Its operations (running, and the last ended).
    pub activities: Vec<workbench_model::Activity>,
    /// The programs it started (running).
    pub processes: Vec<ProcessRow>,
    /// What can be done to it (done by whoever reported it).
    pub actions: Vec<workbench_model::PartAction>,
    /// What only its reporter knows of it (label, value: a language server's state,
    /// latency…), listed in its detail.
    pub facts: Vec<(String, String)>,
}

impl Line {
    /// Part `name` of `owner` (for `project`) in `group`, measuring nothing yet.
    pub fn new(
        name: impl Into<String>,
        owner: impl Into<String>,
        group: &'static str,
        project: Option<ProjectId>,
    ) -> Self {
        let (name, owner) = (name.into(), owner.into());
        Self {
            key: key(&owner, &name, project),
            name,
            owner,
            group,
            cpu: 0.0,
            memory: None,
            gpu: None,
            net: None,
            disk: None,
            detail: String::new(),
            threads: 0,
            thread_cpu: Vec::new(),
            project,
            activities: Vec::new(),
            processes: Vec::new(),
            actions: Vec::new(),
            facts: Vec::new(),
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// The part as its reporter named it (an action's address).
    pub fn part_ref(&self) -> workbench_model::PartRef {
        workbench_model::PartRef {
            owner: self.owner.clone(),
            name: self.name.clone(),
            project: self.project,
        }
    }

    /// Its actions on `target`.
    pub fn actions_on(
        &self,
        target: workbench_model::ActionTarget,
    ) -> impl Iterator<Item = &workbench_model::PartAction> + '_ {
        self.actions.iter().filter(move |a| a.target == target)
    }
}

/// What a look found: the whole app's values now and every part's row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Frame {
    pub lines: Vec<Line>,
    /// The whole app.
    pub cpu: f32,
    pub memory: u64,
    pub gpu: Option<u64>,
    pub net: f32,
    /// The process's disk, bytes a second, as the system counts it (when it tells).
    pub disk: Option<f32>,
    pub threads: usize,
    /// The open projects shown (this one, or all).
    pub projects: Vec<OpenProject>,
    /// The view of every open project.
    pub all: bool,
}

impl Frame {
    /// The row of part `key`.
    pub fn line(&self, key: &str) -> Option<&Line> {
        self.lines.iter().find(|l| l.key == key)
    }
}

/// A look as the hub hands it to a view: its number (counting up, the scope's) and what it
/// found.
#[derive(Debug, Clone)]
pub struct Tick {
    pub generation: u64,
    pub now: std::sync::Arc<Frame>,
}
