//! The data grid: one component for every tabular listing (a database table, a CSV file,
//! later a query's result), reading its rows through a [`GridSource`] it knows nothing
//! else about. It has: pages of rows (`|< < 1–500 of N > >|`,
//! the page's size chosen, or all rows scrolled), sorting by a header click (⇧: more
//! columns, their order numbered), the `WHERE` / `ORDER BY` fields and quick filters from a
//! cell's menu, columns resized, moved, hidden and fitted to their content, row numbers,
//! `NULL` drawn apart, cells / rows / columns selected with the mouse or the keys, ⌘F in
//! the rows, a row shown transposed (column – value), the selection copied through the
//! project's data extractor (chosen in the toolbar's pill: SQL `INSERT`s / `UPDATE`s / a
//! `WHERE` clause, CSV, TSV, JSON, Markdown, HTML, XML… — [`copy`], [`extractors`]), and what
//! the selected numbers add up to.
//!
//! Only what's in view is drawn and asked for; a cell's text is shaped once (the text
//! system keeps it) and cut to its column, nothing is worked out per frame that didn't
//! change. The pure parts are apart and tested: [`filter`], [`paging`], [`selection`],
//! [`layout`], [`copy`], [`aggregate`].

pub mod aggregate;
pub mod copy;
pub mod edit;
pub mod export;
pub mod filter;
mod grid;
pub mod layout;
pub mod paging;
mod paint;
pub mod selection;
pub mod source;
pub mod text_area;
pub mod toolbar;
pub mod value_editor;
pub mod xlsx;

pub use aggregate::Aggregates;
pub use copy::{CopyStream, Extractor, cell_of, column_of, copy_text};
pub use edit::{edit_text, parse_value};
pub use export::ExportFormat;
pub use filter::{Compare, GridFilter, ParseError, cycle_sort, parse_order, parse_where};
pub use grid::{
    DataGrid, GridAction, GridPaint, MOST_PICKED_ROWS, Picked, ToolCommands, export_menu,
};
pub use layout::{ColumnLayout, SavedLayout};
pub use paging::{PAGE_SIZES, Paging};
pub use paint::grouped;
pub use selection::{CellPos, Selection};
pub use source::{
    Align, CellClass, CellMark, CellRef, CellValue, Editing, GridCaps, GridColumn, GridQuery,
    GridSource, NEW_ROWS, RowState, SortKey, SourceStatus, Total, Whole,
};
pub use toolbar::Tool;
pub use value_editor::{ValueEditorProvider, ValueEditorSpec, ValueFormat};

/// The prefix of the grid's menu rows' ids (what a view hands back to
/// [`DataGrid::command`]).
pub const MENU_PREFIX: &str = "grid:";

/// The toolbar's height over the rows (logical pixels): what a view beside the grid lines
/// its own bands up with.
pub const TOOLBAR_HEIGHT: f32 = grid::TOOLBAR_H;

#[cfg(test)]
#[path = "../../tests/unit/data_grid.rs"]
mod tests;
