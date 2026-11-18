//! The sample data files of `DATA` snapshots: the same rows every run.
//! A made-up server with several databases, in memory (the `DATABASE=databases` snapshot):
//! each session on one database, `postgres` empty, `shop` and `analytics` with tables.

use std::sync::Arc;

use database::{
    Cancel, Capabilities, ConsoleCaps, DbError, Driver, DriverFactory, Kind, Language, Node,
    Outcome, Place, RowSink, Secret, Session, TableInfo, TableKind, TableRef, Target,
};

pub const ID: &str = "snapshot-cluster";

pub fn factory() -> DriverFactory {
    DriverFactory::new(ID, "PostgreSQL", Kind::Relational, Some(5432), || Arc::new(Cluster))
}

struct Cluster;

struct OnDatabase(String);

impl Driver for Cluster {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            schemas: true,
            console: Some(ConsoleCaps { databases: true, ..ConsoleCaps::default() }),
            ..Capabilities::default()
        }
    }

    fn connect(&self, target: &Target, _secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        Ok(Box::new(OnDatabase(target.database.clone())))
    }
}

impl Session for OnDatabase {
    fn server(&self) -> String {
        "PostgreSQL 14.13".into()
    }

    fn children(&mut self, _path: &[String]) -> Result<Vec<Node>, DbError> {
        Ok(Vec::new())
    }

    fn execute(
        &mut self,
        _query: &str,
        _sink: &mut dyn RowSink,
        _cancel: &Cancel,
    ) -> Result<Outcome, DbError> {
        Ok(Outcome::default())
    }

    fn tables(&mut self, _path: &[String]) -> Result<Vec<TableInfo>, DbError> {
        let table = |schema: &str, name: &str, kind, rows| TableInfo {
            table: TableRef::in_schema(schema, name),
            kind,
            rows: Some(rows),
        };
        Ok(match self.0.as_str() {
            "shop" => vec![
                table("audit", "events", TableKind::Table, 500),
                table("public", "customers", TableKind::Table, 2000),
                table("sales", "orders", TableKind::Table, 4000),
                table("sales", "order_lines", TableKind::Table, 8000),
                table("sales", "order_totals", TableKind::View, 0),
            ],
            "analytics" => vec![table("public", "visits", TableKind::Table, 3000)],
            _ => Vec::new(),
        })
    }

    fn structure(&mut self, table: &TableRef) -> Result<database::Structure, DbError> {
        let mut id = database::ColumnInfo::new("id", "bigint");
        id.in_key = true;
        id.key_position = Some(1);
        id.nullable = false;
        let customer = database::ColumnInfo::new("customer_id", "integer");
        Ok(database::Structure {
            table: table.clone(),
            columns: vec![id, customer, database::ColumnInfo::new("note", "text")],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            rowid: false,
            lines: false,
        })
    }

    fn databases(&mut self) -> Result<Vec<String>, DbError> {
        Ok(vec!["analytics".into(), "postgres".into(), "shop".into()])
    }

    fn current(&mut self) -> Result<Place, DbError> {
        Ok(Place { database: Some(self.0.clone()), schema: Some("public".into()) })
    }
}
