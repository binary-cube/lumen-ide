//! The catalog read for the explorer and the editors: tables, a table's structure (its
//! columns, keys, indexes), the tree's nodes.

use database::postgres::{self as pg, Postgres as PgDialect};
use database::{
    ColumnInfo, DbError, Dialect, ForeignKey, Generated, IndexInfo, Node, Structure, TableInfo,
    TableKind, TableRef,
};

use crate::{PgSession, catalog, error};

impl PgSession {
    pub(crate) fn dialect(&self) -> PgDialect {
        PgDialect::new(self.version)
    }

    /// A catalog query's rows, each value as text (none: `None`), `params` text too.
    pub(crate) fn catalog(
        &self,
        query: &str,
        params: &[&str],
    ) -> Result<Vec<Vec<Option<String>>>, DbError> {
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let rows = self.runtime.block_on(self.client.query(query, &params)).map_err(error)?;
        Ok(rows
            .iter()
            .map(|row| (0..row.len()).map(|i| row.get::<_, Option<String>>(i)).collect())
            .collect())
    }

    /// The tables and views of every user schema, or of `schema` alone.
    pub(crate) fn list_tables(&self, schema: Option<&str>) -> Result<Vec<TableInfo>, DbError> {
        let sql = catalog::tables(self.version, schema.is_some());
        let params: Vec<&str> = schema.into_iter().collect();
        Ok(self
            .catalog(&sql, &params)?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                let kind = match text(2).as_str() {
                    "v" | "m" => TableKind::View,
                    _ => TableKind::Table,
                };
                let tuples: f64 = text(3).parse().unwrap_or(-1.0);
                let pages: i64 = text(4).parse().unwrap_or(0);
                // Never analyzed: the statistics collector's live rows (a table's, a
                // materialized view's), as cheap.
                let live = matches!(text(2).as_str(), "r" | "m")
                    .then(|| text(5).parse::<u64>().ok())
                    .flatten();
                TableInfo {
                    table: TableRef { schema: Some(text(0)), name: text(1) },
                    kind,
                    rows: pg::estimated_rows(self.version, tuples, pages).or(live),
                }
            })
            .collect())
    }

    /// `table`'s structure from the catalog (as its version keeps it).
    pub(crate) fn read_structure(&self, table: &TableRef) -> Result<Structure, DbError> {
        let name = self.dialect().table_name(table)?;
        let text = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
        let mut columns = Vec::new();
        let mut numbers = Vec::new();
        for r in self.catalog(&catalog::columns(self.version), &[&name])? {
            let declared = text(&r, 1).unwrap_or_default();
            let mut column = ColumnInfo::new(text(&r, 0).unwrap_or_default(), declared);
            column.nullable = text(&r, 2).as_deref() != Some("true");
            let default = text(&r, 3);
            let identity = text(&r, 4).unwrap_or_default();
            let generated = text(&r, 5).unwrap_or_default();
            if generated == "s" || generated == "v" {
                column.generated = Some(Generated {
                    expression: default.clone().unwrap_or_default(),
                    stored: generated == "s",
                });
            } else {
                column.auto_increment = matches!(identity.as_str(), "a" | "d")
                    || default.as_deref().is_some_and(|d| d.starts_with("nextval("));
                column.default = default;
            }
            column.collation = text(&r, 6);
            numbers.push(text(&r, 7).and_then(|n| n.parse::<i64>().ok()).unwrap_or(0));
            column.values = text(&r, 8).map(|labels| catalog::enum_labels(&labels));
            columns.push(column);
        }
        if columns.is_empty() {
            return Err(DbError::Query(format!("no table {table}")));
        }
        // The indexes and their columns, in order.
        let mut keyed: Vec<(String, Vec<String>)> = Vec::new();
        for r in self.catalog(&catalog::index_columns(self.version), &[&name])? {
            let index = text(&r, 0).unwrap_or_default();
            let column = catalog::unquote(&text(&r, 2).unwrap_or_default());
            match keyed.iter_mut().find(|(n, _)| *n == index) {
                Some((_, list)) => list.push(column),
                None => keyed.push((index, vec![column])),
            }
        }
        let mut indexes = Vec::new();
        for r in self.catalog(&catalog::indexes(), &[&name])? {
            let index = text(&r, 0).unwrap_or_default();
            let cols = keyed.iter().find(|(n, _)| *n == index).map(|(_, c)| c.clone());
            let cols = cols.unwrap_or_default();
            let primary = text(&r, 2).as_deref() == Some("true");
            if primary {
                for (place, key) in cols.iter().enumerate() {
                    if let Some(c) = columns.iter_mut().find(|c| c.name == *key) {
                        c.in_key = true;
                        c.key_position = u16::try_from(place + 1).ok();
                    }
                }
            }
            let options = catalog::numbers(&text(&r, 5).unwrap_or_default());
            let descending =
                (0..cols.len()).map(|i| options.get(i).is_some_and(|o| o & 1 == 1)).collect();
            indexes.push(IndexInfo {
                name: index,
                columns: cols,
                unique: text(&r, 1).as_deref() == Some("true"),
                implicit: primary || text(&r, 3).is_some_and(|n| n != "0"),
                descending,
                predicate: text(&r, 4),
            });
        }
        let mut foreign_keys = Vec::new();
        for r in self.catalog(&catalog::foreign_keys(), &[&name])? {
            let here = catalog::numbers(&text(&r, 5).unwrap_or_default());
            let there = catalog::numbers(&text(&r, 6).unwrap_or_default());
            let target_oid = text(&r, 7).unwrap_or_default();
            let named: Vec<(i64, String)> = self
                .catalog(&catalog::column_names(), &[&target_oid])?
                .into_iter()
                .filter_map(|c| Some((text(&c, 0)?.parse().ok()?, text(&c, 1)?)))
                .collect();
            let local =
                |n: &i64| numbers.iter().position(|x| x == n).map(|i| columns[i].name.clone());
            let remote = |n: &i64| named.iter().find(|(x, _)| x == n).map(|(_, c)| c.clone());
            foreign_keys.push(ForeignKey {
                name: text(&r, 0),
                columns: here.iter().filter_map(local).collect(),
                table: TableRef { schema: text(&r, 3), name: text(&r, 4).unwrap_or_default() },
                references: there.iter().filter_map(remote).collect(),
                on_update: catalog::action(&text(&r, 1).unwrap_or_default()),
                on_delete: catalog::action(&text(&r, 2).unwrap_or_default()),
            });
        }
        Ok(Structure {
            table: table.clone(),
            columns,
            indexes,
            foreign_keys,
            rowid: false,
            lines: false,
        })
    }

    /// A query's rows as strings (the explorer's lists).
    pub(crate) fn rows(&self, query: &str, params: &[&str]) -> Result<Vec<Vec<String>>, DbError> {
        let params: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            params.iter().map(|p| p as &(dyn tokio_postgres::types::ToSql + Sync)).collect();
        let rows = self.runtime.block_on(self.client.query(query, &params)).map_err(error)?;
        Ok(rows
            .iter()
            .map(|row| {
                (0..row.len())
                    .map(|i| row.get::<_, Option<String>>(i).unwrap_or_default())
                    .collect()
            })
            .collect())
    }

    pub(crate) fn node(
        kind: &'static str,
        name: &str,
        detail: String,
        parent: &[String],
        expandable: bool,
    ) -> Node {
        let mut path = parent.to_vec();
        path.push(name.to_owned());
        Node { kind, name: name.to_owned(), detail, path, expandable }
    }
}
