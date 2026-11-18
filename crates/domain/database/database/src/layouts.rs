//! How a table's grid was left (its columns' widths, order and hidden ones, its order, its
//! filter, the page size chosen), kept per data source and table beyond the project's
//! session: the grid comes back as it was when the table opens again. A [`LayoutStore`]
//! keeps them (the project's own folder, written by an infra adapter); this is only the
//! model and the rules: what's kept, the newest first, at most [`KEPT`].

use std::io;

use crate::{Order, SchemaChange, TableRef};

/// The layouts kept per project (the oldest forgotten past it).
pub const KEPT: usize = 400;

/// A table's grid as it was left. Columns are named: those the table no longer has are
/// ignored when it's put back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    /// Logical pixels, by column (only those resized or measured).
    pub widths: Vec<(String, f32)>,
    /// The columns' order (names); the others come after.
    pub order: Vec<String>,
    pub hidden: Vec<String>,
    /// The rows' order chosen.
    pub sort: Vec<Order>,
    /// The `WHERE` field's text (read again against the columns: one not understood any
    /// more is dropped).
    pub filter: Option<String>,
    /// The page size chosen in the grid (`Some(None)`: every row); `None`: the setting's.
    pub page: Option<Option<u32>>,
}

impl GridLayout {
    /// The layout following `change` of its table: a column renamed keeps its width, place,
    /// hiding and order under its new name (the filter names it so too); a column dropped
    /// leaves them (and a filter naming it goes).
    pub fn follow(&mut self, change: &SchemaChange) {
        let rename = |names: &mut Vec<String>, from: &str, to: &str| {
            for name in names.iter_mut().filter(|n| *n == from) {
                *name = to.to_owned();
            }
        };
        match change {
            SchemaChange::RenameColumn { from, to, .. } => self.rename(from, to, rename),
            SchemaChange::AlterColumn { column, to, .. } if to.name != *column => {
                self.rename(column, &to.name, rename);
            }
            SchemaChange::DropColumn { column, .. } => {
                self.widths.retain(|(c, _)| c != column);
                self.order.retain(|c| c != column);
                self.hidden.retain(|c| c != column);
                self.sort.retain(|o| o.column != *column);
                if self.filter.as_deref().is_some_and(|f| crate::sqlite::names(f, column)) {
                    self.filter = None;
                }
            }
            _ => {}
        }
    }

    fn rename(&mut self, from: &str, to: &str, rename: impl Fn(&mut Vec<String>, &str, &str)) {
        for (c, _) in &mut self.widths {
            if c == from {
                *c = to.to_owned();
            }
        }
        rename(&mut self.order, from, to);
        rename(&mut self.hidden, from, to);
        for o in &mut self.sort {
            if o.column == from {
                o.column = to.to_owned();
            }
        }
        if let Some(filter) = &mut self.filter {
            *filter = crate::sqlite::rename_name(filter, from, to);
        }
    }
}

/// The layouts of `source` following `changes`: a table renamed keeps its layout, a table
/// dropped loses it, a table's columns renamed or dropped are followed
/// ([`GridLayout::follow`]). Whether any changed.
pub fn follow(
    layouts: &mut Vec<(LayoutKey, GridLayout)>,
    source: &str,
    changes: &[SchemaChange],
) -> bool {
    let before = layouts.clone();
    for change in changes {
        let mine = |k: &LayoutKey| k.source == source && k.table == *change.table();
        match change {
            SchemaChange::DropTable { .. } => layouts.retain(|(k, _)| !mine(k)),
            SchemaChange::RenameTable { table, to } => {
                let renamed = TableRef { schema: table.schema.clone(), name: to.clone() };
                // A layout left by a table of that name before goes: this one is it now.
                layouts.retain(|(k, _)| !(k.source == source && k.table == renamed));
                for (k, _) in layouts.iter_mut().filter(|(k, _)| mine(k)) {
                    k.table = renamed.clone();
                }
            }
            change => {
                for (_, layout) in layouts.iter_mut().filter(|(k, _)| mine(k)) {
                    layout.follow(change);
                }
            }
        }
    }
    *layouts != before
}

/// Whose layout: a data source (a file: its path relative to the project's root, `/`
/// between folders; a configured source: its id) and one of its tables.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LayoutKey {
    pub source: String,
    pub table: TableRef,
}

/// Where a project's layouts are kept (an adapter; the newest last).
pub trait LayoutStore: Send + Sync {
    fn load(&self) -> Vec<(LayoutKey, GridLayout)>;
    fn save(&self, layouts: &[(LayoutKey, GridLayout)]) -> io::Result<()>;
}

/// The layouts, newest last: `key`'s replaced by `layout` (moved to the end), the oldest
/// forgotten past [`KEPT`].
pub fn keep(layouts: &mut Vec<(LayoutKey, GridLayout)>, key: LayoutKey, layout: GridLayout) {
    layouts.retain(|(k, _)| *k != key);
    layouts.push((key, layout));
    if layouts.len() > KEPT {
        let extra = layouts.len() - KEPT;
        layouts.drain(..extra);
    }
}

/// A file's source as a layout names it: its path relative to `root` with `/`, or `None`
/// when it's outside (nothing kept for it).
pub fn file_source(root: &std::path::Path, file: &std::path::Path) -> Option<String> {
    let relative = file.strip_prefix(root).ok()?;
    let parts: Vec<String> =
        relative.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

#[cfg(test)]
#[path = "../tests/unit/layouts.rs"]
mod tests;
