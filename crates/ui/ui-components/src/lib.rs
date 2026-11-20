//! What modules build their views with: the [`Viewer`] contract (a view in a tab or a tool
//! window), the design system ([`ds`]), the [`Palette`], [`Splitter`]s, tooltips, side
//! panels, tree rows and trees on columns, the color picker, code drawing, the log view, charts, metric cards, bars with "…" and columns fitted by priority, the "Services"
//! window's contribution point. Kept apart from the workbench's own
//! screens (settings, editor, tabs…), which change often: changing those recompiles only the
//! app, not every module.

pub mod anim;
pub mod backdrop;
pub mod card_preview;
pub mod changed_files;
pub mod chart;
pub mod code_tokens;
pub mod color_picker;
pub mod columns;
pub mod control;
pub mod data_grid;
pub mod date_range;
pub mod decorations;
pub mod deferred;
pub mod diagram;
pub mod drag_card;
pub mod ds;
pub mod editor_toolbar;
pub mod form;
pub mod icicle;
pub mod level_picker;
pub mod log_view;
pub mod metric_card;
pub mod multiline;
pub mod overflow;
pub mod painter;
pub mod palette;
pub mod picker;
pub mod progress_ring;
pub mod read_text;
pub mod screen_widgets;
pub mod scroll_state;
pub mod scrollbar;
pub mod services;
pub mod severity;
pub mod shortcuts;
pub mod slider;
pub mod snippets;
pub mod splitter;
pub mod table;
pub mod tag_colors;
pub mod toast;
pub mod tool_panel;
pub mod tool_window;
pub mod tooltip;
pub mod tree;
pub mod tree_table;
pub mod viewer;

pub use anim::Tween;
pub use backdrop::{BackdropRole, BackdropStyle};
pub use card_preview::{CardPaint, CardPreview};
pub use code_tokens::{CodeLine, draw_code};
pub use color_picker::{ColorPick, ColorPicker, PICKER_EXTENSION, PickerAction};
pub use date_range::{Date, DateLocale, DateRangePicker, PickerEvent, Preset};
pub use editor_toolbar::{
    EditorState, EditorToolbar, EditorToolbarProvider, MarkState, StatementMark, ToolbarAction,
    ToolbarContext, ToolbarPaint,
};
pub use icicle::{Icicle, IcicleCell, IcicleEvent, IcicleLabels, IcicleNode, IciclePaint};
pub use level_picker::{LEVEL_PICKER_EXTENSION, LevelChoices, LevelClick, LevelPick, LevelPicker};
pub use log_view::{Log, LogKind, LogLine, LogView};
pub use metric_card::{CardAreas, CardForm, CardSeries, MetricCard, metric_card};
pub use multiline::{MultilineField, MultilineSpec};
pub use painter::{LayerPainter, Painter, ScenePainter};
pub use palette::{Palette, SyntaxColors};
pub use read_text::{Block, BlockKind, ReadText, TextPos};
pub use scroll_state::ScrollState;
pub use scrollbar::{
    AutoHide, EdgeScrollbar, ScrollMetrics, Scrollbar, ScrollbarHover, ScrollbarLook,
    ScrollbarStyle, ScrollbarVisibility,
};
pub use services::{
    ProjectServices, ServiceId, ServiceNode, ServiceProvider, ServiceState, ServiceTab,
    ServicesContext,
};
pub use severity::Level;
pub use slider::{SliderArea, SliderSpec, SliderStyle, slider};
pub use splitter::{Orientation, Splitter, SplitterStyle};
pub use toast::{StackPlace, ToastHit, ToastStack};
pub use tool_panel::{
    KeyReach, PanelButton, PanelContext, PanelMemento, Problem, ToolPanel, ToolPanelAction, Usage,
};
pub use tool_window::{Dock, ToolContent, ToolWindow, ToolWindowProvider};
pub use tooltip::{Tip, Tooltips};
pub use tree::TreeList;
pub use tree_table::{
    Align, CellValue, Ink, Span, SpanStyle, TableCell, TableColumn, TableEvent, TablePaint,
    TableRow, TreeTable,
};
pub use viewer::{
    AppRequest, Asked, KeyPress, ModalButton, ModalStatus, OpenView, PressMods, PressedKey,
    Restored, RowLook, ViewMemory, ViewMenuEntry, Viewer, ViewerAction, ViewerCursor, ViewerKey,
    ViewerKind, ViewerPaint, ViewerPriority, ViewerProvider, WebEvent, WebPage, WebSource,
    WithHost,
};
