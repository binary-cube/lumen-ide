use super::*;

/// The offer says what it is, its size, and asks: Download and Not Now; it stays until
/// answered.
#[test]
fn the_offer_asks_with_its_size() {
    let offer = offer();
    assert_eq!(offer.actions.iter().map(|a| a.command).collect::<Vec<_>>(), [DOWNLOAD, NOT_NOW]);
    assert!(offer.detail.as_deref().is_some_and(|d| d.contains(&DOWNLOAD_MB.to_string())));
    assert_eq!(offer.timeout, workbench_model::Timeout::Never);
}

/// A look at the install: `status`, the engine wanted by a page, the offer not refused.
fn look(status: Status) -> Look {
    Look { status, wanted: true, declined: false, updated: None }
}

fn downloading(received: u64, total: u64) -> Status {
    Status::Downloading { received, total }
}

fn installed() -> Status {
    Status::Installed {
        version: "1.2.3".into(),
        dir: std::sync::Arc::from(std::path::Path::new("/e")),
    }
}

/// The notification shown, else a failure naming what was told instead.
fn shown(telling: Telling) -> Notification {
    match telling {
        Telling::Show(n) => n,
        other => panic!("nothing shown: {other:?}"),
    }
}

/// The offer is made once a run, only when a page wanted the engine and it wasn't refused.
#[test]
fn the_offer_is_made_once_and_only_when_wanted_and_not_declined() {
    let mut told = Told::default();
    let declined = Look { declined: true, ..look(Status::Missing) };
    assert_eq!(told.tell(declined), Telling::Nothing, "refused: never offered");
    let unwanted = Look { wanted: false, ..look(Status::Missing) };
    assert_eq!(told.tell(unwanted), Telling::Nothing, "no page wanted it");
    assert_eq!(told.tell(look(Status::Looking)), Telling::Nothing, "wanted while looking: later");
    let offered = shown(told.tell(look(Status::Missing)));
    assert_eq!(offered.actions.iter().map(|a| a.command).collect::<Vec<_>>(), [DOWNLOAD, NOT_NOW]);
    assert_eq!(told.tell(look(Status::Missing)), Telling::Nothing, "once a run");
}

/// Progress is told only for a download the user asked for (an update's in the background
/// isn't), in the offer's notification; a size unknown sweeps and says only what came.
#[test]
fn progress_only_when_the_user_asked() {
    let mut told = Told::default();
    assert_eq!(told.tell(look(downloading(10, 100))), Telling::Nothing, "an update's");
    assert_eq!(told.tell(look(Status::Unpacking)), Telling::Nothing);
    assert_eq!(told.tell(look(installed())), Telling::Nothing, "an update's end");
    let offer = shown(told.tell(look(Status::Missing)));
    told.asked = true;
    let progress = shown(told.tell(look(downloading(25, 100))));
    assert_eq!(progress.id, offer.id, "the offer's notification, updated");
    assert_eq!(progress.progress_fraction(), Some(0.25));
    assert_eq!(progress.actions.iter().map(|a| a.command).collect::<Vec<_>>(), [STOP]);
    let unknown = shown(told.tell(look(downloading(2048, 0))));
    assert_eq!(unknown.progress_fraction(), None, "a size not known: no bar stuck at 0");
    assert_eq!(unknown.detail, Some(downloaded(2048, 0)));
    assert_ne!(downloaded(2048, 0), downloaded(2048, 4096), "not \"of 0 B\"");
}

/// Installed after the user's download: told once, and a later download gets a notification
/// of its own.
#[test]
fn success_ends_what_the_user_asked() {
    let mut told = Told { asked: true, ..Told::default() };
    let progress = shown(told.tell(look(downloading(1, 2))));
    let done = shown(told.tell(look(installed())));
    assert_eq!(done.id, progress.id);
    assert!(!told.asked && told.notification.is_none(), "reset");
    assert_eq!(told.tell(look(installed())), Telling::Nothing, "told once");
    told.asked = true;
    let next = shown(told.tell(look(downloading(1, 2))));
    assert_ne!(next.id, done.id, "a new notification");
}

/// Stopped by the user (missing again while asked): its notification goes.
#[test]
fn a_stop_dismisses_the_notification() {
    let mut told = Told { asked: true, offered: true, ..Told::default() };
    let progress = shown(told.tell(look(downloading(1, 2))));
    assert_eq!(told.tell(look(Status::Missing)), Telling::Dismiss(progress.id));
    assert!(!told.asked && told.notification.is_none());
    assert_eq!(told.tell(look(Status::Missing)), Telling::Nothing, "offered already");
}

/// A change is told by the first window that polls after it, never by the others too.
#[test]
fn only_the_first_window_tells_a_change() {
    let seen = AtomicU64::new(0);
    assert!(!first_to_see(&seen, 0), "nothing changed yet");
    assert!(first_to_see(&seen, 1), "the first window");
    assert!(!first_to_see(&seen, 1), "the second window: told already");
    assert!(first_to_see(&seen, 2));
}

/// A store whose writes fail (a folder that can't be written).
#[derive(Debug, Default)]
struct Store {
    fails: bool,
    sections: Mutex<std::collections::HashMap<String, settings_model::Section>>,
}

impl settings_model::SettingsStore for Store {
    fn section(&self, name: &str) -> Option<settings_model::Section> {
        self.sections.lock().unwrap().get(name).cloned()
    }
    fn update(
        &self,
        name: &str,
        change: &mut dyn FnMut(&mut settings_model::Section),
    ) -> std::io::Result<()> {
        if self.fails {
            return Err(std::io::Error::other("read-only"));
        }
        change(self.sections.lock().unwrap().entry(name.to_owned()).or_default());
        Ok(())
    }
}

/// A host keeping what it's told, with the user's settings it's given.
#[derive(Default)]
struct Host {
    settings: Option<&'static settings_model::SettingsService>,
    told: Vec<Notification>,
    dismissed: Vec<NotificationId>,
    events: kernel::EventBus,
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        None
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn user_settings(&self) -> Option<&'static settings_model::SettingsService> {
        self.settings
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
    fn dismiss_notification(&mut self, id: NotificationId) {
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
impl workbench_model::SystemHost for Host {}

/// Not Now dismisses the offer and says "not offered again" only once that's saved: never
/// with no settings to keep it in, nor when the write failed; a shown result is shown.
#[test]
fn not_now_says_so_only_once_saved() {
    let store = |fails| -> &'static settings_model::SettingsService {
        Box::leak(Box::new(settings_model::SettingsService::new(Arc::new(Store {
            fails,
            ..Store::default()
        }))))
    };
    let declined = t("web_engine.declined");
    for (settings, says) in [(None, false), (Some(store(true)), false), (Some(store(false)), true)]
    {
        let id = told(|t| *t.notification.get_or_insert_with(NotificationId::new));
        let mut host = Host { settings, ..Host::default() };
        assert!(NotNow.run(&mut host));
        assert_eq!(host.dismissed, [id], "the offer goes");
        assert_eq!(host.told.iter().any(|n| n.title == declined), says, "{settings:?}");
    }
    let mut host = Host::default();
    let id = NotificationId::new();
    assert!(!show(&mut host, Telling::Nothing));
    assert!(show(&mut host, Telling::Dismiss(id)));
    assert_eq!(host.dismissed, [id]);
}

/// Every kind of failure is told in words of its own, in both languages: never a path, an
/// address or a message of the system's.
#[test]
fn every_failure_has_its_words() {
    let kinds = [
        Failure::Offline,
        Failure::NoDataFolder,
        Failure::NoPackage,
        Failure::Untrusted,
        Failure::Damaged,
        Failure::Server,
        Failure::Disk,
    ];
    let texts: Vec<String> = kinds.into_iter().map(failure_text).collect();
    for text in &texts {
        assert!(!text.starts_with("web_engine."), "a text, not its key: {text}");
        assert!(!text.contains('/') && !text.contains("http"), "{text}");
    }
    let mut unique = texts.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), texts.len(), "each its own");
}

/// A look for a newer engine the user asked for says how it ended, in the notification that
/// said it started: newer, up to date (its version), or why not (with Try Again); the
/// background's look says nothing.
#[test]
fn a_check_says_how_it_ended() {
    let ended = |updated| Look { updated: Some(updated), ..look(installed()) };
    let mut told = Told::default();
    assert_eq!(told.tell(ended(Updated::UpToDate)), Telling::Nothing, "the background's");
    let id = *told.notification.get_or_insert_with(NotificationId::new);
    told.checking = true;
    assert_eq!(told.tell(look(installed())), Telling::Nothing, "still looking");
    let up_to_date = shown(told.tell(ended(Updated::UpToDate)));
    assert_eq!(up_to_date.id, id, "the checking notification, updated");
    assert_eq!(up_to_date.title, t("web_engine.up_to_date_title"));
    assert!(up_to_date.detail.is_some_and(|d| d.contains("1.2.3")));
    assert!(!told.checking && told.notification.is_none(), "told once");
    assert_eq!(told.tell(ended(Updated::UpToDate)), Telling::Nothing);
    told.checking = true;
    let newer = shown(told.tell(ended(Updated::Newer("1.2.4".into()))));
    assert_eq!(newer.title, t("web_engine.updated_title"));
    assert!(newer.detail.is_some_and(|d| d.contains("1.2.4")));
    told.checking = true;
    let failed = shown(told.tell(ended(Updated::Failed(Failure::Offline))));
    assert_eq!(failed.detail, Some(failure_text(Failure::Offline)));
    assert_eq!(failed.actions.iter().map(|a| a.command).collect::<Vec<_>>(), [CHECK]);
}

/// Remove says it's removing only when it does: with no place for the engine (or a download
/// under way), nothing is said.
#[test]
fn remove_says_so_only_when_it_acts() {
    let mut host = Host::default();
    assert!(Remove.run(&mut host));
    assert!(host.told.is_empty(), "nothing removed, nothing said");
}
