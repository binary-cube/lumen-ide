//! What still runs when something closes (a window, its project, the app): asked of the
//! modules' reporters ([`workbench_model::ResourceReporter::running`]) only then, the
//! question shown in the module's modal ([`workbench_model::running::PROMPT_VIEW`]), what
//! was asked about stopped through the same reporters. The app knows no kind of work.

use workbench_model::running::{self, CloseScope, Closing, RunningPrompt, RunningWork};

use super::*;

/// A module's question before restarting its work for a project (a reconnect): whose work
/// (a module's id), and what runs once it's let go.
pub struct RunningRequest {
    pub project: kernel::ProjectId,
    pub owner: String,
    pub then: workbench_model::OnYes,
}

impl std::fmt::Debug for RunningRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunningRequest").field("owner", &self.owner).finish_non_exhaustive()
    }
}

/// Work `work` is module `owner`'s (`None`: anyone's).
fn owned_by(work: &RunningWork, owner: Option<&str>) -> bool {
    owner.is_none_or(|owner| running::same_module(owner, &work.owner))
}

impl Workbench {
    /// What still runs that closing `scope` stops (background work left out), in the order
    /// it's asked about.
    #[cfg(test)]
    pub fn running_work(&self, scope: &CloseScope) -> Vec<RunningWork> {
        self.running_work_of(scope, None)
    }

    /// What still runs that closing `scope` stops (background work left out) — only module
    /// `owner`'s when given (a restart) — in the order it's asked about.
    pub fn running_work_of(&self, scope: &CloseScope, owner: Option<&str>) -> Vec<RunningWork> {
        let all = self.support.resource_reporters.iter().flat_map(|r| r.running());
        let all = all.filter(|w| owned_by(w, owner));
        let mut items = running::to_ask(all, scope);
        // This window's project first (then the others', the app's last).
        let this = self.project_identity().map(|(id, _)| id);
        items.sort_by_key(|w| (w.project.is_none(), w.project != this));
        items
    }

    /// Everything that closing `scope` stops told to stop (by whoever reported it, as from
    /// the Task Manager; without waiting) — only module `owner`'s when given (a restart).
    /// What can't be stopped finishes on its own.
    pub fn stop_running_of(&self, scope: &CloseScope, owner: Option<&str>) {
        for reporter in &self.support.resource_reporters {
            let items = reporter.running().into_iter().filter(|w| owned_by(w, owner));
            for work in running::to_ask(items, scope) {
                let Some(stop) = &work.stop else { continue };
                let outcome = reporter.act(&stop.part, stop.action, stop.target);
                tracing::info!(owner = %work.owner, ?outcome, "running work stopped before closing");
            }
        }
    }

    /// The question about `items` (closing `closing`): grouped, with the projects' names
    /// (this window's and `others`) and the modules'.
    pub fn running_prompt(
        &self,
        closing: Closing,
        items: Vec<RunningWork>,
        waiting: bool,
        others: &[(kernel::ProjectId, String)],
    ) -> RunningPrompt {
        let own = self.project_identity();
        let this = own.as_ref().map(|(id, _)| *id);
        let mut projects: Vec<(kernel::ProjectId, String)> = own.into_iter().collect();
        projects.extend(others.iter().filter(|(id, _)| Some(*id) != this).cloned());
        let modules = self
            .support
            .modules
            .iter()
            .map(|m| {
                let id = m.id.as_str().to_owned();
                let name = frameworks::localized(&id, "_name", m.name);
                (id, name)
            })
            .collect();
        RunningPrompt { closing, groups: running::grouped(items), projects, modules, waiting }
    }

    /// This window's project: its identity and its name.
    pub fn project_identity(&self) -> Option<(kernel::ProjectId, String)> {
        let root = self.project.as_ref()?.root_path();
        let name = root
            .file_name()
            .map_or_else(|| root.display().to_string(), |n| n.to_string_lossy().into_owned());
        Some((self.data.id, name))
    }

    /// Shows the question (the module's modal). `false`: no module shows it.
    pub fn open_running_prompt(&mut self, prompt: &RunningPrompt) -> bool {
        let title = i18n::t(match prompt.closing {
            Closing::Window => "running.title_window",
            Closing::Project => "running.title_project",
            Closing::App => "running.title_app",
            Closing::Restart => "running.title_restart",
        });
        self.open_modal_view(&title, running::PROMPT_VIEW, prompt)
    }

    /// A module's question before restarting its work, once (asked by the app after the
    /// current input).
    pub fn take_running_request(&mut self) -> Option<RunningRequest> {
        self.running_request.take()
    }

    /// The question is open.
    pub fn running_prompt_open(&self) -> bool {
        self.modal_view.as_ref().is_some_and(|m| m.extension == running::PROMPT_VIEW)
    }

    /// The question shows `prompt` now (the work as it goes, while waiting).
    pub fn update_running_prompt(&mut self, prompt: &RunningPrompt) {
        if self.running_prompt_open()
            && let Some(modal) = &mut self.modal_view
        {
            modal.viewer.set_data(prompt);
        }
    }

    /// The question closed (its work ended while waiting, or answered from elsewhere).
    pub fn close_running_prompt(&mut self) {
        if self.running_prompt_open() {
            self.modal_view = None;
        }
    }
}
