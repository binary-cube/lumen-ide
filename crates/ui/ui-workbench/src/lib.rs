//! The workbench views: editor, tabs, search bar, status bar, project tree.
//!
//! Colors come from [`Palette`] until themes are introduced (phase 3).

// What modules build their views with lives in `ui-components` (it changes rarely; these
// screens often): re-exported, the workbench's paths stay.
pub use ui_components::Painter;
pub use ui_components::backdrop::{self, BackdropStyle};
pub use ui_components::card_preview::{self, CardPreview};
pub use ui_components::charts;
pub use ui_components::control;
pub use ui_components::data_grid;
pub use ui_components::decorations;
pub use ui_components::ds;
pub use ui_components::editor_toolbar;
pub use ui_components::picker;
pub use ui_components::result_pane;
pub use ui_components::screen_widgets;
pub use ui_components::services;
pub use ui_components::severity;
pub use ui_components::shortcuts;
pub use ui_components::snippets;
pub use ui_components::tab_strip;
pub use ui_components::text_options::{self, TextOptions};
pub use ui_components::toast;
pub(crate) use ui_components::{
    anim, code_tokens, color_picker, palette, splitter, tool_panel, tooltip, viewer,
};

mod about;
mod breadcrumbs;
mod choice_menu;
mod code_hover;
mod completion_popup;
mod context_menu;
mod debug_overlay;
mod dialog;
mod editor_view;
mod find_bar;
mod folding;
mod menu_drawer;
mod menu_nav;
mod modal;
mod panel_header;
mod param_hint;
mod projects_picker;
mod settings_view;
mod shortcuts_editor;
mod splash;
mod status_bar;
pub mod status_popover;
pub use status_popover::{PopoverLook, PopoverSpec};
mod tab_bar;
mod tool_strip;
mod top_bar;

pub use about::AboutView;
pub use anim::Tween;
pub use breadcrumbs::{Breadcrumbs, Crumb};
pub use choice_menu::{Choice, ChoiceAction, ChoiceMenu};
pub use code_hover::{CardParam, CodeHover, HoverCard};
pub use code_tokens::draw_code;
pub use color_picker::{ColorPick, ColorPicker, PICKER_EXTENSION, PickerAction};
pub use completion_popup::CompletionPopup;
pub use context_menu::{ContextAction, ContextMenu, MenuEntry, tidy_menu};
pub use debug_overlay::DebugOverlay;
pub use dialog::{ButtonKind, Dialog, DialogButton, DialogCheck, DialogSpec, LIST_ROWS};
pub use editor_view::{
    Bulb, ColorSpan, Decorations, EditorView, ErrorPreviewSpec, ErrorTip, Inlay, LineLens, RowMove,
    StatementLines,
};
pub use find_bar::{FindBar, FindBarSpec, FindField, FindHit};
pub use folding::Folds;
pub use menu_drawer::{DrawerAction, MenuDrawer};
pub use menu_nav::{
    AIM_DELAY, DrawerMenu, DrawerNav, Found, HoverIntent, Intent, NavFocus, NavKey, NavOutcome,
    SWITCH_DELAY, aims_at,
};
pub use modal::{Modal, ModalHit, ModalSection, ModalSize, ModalSpec, ZoomStep};
pub use palette::{Palette, SyntaxColors};
pub use panel_header::{PanelAction, PanelHeader};
pub use param_hint::{ParamHint, ParamHintCard};
pub use projects_picker::{
    HomeContent, HomeFile, HomeSession, HomeSessionWindow, PickerAction as ProjectsAction,
    PickerTarget as ProjectsTarget, ProjectsPicker, RecentList, RecentProject,
};
pub use settings_view::{
    ContributedBlock, FileIconChoice, FrameworkRow, IconChoice, ModuleRow, SettingsAction,
    SettingsModel, SettingsView, ThemeOption, ThemePreview, order_blocks,
};
pub use shortcuts_editor::{BindingRow, ShortcutChanges};
pub use splash::{Splash, SplashFrame};
pub use splitter::{Orientation, Splitter, SplitterStyle};
pub use status_bar::{MORE as STATUS_MORE, Priority, StatusBar, StatusItem};
pub use tab_bar::{Navigation, TabBar, TabHit, TabTitle};
pub use tool_panel::{
    KeyReach, PanelButton, PanelContext, PanelMemento, Problem, ToolPanel, ToolPanelAction, Usage,
};
pub use tool_strip::{ToolButton, ToolStrip, ToolStripStyle};
pub use tooltip::{Tip, Tooltips};
pub use top_bar::{ProjectTint, TopBar, TopBarAction, TopBarHit, TopBarModel, project_initials};
pub use ui_components::scrollbar::{
    self as scrollbar, ScrollMetrics, Scrollbar, ScrollbarHover, ScrollbarLook, ScrollbarStyle,
    ScrollbarVisibility,
};
pub use ui_components::{Dock, ToolContent, ToolWindow, ToolWindowProvider};
pub use ui_components::{LEVEL_PICKER_EXTENSION, LevelChoices, LevelPick};
pub use viewer::{
    AppRequest, KeyPress, ModalButton, ModalStatus, PressMods, PressedKey, Restored, RowLook,
    ViewMemory, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey, ViewerKind,
    ViewerPaint, ViewerPriority, ViewerProvider, WebEvent, WebPage, WebSource,
};
