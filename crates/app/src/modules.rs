//! The modules included in the app and the distribution of their contributions.
//!
//! A new module is added here (and in Cargo.toml); the rest of the app doesn't change.
//! The selection comes from `~/.config/<app id>/settings.toml`:
//!
//! ```toml
//! [modules]
//! mode = "auto"            # "auto": all except `disabled`; "explicit": only `enabled`
//! disabled = ["lang.scss"]
//! ```

use std::sync::Arc;

use code_intelligence::LanguageIntelligence;
use extensions::{ModuleRegistry, ModuleReport, ModuleSelection, ModuleState};
use languages::{
    ContentSniffer, FileIconRule, FolderColorRule, IconSvg, LanguageDescriptor, LanguageRegistry,
};
use syntax_ts::{Grammar, Grammars, InjectionProvider, SyntaxConfig};

/// Everything the modules contributed for languages.
#[derive(Debug)]
pub struct LanguageSupport {
    /// How many modules are active (the About window).
    pub module_count: usize,
    pub languages: LanguageRegistry,
    pub syntax: Arc<SyntaxConfig>,
    /// The SVG icons contributed by modules (besides the built-in set).
    pub icons: Vec<IconSvg>,
    /// The icon packs (other drawings for the interface), read only once chosen.
    pub icon_packs: Vec<Arc<dyn icons::IconPack>>,
    /// The file icon sets (colored icons for files and folders), read only once chosen.
    pub file_icon_sets: Vec<Arc<dyn icons::FileIconSet>>,
    /// Language intelligence (symbols, completion, definitions).
    pub intelligence: Vec<Arc<dyn LanguageIntelligence>>,
    /// Tab expansions (e.g. Emmet in HTML).
    pub expanders: Vec<Arc<dyn code_intelligence::Expander>>,
    /// What the modules answer each other.
    pub answers: Arc<workbench_model::AnswerBook>,
    /// File viewers (images, Markdown preview…).
    pub viewers: Vec<Arc<dyn ui_workbench::ViewerProvider>>,
    /// The bars modules put above the editor of the files they know (a query console's).
    pub toolbars: Vec<Arc<dyn ui_workbench::editor_toolbar::EditorToolbarProvider>>,
    /// The tool windows (their buttons on the window's sides), by side, then order.
    pub tools: Vec<Arc<dyn ui_workbench::ToolWindowProvider>>,
    /// Colors in code (the swatch in the gutter and the color picker).
    pub colors: Vec<Arc<dyn code_intelligence::ColorProvider>>,
    /// The frameworks' detectors (their support is created per project, when used).
    pub frameworks: Vec<Arc<dyn code_intelligence::Framework>>,
    /// The modules' help with the documents they know (a query console's completion from
    /// its source's schema), asked before the frameworks.
    pub assists: Vec<Arc<dyn code_intelligence::DocumentAssist>>,
    /// What modules hide in the documents of their languages (a `.env` file's secrets).
    pub concealments: Vec<Arc<dyn workbench_model::Concealment>>,
    /// The modules' own names at the cursor (a `.env` key): what Rename and Find Usages act
    /// on, asked before the language servers and the built-in engine.
    pub cursor_symbols: Vec<Arc<dyn workbench_model::CursorSymbols>>,
    /// The language servers (the module running them), asked for the files of the
    /// languages whose engine is theirs.
    pub servers: Vec<Arc<dyn code_intelligence::servers::LanguageServers>>,
    /// Every module and whether it started (the settings list).
    pub modules: Vec<ModuleReport>,
    /// Version control systems (e.g. git), contributed by modules.
    pub vcs: Vec<Arc<dyn version_control::VcsProvider>>,
    /// What decorates the documents' lines (the git module: changed lines, blame).
    pub decorators: Vec<Arc<dyn workbench_model::LineDecorator>>,
    /// Commands contributed by modules (with their menus and handlers).
    pub commands: Vec<workbench_model::CommandContribution>,
    /// Menus contributed by modules (`Git ›`…).
    pub menus: Vec<workbench_model::MenuContribution>,
    /// What a click on a gutter mark runs (the git module's change actions).
    pub gutter_actions: Vec<workbench_model::GutterAction>,
    /// The app's commands the modules implement (Search in Project: indexing).
    pub implementations: Vec<workbench_model::CommandImplementation>,
    /// Who takes over renaming / moving files as refactorings (the refactoring module).
    pub file_refactorings: Vec<Arc<dyn workbench_model::FileRefactoring>>,
    /// The context menus' rows: the modules' and the app's own.
    pub context_items: Vec<workbench_model::ContextMenuItem>,
    /// Rows modules list in a context menu from what's open (a tab's other tabs).
    pub listed_items: Vec<workbench_model::ListedMenuItems>,
    /// Modules' services following the open project (the local history…).
    pub project_services: Vec<Arc<dyn workbench_model::ProjectService>>,
    /// Who opens the project's index (the indexing module).
    pub index_providers: Vec<Arc<dyn workbench_model::IndexProvider>>,
    /// What modules know of an action's targets (a remote project's excluded paths…).
    pub context_providers: Vec<Arc<dyn workbench_model::ContextProvider>>,
    /// What the modules' parts cost, for the Task Manager.
    pub resource_reporters: Vec<Arc<dyn workbench_model::ResourceReporter>>,
    /// The threads each module's work runs on (module id → names), for the Task Manager.
    pub module_threads: Vec<(String, &'static [&'static str])>,
    /// What each module counts itself (disk, network), by module id.
    pub module_counters: Vec<(String, workbench_model::ModuleCounters)>,
    /// Whose each viewer is (provider id → module id): its views' memory counts there.
    pub viewer_modules: Vec<(&'static str, String)>,
    /// The pictures of the modules' settings cards (Relax mode's scenes).
    pub card_previews: Vec<Arc<dyn ui_workbench::CardPreview>>,
    /// What modules start after a while without input in the window (Relax mode).
    pub idle_actions: Vec<Arc<dyn workbench_model::IdleAction>>,
    /// The modules' signs in the status bar (the computer kept awake).
    pub status_indicators: Vec<Arc<dyn workbench_model::StatusIndicator>>,
    /// The modules' signs outside the windows (by the system's clock, on the app's icon).
    pub system_indicators: Vec<Arc<dyn workbench_model::SystemIndicator>>,
    /// What the modules say in the top bar of a window's project (its shortcut).
    pub top_bar_badges: Vec<Arc<dyn workbench_model::TopBarBadge>>,
    /// What a click on the top bar's branch runs (a module's list to switch it).
    pub top_bar_label_actions: Vec<workbench_model::TopBarLabelAction>,
    /// Who asks for the region being worked in to stand out (Focus mode).
    pub region_emphasis: Vec<Arc<dyn workbench_model::RegionEmphasis>>,
    /// The modules' blocks in the settings window (their own settings).
    pub settings: Vec<Arc<dyn workbench_model::SettingsContribution>>,
    /// A hand-edited `settings.toml` (read again in the background) reaches the blocks'
    /// domains too, as a change made in the settings window does.
    pub _settings_followed: kernel::Subscription,
    /// Models finding texts by meaning (the indexing module's), for the settings' search.
    pub embeddings: Vec<Arc<dyn workbench_model::EmbeddingSource>>,
    /// The formatters and checkers the languages' modules contribute.
    pub quality_tools: Vec<Arc<dyn code_quality::QualityTool>>,
    /// The formatters the languages' modules build in.
    pub text_formatters: Vec<Arc<dyn code_quality::TextFormatter>>,
    /// Where themes come from (the bundled ones, their format).
    pub themes: Vec<Arc<dyn theming::catalog::ThemeProvider>>,
    /// Who draws the themes' decorations (a garland, snow…), asked only for a theme that
    /// has some.
    pub decorations: Vec<Arc<dyn ui_workbench::decorations::DecorationsProvider>>,
    /// Whose each of them is (its module's id: what a window's decorations hold counts there).
    pub decorations_modules: Vec<String>,
    /// The languages' parameters, for "Change Signature" (PHP, JavaScript).
    pub signatures: Vec<Arc<dyn refactor::signature::SignatureSyntax + Send + Sync>>,
    /// The languages' namespaces (PHP), for moving classes.
    pub namespaces: Vec<Arc<dyn refactor::moving::NamespaceRules>>,
    /// Orchestrates the modules' data: read ahead, let go when unused, what's in memory.
    pub manager: Arc<crate::module_manager::ModuleManager>,
}

/// Module `module` registered, the target its log is written under (its crate's name, as
/// `tracing` writes it) noted for the log's settings (each module's own level).
fn add(registry: &mut ModuleRegistry, module: Box<dyn extensions::Module>, target: &'static str) {
    let manifest = module.manifest();
    crate::logs::note_module(manifest.id.as_str(), manifest.name, manifest.info.icon, target);
    registry.register(module);
}

pub fn builtin() -> ModuleRegistry {
    let mut registry = ModuleRegistry::default();
    registry.set_clock(std::time::Instant::now);
    add(&mut registry, Box::new(lang_html::HtmlModule), "lang_html");
    add(&mut registry, Box::new(lang_javascript::JavaScriptModule), "lang_javascript");
    add(&mut registry, Box::new(lang_css::CssModule), "lang_css");
    add(&mut registry, Box::new(lang_scss::ScssModule), "lang_scss");
    add(&mut registry, Box::new(lang_json::JsonModule), "lang_json");
    add(&mut registry, Box::new(lang_yaml::YamlModule), "lang_yaml");
    add(&mut registry, Box::new(lang_sql::SqlModule), "lang_sql");
    add(&mut registry, Box::new(lang_cpp::CppModule), "lang_cpp");
    add(&mut registry, Box::new(lang_lua::LuaModule), "lang_lua");
    add(&mut registry, Box::new(lang_zig::ZigModule), "lang_zig");
    add(&mut registry, Box::new(lang_bash::BashModule), "lang_bash");
    add(&mut registry, Box::new(lang_toml::TomlModule), "lang_toml");
    add(&mut registry, Box::new(lang_markdown::MarkdownModule), "lang_markdown");
    add(&mut registry, Box::new(lang_dockerfile::DockerfileModule), "lang_dockerfile");
    add(&mut registry, Box::new(lang_dotenv::DotenvModule), "lang_dotenv");
    add(&mut registry, Box::new(lang_http::HttpModule), "lang_http");
    add(&mut registry, Box::new(http_client::HttpClientModule), "http_client");
    add(&mut registry, Box::new(lang_php::PhpModule), "lang_php");
    add(&mut registry, Box::new(lang_twig::TwigModule), "lang_twig");
    add(&mut registry, Box::new(lang_blade::BladeModule), "lang_blade");
    add(&mut registry, Box::new(file_icons::FileIconsModule), "file_icons");
    for pack in icon_packs::modules() {
        add(&mut registry, Box::new(pack), "icon_packs");
    }
    for set in icon_packs::file_set_modules() {
        add(&mut registry, Box::new(set), "icon_packs");
    }
    add(&mut registry, Box::new(themes_builtin::ThemesModule), "themes_builtin");
    add(&mut registry, Box::new(theme_decorations::DecorationsModule), "theme_decorations");
    // Before the code quality's: its blocks lead their shared settings page.
    add(&mut registry, Box::new(language_servers::LanguageServersModule), "language_servers");
    add(&mut registry, Box::new(quality::QualityModule), "quality");
    add(&mut registry, Box::new(viewer_image::ImageViewerModule), "viewer_image");
    add(&mut registry, Box::new(viewer_charts::ChartsModule), "viewer_charts");
    add(&mut registry, Box::new(viewer_markdown::MarkdownViewerModule), "viewer_markdown");
    add(&mut registry, Box::new(emmet::EmmetModule), "emmet");
    add(&mut registry, Box::new(structure::StructureModule), "structure");
    add(&mut registry, Box::new(colors::ColorsModule), "colors");
    add(&mut registry, Box::new(language_levels::LanguageLevelsModule), "language_levels");
    add(&mut registry, Box::new(canvas::CanvasModule), "canvas");
    add(&mut registry, Box::new(diagrams::DiagramsModule), "diagrams");
    add(&mut registry, Box::new(compare::CompareModule), "compare");
    add(&mut registry, Box::new(vcs_git::GitModule), "vcs_git");
    add(&mut registry, Box::new(local_history::LocalHistoryModule), "local_history");
    add(&mut registry, Box::new(folder_sizes::FolderSizesModule), "folder_sizes");
    add(&mut registry, Box::new(remote_projects::RemoteModule), "remote_projects");
    add(&mut registry, Box::new(database_explorer::DatabaseExplorerModule), "database_explorer");
    add(&mut registry, Box::new(database_console::DatabaseConsoleModule), "database_console");
    add(&mut registry, Box::new(data_extractors::DataExtractorsModule), "data_extractors");
    add(&mut registry, Box::new(database_sqlite::DatabaseSqliteModule), "database_sqlite");
    add(&mut registry, Box::new(database_csv::DatabaseCsvModule), "database_csv");
    add(&mut registry, Box::new(database_postgres::DatabasePostgresModule), "database_postgres");
    add(&mut registry, Box::new(database_mysql::DatabaseMySqlModule), "database_mysql");
    add(&mut registry, Box::new(database_external::DatabaseExternalModule), "database_external");
    add(&mut registry, Box::new(database_diagram::DatabaseDiagramModule), "database_diagram");
    add(&mut registry, Box::new(diagram_editor::DiagramEditorModule), "diagram_editor");
    add(&mut registry, Box::new(diagram_files::DiagramFilesModule), "diagram_files");
    add(&mut registry, Box::new(diagram_web::DiagramWebModule), "diagram_web");
    add(&mut registry, Box::new(task_manager::TaskManagerModule), "task_manager");
    add(&mut registry, Box::new(relax::RelaxModule), "relax");
    add(&mut registry, Box::new(path_marks::PathMarksModule), "path_marks");
    add(&mut registry, Box::new(keep_awake::KeepAwakeModule), "keep_awake");
    add(&mut registry, Box::new(profiler::ProfilerModule), "profiler");
    add(&mut registry, Box::new(updates::UpdatesModule), "updates");
    add(&mut registry, Box::new(web_engine_install::WebEngineModule), "web_engine_install");
    add(&mut registry, Box::new(log_viewer::LogViewerModule), "log_viewer");
    add(&mut registry, Box::new(project_switcher::SwitcherModule), "project_switcher");
    add(&mut registry, Box::new(focus_mode::FocusModule), "focus_mode");
    add(&mut registry, Box::new(running_work::RunningWorkModule), "running_work");
    add(&mut registry, Box::new(terminal_panel::TerminalModule), "terminal_panel");
    add(&mut registry, Box::new(agent_claude::ClaudeModule), "agent_claude");
    add(&mut registry, Box::new(agent_cline::ClineModule), "agent_cline");
    add(&mut registry, Box::new(agent_planned::PlannedModule), "agent_planned");
    add(&mut registry, Box::new(agent_sessions::SessionsModule), "agent_sessions");
    add(&mut registry, Box::new(mcp_server::McpModule), "mcp_server");
    // The agents' graphical conversation (ACP, `agent-chat`) stays in the code, tested, but
    // isn't offered for now (docs/plan/TODO.md): the sessions open in terminals.
    add(&mut registry, Box::new(services_panel::ServicesModule), "services_panel");
    add(&mut registry, Box::new(project_tree::ProjectTreeModule), "project_tree");
    add(&mut registry, Box::new(indexing::IndexingModule), "indexing");
    add(&mut registry, Box::new(refactoring::RefactoringModule), "refactoring");
    add(&mut registry, Box::new(theme_studio::ThemeStudioModule), "theme_studio");
    // Asked last: only files no other viewer shows (and only their start is read).
    add(&mut registry, Box::new(binary_files::BinaryFilesModule), "binary_files");
    // Frameworks: only detectors here; their help is created per project, when used.
    add(&mut registry, Box::new(fw_symfony::SymfonyModule), "fw_symfony");
    add(&mut registry, Box::new(fw_laravel::LaravelModule), "fw_laravel");
    add(&mut registry, Box::new(fw_yii::YiiModule), "fw_yii");
    add(&mut registry, Box::new(fw_htmx::HtmxModule), "fw_htmx");
    add(&mut registry, Box::new(fw_react::ReactModule), "fw_react");
    add(&mut registry, Box::new(fw_vue::VueModule), "fw_vue");
    registry
}

/// Tests only: a test using what the whole process shares (the MCP server, one for the
/// process while each test thread has its own settings) holds it whole; another test's
/// window, applying its own settings as it loads, waits meanwhile (never stopping a server
/// it didn't start).
#[cfg(test)]
pub(crate) static PROCESS_WIDE: std::sync::RwLock<()> = std::sync::RwLock::new(());

#[cfg(test)]
thread_local! {
    /// This test thread holds [`PROCESS_WIDE`] whole (its own windows go on).
    pub(crate) static HOLDS_PROCESS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Tests only: this test holds the process whole until the guard goes (however the test
/// ends): no other test's window applies its own settings (each test thread has its own)
/// to what the process shares — the MCP server, the terminal's configuration — meanwhile.
/// Taken after this test's own windows are made (making one waits for it otherwise).
#[cfg(test)]
pub(crate) fn hold_process() -> ProcessHeld {
    let held = PROCESS_WIDE.write().unwrap_or_else(std::sync::PoisonError::into_inner);
    HOLDS_PROCESS.with(|h| h.set(true));
    ProcessHeld(held)
}

/// See [`hold_process`].
#[cfg(test)]
pub(crate) struct ProcessHeld(#[allow(dead_code)] std::sync::RwLockWriteGuard<'static, ()>);

#[cfg(test)]
impl Drop for ProcessHeld {
    fn drop(&mut self) {
        HOLDS_PROCESS.with(|h| h.set(false));
    }
}

/// Tests only: `f` while no other test holds the process whole (a window's project
/// services polled: one would take the calls the shared MCP server queued for another).
#[cfg(test)]
pub(crate) fn shared<R>(f: impl FnOnce() -> R) -> R {
    if HOLDS_PROCESS.with(std::cell::Cell::get) {
        return f();
    }
    let _shared = PROCESS_WIDE.read().unwrap_or_else(std::sync::PoisonError::into_inner);
    f()
}

/// Activates the selected modules and distributes the contributions.
pub fn load(selection: &ModuleSelection) -> (LanguageSupport, Vec<ModuleReport>) {
    // The tests' folders before any module looks at its own.
    crate::config::isolate_tests();
    // The projects' Node tools (ESLint, Prettier from their node_modules) run on the
    // system's Node, else on the language servers' own.
    code_quality::runtime::use_node(Some(Arc::new(language_servers::node_program)));
    let registry = builtin();
    let (mut contributions, reports) = registry.activate(selection);
    crate::logs::set_active(
        reports.iter().filter(|r| r.state == ModuleState::Active).map(|r| r.id.as_str()),
    );
    for report in &reports {
        match &report.state {
            ModuleState::Active => tracing::debug!(module = %report.id, "module active"),
            ModuleState::Disabled => {
                tracing::info!(module = %report.id, "module disabled in the settings")
            }
            ModuleState::Failed(e) => {
                tracing::error!(module = %report.id, error = %e, "the module didn't start")
            }
        }
    }

    // The hidden modules run, but show nothing of their own: what has a button, a row or
    // a key goes here, decided once for every module (none decides it for itself).
    let hidden: Vec<extensions::ModuleId> =
        reports.iter().filter(|r| r.hidden).map(|r| r.id.clone()).collect();
    let is_hidden = |id: &extensions::ModuleId| hidden.contains(id);
    hide_contributions(&mut contributions, &is_hidden);

    let mut languages = LanguageRegistry::default();
    let mut module_languages = Vec::new();
    for (module, language) in contributions.take_by_module::<LanguageDescriptor>() {
        module_languages.push((module, language.id.clone()));
        languages.register(language);
    }
    let active =
        reports.iter().filter(|r| r.state == ModuleState::Active).map(|r| r.id.clone()).collect();
    let mut grammars = Grammars::default();
    for grammar in contributions.take::<Grammar>() {
        grammars.register(grammar);
    }
    let mut syntax = SyntaxConfig::new(grammars);
    for provider in contributions.take::<Arc<dyn InjectionProvider>>() {
        syntax.add_provider(provider);
    }
    for provider in contributions.take::<Arc<dyn syntax_ts::FoldProvider>>() {
        syntax.add_fold_provider(provider);
    }
    for reader in contributions.take::<Arc<dyn syntax_ts::DocComments>>() {
        syntax.add_doc_comments(reader);
    }
    // What frameworks tell the `.env` module (found there: no module names another).
    for sections in contributions.take::<code_intelligence::env::EnvSections>() {
        code_intelligence::env::register_sections(sections);
    }
    for reads in contributions.take::<code_intelligence::env::EnvReads>() {
        code_intelligence::env::register_reads(reads);
    }
    for order in contributions.take::<code_intelligence::env::EnvFileOrder>() {
        code_intelligence::env::register_order(order);
    }
    // What frameworks say of their calls' results (read by the languages' inference).
    for call in contributions.take::<code_intelligence::calls::ClassArgumentCall>() {
        code_intelligence::calls::register(call);
    }
    for sniffer in contributions.take::<Arc<dyn ContentSniffer>>() {
        syntax.add_sniffer(Arc::clone(&sniffer));
        languages.register_sniffer(sniffer);
    }
    for rule in contributions.take::<FileIconRule>() {
        languages.register_icon_rule(rule);
    }
    // The folders languages and frameworks put in projects (dependencies, generated).
    for dirs in contributions.take::<kernel::ProjectDirs>() {
        kernel::paths::register(dirs);
    }
    for rule in contributions.take::<FolderColorRule>() {
        languages.register_folder_rule(rule);
    }
    // The databases' drivers and the files they open: found by id by whoever opens a
    // source (the explorer never names a driver's module).
    for factory in contributions.take::<database::DriverFactory>() {
        database::drivers::register(factory);
    }
    for files in contributions.take::<database::DataFiles>() {
        database::drivers::register_files(files);
    }
    // The grid's value editors a module brings (a driver's own types): chosen by column
    // over the built-in ones.
    for provider in contributions.take::<Arc<dyn ui_workbench::data_grid::ValueEditorProvider>>() {
        ui_workbench::data_grid::value_editor::register(provider);
    }
    // The data extractors a module brings (a grid's copy and export formats): listed in their
    // group, after the built-in ones.
    for extractor in contributions.take::<Arc<dyn extractors::Extractor>>() {
        extractors::catalog().register(extractor);
    }
    // The servers the languages' modules describe: found there by whoever runs them (no
    // module names another).
    for data in contributions.take::<code_intelligence::servers::ServerData>() {
        code_intelligence::servers::register_data(data);
    }
    // What the modules show in the "Services" window: found there by whoever shows it (the
    // window's module never names a contributor).
    for provider in contributions.take::<Arc<dyn ui_workbench::services::ServiceProvider>>() {
        ui_workbench::services::register(provider);
    }
    // The AI agents on the machine (their modules' providers): found there by whoever
    // needs them (the sessions' panel never names an agent's module).
    for agent in contributions.take::<Arc<dyn agents::AgentProvider>>() {
        agents::register(agent);
    }
    for agent in contributions.take::<agents::PlannedAgent>() {
        agents::register_planned(agent);
    }
    // The modules' own figures for a profile recording: found there by the recorder (it
    // never names a module).
    for source in contributions.take::<Arc<dyn contracts::profiling::ProfileSource>>() {
        contracts::profiling::register(source);
    }
    // The panes the modules offer over a tabular result (a chart): found there by whoever
    // shows rows (it never names a module).
    for pane in contributions.take::<Arc<dyn ui_workbench::result_pane::ResultPaneProvider>>() {
        ui_workbench::result_pane::register(pane);
    }
    // The tools the modules offer the agents (the MCP server's): found there by whoever
    // serves them (the server never names a module).
    for tool in contributions.take::<Arc<dyn mcp::McpTool>>() {
        mcp::register(tool);
    }
    // Which of them are installed, looked for on a worker (the sessions' button shows once
    // one is).
    kernel::work::run("agents-detect", || {
        agents::detect_all();
        workbench_model::app_wide::wake();
    });
    // What the modules show on calm screens (Relax mode's scene), with the app's own mark:
    // found there by whoever shows them (the screen never names a widget's owner).
    crate::screen_mark::register();
    for widget in contributions.take::<Arc<dyn ui_workbench::screen_widgets::ScreenWidget>>() {
        ui_workbench::screen_widgets::register(widget);
    }
    let mut tools = contributions.take::<Arc<dyn ui_workbench::ToolWindowProvider>>();
    // The app's own windows, registered like the modules' (one layout for all): the
    // project panel, and the top bar's search and settings, then — a group of its own —
    // the window kept on top.
    for (id, icon, title_key, order, group, command) in [
        ("search", "search", "tooltip.search", 10, "", crate::commands::SEARCH_PROJECT.0),
        ("settings", "settings", "tooltip.settings", 20, "", crate::commands::OPEN_SETTINGS.0),
        ("keep-on-top", "pin", "tooltip.keep_on_top", 30, "window", crate::commands::KEEP_ON_TOP.0),
    ] {
        tools.push(Arc::new(ui_workbench::ToolWindow {
            id,
            icon,
            title_key,
            dock: ui_workbench::Dock::Top,
            order,
            strip_end: false,
            group,
            toggle: Some(command),
            content: ui_workbench::ToolContent::Command { command, tab: None },
        }));
    }
    tools.sort_by_key(|t| (t.dock(), t.strip_end(), t.order()));
    crate::commands::set_tool_windows(tools.clone());
    let commands = contributions.take::<workbench_model::CommandContribution>();
    let mut context_items = contributions.take::<workbench_model::ContextMenuItem>();
    context_items.extend_from_slice(crate::commands::CONTEXT_ITEMS);
    // A hidden module's blocks aren't shown, but its saved values still reach its domain.
    let mut settings: Vec<Arc<dyn workbench_model::SettingsContribution>> = Vec::new();
    let mut hidden_settings = Vec::new();
    for (module, block) in
        contributions.take_by_module::<Arc<dyn workbench_model::SettingsContribution>>()
    {
        if is_hidden(&module) { hidden_settings.push(block) } else { settings.push(block) }
    }
    // The app's own blocks among them: the caret and code insight (Editor), saving and the
    // final line break (Files), the notifications; each says its page and place.
    settings.push(Arc::new(crate::file_settings::AutosaveSettings));
    settings.push(Arc::new(crate::gpu_settings::GpuSettings));
    settings.push(Arc::new(crate::file_settings::FinalNewlineSettings));
    settings.push(Arc::new(crate::file_settings::SessionSettings));
    settings.push(Arc::new(crate::caret::CaretSettings));
    settings.push(Arc::new(crate::backdrop::BackdropSettings));
    settings.push(Arc::new(crate::floating::FloatingSettings));
    settings.push(Arc::new(crate::scrollbars::ScrollbarSettings));
    settings.push(Arc::new(crate::toasts::ToastSettings));
    settings.push(Arc::new(crate::logs::LogSettings));
    // Whether the window's bar has the menu drawer's button: a choice on macOS only (it
    // has its own menu bar); elsewhere it's the only menu.
    if cfg!(target_os = "macos") {
        settings.push(Arc::new(crate::window_menu::WindowMenuSettings));
    }
    settings.push(Arc::new(crate::insight::InsightSettings));
    settings.push(Arc::new(crate::text_options::TextSettings));
    // The theme in use's own settings: the first of Appearance's blocks (its place says so),
    // right under the themes (none shown for a theme without any).
    settings.push(Arc::new(crate::theme_options::ThemeOptionSettings));
    // Their saved values reach their domains (the local history's retention, the remote
    // transfers' limits) before anything uses them.
    {
        #[cfg(test)]
        let _shared = PROCESS_WIDE.read().unwrap_or_else(std::sync::PoisonError::into_inner);
        for contribution in settings.iter().chain(&hidden_settings) {
            contribution.apply(crate::config::settings());
        }
    }
    let applied: Vec<_> = settings.iter().chain(&hidden_settings).cloned().collect();
    let settings_followed = crate::config::settings().subscribe(move |changed| {
        // A whole section read again (or rewritten); a single key set is applied by whoever
        // set it.
        if changed.key == "*" {
            #[cfg(test)]
            let _shared = PROCESS_WIDE.read().unwrap_or_else(std::sync::PoisonError::into_inner);
            for contribution in &applied {
                contribution.apply(crate::config::settings());
            }
        }
    });
    let listed_items = contributions.take::<workbench_model::ListedMenuItems>();
    crate::commands::set_listed(listed_items.iter().map(|l| (l.id, l.icon)).collect());
    let syntax = Arc::new(syntax);
    crate::commands::set_contributed(commands.iter().map(|c| (c.spec.clone(), c.icon)).collect());
    // The menu bar's sections and submenus the modules bring (their own groups: Git's).
    crate::commands::set_contributed_menus(
        contributions.take::<command_model::MenuSectionSpec>(),
        contributions.take::<command_model::SubmenuSpec>(),
    );
    crate::keymaps::set_places(
        commands
            .iter()
            .map(|c| (c.spec.id, c.handler.when()))
            .filter(|(_, w)| *w != command_model::When::Everywhere)
            .collect(),
    );
    crate::keymaps::set_module_presets(
        contributions.take::<command_model::bindings::PresetShortcuts>(),
    );
    let mut module_threads: Vec<(String, &'static [&'static str])> = contributions
        .take_by_module::<workbench_model::ModuleThreads>()
        .into_iter()
        .map(|(module, threads)| (module.as_str().to_owned(), threads.0))
        .collect();
    // Each module's data read ahead on its own thread (`module_manager::preload_threads`).
    module_threads.extend(reports.iter().filter(|r| r.state == ModuleState::Active).map(|r| {
        (r.id.as_str().to_owned(), crate::module_manager::preload_threads(r.id.as_str()))
    }));
    let module_counters = contributions
        .take_by_module::<workbench_model::ModuleCounters>()
        .into_iter()
        .map(|(module, counters)| (module.as_str().to_owned(), counters))
        .collect();
    let viewers_by_module = contributions.take_by_module::<Arc<dyn ui_workbench::ViewerProvider>>();
    let decorations =
        contributions.take_by_module::<Arc<dyn ui_workbench::decorations::DecorationsProvider>>();
    // The views previewing decorations (the studio) find it here.
    if let Some((_, provider)) = decorations.first() {
        ui_workbench::decorations::register(Arc::clone(provider));
    }
    let viewer_modules =
        viewers_by_module.iter().map(|(m, v)| (v.id(), m.as_str().to_owned())).collect();
    let viewers = order_viewers(viewers_by_module, &is_hidden);
    // How each language's code is colored where there's no parser (canvas, diffs, hovers).
    let intelligence: Vec<Arc<dyn code_intelligence::LanguageIntelligence>> = contributions.take();
    for style in intelligence.iter().filter_map(|i| i.code_style()) {
        code_intelligence::code_style::register(style);
    }
    let mut resource_reporters: Vec<Arc<dyn workbench_model::ResourceReporter>> =
        contributions.take();
    // What an earlier run left running (the app's own: no module's).
    resource_reporters.push(Arc::new(crate::left_behind::Reporter));
    // Whether work runs that quitting would stop (Keep awake's "while working"): what the
    // closing asks about, from the same reporters (thread-safe; quick).
    let reporters = resource_reporters.clone();
    workbench_model::app_wide::set_busy_probe(Arc::new(move || {
        let running = reporters.iter().flat_map(|r| r.running());
        !workbench_model::running::to_ask(running, &workbench_model::CloseScope::quit()).is_empty()
    }));
    let answers = workbench_model::AnswerBook::new(
        contributions.take(),
        std::time::Instant::now,
        |request, took| {
            tracing::warn!(request, ?took, "a slow answer on the UI thread");
        },
    );
    for request in answers.twice() {
        tracing::error!(request, "a request has two answers: the first stays");
    }
    let answers = Arc::new(answers);
    // Who answers what: a button with nobody behind it isn't shown.
    let shown_views = viewers.clone();
    workbench_model::presence::set(
        commands.iter().map(|c| c.spec.id),
        Arc::clone(&answers),
        move |extension| {
            let path = std::path::PathBuf::from(format!("view.{extension}"));
            shown_views.iter().any(|v| v.kind(&path).is_some())
        },
    );
    // Which viewer opens a file of each extension (a module offering "Open in …" beside
    // the default hides it while it is the default).
    let openers = viewers.clone();
    workbench_model::presence::set_opener(move |extension| {
        let path = std::path::PathBuf::from(format!("view.{extension}"));
        openers.iter().find(|v| v.kind(&path).is_some()).map(|v| v.id())
    });
    (
        LanguageSupport {
            module_count: reports.iter().filter(|r| r.state == ModuleState::Active).count(),
            languages,
            syntax: Arc::clone(&syntax),
            icons: contributions.take(),
            icon_packs: contributions.take(),
            file_icon_sets: contributions.take(),
            intelligence,
            answers,
            viewers,
            toolbars: contributions.take(),
            expanders: contributions.take(),
            tools,
            colors: contributions.take(),
            frameworks: contributions.take(),
            assists: contributions.take(),
            concealments: contributions.take(),
            cursor_symbols: contributions.take(),
            servers: contributions.take(),
            modules: reports.clone(),
            vcs: contributions.take(),
            decorators: contributions.take(),
            commands,
            menus: contributions.take(),
            context_items,
            listed_items,
            file_refactorings: contributions.take(),
            implementations: contributions.take(),
            gutter_actions: contributions.take(),
            project_services: contributions.take(),
            context_providers: contributions.take(),
            index_providers: contributions.take(),
            resource_reporters,
            module_threads,
            module_counters,
            viewer_modules,
            idle_actions: contributions.take(),
            status_indicators: contributions.take(),
            system_indicators: contributions.take(),
            top_bar_badges: contributions.take(),
            top_bar_label_actions: contributions.take(),
            region_emphasis: contributions.take(),
            card_previews: contributions.take(),
            settings,
            _settings_followed: settings_followed,
            embeddings: contributions.take(),
            themes: contributions.take(),
            decorations: decorations.iter().map(|(_, d)| Arc::clone(d)).collect(),
            decorations_modules: decorations.iter().map(|(m, _)| m.as_str().to_owned()).collect(),
            quality_tools: contributions.take(),
            text_formatters: contributions.take(),
            signatures: contributions.take(),
            namespaces: contributions.take(),
            manager: Arc::new(crate::module_manager::ModuleManager::new(
                registry,
                active,
                module_languages,
                Arc::clone(&syntax),
            )),
        },
        reports,
    )
}

/// A hidden module's contributions that show something of it, let go: its tool windows'
/// buttons, its commands (the palette, the menus, their shortcuts), its menus and menu rows,
/// its gutter actions, its bars above the editor, its signs (the status bar, the system's,
/// the top bar) and its settings cards' pictures. What it does for others stays: its
/// answers, services, languages, icons, its viewers (asked last: [`order_viewers`]).
pub(crate) fn hide_contributions(
    contributions: &mut extensions::Contributions,
    hidden: &dyn Fn(&extensions::ModuleId) -> bool,
) {
    contributions.drop_from::<Arc<dyn ui_workbench::ToolWindowProvider>>(hidden);
    contributions.drop_from::<Arc<dyn ui_workbench::result_pane::ResultPaneProvider>>(hidden);
    contributions.drop_from::<workbench_model::CommandContribution>(hidden);
    contributions.drop_from::<workbench_model::MenuContribution>(hidden);
    contributions.drop_from::<workbench_model::ContextMenuItem>(hidden);
    contributions.drop_from::<workbench_model::ListedMenuItems>(hidden);
    contributions.drop_from::<command_model::MenuSectionSpec>(hidden);
    contributions.drop_from::<command_model::SubmenuSpec>(hidden);
    contributions.drop_from::<command_model::bindings::PresetShortcuts>(hidden);
    contributions.drop_from::<workbench_model::GutterAction>(hidden);
    contributions.drop_from::<Arc<dyn ui_workbench::editor_toolbar::EditorToolbarProvider>>(hidden);
    contributions.drop_from::<Arc<dyn workbench_model::StatusIndicator>>(hidden);
    contributions.drop_from::<Arc<dyn workbench_model::SystemIndicator>>(hidden);
    contributions.drop_from::<Arc<dyn workbench_model::TopBarBadge>>(hidden);
    contributions.drop_from::<Arc<dyn ui_workbench::CardPreview>>(hidden);
}

/// The viewers in the order they're asked: the shown modules' first (the fallbacks last
/// among them: the binary files' notice), each in the modules' order; then the hidden
/// modules' — never the default for a file another one opens, still there for what only
/// they open.
pub(crate) fn order_viewers(
    by_module: Vec<(extensions::ModuleId, Arc<dyn ui_workbench::ViewerProvider>)>,
    hidden: &dyn Fn(&extensions::ModuleId) -> bool,
) -> Vec<Arc<dyn ui_workbench::ViewerProvider>> {
    let mut viewers: Vec<_> = by_module.into_iter().map(|(m, v)| (hidden(&m), v)).collect();
    // Stable: the modules' order within each.
    viewers.sort_by_key(|(hidden, v)| (*hidden, v.priority()));
    viewers.into_iter().map(|(_, v)| v).collect()
}

/// The modules whose manifest says they start hidden until the user shows them (work put
/// aside for now, its code kept), read once from the registered modules.
fn hidden_by_default_ids() -> &'static [String] {
    static IDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        builtin()
            .manifests()
            .filter(|m| m.info.hidden_by_default)
            .map(|m| m.id.as_str().to_owned())
            .collect()
    })
}

/// Module `id` starts hidden until the user shows it (its manifest says so).
pub fn hidden_by_default(id: &str) -> bool {
    hidden_by_default_ids().iter().any(|h| h == id)
}

/// The module selection from the user settings, `[modules]` (default: all, those whose
/// manifest says so hidden).
pub fn selection_from_settings() -> ModuleSelection {
    use crate::settings::{MODULES_DISABLED, MODULES_ENABLED, MODULES_MODE};
    let settings = crate::config::settings();
    let mut hidden = settings.get(&crate::settings::MODULES_HIDDEN);
    for id in hidden_by_default_ids() {
        if crate::settings::module_hidden(settings, id) && !hidden.iter().any(|h| h == id) {
            hidden.push(id.clone());
        }
    }
    ModuleSelection {
        explicit: settings.get(&MODULES_MODE) == "explicit",
        enabled: settings.get(&MODULES_ENABLED),
        disabled: settings.get(&MODULES_DISABLED),
        hidden,
    }
}

#[cfg(test)]
#[path = "../tests/unit/modules.rs"]
mod tests;
