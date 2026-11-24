//! The catalog read for the explorer and the editors: tables, a table's structure and
//! keys, the tree's nodes; the session's own small statements and warnings.

use std::time::Instant;

use database::mysql::MySql as MyDialect;
use database::{
    Cancel, ColumnInfo, DbError, ForeignKey, Generated, IndexInfo, Node, Notice, Outcome, RowSink,
    Structure, TableInfo, TableKind, TableRef,
};
use mysql_async::consts::StatusFlags;
use mysql_async::prelude::Queryable;

use crate::rows::{stream, text_of};
use crate::{MySession, OnStop, TextRows, WARNINGS, catalog, console, error_in};

impl MySession {
    pub(crate) fn dialect(&self) -> MyDialect {
        MyDialect::new(self.version)
    }

    /// `query` streamed to `sink` (see [`stream`]), the transaction's state then read.
    pub(crate) fn stream(
        &mut self,
        query: &str,
        sink: &mut dyn RowSink,
        cancel: &Cancel,
        deadline: Option<Instant>,
        on_stop: OnStop,
    ) -> Result<Outcome, DbError> {
        let killer = self.killer.clone();
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let result =
            self.runtime.block_on(stream(conn, &killer, query, sink, cancel, deadline, on_stop));
        self.read_tx();
        result
    }

    /// The transaction's state as the server last said it (an error said nothing: asked).
    fn read_tx(&mut self) {
        let Some(conn) = self.conn.as_mut() else { return };
        if conn.last_ok_packet().is_none() {
            let _ = self.runtime.block_on(conn.query_drop("DO 0"));
        }
        if let Some(ok) = conn.last_ok_packet() {
            self.in_tx = ok.status_flags().contains(StatusFlags::SERVER_STATUS_IN_TRANS);
        }
    }

    /// Statements run for their effect alone, the transaction's state then read.
    pub(crate) fn simple(&mut self, sql: &str) -> Result<(), DbError> {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let result =
            self.runtime.block_on(conn.query_drop(sql)).map_err(|e| error_in(e, Some(sql)));
        self.read_tx();
        result
    }

    /// A catalog query's rows, each value as text.
    pub(crate) fn catalog(&mut self, sql: &str) -> Result<TextRows, DbError> {
        let conn = self.conn.as_mut().ok_or_else(|| DbError::Connect("closed".to_owned()))?;
        let rows: Vec<mysql_async::Row> =
            self.runtime.block_on(conn.query(sql)).map_err(|e| error_in(e, Some(sql)))?;
        Ok(rows
            .into_iter()
            .map(|row| row.unwrap_raw().into_iter().map(text_of).collect())
            .collect())
    }

    /// The tables and views of every user schema, or of `schema` alone.
    pub(crate) fn list_tables(&mut self, schema: Option<&str>) -> Result<Vec<TableInfo>, DbError> {
        Ok(self
            .catalog(&catalog::tables(schema))?
            .into_iter()
            .map(|r| {
                let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                let kind = catalog::table_kind(&text(2));
                TableInfo {
                    table: TableRef { schema: Some(text(0)), name: text(1) },
                    kind,
                    rows: if kind == TableKind::View { None } else { text(3).parse().ok() },
                }
            })
            .collect())
    }

    /// `table`'s structure from the catalog.
    pub(crate) fn read_structure(&mut self, table: &TableRef) -> Result<Structure, DbError> {
        let schema = table.schema.as_deref();
        let mariadb = self.version.mariadb;
        let text = |r: &Vec<Option<String>>, i: usize| r.get(i).cloned().flatten();
        let mut columns = Vec::new();
        for r in self.catalog(&catalog::columns(schema, &table.name))? {
            let mut column =
                ColumnInfo::new(text(&r, 0).unwrap_or_default(), text(&r, 1).unwrap_or_default());
            column.nullable = text(&r, 2).as_deref() == Some("YES");
            let extra = text(&r, 4).unwrap_or_default();
            let data_type = text(&r, 7).unwrap_or_default();
            match catalog::generated(&extra) {
                Some(stored) => {
                    column.generated =
                        Some(Generated { expression: text(&r, 5).unwrap_or_default(), stored });
                }
                None => {
                    column.auto_increment = catalog::auto_increment(&extra);
                    column.default =
                        catalog::default_of(text(&r, 3).as_deref(), &extra, &data_type, mariadb);
                }
            }
            column.collation = text(&r, 6);
            columns.push(column);
        }
        if columns.is_empty() {
            return Err(DbError::Query(format!("no table {table}")));
        }
        let mut indexes: Vec<IndexInfo> = Vec::new();
        for r in self.catalog(&catalog::indexes(self.version, schema, &table.name))? {
            let name = text(&r, 0).unwrap_or_default();
            let column =
                text(&r, 3).or_else(|| text(&r, 6).map(|e| format!("({e})"))).unwrap_or_default();
            let column = match text(&r, 5) {
                Some(prefix) => format!("{column}({prefix})"),
                None => column,
            };
            let descending = text(&r, 4).as_deref() == Some("D");
            if name == "PRIMARY" {
                let place =
                    indexes.first().filter(|i| i.name == "PRIMARY").map_or(0, |i| i.columns.len());
                if let Some(c) = columns.iter_mut().find(|c| c.name == column) {
                    c.in_key = true;
                    c.key_position = u16::try_from(place + 1).ok();
                }
            }
            match indexes.iter_mut().find(|i| i.name == name) {
                Some(index) => {
                    index.columns.push(column);
                    index.descending.push(descending);
                }
                None => indexes.push(IndexInfo {
                    implicit: name == "PRIMARY",
                    unique: text(&r, 1).as_deref() == Some("0"),
                    name,
                    columns: vec![column],
                    descending: vec![descending],
                    predicate: None,
                }),
            }
        }
        for index in &mut indexes {
            if index.descending.iter().all(|d| !d) {
                index.descending.clear();
            }
        }
        let foreign_keys = self
            .read_keys(schema, Some(&table.name))?
            .into_iter()
            .next()
            .map(|(_, k)| k)
            .unwrap_or_default();
        Ok(Structure {
            table: table.clone(),
            columns,
            indexes,
            foreign_keys,
            rowid: false,
            lines: false,
        })
    }

    /// Foreign keys from the catalog, grouped by table (see [`catalog::foreign_keys`]).
    pub(crate) fn read_keys(
        &mut self,
        schema: Option<&str>,
        table: Option<&str>,
    ) -> Result<Vec<database::TableKeys>, DbError> {
        let mut all: Vec<database::TableKeys> = Vec::new();
        let mut last_key: Option<(TableRef, String)> = None;
        for r in self.catalog(&catalog::foreign_keys(schema, table))? {
            let text = |i: usize| r.get(i).cloned().flatten();
            let owner = TableRef { schema: text(0), name: text(1).unwrap_or_default() };
            let name = text(2).unwrap_or_default();
            if all.last().is_none_or(|(t, _)| *t != owner) {
                all.push((owner.clone(), Vec::new()));
            }
            let Some((_, keys)) = all.last_mut() else { continue };
            let same = last_key.as_ref().is_some_and(|(t, n)| *t == owner && *n == name);
            if !same {
                keys.push(ForeignKey {
                    name: Some(name.clone()),
                    columns: Vec::new(),
                    table: TableRef { schema: text(3), name: text(4).unwrap_or_default() },
                    references: Vec::new(),
                    on_update: catalog::action(&text(7).unwrap_or_default()),
                    on_delete: catalog::action(&text(8).unwrap_or_default()),
                });
                last_key = Some((owner, name));
            }
            if let Some(key) = keys.last_mut() {
                key.columns.extend(text(5));
                key.references.extend(text(6));
            }
        }
        Ok(all)
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

    /// The warnings the last statement left (`SHOW WARNINGS`), as notices.
    pub(crate) fn take_warnings(&mut self) -> Vec<Notice> {
        let Some(conn) = self.conn.as_mut() else { return Vec::new() };
        if conn.get_warnings() == 0 {
            return Vec::new();
        }
        match self.catalog(&format!("SHOW WARNINGS LIMIT {WARNINGS}")) {
            Ok(rows) => rows
                .into_iter()
                .map(|r| {
                    let text = |i: usize| r.get(i).cloned().flatten().unwrap_or_default();
                    console::warning(&text(0), &text(1), &text(2))
                })
                .collect(),
            Err(e) => {
                tracing::debug!(error = %e, "the warnings weren't read");
                Vec::new()
            }
        }
    }
}
