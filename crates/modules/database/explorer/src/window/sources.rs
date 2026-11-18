//! Adding, showing and removing data sources.

use super::*;

impl DatabaseWindow {
    /// The data sources' modal (through the app: the module's command), `select`ed or with
    /// a new source of driver `add`, `focus` given the keys.
    pub fn open_sources(
        &mut self,
        select: Option<DataSourceId>,
        add: Option<String>,
        focus: Option<crate::sources::Field>,
    ) {
        self.open_sources_with(select, None, add, focus);
    }

    /// Folder `id` marked as production, or not any more (every source in it follows).
    pub(super) fn toggle_production(&mut self, id: database::FolderId) {
        let Some(sessions) = self.sessions.clone() else { return };
        let Some(mut folder) = sessions.catalog().folders().into_iter().find(|f| f.id == id) else {
            return;
        };
        folder.production = !folder.production;
        let told = if folder.production {
            i18n::tr!("datasources.folder_production_on", name = folder.name.as_str())
        } else {
            i18n::tr!("datasources.folder_production_off", name = folder.name.as_str())
        };
        match sessions.catalog().put_folder(folder) {
            Ok(()) => self.notify(workbench_model::Notification::info(told)),
            Err(why) => self.notify(workbench_model::Notification::error(why)),
        }
        self.stale = true;
    }

    /// The data sources' modal with folder `folder` selected (its name and color).
    pub fn open_folder(&mut self, folder: database::FolderId) {
        self.open_sources_with(None, Some(folder), None, None);
    }

    pub(super) fn open_sources_with(
        &mut self,
        select: Option<DataSourceId>,
        folder: Option<database::FolderId>,
        add: Option<String>,
        focus: Option<crate::sources::Field>,
    ) {
        let Some(sessions) = self.sessions.clone() else { return };
        let mut request = crate::sources::SourcesRequest::new(sessions);
        request.select = select;
        request.select_folder = folder;
        request.add = add;
        request.focus = focus;
        request.wake = self.wake();
        self.action =
            Some(ToolPanelAction::View(ViewerAction::OpenView(ui_components::OpenView::modal(
                i18n::t("datasources.modal_title"),
                crate::sources::EXTENSION,
                request,
            ))));
    }

    /// The system's file picker, at the project's folder.
    pub(super) fn pick_file(&mut self, sessions: &Sessions) {
        let start = Some(sessions.root().to_path_buf()).filter(|r| !r.as_os_str().is_empty());
        self.action =
            Some(ToolPanelAction::View(ViewerAction::Request(AppRequest::PickFile(start))));
    }

    /// File `path` picked: added as a source of the driver chosen (or of the one that opens
    /// it), in the project's list; one already there is only shown.
    pub fn add_file(&mut self, path: &Path, driver: Option<&str>) {
        let Some(sessions) = self.sessions.clone() else { return };
        let driver = match driver {
            Some(driver) => driver.to_owned(),
            None => match crate::provider::data_files(path, &fs_local::LocalFileSystem) {
                Some(files) => files.driver.into_owned(),
                None => {
                    self.notify(workbench_model::Notification::warning(i18n::tr!(
                        "datasources.not_data",
                        file = kernel::paths::file_name(path).as_str()
                    )));
                    return;
                }
            },
        };
        let root = sessions.root().to_path_buf();
        let catalog = sessions.catalog();
        if let Some(known) = catalog.source_of_file_with(&root, path, |s| s.driver == driver) {
            self.selected = Some(NodeKey::Source(known.id));
            self.notify(workbench_model::Notification::info(i18n::tr!(
                "datasources.already",
                name = known.name.as_str()
            )));
            return;
        }
        let names: Vec<String> = catalog.all().into_iter().map(|s| s.name).collect();
        let base = kernel::paths::file_name(path);
        let name = database::unique_name(&base, names.iter().map(String::as_str));
        let mut source = DataSource::new(name, driver, Scope::Project);
        source.file = Some(database::source::stored_path(path, &root));
        source.secrets = database::SecretPolicy::Never;
        let id = source.id;
        // A file shown by itself now is that source from now on: its rows stay where they are.
        match catalog.add(source) {
            Ok(()) => self.selected = Some(NodeKey::Source(id)),
            Err(why) => self.notify(workbench_model::Notification::error(why)),
        }
    }

    /// A file shown by itself kept as a source of the project.
    pub(super) fn keep_file(&mut self, id: DataSourceId) {
        let Some(source) = self.source(id).cloned() else { return };
        if let Some(file) = &source.file {
            self.add_file(&file.clone(), Some(&source.driver));
        }
    }

    /// Source `id` removed from its list (its session closed). Its data isn't touched.
    pub(super) fn remove(&mut self, id: DataSourceId) {
        let Some(sessions) = self.sessions.clone() else { return };
        match sessions.catalog().remove(id) {
            Ok(Some(source)) => {
                self.loaders.remove(&id);
                // A file still shown by itself keeps its own session.
                if source.file_in(sessions.root()).is_none_or(|f| sessions.file_users(&f) == 0) {
                    sessions.close(id);
                }
                // Its password goes too (in the background: the keychain may make it wait).
                let credentials = sessions.credentials();
                let gone = source.clone();
                sessions.runtime().spawn(move || {
                    if let Err(why) = credentials.remove(&gone) {
                        tracing::warn!("a removed data source's password wasn't removed: {why}");
                    }
                });
                self.notify(workbench_model::Notification::success(i18n::tr!(
                    "datasources.removed",
                    name = source.name.as_str()
                )));
            }
            Ok(None) => {}
            Err(why) => self.notify(workbench_model::Notification::error(why)),
        }
    }
}
