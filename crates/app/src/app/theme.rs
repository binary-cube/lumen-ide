//! Settings and theme: the settings model, applying the theme (global or the project's),
//! theme studio requests.

use super::*;

impl EditorApp {
    pub(super) fn settings_model(&self) -> SettingsModel {
        settings_model(&self.shared.themes.borrow())
    }

    /// A choice from settings: applied immediately and saved.
    pub(super) fn settings_action(&mut self, action: SettingsAction) {
        let changed = match action {
            SettingsAction::SelectTheme(id) => self.shared.themes.borrow_mut().choose(&id),
            SettingsAction::ToggleFollowSystem => {
                let system = self.window.as_ref().and_then(|w| w.system_appearance());
                let mut themes = self.shared.themes.borrow_mut();
                let on = !themes.follows_system();
                themes.set_follow_system(on, system.map(theme_appearance))
            }
            SettingsAction::SelectKeymap(id) => {
                if crate::keymaps::choose(&id) {
                    self.shared.keymap_version.set(self.shared.keymap_version.get() + 1);
                }
                false
            }
            SettingsAction::Shortcuts(changes) => {
                for (command, shortcuts) in changes {
                    if let Some(id) = cmd::by_id(&command) {
                        crate::keymaps::set_user(id, shortcuts);
                    }
                }
                // Every window: its keymap, its menus, these settings.
                self.shared.keymap_version.set(self.shared.keymap_version.get() + 1);
                false
            }
            SettingsAction::SelectLanguage(code) => {
                if crate::language::choose(&code) {
                    self.shared.locale_version.set(self.shared.locale_version.get() + 1);
                }
                false
            }
            SettingsAction::OpenThemeStudio => {
                self.workbench.close_settings();
                self.open_theme_studio()
            }
            SettingsAction::Close => {
                self.workbench.close_settings();
                false
            }
            SettingsAction::SelectFileIcons(set) => {
                let icons = std::sync::Arc::clone(self.workbench.icons());
                crate::icon_style::save_file_set(&icons, set);
                icons.set_file_set(set);
                true
            }
            SettingsAction::SelectIcons(look) => {
                let icons = std::sync::Arc::clone(self.workbench.icons());
                crate::icon_style::save(&icons, look);
                icons.set_look(look);
                // The other windows share the set: they redraw with the theme's signal.
                true
            }
            SettingsAction::SelectCodeLanguage(id) => {
                self.workbench.select_settings_language(&id);
                false
            }
            SettingsAction::ToggleFramework(id) => {
                self.workbench.toggle_framework(&id);
                false
            }
            SettingsAction::ToggleModule(id) => {
                self.workbench.support().manager.toggle_enabled(&id);
                false
            }
            SettingsAction::ToggleModuleHidden(id) => {
                self.workbench.support().manager.toggle_hidden(&id);
                false
            }
            SettingsAction::TogglePreload(id) => {
                self.workbench.support().manager.toggle_preload(&id);
                false
            }
            SettingsAction::SetModulesAtStartup(startup) => {
                self.workbench.support().manager.set_at_startup(startup);
                false
            }
            SettingsAction::Choose { block, key, value }
            | SettingsAction::Reset { block, key, value } => {
                self.workbench.settings_choose(block, key, value);
                // The backdrop: seen at once (under the settings themselves, while a slider
                // is dragged); the other windows take it with the theme's signal.
                if block == "backdrop" || block == "floating" {
                    self.workbench.refresh_backdrop();
                    self.shared.theme_version.set(self.shared.theme_version.get() + 1);
                    self.applied.theme_version = self.shared.theme_version.get();
                }
                false
            }
            SettingsAction::Press { block, action } => {
                match self.workbench.settings_command(block, action) {
                    // A button running a command: the app's own too (the settings stay open).
                    Some(id) => self.settings_command(id),
                    None => self.workbench.settings_press(block, action),
                }
                false
            }
            SettingsAction::SetText { block, key, value } => {
                self.workbench.settings_set_text(block, key, &value);
                false
            }
            SettingsAction::OpenResult | SettingsAction::ToggleAdvanced(_) => false,
        };
        if changed {
            self.theme_changed();
        }
        self.sync_shared();
        let model = self.settings_model();
        self.workbench.set_settings_model(model);
    }

    /// The theme changed (in this window): applies it and notifies the others.
    pub(super) fn theme_changed(&mut self) {
        self.shared.theme_version.set(self.shared.theme_version.get() + 1);
        self.apply_theme();
    }

    pub(super) fn apply_theme(&mut self) {
        let themes = self.shared.themes.borrow();
        // The project's own theme (if it still exists), otherwise the global one.
        let own = self.workbench.project_theme().and_then(|id| themes.palette_of(id));
        match own {
            Some((palette, theme)) => {
                self.workbench.set_palette(palette);
                crate::theme_options::set_active(&theme);
                self.workbench.set_theme(theme);
            }
            None => {
                self.workbench.set_palette(themes.palette());
                if let Some(theme) = themes.resolved() {
                    crate::theme_options::set_active(&theme);
                    self.workbench.set_theme(theme);
                }
            }
        }
        drop(themes);
        self.applied.theme_version = self.shared.theme_version.get();
    }

    /// The theme configurator (its module's command).
    pub(super) fn open_theme_studio(&mut self) -> bool {
        self.workbench.run_command(workbench_model::THEME_STUDIO).unwrap_or(false)
    }

    /// A request from a viewer (the theme configurator).
    pub(super) fn viewer_request(&mut self, request: ui_workbench::AppRequest) {
        use ui_workbench::AppRequest;
        match request {
            // The eyedropper: the result comes back as `PlatformEvent::ScreenColor`.
            AppRequest::SampleScreen => {
                let started = self.window.as_ref().is_some_and(|w| w.sample_screen_color());
                self.window_state.sampling = started;
                tracing::debug!(started, "eyedropper requested");
                return;
            }
            // The work-in-progress theme, only in this window (not saved).
            AppRequest::PreviewTheme(source) => {
                let mut themes = self.shared.themes.borrow_mut();
                match themes.register_source(&source) {
                    Ok(id) => {
                        if let Some((palette, theme)) = themes.palette_of(&id) {
                            drop(themes);
                            self.workbench.set_palette(palette);
                            self.workbench.set_theme(theme);
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "the theme from the studio is invalid"),
                }
            }
            AppRequest::OpenProject(path) => {
                self.open_project(&path);
                return;
            }
            // An update staged: quit as Quit does (what still runs and the unsaved files
            // asked about); once its windows are closed, the app starts again (the update in place).
            AppRequest::RestartToUpdate(staged) => {
                self.pending.update = Some(staged);
                let control = self.quit();
                if control != Control::Continue {
                    self.pending.exit_request = Some(control);
                }
                return;
            }
            // The answer to the question about the work still running before a closing.
            AppRequest::RunningAnswer(answer) => {
                self.on_running_answer(answer);
                return;
            }
            // The language version chosen in its picker.
            AppRequest::SetLevel(pick) => {
                self.workbench.apply_level_pick(pick);
                return;
            }
            // Another window's project chosen (the open projects' list): to the front.
            AppRequest::FocusProject(id) => {
                if let Some(window) = &self.window {
                    window.focus_window(platform_api::WindowId(id));
                }
                return;
            }
            // A module's "Settings…": the settings, at its page.
            AppRequest::OpenSettings(page) => {
                let model = self.settings_model();
                self.workbench.open_settings(model);
                self.workbench.settings_show_page(&page);
                self.relayout();
                return;
            }
            // A folder (from the project search): shown in the tree.
            AppRequest::ShowFolder(path) => {
                self.workbench.show_folder(&path);
                self.relayout();
                return;
            }
            // A color picked for its literal: written into the active file.
            AppRequest::ReplaceInActive { range, text } => {
                self.with_text(|wb, t| wb.replace_in_active(t, range, &text));
                self.relayout();
                return;
            }
            AppRequest::PickSavePath { start, name } => {
                let Some(window) = self.window.clone() else { return };
                let picked = crate::watchdog::waiting_for_user(|| {
                    window.pick_save_path(start.as_deref(), &name)
                });
                if let Some(path) = picked {
                    self.workbench.viewer_command(&format!("picked-save:{}", path.display()));
                }
            }
            // A file for whoever asked (the "Database" window adding a data file).
            AppRequest::PickFile(start) => {
                let Some(window) = self.window.clone() else { return };
                let picked =
                    crate::watchdog::waiting_for_user(|| window.pick_file(start.as_deref()));
                match picked {
                    Some(path) => {
                        self.workbench.picked_file(&path);
                    }
                    None => self.workbench.pick_cancelled(),
                }
            }
            AppRequest::PickFolder(start) => {
                let Some(window) = self.window.clone() else { return };
                let picked =
                    crate::watchdog::waiting_for_user(|| window.pick_folder(start.as_deref()));
                if let Some(path) = picked {
                    self.workbench.viewer_command(&format!("picked-folder:{}", path.display()));
                }
            }
            AppRequest::FullScreenWhileOpen => {
                if let Some(window) = &self.window
                    && !window.is_fullscreen()
                {
                    window.toggle_fullscreen();
                    self.window_state.full_screen_for_view = true;
                }
                return;
            }
            // A view's copy from its own menu (a grid's rows as JSON).
            AppRequest::CopyText(text) => {
                if let Some(window) = &self.window {
                    window.set_clipboard_text(&text);
                }
                return;
            }
            AppRequest::CopyImage { width, height, rgba } => {
                if let Some(window) = &self.window {
                    window.set_clipboard_image(width, height, &rgba);
                }
                return;
            }
            AppRequest::Reveal(path) => {
                if let Some(window) = &self.window {
                    window.reveal_in_file_manager(&path);
                }
                return;
            }
            AppRequest::OpenInBrowser(path) => {
                if let Some(window) = &self.window {
                    window.open_in_browser(&path);
                }
                return;
            }
            // A view writing a settings block's field (the studio's decorations for every
            // theme): saved by the block's module; every window follows its version.
            AppRequest::CancelTheme => {
                self.apply_theme();
                self.workbench.close_asking_view();
            }
            AppRequest::SaveTheme { source, global } => {
                let saved = self.shared.themes.borrow_mut().save_user_theme(&source);
                match saved {
                    Ok(id) if global => {
                        self.workbench.set_project_theme(None);
                        self.shared.themes.borrow_mut().choose(&id);
                        self.theme_changed();
                    }
                    Ok(id) => {
                        self.workbench.set_project_theme(Some(id));
                        self.apply_theme();
                        // The other windows see the new theme in the list.
                        self.shared.theme_version.set(self.shared.theme_version.get() + 1);
                        self.applied.theme_version = self.shared.theme_version.get();
                    }
                    Err(e) => tracing::error!(error = %e, "the theme wasn't saved"),
                }
                self.workbench.close_asking_view();
            }
        }
        self.relayout();
    }

    /// Syncs with the shared changes made from another window (theme, language).
    /// A view this window opened or closed for every window: the app told (each window
    /// follows at its [`Self::sync_shared`]); what it is now, if it changed.
    pub fn publish_everywhere(&mut self) -> Option<Option<crate::over::Over>> {
        let wanted = self.workbench.everywhere_changed()?;
        let over = wanted.map(|extension| {
            // On the whole screen: this window takes its screen, with its ticket.
            let full = self.workbench.everywhere_full().then(|| {
                let display = self.window.as_ref().and_then(|w| w.display_id());
                crate::over::Ticket::now(display.unwrap_or_default())
            });
            if full.is_some()
                && let Some(window) = &self.window
                && !window.is_fullscreen()
            {
                window.toggle_fullscreen();
                self.window_state.full_screen_for_view = true;
            }
            crate::over::Over { extension, full }
        });
        self.window_state.ticket = over.as_ref().and_then(|o| o.full.clone());
        let mut everywhere = self.shared.everywhere.borrow_mut();
        everywhere.0 += 1;
        everywhere.1.clone_from(&over);
        // Already as it is here.
        self.applied.everywhere_turn = everywhere.0;
        Some(over)
    }

    /// Follows the view over every window: one on the whole screen started by another
    /// window leaves this one as it is — unless this window shows its own on that screen,
    /// started first (then the other gives way: it keeps it).
    fn follow_over(&mut self, wanted: Option<&crate::over::Over>) -> bool {
        let Some(over) = wanted else {
            self.window_state.ticket = None;
            return self.workbench.follow_everywhere(None);
        };
        let Some(theirs) = &over.full else {
            self.window_state.ticket = None;
            return self.workbench.follow_everywhere(Some(&over.extension));
        };
        if let Some(mine) = &self.window_state.ticket
            && (mine == theirs || !mine.yields_to(theirs))
        {
            return false;
        }
        // Given way (or never started here): its own closed quietly, nothing told.
        self.window_state.ticket = None;
        self.workbench.follow_elsewhere(&over.extension)
    }

    pub fn sync_shared(&mut self) {
        // `settings.toml` edited by hand (read again, its blocks applied): what the windows
        // derived from it is resolved again — the theme with its options and the overlay
        // backdrop (the palette), the keymap (the preset, the user's own shortcuts).
        let outside = crate::config::outside_changes();
        if self.shared.outside_version.get() != outside {
            self.shared.outside_version.set(outside);
            crate::keymaps::load();
            self.shared.keymap_version.set(self.shared.keymap_version.get() + 1);
            let system = self.window.as_ref().and_then(|w| w.system_appearance());
            self.shared.themes.borrow_mut().adopt_settings(system.map(theme_appearance));
            if crate::language::adopt_saved() {
                self.shared.locale_version.set(self.shared.locale_version.get() + 1);
            }
            self.shared.theme_version.set(self.shared.theme_version.get() + 1);
        }
        // The view of every project is open somewhere: this window's picture kept fresh too.
        self.workbench.refresh_task_data_if_watched();
        // A theme's own setting changed (in the settings): every window applies it again.
        let options = crate::theme_options::version();
        if self.shared.options_version.get() != options {
            self.shared.options_version.set(options);
            self.shared.theme_version.set(self.shared.theme_version.get() + 1);
        }
        let mut changed = false;
        let (turn, wanted) = self.shared.everywhere.borrow().clone();
        if self.applied.everywhere_turn != turn {
            self.applied.everywhere_turn = turn;
            changed |= self.follow_over(wanted.as_ref());
        }
        if self.applied.theme_version != self.shared.theme_version.get() {
            self.apply_theme();
            changed = true;
        }
        if self.applied.locale_version != self.shared.locale_version.get() {
            self.applied.locale_version = self.shared.locale_version.get();
            self.workbench.locale_changed();
            // The native menus are translated too.
            if let Some(window) = &self.window {
                window.set_menus(cmd::menus());
            }
            let model = self.settings_model();
            self.workbench.set_settings_model(model);
            changed = true;
        }
        if self.applied.keymap_version != self.shared.keymap_version.get() {
            self.applied.keymap_version = self.shared.keymap_version.get();
            self.apply_keymap();
            let model = self.settings_model();
            self.workbench.set_settings_model(model);
            changed = true;
        }
        if changed {
            self.relayout();
        }
    }

    /// The active keymap preset: the key lookup and the native menu's shortcuts.
    pub(super) fn apply_keymap(&mut self) {
        match crate::keymaps::keymap(MAC) {
            Ok(keymap) => self.keymap = keymap,
            Err(e) => tracing::warn!(error = %e, "invalid keymap preset"),
        }
        if let Some(window) = &self.window {
            window.set_menus(cmd::menus());
        }
    }
}

/// What the settings show, from the app's current state (also used by `--snapshot`).
pub(crate) fn settings_model(themes: &crate::themes::Themes) -> SettingsModel {
    SettingsModel {
        themes: themes.options(),
        current_theme: themes.current_id().to_owned(),
        follow_system: themes.follows_system(),
        languages: crate::language::options(),
        current_language: i18n::locale(),
        // Filled in by the workbench (its icons know the packs).
        icon_look: icons::IconLook::default(),
        icon_looks: Vec::new(),
        file_icon_set: None,
        icon_samples: Vec::new(),
        file_icon_sets: Vec::new(),
        keymaps: crate::keymaps::PRESETS
            .iter()
            .map(|p| (p.id.to_owned(), kernel::brand::named(p.name)))
            .collect(),
        current_keymap: crate::keymaps::current().id.to_owned(),
        bindings: cmd::keymap_bindings(),
        // Filled in by the workbench (its modules and its project's frameworks).
        modules: Vec::new(),
        modules_at_startup: false,
        project: None,
        frameworks: Vec::new(),
        blocks: Vec::new(),
        code_languages: Vec::new(),
        code_language: None,
    }
}

/// The themes for the configurator (without the preview), the project's own theme (if it has
/// one) as the current one.
pub(super) fn studio_input(
    shared: &Shared,
    project_theme: Option<&str>,
) -> theming::studio::StudioInput {
    let themes = shared.themes.borrow();
    let current = project_theme.map_or_else(|| themes.current_id().to_owned(), str::to_owned);
    themes.studio_input(&current)
}
