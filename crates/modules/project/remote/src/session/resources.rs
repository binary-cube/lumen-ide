//! What a remote session costs, for the Task Manager: its transfers, its traffic, the wizard's.

use super::*;

impl workbench_model::ResourceReporter for RemoteResources {
    fn report(&self) -> Vec<workbench_model::ResourceUse> {
        let live: Vec<Arc<State>> = SESSIONS
            .lock()
            .map(|s| s.iter().filter_map(|(.., s)| s.upgrade()).collect())
            .unwrap_or_default();
        live.iter()
            .map(|state| {
                let name = state.saved.connection.name.as_str();
                let mut part = workbench_model::ResourceUse::new(
                    i18n::tr!("tasks.remote", name = name),
                    crate::MODULE_ID,
                );
                let (transfers, logged) = state.log.lock().map_or((0, 0), |log| {
                    (log.len(), log.iter().map(|t| t.path.len() + 64).sum::<usize>())
                });
                let (folders, listed) = state.listings.lock().map_or((0, 0), |l| {
                    (l.len(), l.values().flatten().map(|e| e.path.len() + 48).sum::<usize>())
                });
                part.memory = Some((logged + listed) as u64);
                part.detail = i18n::tr!(
                    "tasks.remote_detail",
                    transfers = transfers,
                    folders = folders,
                    pending = state.now().to_upload
                );
                part.threads = vec!["remote-"];
                part.group = workbench_model::resources::SERVICES;
                state.traffic.report(&mut part);
                state.disk.report(&mut part);
                part.project = Some(state.project);
                state.activities.report(&mut part);
                part
            })
            .chain(opening())
            .collect()
    }

    /// A transfer stopped (its "Stop" in the Task Manager): it ends with what it did.
    fn act(
        &self,
        part: &workbench_model::PartRef,
        _action: &str,
        target: workbench_model::ActionTarget,
    ) -> workbench_model::ActOutcome {
        let live: Vec<Arc<State>> = SESSIONS
            .lock()
            .map(|s| s.iter().filter_map(|(.., s)| s.upgrade()).collect())
            .unwrap_or_default();
        live.iter()
            .filter(|state| part.project == Some(state.project))
            .map(|state| state.activities.act(target))
            .find(|o| *o == workbench_model::ActOutcome::Done)
            .unwrap_or(workbench_model::ActOutcome::Unsupported)
    }
}

/// The wizard's downloads going on, as a part of their own.
pub(super) fn opening() -> Option<workbench_model::ResourceUse> {
    let running = OPENING.0.load(std::sync::atomic::Ordering::Relaxed);
    (running > 0).then(|| {
        let mut part =
            workbench_model::ResourceUse::new(i18n::t("tasks.remote_opening"), crate::MODULE_ID);
        part.threads = vec!["remote-download", "remote-browse"];
        part.group = workbench_model::resources::SERVICES;
        OPENING.1.report(&mut part);
        part
    })
}
