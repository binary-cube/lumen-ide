//! The project's frameworks in the editor: their suggestions, definitions and hover cards
//! come before the language's (e.g. a template name in `render('…')`).

use code_intelligence::{FrameworkCompletion, FrameworkContext, FrameworkSupport, HoverInfo};

use super::*;
use command_model::CommandId;

impl Workbench {
    /// The language at `offset` in the active document (in PHP: the HTML outside `<?php`).
    pub(super) fn language_at(&self, offset: usize) -> Option<LanguageId> {
        let tab = self.tabs.get(self.active)?;
        Some(
            tab.syntax
                .snapshot()
                .map_or_else(|| tab.syntax.language().clone(), |s| s.language_at(offset).clone()),
        )
    }

    /// The project's frameworks, detected and chosen, following its changes: each one's
    /// model made in the background as it changes (see [`crate::frameworks`]).
    pub(super) fn load_frameworks(&mut self, root: &Path) {
        let support = Arc::clone(&self.support);
        let store = Arc::clone(&self.ports.frameworks);
        let mut frameworks = self.with_project(|project| {
            crate::frameworks::ProjectFrameworks::load(&*store, root, &support.frameworks, project)
        });
        let generations = Arc::clone(&self.data.generations);
        let background = crate::frameworks::Background {
            root: root.to_path_buf(),
            levels: self.data.levels.for_file(None),
            languages: support.intelligence.clone(),
            generations: Arc::new(move || (generations.files(), generations.entries())),
            index: Arc::clone(&self.data.symbols),
        };
        frameworks.follow(&self.data.events, background);
        self.data.frameworks = frameworks;
    }

    /// The active document checked by the frameworks (in the background, their problems
    /// shown as diagnostics) when it changed, another one became active, or a framework's
    /// model did since it was last checked. Cheap when nothing did (asked at each wake).
    pub fn poll_frameworks(&mut self) {
        let active = self.data.frameworks.active();
        if active.is_empty() {
            return;
        }
        let Some(tab) = self.tabs.get(self.active) else { return };
        let Some(path) = tab.doc.path().filter(|p| !is_virtual(p)) else { return };
        let models: u64 = active.iter().map(crate::frameworks::Active::own).sum();
        let now = (tab.doc.id(), tab.doc.version(), models);
        if self.framework_checked == Some(now) {
            return;
        }
        self.framework_checked = Some(now);
        let doc = crate::frameworks::Document {
            path: path.to_path_buf(),
            language: tab.syntax.language().as_str().to_owned(),
            text: tab.text(),
        };
        self.data.frameworks.check(doc, &self.data.events);
    }

    /// Asks the active frameworks, in order, about `offset` in `source` (the active
    /// document), until one answers; each answer timed (a slow one counted).
    fn ask_frameworks<R>(
        &self,
        source: &str,
        offset: usize,
        ask: impl Fn(&crate::frameworks::Active, &FrameworkContext<'_>) -> Option<R>,
    ) -> Option<R> {
        let active = self.data.frameworks.active();
        if active.is_empty() {
            return None;
        }
        let tab = self.tabs.get(self.active)?;
        let path = Self::index_path(tab);
        let language = self.language_at(offset)?;
        self.with_project(|project| {
            active.iter().find_map(|framework| {
                let at = FrameworkContext {
                    project,
                    path: &path,
                    language: language.as_str(),
                    text: source,
                    offset,
                    generation: self.data.generations.files(),
                    structure: self.data.generations.entries(),
                    own: framework.answering(),
                };
                let started = std::time::Instant::now();
                let answer = ask(framework, &at);
                framework.answered(started.elapsed());
                answer
            })
        })
    }

    pub(super) fn framework_completion(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<FrameworkCompletion> {
        self.ask_frameworks(source, offset, |f, at| f.support.complete(at))
    }

    pub(super) fn framework_definition(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<code_intelligence::Location> {
        self.ask_frameworks(source, offset, |f, at| f.support.definition(at))
    }

    pub(super) fn framework_hover(&self, source: &str, offset: usize) -> Option<HoverInfo> {
        self.ask_frameworks(source, offset, |f, at| f.support.hover(at))
    }

    /// The framework's own name at `offset` (a route's), with the framework's support.
    pub(super) fn framework_name(
        &self,
        source: &str,
        offset: usize,
    ) -> Option<(Arc<dyn FrameworkSupport>, code_intelligence::FrameworkName)> {
        self.ask_frameworks(source, offset, |f, at| {
            f.support.name_at(at).map(|name| (Arc::clone(&f.support), name))
        })
    }

    /// The settings' "Module" section: the modules always on, and the frameworks with
    /// their state in the open project.
    pub(super) fn fill_modules(&self, model: &mut ui_workbench::SettingsModel) {
        let report = |id: &str| self.support.modules.iter().find(|m| m.id.as_str() == id);
        let settings = crate::config::settings();
        let disabled = settings.get(&crate::settings::MODULES_DISABLED);
        let preload = settings.get(&crate::settings::MODULES_PRELOAD);
        let enabled = |id: &str| !disabled.iter().any(|d| d == id);
        model.modules_at_startup =
            settings.get(&crate::settings::MODULE_LOADING) == crate::settings::STARTUP;
        let modules = &self.support.modules;
        model.modules = modules
            .iter()
            .filter(|m| !m.id.as_str().starts_with("framework."))
            .map(|m| {
                let id = m.id.as_str();
                // The modules on that can't work without it.
                let required_by = modules
                    .iter()
                    .filter(|o| o.depends_on.contains(&id) && enabled(o.id.as_str()))
                    .map(|o| localized(o.id.as_str(), "_name", o.name))
                    .collect();
                ui_workbench::ModuleRow {
                    id: id.to_owned(),
                    name: localized(id, "_name", m.name),
                    description: localized(id, "", m.description),
                    icon: m.info.icon,
                    // One that comes with the app: its makers', its version and date.
                    author: m.info.author.unwrap_or(kernel::brand::PUBLISHER).to_owned(),
                    version: m.info.version.unwrap_or(kernel::brand::version()).to_owned(),
                    updated: m.info.updated.unwrap_or(kernel::brand::released()).to_owned(),
                    enabled: enabled(id),
                    active: m.state == extensions::ModuleState::Active,
                    preload: preload.iter().any(|p| p == id),
                    loaded: self.support.manager.loaded(id),
                    required_by,
                    hidden: crate::settings::module_hidden(settings, id),
                    hidden_now: m.hidden,
                }
            })
            .collect();
        // The modules' own settings (only the ones that started contribute).
        model.blocks = self.settings_blocks();
        model.code_languages =
            self.settings_languages().into_iter().map(|l| (l.id, l.name)).collect();
        model.code_language = self.shown_settings_language();
        // The file icons: ours (by language), then the sets; the preview's files as the
        // languages give them.
        let mut samples: Vec<_> = self
            .support
            .languages
            .languages()
            .iter()
            .filter_map(|l| l.icon_sample.map(|(order, name)| (order, name, l.icon, l.id.as_str())))
            .collect();
        samples.sort_by_key(|(order, ..)| *order);
        model.icon_samples = samples
            .into_iter()
            .map(|(_, name, icon, language)| (name.to_owned(), icon, language.to_owned()))
            .collect();
        model.file_icon_set = self.icons.file_set();
        model.file_icon_sets = std::iter::once(ui_workbench::FileIconChoice {
            set: None,
            name: i18n::t("settings.file_icons_ours"),
            detail: i18n::t("settings.file_icons_ours_detail"),
        })
        .chain(self.icons.file_sets().iter().enumerate().map(|(i, s)| {
            ui_workbench::FileIconChoice {
                set: Some(i as u8),
                name: s.name().to_owned(),
                detail: s.credit().to_owned(),
            }
        }))
        .collect();
        // The icons' looks: our styles, then the packs (a pack's author and license).
        model.icon_look = self.icons.look();
        model.icon_looks = self
            .icons
            .looks()
            .into_iter()
            .map(|look| match (look, self.icons.pack(look)) {
                (_, Some(pack)) => ui_workbench::IconChoice {
                    look,
                    name: pack.name().to_owned(),
                    detail: pack.credit().to_owned(),
                },
                (look, None) => {
                    let id = self.icons.look_id(look);
                    ui_workbench::IconChoice {
                        look,
                        name: kernel::brand::named(&i18n::t(&format!("settings.icons_{id}"))),
                        detail: i18n::t(&format!("settings.icons_{id}_detail")),
                    }
                }
            })
            .collect();
        model.project = self.project.as_ref().map(|p| kernel::paths::file_name(p.root_path()));
        let data = &self.data.frameworks;
        model.frameworks = self
            .support
            .frameworks
            .iter()
            .map(|f| ui_workbench::FrameworkRow {
                id: f.id().to_owned(),
                name: f.name().to_owned(),
                description: report(&format!("framework.{}", f.id()))
                    .map_or_else(String::new, |m| localized(m.id.as_str(), "", m.description)),
                on: data.is_active(f.id()),
                detected: data.detected.iter().find(|(d, _)| d == f.id()).map(|(_, s)| s.clone()),
                chosen: data.choices.iter().any(|(c, _)| c == f.id()),
            })
            .collect();
    }

    /// Flips a framework for the open project (from settings).
    pub fn toggle_framework(&mut self, id: &str) -> bool {
        let on = self.data.frameworks.is_active(id);
        let changed = self.set_framework(id, !on);
        let model = self.settings_model.clone();
        self.set_settings_model(model);
        changed
    }

    /// The active file's requirements its project's manifest doesn't declare (e.g. an
    /// extension missing from `composer.json`), with the manifest's name. Nothing without
    /// a project or a manifest. The manifest is read once per change of the files; asked only
    /// when the marks are rebuilt (`sync_marks`).
    pub(super) fn missing_requirements(
        &mut self,
    ) -> Vec<(code_intelligence::Requirement, &'static str)> {
        let Some(tab) = self.tabs.get(self.active) else { return Vec::new() };
        if tab.diagnostics.requirements.is_empty() {
            return Vec::new();
        }
        let language = tab.syntax.language();
        let Some(manifest) = self
            .support
            .intelligence
            .iter()
            .find(|i| i.language() == *language)
            .and_then(|i| i.manifest())
        else {
            return Vec::new();
        };
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return Vec::new();
        };
        let generation = self.data.generations.files();
        let cached = self.data.manifests.get(manifest).filter(|(g, _)| *g == generation);
        if cached.is_none() {
            let text = read_text(&*self.ports.disk, &root.join(manifest));
            self.data.manifests.insert(manifest, (generation, text));
        }
        let Some((_, Some(text))) = self.data.manifests.get(manifest) else { return Vec::new() };
        let tab = &self.tabs[self.active];
        tab.diagnostics
            .requirements
            .iter()
            .filter(|r| !code_intelligence::has_dependency(text, &r.name))
            .map(|r| (r.clone(), manifest))
            .collect()
    }

    /// Turns a framework on or off for the open project (saved in `<project dir>/project.toml`).
    pub fn set_framework(&mut self, id: &str, on: bool) -> bool {
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return false;
        };
        let support = Arc::clone(&self.support);
        let frameworks = &support.frameworks;
        let store = Arc::clone(&self.ports.frameworks);
        let mut data = std::mem::take(&mut self.data.frameworks);
        self.with_project(|project| data.set(&*store, &root, frameworks, project, id, on));
        self.data.frameworks = data;
        self.completion.close();
        self.code_hover.hide();
        true
    }

    /// The modules' blocks in the settings, drawn for the open project, in their order (by
    /// page and place: never the modules'), and the Languages page's languages.
    pub(super) fn settings_blocks(&self) -> Vec<ui_workbench::ContributedBlock> {
        let mut blocks = self.with_settings_scope(|scope| {
            self.support
                .settings
                .iter()
                .map(|c| (c, c.block(scope)))
                // A block with nothing to show isn't (a theme without settings of its own).
                .filter(|(_, block)| !block.rows.is_empty())
                .map(|(c, block)| ui_workbench::ContributedBlock {
                    id: c.id(),
                    page: c.page(),
                    page_info: c.page_info(),
                    order: c.order(),
                    advanced: c.advanced(),
                    language: c.language().map(|l| l.id),
                    keywords: i18n::in_every_language(&format!(
                        "settings.keywords_{}",
                        c.id().replace('-', "_")
                    )),
                    block,
                })
                .collect::<Vec<_>>()
        });
        ui_workbench::order_blocks(&mut blocks);
        blocks
    }

    /// The Languages page's chooser: every block's languages, once, in their order.
    pub(super) fn settings_languages(&self) -> Vec<workbench_model::SettingsLanguage> {
        let data = self.project_storage_dir();
        let scope = workbench_model::SettingsScope {
            settings: crate::config::settings(),
            project: self.data.settings.as_deref(),
            project_data: data.as_deref(),
            wake: self.waker.as_ref().map(|w| &w.0),
            language: None,
        };
        workbench_model::merge_languages(
            self.support
                .settings
                .iter()
                .flat_map(|c| c.languages(scope).into_iter().chain(c.language())),
        )
    }

    /// The language the Languages page shows: the one chosen, while it's listed; else the
    /// first.
    fn shown_settings_language(&self) -> Option<String> {
        let all = self.settings_languages();
        self.settings_language
            .as_ref()
            .filter(|l| all.iter().any(|a| &a.id == *l))
            .cloned()
            .or_else(|| all.first().map(|l| l.id.clone()))
    }

    /// Another language chosen on the Languages page: every block of it shows it.
    pub fn select_settings_language(&mut self, language: &str) {
        self.settings_language = Some(language.to_owned());
        let model = self.settings_model.clone();
        self.set_settings_model(model);
    }

    /// A settings search result was opened: the language it's about (another of the
    /// Languages page's) shown.
    pub(super) fn settings_result_opened(&mut self) {
        if let Some(language) = self.settings.take_wanted_language() {
            self.select_settings_language(&language);
        }
    }

    /// What the modules' settings blocks see: the settings, the open project's own, its
    /// data folder, the Languages page's language, how to wake the app.
    fn with_settings_scope<R>(&self, f: impl FnOnce(workbench_model::SettingsScope<'_>) -> R) -> R {
        let data = self.project_storage_dir();
        let language = self.shown_settings_language();
        f(workbench_model::SettingsScope {
            settings: crate::config::settings(),
            project: self.data.settings.as_deref(),
            project_data: data.as_deref(),
            wake: self.waker.as_ref().map(|w| &w.0),
            language: language.as_deref(),
        })
    }

    /// While the settings are open: a block whose slow figures were computed in the
    /// background is drawn again. `true` if one was.
    pub(super) fn poll_settings_blocks(&mut self) -> bool {
        if !self.settings.is_open() {
            return false;
        }
        let any = self.support.settings.iter().fold(false, |any, c| c.poll() | any);
        if any {
            self.settings_model.blocks = self.settings_blocks();
        }
        any
    }

    /// A module block's choice: saved by its module, handed to its domain.
    pub fn settings_choose(&mut self, block: &str, key: &'static str, value: i64) {
        let Some(contribution) = self.support.settings.iter().find(|c| c.id() == block) else {
            return;
        };
        let contribution = Arc::clone(contribution);
        self.with_settings_scope(|scope| contribution.choose(scope, key, value));
        self.apply_settings_page(contribution.page());
    }

    /// Every block of page `page` hands the saved values to its domain again: a module with
    /// several blocks (the terminal's shell, display, behavior) often reads them all in one
    /// place, which only one of its blocks may say how to reach.
    fn apply_settings_page(&self, page: &str) {
        for block in self.support.settings.iter().filter(|c| c.page() == page) {
            block.apply(crate::config::settings());
        }
    }

    /// A module block's text field, written: saved, handed to the module, shown.
    pub fn settings_set_text(&mut self, block: &str, key: &'static str, value: &str) {
        let Some(contribution) = self.support.settings.iter().find(|c| c.id() == block) else {
            return;
        };
        let contribution = Arc::clone(contribution);
        self.with_settings_scope(|scope| contribution.set_text(scope, key, value));
        self.apply_settings_page(contribution.page());
        self.settings_model.blocks = self.settings_blocks();
    }

    /// Enter in a module's text field: what was written is saved.
    pub(super) fn settings_commit_text(&mut self) {
        if let Some(ui_workbench::SettingsAction::SetText { block, key, value }) =
            self.settings.commit_text()
        {
            self.settings_set_text(block, key, &value);
        }
    }

    /// The command a settings block's button runs (`None`: it asks its block instead,
    /// [`Self::settings_press`]): run by the app, which has the app's own commands too.
    pub fn settings_command(&self, block: &str, action: &'static str) -> Option<CommandId> {
        self.support.settings.iter().find(|c| c.id() == block)?.command(action)
    }

    /// A module block's button: what it asks first, then what it does on "yes" (the
    /// settings show the outcome).
    pub fn settings_press(&mut self, block: &str, action: &'static str) {
        let Some(contribution) = self.support.settings.iter().find(|c| c.id() == block).cloned()
        else {
            return;
        };
        // A button running a command (a preview): over the settings, which stay open.
        if let Some(command) = contribution.command(action) {
            self.run_command(command);
            return;
        }
        if let Some((question, then)) =
            self.with_settings_scope(|scope| contribution.press(scope, action))
        {
            workbench_model::WorkbenchHost::confirm(self, question, then);
        }
    }
}

impl Workbench {
    /// When the settings' live card wants its next frame (none under a view drawing the
    /// whole window: the settings aren't drawn then).
    pub fn settings_card_deadline(&self) -> Option<std::time::Instant> {
        if self.modal_view_covers() {
            return None;
        }
        self.settings.card_deadline()
    }
}

/// A module's text translated from `[module]` in the locales (`<id>_name` for its name,
/// `<id>` for its description; the manifest's English otherwise).
pub(crate) fn localized(id: &str, suffix: &str, fallback: &str) -> String {
    let key = format!("module.{}{suffix}", id.replace(['.', '-'], "_"));
    let found = i18n::t(&key);
    if found == key { fallback.to_owned() } else { found }
}

#[cfg(test)]
#[path = "../../tests/unit/workbench/frameworks.rs"]
mod tests;
