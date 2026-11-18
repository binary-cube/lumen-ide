//! Language levels (e.g. PHP 8.2), per project or per file.

use super::*;

impl Workbench {
    /// The language at the cursor (or the file's) that has levels, with the chosen level.
    pub(super) fn level_at_cursor(&self) -> Option<(LanguageId, String)> {
        let tab = self.tabs.get(self.active)?;
        let head = tab.doc.selections().primary().head;
        let candidates =
            [self.intelligence_at(head).map(|i| i.language()), Some(tab.syntax.language().clone())];
        let file = self.active_relative_path();
        candidates.into_iter().flatten().find_map(|language| {
            let project = crate::levels::LanguageLevels::get(&self.data.levels.chosen, &language)?;
            let level = file
                .as_deref()
                .and_then(|f| self.data.levels.file_level(f, &language))
                .unwrap_or(project);
            Some((language.clone(), level.to_owned()))
        })
    }

    /// The active file's path relative to the project, with `/` (the key of per-file exceptions).
    pub(super) fn active_relative_path(&self) -> Option<String> {
        let path = self.tabs.get(self.active)?.doc.path()?;
        let root = self.project.as_ref()?.root_path();
        let relative = path.strip_prefix(root).ok()?;
        Some(
            relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
        )
    }

    /// The active file has its own level for the language.
    pub(super) fn has_file_level(&self, language: &LanguageId) -> bool {
        self.active_relative_path()
            .is_some_and(|f| self.data.levels.file_level(&f, language).is_some())
    }

    /// Chooses the language level only for the active file (`None` = same as the project).
    pub fn set_file_language_level(&mut self, language: &LanguageId, level: Option<&str>) -> bool {
        let (Some(root), Some(file)) = (
            self.project.as_ref().map(|p| p.root_path().to_path_buf()),
            self.active_relative_path(),
        ) else {
            return false;
        };
        self.data.levels.set_file(&file, language, level);
        crate::levels::save(&*self.ports.levels, &root, &self.data.levels);
        self.completion.close();
        true
    }

    /// What the language level menu shows: the language, the name, the levels, the chosen
    /// one, whether it's automatic and what was detected.
    pub fn level_menu(&self) -> Option<LevelMenu> {
        let (language, current) = self.level_at_cursor()?;
        let intel = self.support.intelligence.iter().find(|i| i.language() == language)?;
        Some(LevelMenu {
            name: self
                .support
                .languages
                .get(&language)
                .map_or_else(String::new, |l| l.name.to_owned()),
            levels: intel.levels().to_vec(),
            project: crate::levels::LanguageLevels::get(&self.data.levels.chosen, &language)
                .unwrap_or(&current)
                .to_owned(),
            file: self.has_file_level(&language).then(|| current.clone()),
            file_name: self
                .tabs
                .get(self.active)
                .and_then(|t| t.doc.path())
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
            auto: self.data.levels.is_auto(&language),
            detected: crate::levels::LanguageLevels::get(&self.data.levels.detected, &language)
                .map(str::to_owned),
            source: crate::levels::LanguageLevels::get(&self.data.levels.sources, &language)
                .map(str::to_owned),
            language,
        })
    }

    /// Chooses the language level for the project (`None` = automatic, from its config).
    pub fn set_language_level(&mut self, language: &LanguageId, level: Option<&str>) -> bool {
        let Some(root) = self.project.as_ref().map(|p| p.root_path().to_path_buf()) else {
            return false;
        };
        let support = Arc::clone(&self.support);
        let intelligence = &support.intelligence;
        self.data.levels.set(intelligence, language, level);
        crate::levels::save(&*self.ports.levels, &root, &self.data.levels);
        self.completion.close();
        true
    }

    /// The file's language and, if different, the one at the cursor (e.g. „PHP · JSON”).
    pub(super) fn language_label(&self, tab: &Tab) -> String {
        let name = |id: &LanguageId| {
            self.support.languages.get(id).map_or_else(|| id.to_string(), |l| l.name.to_owned())
        };
        let file = tab.syntax.language();
        let head = tab.doc.selections().primary().head;
        match tab.syntax.snapshot().map(|s| s.language_at(head)) {
            Some(at) if at != file => format!("{} · {}", name(file), name(at)),
            _ => name(file),
        }
    }

    /// The version options for the popover (from `level_menu`), above the item at `anchor`.
    fn level_choices(&self, anchor: Rect) -> Option<ui_workbench::LevelChoices> {
        let menu = self.level_menu()?;
        Some(ui_workbench::LevelChoices {
            anchor,
            name: menu.name,
            levels: menu.levels.iter().map(|l| (*l).to_owned()).collect(),
            project: menu.project,
            auto: menu.auto,
            detected: menu.detected,
            source: menu.source,
            file_name: menu.file_name,
            file: menu.file,
        })
    }

    /// Opens the language version picker (its module's popover, above the status bar item).
    pub fn open_level_picker(&mut self) -> bool {
        let Some(menu) = self.level_menu() else { return false };
        let anchor =
            self.status.action_rect(crate::commands::LANGUAGE_LEVEL.0).unwrap_or_else(|| {
                Rect::new(self.size.width as f32 - 200.0, self.size.height as f32, 0.0, 0.0)
            });
        let Some(choices) = self.level_choices(anchor) else { return false };
        if !self.open_bare_view(ui_workbench::LEVEL_PICKER_EXTENSION, &choices, true) {
            return false;
        }
        self.level_language = Some(menu.language);
        true
    }

    /// The choice from the popover: the project's or the file's version.
    pub(crate) fn apply_level_pick(&mut self, pick: ui_workbench::LevelPick) {
        let Some(language) = self.level_language.take() else { return };
        match pick {
            ui_workbench::LevelPick::Project(level) => {
                self.set_language_level(&language, level.as_deref());
            }
            ui_workbench::LevelPick::File(level) => {
                self.set_file_language_level(&language, level.as_deref());
            }
        }
    }

    // ─── Zoom ───
}
