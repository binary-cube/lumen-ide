use super::*;
use std::path::{Path, PathBuf};

use self_update::check::Available;
use self_update::stage::Staged;

/// A host keeping what it's told and what it's asked.
#[derive(Default)]
pub(crate) struct Host {
    pub told: Vec<Notification>,
    pub dismissed: Vec<workbench_model::NotificationId>,
    pub restarted: Vec<PathBuf>,
    pub can_restart: bool,
    events: kernel::EventBus,
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<PathBuf> {
        None
    }
    /// The window the user is in (what's found is told there).
    fn window_focused(&self) -> bool {
        true
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        None
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, notification: Notification) {
        self.told.push(notification);
    }
    fn dismiss_notification(&mut self, id: workbench_model::NotificationId) {
        self.dismissed.push(id);
    }
}

impl workbench_model::ViewHost for Host {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}
impl workbench_model::EditorHost for Host {}
impl workbench_model::RefactoringHost for Host {}
impl workbench_model::ServicesHost for Host {}
impl workbench_model::SystemHost for Host {
    fn restart_to_update(&mut self, staged: &Path) -> bool {
        self.restarted.push(staged.to_path_buf());
        self.can_restart
    }
}

/// A version found: `2.0.0`, with its notes.
pub(crate) fn found() -> Arc<Available> {
    crate::snapshot::made_up("2.0.0", "<!-- app -->\n## New\n- tabs\n<!-- /app -->\n## Download")
        .unwrap()
}

/// A version found is told once, its notification offering What's New and Update; later
/// news (ready) replace it, offering Restart Now.
#[test]
fn a_version_found_is_told_once_with_what_s_new_and_update() {
    let _serial = state::serial();
    let mut host = Host::default();
    state::set(Phase::Available(found()), Some(Tell::Available));
    assert!(Announcer.poll(&mut host));
    assert!(!Announcer.poll(&mut host), "told once");
    let first = host.told[0].id;
    let actions: Vec<CommandId> = host.told[0].actions.iter().map(|a| a.command).collect();
    assert_eq!(actions, [WHATS_NEW, INSTALL]);
    let staged = Staged {
        dir: PathBuf::from("/updates/2.0.0"),
        version: "2.0.0".into(),
        app: PathBuf::from("/updates/2.0.0/app/app.exe"),
    };
    state::set(Phase::Ready { available: found(), staged }, Some(Tell::Ready));
    Announcer.poll(&mut host);
    let ready = &host.told[1];
    assert_eq!(ready.id, first, "the same notification, updated");
    assert_eq!(ready.actions[0].command, INSTALL);
}

/// Ready: Update and Restart asks the app to restart into it (the app asks what it asks
/// before quitting); a host that can't is said so. Nothing ready: nothing restarts.
#[test]
fn ready_restarts_into_the_update_through_the_app() {
    let _serial = state::serial();
    let mut host = Host { can_restart: true, ..Host::default() };
    install(&mut host);
    assert!(host.restarted.is_empty(), "nothing found: nothing done");
    let dir = PathBuf::from("/updates/2.0.0");
    let staged = Staged { dir: dir.clone(), version: "2.0.0".into(), app: dir.join("app") };
    state::set(Phase::Ready { available: found(), staged }, None);
    install(&mut host);
    assert_eq!(host.restarted, [dir]);
    let mut cant = Host::default();
    install(&mut cant);
    assert_eq!(cant.told.len(), 1, "said it can't");
    // Not put in place: said, no restart (the app as it was).
    let dir = PathBuf::from("/updates/broken");
    let staged = Staged { dir: dir.clone(), version: "2.0.0".into(), app: dir.join("app") };
    state::set(Phase::Ready { available: found(), staged }, None);
    let mut host = Host { can_restart: true, ..Host::default() };
    install(&mut host);
    assert!(host.restarted.is_empty(), "not in place: no restart");
    assert!(host.told[0].title.contains("disk full"), "{}", host.told[0].title);
}

/// Skip This Version: nothing offered any more, the notification closed, said.
#[test]
fn a_skipped_version_is_offered_no_more() {
    let _serial = state::serial();
    let mut host = Host::default();
    state::set(Phase::Available(found()), Some(Tell::Available));
    Announcer.poll(&mut host);
    skip(&mut host);
    assert!(state::phase().available().is_none());
    assert_eq!(host.dismissed, [host.told[0].id]);
    assert!(host.told.last().unwrap().title.contains("2.0.0"));
}

/// The app started again after an update: how it went, said once.
#[test]
fn how_an_update_went_is_said() {
    let _serial = state::serial();
    let mut host = Host::default();
    let outcome =
        self_update::install::Outcome { version: "2.0.0".into(), error: Some("disk full".into()) };
    state::tell(Tell::Updated(outcome));
    Announcer.poll(&mut host);
    assert!(host.told[0].title.contains("disk full"), "{}", host.told[0].title);
}
