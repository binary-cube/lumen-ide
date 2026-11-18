//! `<project dir>/data-grids.toml`: the TOML adapter for [`LayoutStore`] — how each
//! table's grid was left (the newest last).
//!
//! ```toml
//! [[grid]]
//! source = "data/shop.sqlite"
//! schema = "main"
//! table = "orders"
//! order = ["id", "total", "paid"]
//! hidden = ["notes"]
//! sort = [{ column = "total", descending = true }]
//! filter = "paid = 1"
//! page = 100            # or "all"
//! widths = { id = 64.0, total = 120.0 }
//! ```

use std::io;
use std::path::{Path, PathBuf};

use database::layouts::{GridLayout, LayoutKey, LayoutStore};
use database::{Order, TableRef};
use toml::Value;

use crate::write_atomic;

/// The project's grid layouts file.
#[derive(Debug, Clone)]
pub struct TomlGridLayouts {
    file: PathBuf,
}

impl TomlGridLayouts {
    /// The layouts of the project at `root`.
    pub fn new(root: &Path) -> Self {
        Self { file: root.join(kernel::brand::PROJECT_DIR).join("data-grids.toml") }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

fn entry(table: &toml::Table) -> Option<(LayoutKey, GridLayout)> {
    let source = table.get("source")?.as_str()?.to_owned();
    let name = table.get("table")?.as_str()?.to_owned();
    let schema = table.get("schema").and_then(Value::as_str).map(str::to_owned);
    let widths = table
        .get("widths")
        .and_then(Value::as_table)
        .map(|w| {
            w.iter()
                .filter_map(|(name, v)| {
                    let width = v.as_float().or_else(|| v.as_integer().map(|i| i as f64))?;
                    Some((name.clone(), width as f32))
                })
                .collect()
        })
        .unwrap_or_default();
    let sort = table
        .get("sort")
        .and_then(Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(|k| {
                    let k = k.as_table()?;
                    Some(Order {
                        column: k.get("column")?.as_str()?.to_owned(),
                        descending: k.get("descending").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let page = match table.get("page") {
        Some(Value::Integer(n)) => u32::try_from(*n).ok().map(Some),
        Some(Value::String(s)) if s == "all" => Some(None),
        _ => None,
    };
    let layout = GridLayout {
        widths,
        order: strings(table.get("order")),
        hidden: strings(table.get("hidden")),
        sort,
        filter: table.get("filter").and_then(Value::as_str).map(str::to_owned),
        page,
    };
    Some((LayoutKey { source, table: TableRef { schema, name } }, layout))
}

fn table_of(key: &LayoutKey, layout: &GridLayout) -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("source".into(), Value::String(key.source.clone()));
    if let Some(schema) = &key.table.schema {
        t.insert("schema".into(), Value::String(schema.clone()));
    }
    t.insert("table".into(), Value::String(key.table.name.clone()));
    let list = |names: &[String]| Value::Array(names.iter().cloned().map(Value::String).collect());
    if !layout.order.is_empty() {
        t.insert("order".into(), list(&layout.order));
    }
    if !layout.hidden.is_empty() {
        t.insert("hidden".into(), list(&layout.hidden));
    }
    if !layout.sort.is_empty() {
        let keys = layout
            .sort
            .iter()
            .map(|o| {
                let mut k = toml::Table::new();
                k.insert("column".into(), Value::String(o.column.clone()));
                k.insert("descending".into(), Value::Boolean(o.descending));
                Value::Table(k)
            })
            .collect();
        t.insert("sort".into(), Value::Array(keys));
    }
    if let Some(filter) = layout.filter.as_ref().filter(|f| !f.trim().is_empty()) {
        t.insert("filter".into(), Value::String(filter.clone()));
    }
    match layout.page {
        Some(Some(n)) => {
            t.insert("page".into(), Value::Integer(i64::from(n)));
        }
        Some(None) => {
            t.insert("page".into(), Value::String("all".into()));
        }
        None => {}
    }
    if !layout.widths.is_empty() {
        let widths = layout
            .widths
            .iter()
            .map(|(name, w)| (name.clone(), Value::Float(f64::from(w.round()))))
            .collect();
        t.insert("widths".into(), Value::Table(widths));
    }
    t
}

impl LayoutStore for TomlGridLayouts {
    fn load(&self) -> Vec<(LayoutKey, GridLayout)> {
        let Ok(text) = std::fs::read_to_string(&self.file) else { return Vec::new() };
        let table: toml::Table = match text.parse() {
            Ok(table) => table,
            Err(e) => {
                tracing::warn!("{} is invalid: {e}", self.file.display());
                return Vec::new();
            }
        };
        table
            .get("grid")
            .and_then(Value::as_array)
            .map(|grids| grids.iter().filter_map(|g| entry(g.as_table()?)).collect())
            .unwrap_or_default()
    }

    fn save(&self, layouts: &[(LayoutKey, GridLayout)]) -> io::Result<()> {
        let grids = layouts.iter().map(|(k, l)| Value::Table(table_of(k, l))).collect();
        let mut root = toml::Table::new();
        root.insert("grid".into(), Value::Array(grids));
        write_atomic(&self.file, &root.to_string())
    }
}

#[cfg(test)]
#[path = "../tests/unit/grid_layouts.rs"]
mod tests;
