//! The window made: every part at its start, and what the app gives it afterwards (the
//! waker, the clipboards, the themes, the resources' figures).

use std::collections::HashMap;
use std::sync::Arc;

use icons::Icons;
use kernel::{Memo, PhysicalSize, Rect};
use syntax_ts::Highlighter;
use ui_workbench::{CompletionPopup, Palette, SettingsModel, SettingsView, StatusBar, TabBar};

use crate::modules::LanguageSupport;
use crate::resources::Sample;

use super::project_data::ProjectData;
use super::{
    ChoicePurpose, ClipboardReader, FileIcons, FilesClipboard, Focus, Ports, ThemeSource, Themes,
    Waker, Workbench, chrome, dialogs, emphasis, entry_events, everywhere, groups, home,
    paint_phases, recent_writes, saving, session, shared_icons, task_data, tool_area,
};

impl Workbench {
    pub fn new(support: Arc<LanguageSupport>, ports: Ports) -> Self {
        crate::config::isolate_tests();
        let reporters = support.resource_reporters.clone();
        let modules = Arc::clone(&support.manager);
        let mut settings = SettingsView::default();
        settings.set_card_previews(support.card_previews.clone());
        let idle = vec![
            workbench_model::IdleWatch::new(std::time::Instant::now());
            support.idle_actions.len()
        ];
        let data = ProjectData::new();
        let entry_changes = entry_events::EntryChanges::listen(&data.events);
        let palette = Palette::dark();
        let tools = tool_area::ToolArea::new(&palette);
        let icons = shared_icons(&support);
        let sessions = session::QueuedSessions::new(Arc::clone(&ports.sessions));
        let recent_writes = recent_writes::QueuedRecentFiles::new(Arc::clone(&ports.recent_files));
        Self {
            code_font: palette.code_font.clone(),
            palette,
            blur: true,
            renderer: None,
            ports,
            project: None,
            tabs: Vec::new(),
            active: 0,
            tab_bar: TabBar::default(),
            status: StatusBar::default(),
            editor_full: None,
            focus: Focus::Editor,
            focus_moves: 0,
            next_untitled: 1,
            size: PhysicalSize::default(),
            scale: 1.0,
            editor_area: Rect::default(),
            card: Rect::default(),
            support,
            worker: None,
            highlighter: Highlighter::new(),
            spans: Vec::new(),
            colors: Vec::new(),
            theme: None,
            scope_colors: HashMap::new(),
            levels_warmed: 0,
            theme_generation: 0,
            file_icons: FileIcons::default(),
            resources: None,
            resource_trail: Default::default(),
            resources_pinned: None,
            laid_for_headers: Vec::new(),
            icons,
            drag_unit: None,
            settings,
            settings_model: SettingsModel::default(),
            settings_language: None,
            choice_menu: Default::default(),
            dropdown_list: Default::default(),
            dropdown_anchor: kernel::Rect::default(),
            dropdown_ids: Vec::new(),
            context_menu: Default::default(),
            menu_drawer: None,
            choice_purpose: ChoicePurpose::None,
            toasts: Default::default(),
            hint: None,
            tab_tints: Default::default(),
            hint_tab: None,
            hint_card: Default::default(),
            hint_focused: false,
            status_popover: Default::default(),
            page_pictures: Vec::new(),
            page_popups: Vec::new(),
            overlays_floating: false,
            page_access: Vec::new(),
            page_holes_ok: false,
            resource_pills: Default::default(),
            toast_clock: None,
            file_drop: None,
            toast_position: None,
            pending_goto: None,
            pending_offset: None,
            menu_context: None,
            menu_text: std::cell::RefCell::default(),
            menu_probe: std::cell::RefCell::default(),
            status_cursor: std::cell::RefCell::default(),
            request: None,
            running_request: None,
            held_view_action: None,
            panel_actions: Vec::new(),
            about: Default::default(),
            editor_zoom: 1.0,
            project_theme: None,
            level_language: None,
            entry_changes,
            progress: Vec::new(),
            files_clipboard: None,
            session: workbench_model::SessionKeeper::default(),
            sessions,
            recent_writes,
            data,
            syntax_swept: std::time::Instant::now(),
            paint_phases: paint_phases::PaintPhases::default(),
            wheel_zoom: false,
            link_mode: false,
            link: None,
            pointer_text: None,
            tooltips: ui_workbench::Tooltips::default(),
            error_tip: None,
            code_hover: ui_workbench::CodeHover::default(),
            typed_insert: None,
            framework_checked: None,
            shown_indicators: Vec::new(),
            project_windows: Vec::new(),
            capture_wanted: false,
            param_hint: ui_workbench::ParamHint::default(),
            last_edit: None,
            lock_hint: None,
            tree_watcher: None,
            completion: CompletionPopup::default(),
            completion_replace: None,
            completion_assist: None,
            remember_projects: true,
            waker: None,
            window_subscriptions: Vec::new(),
            saving: saving::Saving::default(),
            reloads: std::sync::mpsc::channel(),
            files_checking: Arc::default(),
            format_reads: std::sync::mpsc::channel(),
            found: std::sync::mpsc::channel(),
            link_files: None,
            clipboard: None,
            themes: None,
            back: Vec::new(),
            forward: Vec::new(),
            breadcrumbs: Default::default(),
            crumbs: Memo::default(),
            swatches: Memo::default(),
            viewer_drag: false,
            modal_view: None,
            everywhere: everywhere::Everywhere::default(),
            idle,
            fetching: Vec::new(),
            task_data: task_data::AppResources::new(reporters, Some(modules)),
            task_refreshed: None,
            occluded: false,
            index_memory: std::cell::Cell::new(None),
            beside_compare: None,
            chrome: chrome::Chrome::default(),
            dialogs: dialogs::Dialogs::default(),
            home: home::HomeScreen::default(),
            tools,
            dividers: ui_workbench::control::Controls::default(),
            editors: groups::EditorGroups::default(),
            emphasis: emphasis::WindowEmphasis::default(),
        }
    }

    /// The icon set (also for the renderer, which rasterizes them).
    /// The modules and languages (shared by all windows).
    pub fn support(&self) -> Arc<LanguageSupport> {
        Arc::clone(&self.support)
    }

    pub fn icons(&self) -> &Arc<Icons> {
        &self.icons
    }

    /// Updates the displayed usage. `true` if it changed.
    pub fn set_resources(&mut self, sample: Sample, gpu: Option<render_api::GpuMemory>) -> bool {
        let value = Some((sample, gpu));
        let changed = self.resources != value;
        self.resource_trail.push(
            std::time::Instant::now(),
            sample.cpu,
            sample.ram_bytes as f32,
            gpu.map(|g| g.total() as f32),
        );
        self.resources = value;
        // Only a new sample, and no more than the Task Manager's own pace.
        if changed {
            self.refresh_task_data_if_watched();
        }
        changed
    }

    // ─── Syntax ───

    /// Wakes the event loop when a background job finishes (indexing).
    pub fn set_waker(&mut self, waker: Arc<dyn Fn() + Send + Sync>) {
        // The window's own, once: the modules' services follow it, a project or none.
        if self.waker.is_none() {
            let support = Arc::clone(&self.support);
            for service in &support.project_services {
                let subscriptions = service.window_opened(&waker);
                self.window_subscriptions.extend(subscriptions);
            }
        }
        self.waker = Some(Waker(waker));
    }

    /// How the system clipboard's text is read (the window's clipboard).
    /// How the system clipboard's files are read and written (the window's clipboard).
    pub fn set_files_clipboard(&mut self, clipboard: FilesClipboard) {
        self.files_clipboard = Some(clipboard);
    }

    pub fn set_clipboard_reader(&mut self, read: std::rc::Rc<dyn Fn() -> Option<String>>) {
        self.clipboard = Some(ClipboardReader(read));
    }

    /// Where the configurator's themes come from (the app's).
    pub fn set_theme_source(&mut self, themes: ThemeSource) {
        self.themes = Some(Themes(themes));
    }

    /// The configurator's themes, with this project's own theme as the current one.
    pub(crate) fn studio_input(&self) -> Option<theming::studio::StudioInput> {
        self.themes.as_ref().map(|themes| (themes.0)(self.project_theme()))
    }
}
