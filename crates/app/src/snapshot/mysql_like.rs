//! A made-up MySQL server, in memory (the `DATABASE=mysql` snapshot): one session reads
//! every database as a schema (`analytics`, `shop`), as the MySQL driver does.

use std::sync::Arc;

use database::{
    Cancel, Capabilities, ConsoleCaps, DbError, Driver, DriverFactory, Kind, Language, Node,
    Outcome, Place, RowSink, Secret, Session, TableInfo, TableKind, TableRef, Target,
};

pub const ID: &str = "snapshot-mysql";

pub fn factory() -> DriverFactory {
    DriverFactory::new(ID, "MySQL", Kind::Relational, Some(3306), || Arc::new(Server))
}

struct Server;

struct Connected;

impl Driver for Server {
    fn kind(&self) -> Kind {
        Kind::Relational
    }

    fn language(&self) -> Language {
        Language::Sql
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            schemas: true,
            console: Some(ConsoleCaps::default()),
            ..Capabilities::default()
        }
    }

    fn connect(&self, _target: &Target, _secret: &Secret) -> Result<Box<dyn Session>, DbError> {
        Ok(Box::new(Connected))
    }
}

impl Session for Connected {
    fn server(&self) -> String {
        "MySQL 8.4.3".into()
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
        Ok(vec![
            table("analytics", "visits", TableKind::Table, 3000),
            table("analytics", "daily", TableKind::View, 0),
            table("shop", "countries", TableKind::Table, 6),
            table("shop", "customers", TableKind::Table, 2000),
            table("shop", "orders", TableKind::Table, 4000),
            table("shop", "order_lines", TableKind::Table, 8000),
            table("shop", "order_totals", TableKind::View, 0),
        ])
    }

    fn structure(&mut self, table: &TableRef) -> Result<database::Structure, DbError> {
        let mut id = database::ColumnInfo::new("id", "bigint");
        id.in_key = true;
        id.key_position = Some(1);
        id.nullable = false;
        id.auto_increment = true;
        let status = database::ColumnInfo::new("status", "enum('new','paid','shipped')");
        Ok(database::Structure {
            table: table.clone(),
            columns: vec![id, database::ColumnInfo::new("customer_id", "int"), status],
            indexes: Vec::new(),
            foreign_keys: Vec::new(),
            rowid: false,
            lines: false,
        })
    }

    fn current(&mut self) -> Result<Place, DbError> {
        Ok(Place { database: None, schema: Some("shop".into()) })
    }
}
