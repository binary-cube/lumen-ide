use super::*;

#[test]
fn contributes_its_views_its_service_its_commands_and_its_button() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RemoteModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    assert_eq!(contributions.take::<Arc<dyn ViewerProvider>>().len(), 6);
    assert_eq!(contributions.take::<Arc<dyn ProjectService>>().len(), 1);
    let ids: Vec<&str> =
        contributions.take::<CommandContribution>().iter().map(|c| c.spec.id.0).collect();
    assert_eq!(
        ids,
        [
            OPEN.0,
            SHOW_LOG.0,
            SHOW_PENDING.0,
            PULL.0,
            PUSH.0,
            SYNC.0,
            PULL_PROJECT.0,
            PUSH_PROJECT.0,
            SYNC_PROJECT.0,
            CONNECTION.0,
            UPLOAD_HERE.0,
            COMPARE_HERE.0,
            DISCARD_HERE.0,
            PROPERTIES.0,
            BRING.0,
            RELOAD.0
        ]
    );
    let menu = &contributions.take::<MenuContribution>()[0];
    assert_eq!(menu.id, TOOL);
    // The button opens what's to upload (what its count counts), the log next; its menu
    // works on the project, the context menus on what was right-clicked.
    let rows = |place| menu.rows_in(place, &|_| true);
    assert_eq!(rows(MenuPlace::ToolStrip)[0], MenuRow::Command(SHOW_PENDING, STRIP));
    assert_eq!(rows(MenuPlace::ToolStrip)[1], MenuRow::Command(SHOW_LOG, STRIP));
    assert!(rows(MenuPlace::ToolStrip).contains(&MenuRow::Command(PULL_PROJECT, STRIP)));
    assert!(rows(MenuPlace::TreeFolder).contains(&MenuRow::Command(SYNC, HERE)));
    assert!(!rows(MenuPlace::TreeFile).contains(&MenuRow::Command(PULL_PROJECT, STRIP)));
    assert!(rows(MenuPlace::TreeFolder).contains(&MenuRow::Command(BRING, HERE)));
    // What's to upload of the right-clicked files / folders.
    for id in [UPLOAD_HERE, COMPARE_HERE, DISCARD_HERE] {
        assert!(rows(MenuPlace::TreeFolder).contains(&MenuRow::Command(id, HERE)));
        assert!(rows(MenuPlace::Editor).contains(&MenuRow::Command(id, HERE)));
    }
    // The bar over a waiting file's editor.
    assert_eq!(
        contributions.take::<Arc<dyn ui_components::editor_toolbar::EditorToolbarProvider>>().len(),
        1
    );
    // Nothing while the project isn't a remote one (no rows enabled: no button, no menu).
    assert!(menu.rows_in(MenuPlace::Editor, &|_| false).is_empty());
}

/// Against the demo servers (see the module's notes): `EDITOR_REMOTE_DEMO=1 cargo test -p
/// remote-projects -- --ignored`. Downloads the demo project over SFTP and FTPS, uploads a
/// save, and refuses to overwrite a file changed on the server meanwhile.
#[test]
#[ignore = "needs the demo servers"]
fn a_remote_project_round_trip_on_real_servers() {
    use remote::application::{Direction, Run, apply, list_remote, plan};
    use remote::{Auth, Connection, Mode, Protocol, SavedIn, SecretsIn};
    if std::env::var("EDITOR_REMOTE_DEMO").is_err() {
        return;
    }
    for (protocol, port) in [(Protocol::Sftp, 2222), (Protocol::Ftps { implicit: false }, 2121)] {
        let local = std::env::temp_dir().join(format!(
            "remote-demo-{}-{}",
            protocol.id(),
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&local);
        let saved = config::Saved {
            connection: Connection {
                id: "demo".into(),
                name: "Demo".into(),
                protocol,
                host: "127.0.0.1".into(),
                port,
                user: "demo".into(),
                auth: Auth::Password,
                root: if protocol == Protocol::Sftp {
                    "/www/shop".into()
                } else {
                    "/home/demo/www/shop".into()
                },
                mode: Mode::Mirror,
                excluded: vec!["vendor".into()],
                saved_in: SavedIn::Project,
                secrets_in: SecretsIn::Keychain,
                retry: None,
                uploads: None,
                downloads: None,
                moves: false,
                auto_upload: true,
                write_as: Default::default(),
            },
            local: local.clone(),
            trusted: Vec::new(),
        };
        // First time: the server's key / certificate is unknown; trusted, it connects.
        let probe = session::connector(&saved, Some("demo123".into()), None);
        let accepted = match probe.connect() {
            Ok(_) => None,
            Err(remote::RemoteError::HostKey(f)) => Some(f),
            Err(e) => panic!("{protocol:?}: {e}"),
        };
        let connector = session::connector(&saved, Some("demo123".into()), accepted);
        let mut fs = connector.connect().unwrap();
        let listed = list_remote(&saved.connection, fs.as_mut(), &|| false).unwrap();
        assert!(listed.iter().any(|e| e.path == "app/Models/User.php"), "{protocol:?}: {listed:?}");
        assert!(listed.iter().all(|e| !e.path.starts_with("vendor")), "excluded");
        let copy = session::local_copy(&local);
        let mut manifest = remote::manifest::Manifest::default();
        let changes = plan(&saved.connection, &manifest, &[], &listed, Direction::Pull, &|_| None);
        let run = Run {
            workers: 4,
            retry: Default::default(),
            now: &session::now,
            on_transfer: &|_| {},
            on_bytes: &|_, _| {},
            cancelled: &|| false,
            parallel: &crate::session::threads,
            pause: &std::thread::sleep,
        };
        let done = apply(&changes, connector.as_ref(), &copy, &mut manifest, &run);
        assert!(done.iter().all(|t| t.error.is_none()), "{done:?}");
        assert!(local.join("app/Http/HomeController.php").exists());
        let _ = std::fs::remove_dir_all(&local);
    }
}

/// A host with a project open (its root, its identity) that records what's asked of it.
#[derive(Default)]
pub(crate) struct Host {
    root: Option<PathBuf>,
    id: Option<kernel::ProjectId>,
    events: kernel::EventBus,
    pub asked: Option<(kernel::ProjectId, String)>,
    pub then: Option<workbench_model::OnYes>,
    pub relisted: Vec<PathBuf>,
    pub opened: Vec<String>,
    /// The Remote window's views (their keys, in order) and the one shown.
    pub panel: Vec<String>,
    pub shown: Option<String>,
}

impl Host {
    pub(crate) fn at(root: &std::path::Path, id: kernel::ProjectId) -> Self {
        Self { root: Some(root.to_path_buf()), id: Some(id), ..Self::default() }
    }
}

use std::path::PathBuf;

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        self.id
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
    fn relist(&mut self, dir: &std::path::Path) {
        self.relisted.push(dir.to_path_buf());
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn ask_running(
        &mut self,
        project: kernel::ProjectId,
        owner: &str,
        then: workbench_model::OnYes,
    ) -> bool {
        self.asked = Some((project, owner.to_owned()));
        self.then = Some(then);
        true
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for Host {
    fn open_view(&mut self, title: &str, _: &str, _: &dyn std::any::Any) -> bool {
        self.opened.push(title.to_owned());
        true
    }
    fn show_in_panel_keyed(
        &mut self,
        _: &str,
        key: &str,
        _: &str,
        _: &str,
        _: &dyn std::any::Any,
    ) -> bool {
        if !self.panel.iter().any(|k| k == key) {
            self.panel.push(key.to_owned());
        }
        self.shown = Some(key.to_owned());
        true
    }
    fn in_panel(&self, _: &str, key: &str) -> bool {
        self.panel.iter().any(|k| k == key)
    }
}

/// The Remote window opens with all its tabs, "To upload", the transfers (what moved while
/// it was closed is there), then the files changed in a span, whichever is asked for; asked
/// again, none is doubled.
#[test]
fn the_remote_window_has_all_its_tabs_whichever_is_shown() {
    let root = std::env::temp_dir().join(format!("remote-tabs-{}", std::process::id()));
    let mut host = Host::at(&root, kernel::ProjectId::new());
    assert!(ShowPending.run(&mut host));
    assert_eq!(host.panel, [PENDING_KEY, LOG_KEY, CHANGED_KEY]);
    assert_eq!(host.shown.as_deref(), Some(PENDING_KEY));
    let mut host = Host::at(&root, kernel::ProjectId::new());
    assert!(ShowLog.run(&mut host));
    assert_eq!(host.panel, [PENDING_KEY, LOG_KEY, CHANGED_KEY], "the same order");
    assert_eq!(host.shown.as_deref(), Some(LOG_KEY));
    assert!(ShowPending.run(&mut host) && ShowLog.run(&mut host));
    assert_eq!(host.panel.len(), 3, "none doubled");
}
impl workbench_model::EditorHost for Host {}
impl workbench_model::RefactoringHost for Host {}
impl workbench_model::ServicesHost for Host {}
impl workbench_model::SystemHost for Host {}

/// "Connection Settings…": greyed (with why) without a project, greyed for a local one; in
/// Tools › Remote ›, the tool window's menu and the right-click menus' Remote ›; the projects list offers it for a remote
/// project's row and the current one.
#[test]
fn the_connection_settings_are_offered_only_for_a_remote_project() {
    use command_model::Requires;
    assert_eq!(Connection.requires(), &[] as &[Requires], "the bare handler");
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(RemoteModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let commands = contributions.take::<CommandContribution>();
    let connection = commands.iter().find(|c| c.spec.id == CONNECTION).expect("contributed");
    assert_eq!(connection.handler.requires(), [Requires::Project], "no project: greyed, with why");
    assert_eq!(connection.spec.menu.map(|m| m.section), Some(MENU_CONNECTION));
    let local = std::env::temp_dir().join(format!("remote-local-{}", std::process::id()));
    std::fs::create_dir_all(&local).unwrap();
    let mut host = Host::at(&local, kernel::ProjectId::new());
    assert!(!connection.handler.enabled(&host), "a local project: greyed");
    assert!(!connection.handler.run(&mut host) && host.opened.is_empty());
    assert!(RemoteService.describe(&local).is_none(), "nothing offered in the projects list");
    let menu = &contributions.take::<MenuContribution>()[0];
    for place in
        [MenuPlace::ToolStrip, MenuPlace::Editor, MenuPlace::TreeFile, MenuPlace::TreeFolder]
    {
        let rows = menu.rows_in(place, &|_| true);
        assert!(rows.contains(&MenuRow::Command(CONNECTION, EVERYWHERE)), "{place:?}");
    }
    let submenus = contributions.take::<SubmenuSpec>();
    assert!(submenus.iter().any(|s| s.id == REMOTE_MENU && s.at.section == TOOLS_SECTION));
    let _ = std::fs::remove_dir_all(local);
}

/// The Remote window's two tabs are fixed (as Git's): no × on them, never closed.
#[test]
fn the_remote_window_s_tabs_cannot_be_closed() {
    use ui_components::Viewer;
    assert!(!pending_view::PendingList::default().closable());
    assert!(!TransferLog::default().closable());
}

/// On demand, a Pull asked for a file brings it (never brought: it was skipped); a folder's
/// or the project's keeps to what was brought.
#[test]
fn on_demand_a_file_s_pull_brings_it() {
    use workbench_model::{ActionContext, MenuPlace, Target};
    let on = |path: &str, is_dir: bool| {
        ActionContext::new(MenuPlace::TreeFile, vec![Target::new(path.into(), is_dir)])
    };
    let pull = Transfer { direction: Direction::Pull, project: false, bring: false };
    assert!(!pull.fetched_only(true, &on("/p/app/index.php", false)));
    assert!(pull.fetched_only(true, &on("/p/app", true)));
    assert!(!pull.fetched_only(false, &on("/p/app", true)), "mirrored: everything");
    let project = Transfer { direction: Direction::Pull, project: true, bring: false };
    assert!(project.fetched_only(true, &on("/p/app/index.php", false)));
    let bring = Transfer { direction: Direction::Pull, project: false, bring: true };
    assert!(!bring.fetched_only(true, &on("/p/app", true)));
}
