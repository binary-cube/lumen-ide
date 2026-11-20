//! What still runs when a window, a project or the app closes, so the user is asked first:
//! each module answers through its [`ResourceReporter::running`] (the Task Manager's
//! reporter — one source of truth for what runs), by default its parts' operations still
//! running. Asked only when something closes, never per frame.
//!
//! Every item says what stopping it costs ([`StopImpact`]) and how it's stopped
//! ([`StopVia`]: the same action the Task Manager offers, done by its reporter). The closing
//! asks once: [`to_ask`] keeps the items of what closes ([`CloseScope`]) that matter,
//! [`grouped`] arranges them by project and module for the question ([`RunningPrompt`]).

use std::time::Instant;

use kernel::ProjectId;

use crate::resources::{
    ActionTarget, Activity, ActivityKind, ActivityUnit, PartRef, ResourceUse, STOP_OPERATION,
};

/// What stopping a piece of work costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StopImpact {
    /// Something is lost or left half done (an upload, a program the user started, a
    /// query's results).
    LosesWork,
    /// Safe to stop: it starts again later (a fetch, a download).
    Interrupts,
    /// Never asked about (an index, the local history).
    Background,
}

impl ActivityKind {
    /// What stopping an operation of this kind costs.
    pub const fn stop_impact(self) -> StopImpact {
        match self {
            Self::Upload
            | Self::Sync
            | Self::Copy
            | Self::Move
            | Self::Delete
            | Self::Save
            | Self::Export
            | Self::Query
            | Self::Permissions => StopImpact::LosesWork,
            Self::Download => StopImpact::Interrupts,
            Self::Index | Self::History | Self::Measure => StopImpact::Background,
        }
    }
}

/// How a piece of work is stopped: action `action` on `target` of part `part`, done by the
/// reporter that listed it ([`crate::ResourceReporter::act`]) — as from the Task Manager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopVia {
    pub part: PartRef,
    pub action: &'static str,
    pub target: ActionTarget,
}

/// How far a piece of work got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkProgress {
    pub done: u64,
    /// How much in all, when known.
    pub total: Option<u64>,
    pub unit: ActivityUnit,
}

impl WorkProgress {
    /// The part done (0–1), when the total is known.
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total.filter(|t| *t > 0)?;
        Some((self.done as f64 / total as f64).clamp(0.0, 1.0) as f32)
    }
}

/// What a piece of work is: an operation (its kind names it) or something else (its icon).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkKind {
    Operation(ActivityKind),
    /// A program running (a terminal's).
    Program,
    /// A query on a database.
    Query,
    /// A version control operation on the network.
    Network,
}

/// A piece of work still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningWork {
    pub kind: WorkKind,
    /// What it is or works on (a file, a program, a console), in the user's language.
    pub title: String,
    /// More about it (where, how far), in the user's language; may be empty.
    pub detail: String,
    /// Whose it is: a module's id.
    pub owner: String,
    /// The project it works for (`None`: the app's, every project's).
    pub project: Option<ProjectId>,
    pub impact: StopImpact,
    pub progress: Option<WorkProgress>,
    /// When it started, when known.
    pub since: Option<Instant>,
    /// How it's stopped (`None`: it can't be safely; it finishes on its own).
    pub stop: Option<StopVia>,
}

impl RunningWork {
    /// Work `title` of `owner`, for `project`, costing `impact` if stopped.
    pub fn new(
        kind: WorkKind,
        title: impl Into<String>,
        owner: impl Into<String>,
        project: Option<ProjectId>,
        impact: StopImpact,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: String::new(),
            owner: owner.into(),
            project,
            impact,
            progress: None,
            since: None,
            stop: None,
        }
    }

    /// Operation `a` (a running one) of part `part`: stopped through the part's "Stop" when
    /// it can be.
    pub fn of_activity(part: &ResourceUse, a: &Activity) -> Self {
        let mut work = Self::new(
            WorkKind::Operation(a.kind),
            a.subject.clone(),
            part.owner.clone(),
            a.project.or(part.project),
            a.kind.stop_impact(),
        );
        // Where it runs (a query's source and console), else whose part it is.
        work.detail = if a.detail.is_empty() { part.name.clone() } else { a.detail.clone() };
        work.progress = Some(WorkProgress { done: a.done, total: a.total, unit: a.unit });
        work.since = Some(a.started);
        work.stop = a.stoppable.then(|| StopVia {
            part: PartRef {
                owner: part.owner.clone(),
                name: part.name.clone(),
                project: part.project,
            },
            action: STOP_OPERATION,
            target: ActionTarget::Activity(a.id),
        });
        work
    }
}

/// The operations still running in `parts` (what a reporter runs, by default).
pub fn of_parts(parts: &[ResourceUse]) -> Vec<RunningWork> {
    parts
        .iter()
        .flat_map(|part| {
            part.activities
                .iter()
                .filter(|a| a.running())
                .map(move |a| RunningWork::of_activity(part, a))
        })
        .collect()
}

/// The projects a closing ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosedProjects {
    /// None (a window without a project).
    Nothing,
    One(ProjectId),
    /// Every one (the app quits).
    All,
}

/// What a closing ends: its projects, and the app's own work (what's every project's)
/// when the app goes with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloseScope {
    pub projects: ClosedProjects,
    pub app_wide: bool,
}

impl CloseScope {
    /// A window closed (with its project, if it has one); the app's work goes too when it
    /// was the last window.
    pub const fn window(project: Option<ProjectId>, last: bool) -> Self {
        let projects = match project {
            Some(id) => ClosedProjects::One(id),
            None => ClosedProjects::Nothing,
        };
        Self { projects, app_wide: last }
    }

    /// A project closed, its window kept (Close Project, another project opened there).
    pub const fn project(id: ProjectId) -> Self {
        Self { projects: ClosedProjects::One(id), app_wide: false }
    }

    /// The app quits: everything.
    pub const fn quit() -> Self {
        Self { projects: ClosedProjects::All, app_wide: true }
    }

    /// Whether work for `project` (`None`: the app's) ends with it.
    pub fn includes(&self, project: Option<ProjectId>) -> bool {
        match (project, self.projects) {
            (None, _) => self.app_wide,
            (Some(_), ClosedProjects::All) => true,
            (Some(p), ClosedProjects::One(id)) => p == id,
            (Some(_), ClosedProjects::Nothing) => false,
        }
    }
}

/// What to ask about before closing `scope`: its items that aren't background work, those
/// losing work first, then by project (the app's last) and module.
pub fn to_ask(
    items: impl IntoIterator<Item = RunningWork>,
    scope: &CloseScope,
) -> Vec<RunningWork> {
    let mut asked: Vec<RunningWork> = items
        .into_iter()
        .filter(|w| w.impact != StopImpact::Background && scope.includes(w.project))
        .collect();
    asked.sort_by(|a, b| {
        (a.project.is_none(), a.project, &a.owner, a.impact).cmp(&(
            b.project.is_none(),
            b.project,
            &b.owner,
            b.impact,
        ))
    });
    asked
}

/// One module's work for one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkGroup {
    pub project: Option<ProjectId>,
    pub owner: String,
    pub items: Vec<RunningWork>,
}

/// `items` ([`to_ask`]'s, in its order) grouped by project, then module.
pub fn grouped(items: Vec<RunningWork>) -> Vec<WorkGroup> {
    let mut groups: Vec<WorkGroup> = Vec::new();
    for item in items {
        match groups.iter_mut().find(|g| g.project == item.project && g.owner == item.owner) {
            Some(group) => group.items.push(item),
            None => groups.push(WorkGroup {
                project: item.project,
                owner: item.owner.clone(),
                items: vec![item],
            }),
        }
    }
    groups
}

/// What the user chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseAnswer {
    /// Each item stopped (those that can be), then the closing goes on.
    StopAndClose,
    /// The closing waits for them to end, then goes on by itself.
    Wait,
    /// Nothing closes.
    Cancel,
}

/// What closes (the question's title says it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Closing {
    Window,
    Project,
    App,
    /// Nothing closes: a module restarts its work for a project (a remote project
    /// reconnecting with new settings), asked through [`crate::WorkbenchHost::ask_running`].
    Restart,
}

/// What the question shows: the work grouped, the names of its projects and modules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningPrompt {
    pub closing: Closing,
    pub groups: Vec<WorkGroup>,
    /// The projects' names (by identity).
    pub projects: Vec<(ProjectId, String)>,
    /// The modules' names (by id), in the user's language.
    pub modules: Vec<(String, String)>,
    /// The user chose to wait: the list follows the work until it ends.
    pub waiting: bool,
    /// The app is ending and what it started is being stopped: the question is past, the
    /// window shows how far that is (and can't be closed meanwhile).
    pub stopping: Option<Stopping>,
}

/// The programs the app started, being stopped as it ends: each one and where it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stopping {
    pub programs: Vec<StoppingProgram>,
}

impl Stopping {
    /// How many have ended (by themselves or made to).
    pub fn ended(&self) -> usize {
        self.programs.iter().filter(|p| p.state != ProgramState::Asked).count()
    }

    /// Every one has ended.
    pub fn done(&self) -> bool {
        self.ended() == self.programs.len()
    }
}

/// A program being stopped: its name, its id, where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoppingProgram {
    pub name: String,
    pub pid: u32,
    pub state: ProgramState,
}

/// Where a program being stopped is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramState {
    /// Asked to end, not yet gone.
    Asked,
    /// Ended when asked.
    Ended,
    /// Didn't end in time: made to.
    Forced,
}

impl RunningPrompt {
    /// How many items it lists.
    pub fn count(&self) -> usize {
        self.groups.iter().map(|g| g.items.len()).sum()
    }

    /// The name of the module owning work of `owner` ([`same_module`]).
    pub fn module_name(&self, owner: &str) -> Option<&str> {
        self.modules.iter().find(|(id, _)| same_module(id, owner)).map(|(_, n)| n.as_str())
    }
}

/// Whether owner `owner` is module `id`: the same id, written with `.`, `-` or `_` alike
/// (`project-tree`, `project.tree`), so a part's owner always finds its module.
pub fn same_module(id: &str, owner: &str) -> bool {
    let plain = |c: char| if matches!(c, '.' | '-') { '_' } else { c.to_ascii_lowercase() };
    id.len() == owner.len() && id.chars().map(plain).eq(owner.chars().map(plain))
}

/// The question's view (its virtual extension), contributed by a module.
pub const PROMPT_VIEW: &str = "view-running-work";

#[cfg(test)]
#[path = "../tests/unit/running.rs"]
mod tests;
