use std::path::PathBuf;
use std::sync::Arc;

use super::*;
use workbench_model::ActiveFile;

use crate::memory;

struct Host {
    project: kernel::ProjectId,
    own: settings_model::SettingsService,
    user: &'static settings_model::SettingsService,
    events: kernel::EventBus,
    root: Option<PathBuf>,
    jobs: Vec<workbench_model::Job>,
    opened: Vec<PathBuf>,
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<PathBuf> {
        self.root.clone()
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        Some(self.project)
    }
    fn active_file(&self) -> Option<ActiveFile> {
        None
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
    fn project_settings(&self) -> Option<&settings_model::SettingsService> {
        Some(&self.own)
    }
    fn user_settings(&self) -> Option<&'static settings_model::SettingsService> {
        Some(self.user)
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, job: workbench_model::Job) {
        self.jobs.push(job);
    }
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for Host {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}
impl workbench_model::EditorHost for Host {
    fn open_file(&mut self, path: &std::path::Path) -> bool {
        self.opened.push(path.to_path_buf());
        true
    }
}
impl workbench_model::RefactoringHost for Host {}
impl workbench_model::ServicesHost for Host {}
impl workbench_model::SystemHost for Host {}

fn host() -> Host {
    Host {
        project: kernel::ProjectId::new(),
        own: memory::settings(),
        user: Box::leak(Box::new(memory::settings())),
        events: kernel::EventBus::new(),
        root: None,
        jobs: Vec::new(),
        opened: Vec::new(),
    }
}

/// "Open extractors directory": nothing read or written on the click; in the background
/// the folder is made with its examples, then its first template opened.
#[test]
fn the_extractors_folder_is_made_and_opened_in_the_background() {
    let root = std::env::temp_dir().join(format!("extractors-open-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let mut host = Host { root: Some(root.clone()), ..host() };
    assert!(open_directory(&mut host));
    let dir = templates::own_dir(&root);
    assert!(!dir.exists(), "not on the click");
    assert!(host.opened.is_empty());
    let job = host.jobs.pop().expect("work in the background");
    let done = job();
    done(&mut host);
    assert!(dir.exists(), "made with its examples");
    assert_eq!(host.opened.len(), 1, "its first template opened");
    assert!(host.opened[0].starts_with(&dir));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_choice_is_kept_in_the_project_s_own_settings() {
    let mut host = host();
    let catalog = extractors::catalog();
    catalog.choose(Some(host.project), "sql-update");
    assert!(save(&mut host));
    assert_eq!(host.own.get(&settings::CHOSEN), "sql-update");
    // The skips in the user's.
    assert!(!host.user.get(&settings::SKIP_COMPUTED));
    // Opened again: the project's choice comes back from its settings.
    catalog.forget(host.project);
    let own = Arc::new(memory::settings());
    own.set(&settings::CHOSEN, "json".to_owned()).unwrap();
    let id = kernel::ProjectId::new();
    let root = std::path::Path::new("/p");
    let ctx = ProjectContext {
        marks: Default::default(),
        id,
        root,
        storage: root,
        events: &host.events,
        wake: Arc::new(|| {}),
        settings: Some(own),
        language_of: Arc::new(|_| None),
        quality_tools: Vec::new(),
        text_formatters: Vec::new(),
        notifier: Arc::new(workbench_model::NoNotifier),
    };
    let subscriptions = ExtractorsService { user: UserDir::None }.attach(&ctx);
    assert_eq!(catalog.chosen(Some(id)).id(), "json");
    drop(subscriptions);
    assert_eq!(catalog.chosen_id(Some(id)), None, "let go with the project");
}

#[test]
fn the_formats_given_are_saved_and_offered() {
    let mut host = host();
    let mut formats = extractors::CsvFormat::defaults();
    formats.push(extractors::CsvFormat::new("csv-x", "Hash", "#", true));
    assert!(csv_formats::give(&mut host, formats.clone()));
    assert_eq!(settings::formats_of(&host.user.get(&settings::CSV_FORMATS)), Some(formats));
    assert!(extractors::catalog().find(None, "csv-x").is_some());
    // Back to the defaults: nothing kept.
    assert!(csv_formats::give(&mut host, extractors::CsvFormat::defaults()));
    assert!(host.user.get(&settings::CSV_FORMATS).is_empty());
    assert!(extractors::catalog().find(None, "csv-x").is_none());
}
