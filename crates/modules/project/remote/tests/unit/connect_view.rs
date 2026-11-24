use super::suggestions::{folder_suggestions, server_suggestions, typed_server_path};
use super::*;

fn entry(path: &str, is_dir: bool) -> RemoteEntry {
    RemoteEntry { path: path.into(), is_dir, size: 0, modified: 0 }
}

/// A wizard with the connection typed (step 1).
fn typed() -> ConnectView {
    let mut view = ConnectView::new();
    view.type_text("example.com");
    view.focus(Field::User);
    view.type_text("ana");
    view.focus(Field::Secret);
    view.type_text("s3cret");
    view
}

#[test]
fn the_connection_step_follows_the_protocol_and_hides_the_secret() {
    let mut view = ConnectView::new();
    assert!(view.connection("/").is_err(), "incomplete");
    let mut view2 = typed();
    assert_eq!(view2.input(Field::Secret).selected_text(), "", "a secret isn't copied");
    let (saved, secret) = view2.connection("/").unwrap();
    assert_eq!(saved.connection.port, 22);
    assert_eq!(secret.as_ref().map(Secret::expose), Some("s3cret"));
    // Name and local folder aren't typed on this step.
    view2.type_text("");
    assert_eq!(view2.fields(), vec![Field::Host, Field::Port, Field::User, Field::Secret]);
    // FTPS: its port, a password only.
    view.auth = AuthKind::Agent;
    view.select_protocol(2);
    assert_eq!(view.value(Field::Port), "21");
    assert_eq!(view.auth, AuthKind::Password);
    // The eye shows / hides the password.
    assert!(view.input(Field::Secret).is_masked());
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Eye)];
    view.click(Point::new(5.0, 5.0));
    assert!(!view.input(Field::Secret).is_masked());
}

#[test]
fn browsing_the_server_goes_up_and_into_folders() {
    assert_eq!(join("/", "www"), "/www");
    assert_eq!(join("/www", "shop"), "/www/shop");
    assert_eq!(parent("/www/shop"), "/www");
    assert_eq!(parent("/www"), "/");
    assert_eq!(parent("/"), "/");
    let mut view = typed();
    view.step = Step::Folder;
    view.current = "/www".into();
    view.listing = vec![entry("shop", true), entry("readme.txt", false)];
    view.hits = vec![
        (Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Enter(0)),
        (Rect::new(0.0, 20.0, 10.0, 10.0), Hit::Enter(1)),
    ];
    view.click(Point::new(5.0, 25.0));
    assert_eq!(view.current, "/www", "a file isn't entered");
    view.click(Point::new(5.0, 5.0));
    assert_eq!(view.current, "/www/shop");
}

#[test]
fn a_typed_server_path_splits_into_its_folder_and_the_name_begun() {
    let split = |typed: &str| typed_server_path(typed, "/home/ana");
    assert_eq!(split("/public/web/den"), Some(("/public/web".into(), "den".into())));
    assert_eq!(split("/public/"), Some(("/public".into(), String::new())));
    assert_eq!(split("/pu"), Some(("/".into(), "pu".into())));
    assert_eq!(split("www/s"), Some(("/home/ana/www".into(), "s".into())), "from where we are");
    assert_eq!(split(""), None);
    let names: Vec<String> =
        ["Shop", "shared", ".cache", "site", "sh"].iter().map(|n| (*n).to_owned()).collect();
    assert_eq!(server_suggestions("/www", "sh", &names), ["/www/shared", "/www/Shop"]);
    assert_eq!(server_suggestions("/", "s", &names), ["/sh", "/shared", "/Shop", "/site"]);
    assert_eq!(server_suggestions("/www", ".", &names), ["/www/.cache"], "hidden when asked");
}

#[test]
fn the_server_path_is_completed_as_typed() {
    let mut view = typed();
    let (tx, requests) = std::sync::mpsc::channel();
    let (replies, rx) = std::sync::mpsc::channel();
    view.browser = Some((tx, rx));
    view.step = Step::Folder;
    view.current = "/www".into();
    // The folder shown completes at once (its listing kept): only folders.
    replies
        .send(Reply::Listed(
            "/www".into(),
            Ok(vec![entry("/www/shop", true), entry("/www/shared.txt", false)]),
        ))
        .unwrap();
    view.waiting = 1;
    view.poll();
    view.focus(Field::Path);
    view.on_list = false;
    view.input_mut(Field::Path).set_text("/www/");
    view.input_mut(Field::Path).end(false);
    view.type_text("sh");
    assert_eq!(view.suggestions, ["/www/shop"]);
    assert!(requests.try_recv().is_err(), "nothing asked for a folder already read");
    // Tab completes it; its folders are asked for in the background, never waited for.
    assert!(view.edit_key(ViewerKey::Tab));
    assert_eq!(view.value(Field::Path), "/www/shop/");
    assert!(matches!(requests.try_recv(), Ok(Request::Folders(dir)) if dir == "/www/shop"));
    assert!(view.suggestions.is_empty() && view.waiting == 0);
    replies.send(Reply::Folders("/www/shop".into(), vec!["public".into(), "logs".into()])).unwrap();
    view.poll();
    assert_eq!(view.suggestions, ["/www/shop/logs", "/www/shop/public"]);
    assert_eq!(view.waiting, 0, "no spinner for a completion");
    // ↓ chooses, ↵ completes; Esc closes the list, ↵ then goes there.
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.value(Field::Path), "/www/shop/public/");
    view.suggestions = vec!["/www/shop/public/x".into()];
    view.edit_key(ViewerKey::Escape);
    assert!(view.suggestions.is_empty() && view.take_action().is_none());
    view.edit_key(ViewerKey::Enter);
    assert_eq!(view.current, "/www/shop/public/");
    let asked: Vec<Request> = requests.try_iter().collect();
    assert!(matches!(&asked[..], [Request::Folders(f), Request::List(dir)]
        if f == "/www/shop/public" && dir == "/www/shop/public/"));
}

#[test]
fn who_files_written_belong_to_is_chosen_from_the_server_s_accounts() {
    use remote::properties::{Accounts, Principal};
    let mut view = typed();
    view.step = Step::Contents;
    view.root = "/www/shop".into();
    let www = Principal::named("www-data", Some(33));
    let deploy = Principal::named("deploy", Some(1001));
    view.server_accounts = Some(ServerAccounts {
        accounts: Accounts { users: vec![deploy, www.clone()], groups: vec![www.clone()] },
        project: None,
    });
    assert_eq!(view.connection("/www/shop").unwrap().0.connection.write_as, WriteAs::Server);
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::WriteAs(1))];
    view.click(Point::new(5.0, 5.0));
    assert_eq!(view.write_as, WriteAs::Project);
    view.write_as_kind(2);
    // The owner's list: "as the server makes it" first, then the accounts.
    view.open_accounts(Account::Owner);
    assert!(view.picker.is_open());
    view.picked(Some(ui_components::picker::PickerEvent::Chosen(2)));
    assert!(!view.picker.is_open());
    view.open_accounts(Account::Group);
    view.picked(Some(ui_components::picker::PickerEvent::Chosen(1)));
    let chosen = WriteAs::Chosen { owner: Some(www.clone()), group: Some(www) };
    assert_eq!(view.connection("/www/shop").unwrap().0.connection.write_as, chosen);
    view.open_accounts(Account::Group);
    view.picked(Some(ui_components::picker::PickerEvent::Chosen(0)));
    assert!(matches!(&view.write_as, WriteAs::Chosen { group: None, .. }), "unset again");
    // FTP can't change owners: the server's way.
    view.select_protocol(1);
    assert_eq!(view.connection("/www/shop").unwrap().0.connection.write_as, WriteAs::Server);
}

#[test]
fn the_project_folder_s_owner_and_group_are_shown_apart_from_the_pills() {
    use remote::properties::{Principal, Properties};
    let mut view = typed();
    assert!(view.project_account(Account::Owner).is_none(), "nothing read yet");
    let site = Principal::named("web004_northwind", Some(1002));
    let www = Principal::named("www-data", Some(33));
    view.server_accounts = Some(ServerAccounts {
        accounts: Default::default(),
        project: Some(Properties {
            path: String::new(),
            is_dir: true,
            size: 0,
            modified: 0,
            mode: None,
            owner: Some(site.clone()),
            group: Some(www.clone()),
        }),
    });
    assert_eq!(view.project_account(Account::Owner), Some(&site));
    assert_eq!(view.project_account(Account::Group), Some(&www));
}

#[test]
fn the_chosen_folder_names_the_project_and_its_tree_sets_the_exclusions() {
    let mut view = typed();
    view.step = Step::Folder;
    view.current = "/www/shop".into();
    view.choose();
    assert_eq!(view.step(), Step::Contents);
    assert_eq!(view.value(Field::Name), "shop", "named after the folder");
    assert_eq!(view.value(Field::Label), "shop", "the connection named as the project");
    // As a path: `\\` between them on Windows.
    assert!(std::path::Path::new(&view.value(Field::Local)).ends_with("remote-projects/shop"));
    // The tree: folders first, git's and the project data folder hidden.
    view.place_listing(
        "/www/shop",
        vec![
            entry("composer.json", false),
            entry("vendor", true),
            entry(".git", true),
            entry("app", true),
        ],
    );
    let names: Vec<_> = view.tree.iter().map(|n| n.path.as_str()).collect();
    assert_eq!(names, ["app", "vendor", "composer.json"]);
    // A folder opened: its rows under it.
    view.place_listing("/www/shop/vendor", vec![entry("acme", true), entry("autoload.php", false)]);
    let names: Vec<_> = view.tree.iter().map(|n| (n.path.as_str(), n.depth)).collect();
    assert_eq!(
        names,
        [
            ("app", 0),
            ("vendor", 0),
            ("vendor/acme", 1),
            ("vendor/autoload.php", 1),
            ("composer.json", 0)
        ]
    );
    // Something inside left out, then its folder: the folder takes it in.
    view.toggle(2);
    assert!(view.is_excluded("vendor/acme/lib.php"));
    view.toggle(1);
    assert_eq!(view.excluded.iter().collect::<Vec<_>>(), ["vendor"]);
    assert!(view.is_excluded("vendor/autoload.php"));
    view.toggle(3);
    assert_eq!(view.excluded.len(), 1, "decided by the folder above");
    assert_eq!(view.exclusions(), ["/www/shop/vendor"], "full paths on the server");
    // Closing a folder drops its rows.
    view.tree[1].expanded = true;
    view.expand(1);
    assert_eq!(view.tree.len(), 3);
    // The connection: that folder, those exclusions (and git's, always).
    let (saved, _) = view.saved().unwrap();
    assert_eq!(saved.connection.root, "/www/shop");
    // Its identity: a UUID v4 (never the name: two named alike don't share a secret).
    let id = saved.connection.id;
    assert_eq!(uuid::Uuid::parse_str(&id).map(|u| u.get_version_num()), Ok(4));
    // The connection named apart: its own name, the project's kept.
    view.on_list = false;
    view.focus(Field::Label);
    view.input_mut(Field::Label).end(false);
    view.type_text(" (prod)");
    let (saved, _) = view.saved().unwrap();
    assert_eq!(saved.connection.name, "shop (prod)");
    assert_eq!(saved.connection.id, id, "renaming keeps the identity");
    assert!(saved.local.ends_with("remote-projects/shop"));
    assert!(saved.connection.is_excluded("vendor/acme/lib.php"));
    assert!(saved.connection.is_excluded(".git/config"));
    assert!(!saved.connection.is_excluded("app/Models/User.php"));
    // Back keeps the choice's folder to browse from.
    view.back();
    assert_eq!(view.step(), Step::Folder);
}

#[test]
fn the_local_folder_is_completed_as_typed_or_picked() {
    let dir = std::env::temp_dir().join(format!("remote-suggest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for name in ["alpha", "Alps", "beta", ".hidden"] {
        std::fs::create_dir_all(dir.join(name)).unwrap();
    }
    std::fs::write(dir.join("almanac.txt"), "a file").unwrap();
    let sep = std::path::MAIN_SEPARATOR;
    let base = format!("{}{sep}", dir.display());
    let names = |typed: &str| -> Vec<String> {
        folder_suggestions(typed)
            .into_iter()
            .map(|p| p.rsplit(['/', sep]).next().unwrap().to_owned())
            .collect()
    };
    assert_eq!(names(&format!("{base}al")), ["alpha", "Alps"], "folders only, any case");
    assert_eq!(names(&base), ["alpha", "Alps", "beta"], "hidden ones only when asked");
    assert_eq!(names(&format!("{base}.h")), [".hidden"]);
    assert!(names(&format!("{base}beta")).is_empty(), "named exactly: nothing to add");

    // Typed in the field: ↓ chooses, Tab completes (and lists what's inside next).
    let mut view = typed();
    view.step = Step::Contents;
    view.on_list = false;
    view.focus(Field::Local);
    view.input_mut(Field::Local).set_text("");
    view.type_text(&format!("{base}al"));
    // Read on a worker (a key never reads the disk): shown at a later poll.
    assert!(view.suggestions.is_empty());
    suggested(&mut view);
    assert_eq!(view.suggestions.len(), 2);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Down);
    view.edit_key(ViewerKey::Tab);
    assert_eq!(view.value(Field::Local), format!("{base}Alps{sep}"));
    suggested(&mut view);
    assert!(view.suggestions.is_empty(), "nothing inside");
    // Esc closes the list, not the wizard.
    view.type_text("");
    view.input_mut(Field::Local).set_text(&base);
    view.suggest();
    suggested(&mut view);
    assert!(!view.suggestions.is_empty());
    view.edit_key(ViewerKey::Escape);
    assert!(view.suggestions.is_empty() && view.take_action().is_none());

    // The folder button asks for the system's picker, opened at the typed folder.
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Browse)];
    assert!(view.click(Point::new(5.0, 5.0)).is_none(), "looked for in the background");
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !view.poll() && std::time::Instant::now() < until {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::PickFolder(Some(start)))) => assert_eq!(start, dir),
        other => panic!("expected the folder picker, got {other:?}"),
    }
    // The chosen folder comes back.
    assert!(view.command("picked-folder:/somewhere/else"));
    assert_eq!(view.value(Field::Local), "/somewhere/else");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_project_is_worked_on_as_a_synced_copy_or_on_the_server() {
    let mut view = typed();
    view.step = Step::Folder;
    view.current = "/www/shop".into();
    view.choose();
    assert_eq!(view.saved().unwrap().0.connection.mode, Mode::Mirror, "a synced copy by default");
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Mode(Mode::OnDemand))];
    view.click(Point::new(5.0, 5.0));
    assert_eq!(view.saved().unwrap().0.connection.mode, Mode::OnDemand);
    // Its main button connects only (nothing downloaded now).
    let hints = view.key_hints();
    assert_eq!(hints[1].1, t("remote.connect_on_demand").to_lowercase());
    // Transfers at once: as in the settings by default, or its own.
    let saved = view.saved().unwrap().0.connection;
    assert_eq!((saved.uploads, saved.downloads), (None, None));
    view.hits = vec![
        (Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Uploads(Some(1))),
        (Rect::new(20.0, 0.0, 10.0, 10.0), Hit::Downloads(Some(8))),
    ];
    view.click(Point::new(5.0, 5.0));
    view.click(Point::new(25.0, 5.0));
    let saved = view.saved().unwrap().0.connection;
    assert_eq!((saved.uploads, saved.downloads), (Some(1), Some(8)));
    // Moves and deletions: off by default, on when ticked.
    assert!(!saved.moves);
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Moves)];
    view.click(Point::new(5.0, 5.0));
    assert!(view.saved().unwrap().0.connection.moves);
}

/// Against the demo servers: what the wizard downloads is all in the manifest, read back
/// whole when the project opens.
#[test]
#[ignore = "needs the demo servers"]
fn the_wizard_s_manifest_is_read_back_whole() {
    if std::env::var("EDITOR_REMOTE_DEMO").is_err() {
        return;
    }
    let local = std::env::temp_dir().join(format!("remote-wizard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&local);
    let mut view = typed();
    view.input_mut(Field::Host).set_text("127.0.0.1");
    view.input_mut(Field::Port).set_text("2222");
    view.input_mut(Field::User).set_text("demo");
    view.input_mut(Field::Secret).set_text("demo123");
    view.root = "/www/shop".into();
    view.input_mut(Field::Name).set_text("shop");
    view.input_mut(Field::Local).set_text(&local.display().to_string());
    view.saved_in = SavedIn::Project;
    let (saved, _) = view.saved().unwrap();
    let probe = session::connector(&saved, Some("demo123".into()), None);
    let accepted = match probe.connect() {
        Ok(_) => None,
        Err(RemoteError::HostKey(f)) => Some(f),
        Err(e) => panic!("{e}"),
    };
    let connector = session::connector(&saved, Some("demo123".into()), accepted);
    let progress = (AtomicUsize::new(0), AtomicUsize::new(0));
    let (_, done, failed) =
        download(&saved, None, connector.as_ref(), &progress, &AtomicBool::new(false), &|| {})
            .unwrap();
    assert_eq!(failed, 0);
    let manifest = session::load_manifest(&local);
    let file = local.join(kernel::brand::PROJECT_DIR).join("remote-manifest");
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(manifest.len(), done, "every downloaded file read back:\n{text}");
    // The project opens: its session, then a save goes up — the manifest keeps everything.
    use workbench_model::ProjectService;
    let events = kernel::EventBus::new();
    let storage = local.join(kernel::brand::PROJECT_DIR);
    let id = kernel::ProjectId::new();
    let context = workbench_model::ProjectContext {
        marks: Default::default(),
        id,
        root: &local,
        storage: &storage,
        events: &events,
        wake: std::sync::Arc::new(|| {}),
        settings: None,
        language_of: std::sync::Arc::new(|_: &std::path::Path| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: std::sync::Arc::new(workbench_model::NoNotifier),
    };
    let held = session::RemoteService.attach(&context);
    let state = session::session(id).expect("a remote project");
    assert_eq!(state.manifest.lock().unwrap().len(), done, "loaded whole");
    std::fs::write(local.join("composer.json"), "{\"saved\": \"here\"}").unwrap();
    events.publish(&document::events::FileSaved { path: local.join("composer.json") });
    // The upload runs on the session's thread.
    let start = std::time::Instant::now();
    while state.log.lock().unwrap().is_empty()
        && start.elapsed() < std::time::Duration::from_secs(10)
    {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let after = session::load_manifest(&local);
    let text = std::fs::read_to_string(&file).unwrap();
    assert_eq!(after.len(), done, "after a save, still every file:\n{text}");
    drop(held);
    // The server as it was.
    let mut fs = connector.connect().unwrap();
    fs.write(
        "composer.json",
        br#"{"name": "demo/shop", "autoload": {"psr-4": {"App\\": "app/"}}}"#,
    )
    .unwrap();
    let _ = std::fs::remove_dir_all(&local);
}

// ─── Editing a saved connection ───

/// Secrets in memory (the tests never touch the keychain), by connection id.
#[derive(Default)]
struct Memory(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for Memory {
    fn get(&self, id: &str) -> Option<Secret> {
        self.0.lock().unwrap().get(id).map(|s| Secret::new(s.clone()))
    }
    fn set(&self, id: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(id.into(), secret.into());
        Ok(())
    }
    fn remove(&self, id: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

static MEMORY: std::sync::LazyLock<Arc<Memory>> = std::sync::LazyLock::new(Arc::default);

fn memory(_: &Saved) -> Arc<dyn SecretStore> {
    Arc::clone(&*MEMORY) as Arc<dyn SecretStore>
}

/// A server accepting only `right-secret`; it remembers the secrets it was given.
struct Fake {
    secret: Option<Secret>,
}

static TRIED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct FakeFs;

impl remote::application::RemoteFs for FakeFs {
    fn list(&mut self, _: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        Ok(vec![entry("index.php", false)])
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        Err(RemoteError::NotFound(path.into()))
    }
    fn write(&mut self, path: &str, _: &[u8]) -> Result<RemoteEntry, RemoteError> {
        Err(RemoteError::NotFound(path.into()))
    }
    fn remove(&mut self, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn rename(&mut self, _: &str, _: &str) -> Result<(), RemoteError> {
        Ok(())
    }
    fn stat(&mut self, _: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        Ok(None)
    }
    fn home(&mut self) -> Result<String, RemoteError> {
        Ok("/home/ana".into())
    }
}

impl Connector for Fake {
    fn connect(&self) -> Result<Box<dyn remote::application::RemoteFs>, RemoteError> {
        let given = self.secret.as_ref().map(|s| s.expose().to_owned()).unwrap_or_default();
        TRIED.lock().unwrap().push(given.clone());
        if given == "right-secret" {
            Ok(Box::new(FakeFs))
        } else {
            Err(RemoteError::Auth("wrong password".into()))
        }
    }
}

fn fake(_: &Saved, secret: Option<Secret>, _: Option<String>) -> Arc<dyn Connector> {
    Arc::new(Fake { secret })
}

/// A project's saved connection (kept in it, its secret in the memory store).
fn kept(name: &str) -> (PathBuf, Saved) {
    let root = std::env::temp_dir().join(format!("remote-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let saved = Saved {
        connection: Connection {
            id: format!("edit-{name}"),
            name: "Shop".into(),
            protocol: Protocol::Ftp,
            host: "ftp.example.com".into(),
            port: 21,
            user: "ana".into(),
            auth: Auth::Password,
            root: "/www/shop".into(),
            mode: Mode::Mirror,
            excluded: vec![".git".into(), kernel::brand::PROJECT_DIR.into(), "vendor".into()],
            saved_in: SavedIn::Project,
            secrets_in: SecretsIn::Keychain,
            retry: None,
            uploads: Some(2),
            downloads: None,
            moves: true,
            auto_upload: true,
            write_as: Default::default(),
        },
        local: root.clone(),
        trusted: vec!["SHA256:kept".into()],
    };
    config::save(&saved).unwrap();
    MEMORY.set(&saved.connection.id, "right-secret").unwrap();
    (root, saved)
}

fn editing(saved: &Saved) -> ConnectView {
    let mut view = ConnectView::new();
    view.connectors = fake;
    view.stores = memory;
    view.set_data(&EditConnection { saved: saved.clone() });
    view
}

/// Polls `view` until `done` (or a few seconds).
fn until(view: &mut ConnectView, done: impl Fn(&ConnectView) -> bool) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(5) {
        view.poll();
        if done(view) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    false
}

#[test]
fn editing_prefills_the_form_and_never_shows_the_password() {
    let (root, saved) = kept("prefill");
    let mut view = editing(&saved);
    assert!(view.editing());
    assert_eq!(view.step(), Step::Connection);
    assert_eq!(view.protocol, Protocol::Ftp);
    assert_eq!(view.value(Field::Host), "ftp.example.com");
    assert_eq!(view.value(Field::Port), "21");
    assert_eq!(view.value(Field::User), "ana");
    assert_eq!(view.value(Field::Label), "Shop");
    // The kept password read in the background, in its field, masked.
    assert!(until(&mut view, |v| v.reading_secret.is_none()), "read");
    assert_eq!(view.input(Field::Secret).text(), "right-secret", "the kept password filled in");
    assert!(view.input(Field::Secret).is_masked());
    assert!(!format!("{view:?}").contains("right-secret"));
    assert_eq!((view.uploads, view.moves), (Some(2), true));
    assert_eq!(view.excluded.iter().collect::<Vec<_>>(), ["vendor"], "git's and ours implied");
    // As it was, unchanged: the same connection (its identity, its certificates, its folder).
    let (again, secret) = view.saved().unwrap();
    assert_eq!(again, saved);
    assert!(secret.is_none(), "the kept one, unchanged: not written again");
    // The project's folder stays: its name and local copy aren't typed in.
    let mut options = editing(&saved);
    options.step = Step::Contents;
    assert_eq!(options.fields(), vec![Field::Label]);
    // The projects list offers it (its row's menu, the current project's list), opening
    // this form.
    use workbench_model::ProjectService;
    let kind = crate::RemoteService.describe(&root).unwrap();
    assert_eq!(kind.edit.map(|e| e.menu_label), Some(t("remote.edit_row")));
    let mut host = crate::tests::Host::at(&root, kernel::ProjectId::new());
    assert!(crate::RemoteService.edit(&mut host, &root));
    assert_eq!(host.opened, [t("remote.edit_title")]);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn saving_with_the_password_left_empty_keeps_the_secret() {
    let (root, saved) = kept("keep");
    let mut view = editing(&saved);
    view.input_mut(Field::Port).set_text("2121");
    view.input_mut(Field::Host).set_text("10.0.0.7");
    view.edit_key(ViewerKey::Enter);
    assert!(until(&mut view, |v| v.saving.is_none()), "saved");
    assert!(matches!(view.take_action(), Some(ViewerAction::Close)), "{:?}", view.status);
    let found = config::find(&root).unwrap();
    assert_eq!((found.connection.host.as_str(), found.connection.port), ("10.0.0.7", 2121));
    assert_eq!(found.connection.id, saved.connection.id, "the same connection");
    assert_eq!(found.trusted, saved.trusted);
    assert_eq!(MEMORY.get(&saved.connection.id).unwrap().expose(), "right-secret");
    let text = std::fs::read_to_string(config::data_dir(&root).join("remote.toml")).unwrap();
    assert!(!text.contains("right-secret"), "never in the file");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_password_typed_before_the_kept_one_is_read_stays() {
    let (root, saved) = kept("typed-first");
    let mut view = editing(&saved);
    // Typed before any poll: the kept one, arriving later, doesn't replace it.
    view.focus(Field::Secret);
    view.type_text("typed");
    assert!(until(&mut view, |v| v.reading_secret.is_none()));
    assert_eq!(view.input(Field::Secret).text(), "typed");
    let (_, secret) = view.saved().unwrap();
    assert_eq!(secret.map(|s| s.expose().to_owned()).as_deref(), Some("typed"), "a new one");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_new_password_replaces_the_secret() {
    let (root, saved) = kept("replace");
    let mut view = editing(&saved);
    view.focus(Field::Secret);
    view.type_text("new-secret");
    view.hits = vec![(Rect::new(0.0, 0.0, 10.0, 10.0), Hit::Save)];
    view.click(Point::new(5.0, 5.0));
    assert!(until(&mut view, |v| v.saving.is_none()));
    assert_eq!(MEMORY.get(&saved.connection.id).unwrap().expose(), "new-secret");
    // Signing in with the agent (SFTP) drops it.
    let saved = config::find(&root).unwrap();
    let mut view = editing(&saved);
    view.select_protocol(0);
    view.auth = AuthKind::Agent;
    view.save();
    assert!(until(&mut view, |v| v.saving.is_none()));
    assert!(MEMORY.get(&saved.connection.id).is_none(), "not needed: removed");
    assert_eq!(config::find(&root).unwrap().connection.auth, Auth::Agent);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn test_connection_signs_in_with_the_kept_or_the_typed_password() {
    let (root, saved) = kept("test");
    let mut view = editing(&saved);
    view.test();
    assert!(until(&mut view, |v| !v.busy()));
    assert_eq!(view.status, Some((t("remote.test_ok"), Tone::Success)), "the kept secret used");
    assert_eq!(view.step(), Step::Connection, "a test stays on the form");
    // A wrong one typed over it: the server's refusal, inline.
    view.input_mut(Field::Secret).set_text("");
    view.focus(Field::Secret);
    view.type_text("wrong");
    view.test();
    assert!(until(&mut view, |v| !v.busy()));
    let (message, tone) = view.status.clone().unwrap();
    assert_eq!(tone, Tone::Danger);
    assert!(message.contains("wrong password"), "{message}");
    assert!(TRIED.lock().unwrap().iter().any(|s| s == "wrong"));
    // Continue browses from the saved folder, not the user's home.
    view.input_mut(Field::Secret).set_text("");
    view.browse();
    assert!(until(&mut view, |v| v.step() == Step::Folder));
    assert_eq!(view.current, "/www/shop");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn the_form_is_checked_like_the_wizard_s() {
    let (root, saved) = kept("check");
    let mut view = editing(&saved);
    view.input_mut(Field::Port).set_text("70000");
    view.save();
    assert_eq!(view.invalid, Some(Field::Port));
    assert_eq!(view.status, Some((t("remote.invalid_port"), Tone::Danger)));
    assert!(view.saving.is_none(), "nothing saved");
    // Typed in again: no longer marked.
    view.focus(Field::Port);
    view.edit_key(ViewerKey::Backspace);
    assert_eq!(view.invalid, None);
    view.input_mut(Field::Host).set_text("ftp example.com");
    view.test();
    assert_eq!(view.invalid, Some(Field::Host));
    view.input_mut(Field::Host).set_text("");
    assert!(view.connection("/").is_err());
    assert_eq!(config::find(&root).unwrap(), saved, "unchanged");
    let _ = std::fs::remove_dir_all(root);
}

/// The step's scrollbar: its thumb pressed and dragged down scrolls the step.
#[test]
fn the_step_scrolls_by_its_thumb() {
    let mut view = ConnectView::new();
    let area = Rect::new(0.0, 0.0, 400.0, 200.0);
    let palette = ui_components::Palette::dark();
    view.max_scroll = 800.0;
    view.bar.paint(&mut render_api::Scene::default(), &palette, 1.0, area, 1000.0, 0.0);
    let edge = area.max_x() - 4.0;
    assert!(view.click(Point::new(edge, 6.0)).is_none());
    assert!(view.drag(Point::new(edge, 80.0)), "the thumb followed");
    assert!(view.scroll > 0.0, "the step scrolled");
    assert!(view.release());
}

/// The fields are the wizard's controls: one always has the keys (a press beside them
/// leaves it there), Tab walks the step's fields by the wizard's own order, and the editing
/// keys go to the focused one.
#[test]
fn a_field_keeps_the_keys_and_the_wizard_walks_them() {
    let mut view = ConnectView::new();
    view.type_text("example.co");
    assert!(view.click(Point::new(-50.0, -50.0)).is_none());
    view.type_text("mx");
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.input(Field::Host).text(), "example.com", "still the server's field");
    assert!(view.edit_key(ViewerKey::Tab));
    assert_eq!(view.focused(), Field::Port);
    assert!(view.edit_key(ViewerKey::Up));
    assert_eq!(view.focused(), Field::Host);
    assert!(view.command("select_all"));
    assert_eq!(view.input(Field::Host).selected_text(), "example.com");
}

/// The local folders asked for put in the list (read on a worker).
fn suggested(view: &mut ConnectView) {
    let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while view.local_folders.is_some() {
        assert!(std::time::Instant::now() < until, "the folders are read");
        std::thread::sleep(std::time::Duration::from_millis(5));
        view.poll();
    }
}
