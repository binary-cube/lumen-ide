//! A result's rows lent to a pane over it (`ui_components::result_pane`): the runner's chunks
//! shared as they are ([`database::lent::Lent`]), under the grid's columns.

use ui_components::data_grid::GridColumn;

use crate::results::Row;

/// A result's rows as of one moment.
pub type LentRows = database::lent::Lent<Row>;

/// `view`'s rows under the grid's `columns` (their kinds read from their classes).
pub fn lent(view: &crate::runner::ResultView, columns: &[GridColumn]) -> LentRows {
    LentRows::new(
        columns.iter().map(GridColumn::chart_column).collect(),
        view.chunks.clone(),
        view.starts.clone(),
    )
}
