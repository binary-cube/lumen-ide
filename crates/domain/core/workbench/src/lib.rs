//! Workbench bounded context: UI state (layout, panels, tabs, focus), without rendering.

pub mod answers;
pub mod app_wide;
pub mod concealment;
pub mod context;
pub mod cursor_symbols;
pub mod decorations;
pub mod diagnostics;
pub mod embeddings;
pub mod emphasis;
pub mod formatting;
pub mod framework_names;
pub mod host;
pub mod idle;
pub mod indexing;
pub mod menus;
pub mod notifications;
pub mod per_project;
pub mod pictures;
pub mod presence;
pub mod queries;
pub mod resources;
pub mod running;
pub mod services;
pub mod session;
pub mod settings_page;
pub mod sizes;
pub mod status;
pub mod windows;

pub use answers::{AnswerBook, Answers};
/// What a language offers the editor (a menu row's [`MenuWhen::Provides`]).
pub use code_intelligence::LanguageFeature;
pub use concealment::Concealment;
pub use context::{ActionContext, ContextProvider, Target};
pub use cursor_symbols::{CursorContext, CursorSymbols};
pub use decorations::{
    DecoratorContext, DocumentDecorations, LineAnnotation, LineDecorator, LineMark, MarkKind,
    ProjectDecorations, ShownDocument,
};
pub use diagnostics::{
    Diagnostic, DiagnosticsFound, DiagnosticsScope, ProjectDiagnostics, Severity,
};
pub use embeddings::EmbeddingSource;
pub use emphasis::{
    EmphasisRequest, Follow, Region, RegionEmphasis, RegionFocus, RegionKind, region_at,
};
pub use formatting::{FormatOutcome, format_active};
pub use host::{
    ActiveFile, BadgeKind, CHOOSE_ENGINE, CLOSE_TAB, COMMENT_LINE, ChoiceItem, ChoiceList,
    CommandContribution, CommandHandler, CommandImplementation, Completion, EditorHost,
    FIND_USAGES, FileRefactoring, FixedTab, GOTO_DECLARATION, GutterAction, GutterMark,
    IMPLEMENTED, Job, OPEN_REMOTE_PROJECT, OnChoice, OnYes, PROJECT_PANEL, Progress, Question,
    REFORMAT_FILE, RENAME_SYMBOL, RefactoringHost, Requiring, SEARCH_PROJECT, SERVICES_TOOL,
    SHOW_RESOURCES, SHOW_TYPE_DIAGRAM, ServicesHost, SystemHost, THEME_STUDIO,
    TOGGLE_PROJECT_PANEL, TOGGLE_SERVICES, TabInfo, ToggleTool, ToolBadge, ViewAction, ViewHost,
    WorkbenchHost, open_fixed_tabs,
};
pub use idle::{IdleAction, IdleWatch};
pub use indexing::{IndexContext, IndexProvider, ProjectIndex};
pub use menus::{
    Arranged, ContextMenuItem, EVERYWHERE, ListedMenuItems, ListedRow, MenuContribution, MenuList,
    MenuPlace, MenuRow, MenuSection, MenuWhen, arrange,
};
pub use notifications::{
    NoNotifier, Notification, NotificationAction, NotificationId, NotificationKind, Notifications,
    Notifier, Recent, Timeout, Toast, ToastPosition,
};
pub use per_project::PerProject;
pub use queries::{EntriesRequest, ProjectQueries, ProjectView};
pub use resources::{
    ActOutcome, ActionKind, ActionTarget, Activities, Activity, ActivityKind, ActivityUnit,
    CpuMeter, DiskIo, ModuleCounters, ModuleThreads, OpenProject, Outcome, PartAction, PartRef,
    ResourceData, ResourceReporter, ResourceSnapshot, ResourceSource, ResourceUse,
};
pub use running::{CloseAnswer, CloseScope, RunningPrompt, RunningWork, StopImpact, StopVia};
pub use services::{LanguageOf, ProjectContext, ProjectEdit, ProjectKind, ProjectService};
pub use session::{
    Current, OpenGroup, OpenTab, Reopen, SavedGroup, SavedTab, SavedToolWindow, SavedView, Session,
    SessionKeeper, SessionStore,
};
pub use settings_page::{
    SettingsBlock, SettingsCard, SettingsContribution, SettingsGroup, SettingsLanguage,
    SettingsPage, SettingsRow, SettingsScope, StatusTone, merge_languages,
};
pub use sizes::LayoutSizes;
pub use status::{Indicator, StatusIndicator, SystemIndicator, SystemSign};
pub use windows::{ProjectWindow, TopBadge, TopBarBadge};
