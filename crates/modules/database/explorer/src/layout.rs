//! A grid's layout as the grid keeps it ([`SavedLayout`]) and as the project keeps it
//! ([`GridLayout`], the domain's): the same, column by column.

use database::Order;
use database::layouts::GridLayout;
use ui_components::data_grid::{SavedLayout, SortKey};

/// The grid's layout, to keep.
pub fn from_grid(saved: &SavedLayout) -> GridLayout {
    GridLayout {
        widths: saved.widths.clone(),
        order: saved.order.clone(),
        hidden: saved.hidden.clone(),
        sort: saved
            .sort
            .iter()
            .map(|k| Order { column: k.column.clone(), descending: k.descending })
            .collect(),
        filter: (!saved.filter.trim().is_empty()).then(|| saved.filter.clone()),
        page: saved.page,
    }
}

/// A kept layout, for the grid.
pub fn to_grid(kept: GridLayout) -> SavedLayout {
    SavedLayout {
        widths: kept.widths,
        order: kept.order,
        hidden: kept.hidden,
        sort: kept
            .sort
            .into_iter()
            .map(|o| SortKey { column: o.column, descending: o.descending })
            .collect(),
        filter: kept.filter.unwrap_or_default(),
        page: kept.page,
    }
}
