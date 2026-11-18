//! MySQL's and MariaDB's SQL, for every server from MySQL 5.7 and MariaDB 10.x to the
//! newest: one dialect that knows the server it writes for ([`ServerVersion`]: the engine and
//! its version, read from `VERSION()` when a session connects) and never writes what that
//! server lacks — a column renamed on MySQL 5.7 is `CHANGE`d with its whole definition, an
//! expression default before MySQL 8.0.13 is refused, `RETURNING` only on MariaDB 10.5+.
//! [`Feature`] says what each engine and version has, so a driver's queries and its UI ask
//! the same rules. Pure: no connection.
//!
//! Names are quoted with backticks. Text with a backslash (which the server reads as an
//! escape unless `NO_BACKSLASH_ESCAPES`) is written as hex with its character set
//! (`_utf8mb4 X'…'`): read the same whatever the session's `sql_mode`.

use crate::dialect::{
    Dialect, check_expression, check_type, standard_column_definition, standard_literal,
};
use crate::sql::Flavor;
use crate::{
    ColumnDef, DbError, DefaultValue, IndexInfo, KeyKind, SchemaChange, Structure, TableRef, Value,
};

/// The oldest MySQL the driver is made for (5.7: JSON, generated columns, `max_execution_time`).
pub const OLDEST_MYSQL: u32 = 50700;
/// The oldest MariaDB (10.2: expression defaults, `CHECK`, window functions, CTEs).
pub const OLDEST_MARIADB: u32 = 100200;

/// A server's engine and version (`major * 10000 + minor * 100 + patch`: 8.0.36 is 80036,
/// 10.11.6 is 101106).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerVersion {
    pub mariadb: bool,
    pub number: u32,
}

impl Default for ServerVersion {
    /// Not connected yet: what the oldest MySQL takes.
    fn default() -> Self {
        Self { mariadb: false, number: OLDEST_MYSQL }
    }
}

impl ServerVersion {
    pub const fn mysql(number: u32) -> Self {
        Self { mariadb: false, number }
    }

    pub const fn mariadb(number: u32) -> Self {
        Self { mariadb: true, number }
    }

    /// From `VERSION()` (`8.0.36`, `5.7.44-log`, `10.11.6-MariaDB-1:10.11.6+maria~ubu2204`,
    /// a handshake's `5.5.5-10.11.6-MariaDB`); `None` if it isn't a version.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mariadb = text.to_ascii_lowercase().contains("mariadb");
        // MariaDB's handshake says `5.5.5-` first (for old clients).
        let text = if mariadb { text.strip_prefix("5.5.5-").unwrap_or(text) } else { text };
        let numbers = text.split(|c: char| !c.is_ascii_digit() && c != '.').next()?;
        let mut parts = numbers.split('.').map(|p| p.parse::<u32>().ok());
        let major = parts.next().flatten()?;
        let minor = parts.next().flatten().unwrap_or(0);
        let patch = parts.next().flatten().unwrap_or(0);
        (major > 0 && minor < 100 && patch < 100)
            .then_some(Self { mariadb, number: major * 10000 + minor * 100 + patch })
    }

    /// The engine's name (`MySQL`, `MariaDB`).
    pub fn engine(self) -> &'static str {
        if self.mariadb { "MariaDB" } else { "MySQL" }
    }

    /// As people write it: `8.0.36`, `10.11.6`.
    pub fn text(self) -> String {
        format!("{}.{}.{}", self.number / 10000, self.number / 100 % 100, self.number % 100)
    }

    /// Whether the server has `feature`.
    pub fn has(self, feature: Feature) -> bool {
        feature.since(self.mariadb).is_some_and(|since| self.number >= since)
    }
}

/// What a MySQL or MariaDB version may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// `ALTER TABLE … RENAME COLUMN` (MySQL 8.0, MariaDB 10.5.2).
    RenameColumn,
    /// A default that is an expression, between parentheses (MySQL 8.0.13, MariaDB 10.2.1).
    DefaultExpression,
    /// An index on an expression (MySQL 8.0.13; MariaDB: never, a virtual column instead).
    FunctionalIndex,
    /// A descending index kept so (MySQL 8.0; MariaDB 10.8.1; before: parsed, ignored).
    DescendingIndex,
    /// `EXPLAIN FORMAT=TREE` (MySQL 8.0.16).
    ExplainTree,
    /// `EXPLAIN ANALYZE`, the tree as it ran (MySQL 8.0.18).
    ExplainAnalyze,
    /// `ANALYZE FORMAT=JSON` (MariaDB 10.1).
    AnalyzeJson,
    /// `INSERT … RETURNING` (MariaDB 10.5).
    Returning,
    /// Sequences (`CREATE SEQUENCE`, MariaDB 10.3).
    Sequences,
    /// `CHECK` constraints enforced (MySQL 8.0.16, MariaDB 10.2.1).
    Checks,
    /// Window functions and common table expressions (MySQL 8.0, MariaDB 10.2).
    WindowFunctions,
}

impl Feature {
    /// The first version having it on MariaDB (`mariadb`) or MySQL; `None`: never there.
    pub fn since(self, mariadb: bool) -> Option<u32> {
        match (self, mariadb) {
            (Self::RenameColumn, false) => Some(80000),
            (Self::RenameColumn, true) => Some(100502),
            (Self::DefaultExpression, false) => Some(80013),
            (Self::DefaultExpression, true) => Some(100201),
            (Self::FunctionalIndex, false) => Some(80013),
            (Self::FunctionalIndex, true) => None,
            (Self::DescendingIndex, false) => Some(80000),
            (Self::DescendingIndex, true) => Some(100801),
            (Self::ExplainTree, false) => Some(80016),
            (Self::ExplainAnalyze, false) => Some(80018),
            (Self::ExplainTree | Self::ExplainAnalyze, true) => None,
            (Self::AnalyzeJson, true) => Some(100100),
            (Self::AnalyzeJson, false) => None,
            (Self::Returning, true) => Some(100500),
            (Self::Returning, false) => None,
            (Self::Sequences, true) => Some(100300),
            (Self::Sequences, false) => None,
            (Self::Checks, false) => Some(80016),
            (Self::Checks, true) => Some(100201),
            (Self::WindowFunctions, false) => Some(80000),
            (Self::WindowFunctions, true) => Some(100200),
        }
    }
}

/// The schemas the server keeps for itself (never listed as the user's).
pub const SYSTEM_SCHEMAS: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

/// Whether `schema` is one of the server's own.
pub fn is_system_schema(schema: &str) -> bool {
    SYSTEM_SCHEMAS.iter().any(|s| s.eq_ignore_ascii_case(schema))
}

/// `name` between backticks, its own doubled.
pub fn quote_backtick(name: &str) -> Result<String, DbError> {
    if name.is_empty() {
        return Err(DbError::Unsupported("an empty name".to_owned()));
    }
    if name.contains('\0') {
        return Err(DbError::Unsupported("a name with a NUL character".to_owned()));
    }
    Ok(format!("`{}`", name.replace('`', "``")))
}

/// `text` as a literal every server reads the same whatever its `sql_mode`: plain, its
/// quotes doubled; with a backslash or a NUL, as hex in `utf8mb4` (`_utf8mb4 X'…'`).
pub fn mysql_text(text: &str) -> String {
    if text.contains(['\\', '\0']) {
        let hex: String = text.bytes().map(|b| format!("{b:02X}")).collect();
        format!("_utf8mb4 X'{hex}'")
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

/// A `LIKE` pattern matching `text` anywhere (`%`, `_` and `!` escaped by `!`: a backslash
/// would need its own escaping by `sql_mode`).
fn contains_pattern(text: &str) -> String {
    let mut pattern = String::from("%");
    for c in text.to_lowercase().chars() {
        if matches!(c, '%' | '_' | '!') {
            pattern.push('!');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// Whether `expr` is a default MySQL 5.7 takes without parentheses: a literal or
/// `CURRENT_TIMESTAMP`.
fn literal_default(expr: &str) -> bool {
    let e = expr.trim();
    let upper = e.to_ascii_uppercase();
    let quoted = e.len() >= 2
        && ((e.starts_with('\'') && e.ends_with('\'')) || (e.starts_with('"') && e.ends_with('"')));
    quoted
        || e.parse::<f64>().is_ok()
        || matches!(upper.as_str(), "NULL" | "TRUE" | "FALSE")
        || upper.starts_with("CURRENT_TIMESTAMP")
        || upper.starts_with("NOW(")
        || upper.starts_with("LOCALTIMESTAMP")
        || upper.starts_with("B'")
        || upper.starts_with("X'")
        || upper.starts_with("0X")
        || upper.starts_with("_UTF8")
}

/// MySQL's and MariaDB's dialect, for `version`'s server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MySql {
    pub version: ServerVersion,
}

impl MySql {
    pub fn new(version: ServerVersion) -> Self {
        Self { version }
    }

    fn has(&self, feature: Feature) -> bool {
        self.version.has(feature)
    }
}

impl Dialect for MySql {
    fn name(&self) -> &'static str {
        "mysql"
    }

    fn flavor(&self) -> Flavor {
        Flavor::MySql
    }

    fn quote_identifier(&self, name: &str) -> Result<String, DbError> {
        quote_backtick(name)
    }

    fn literal(&self, value: &Value) -> Result<String, DbError> {
        match value {
            Value::Text(t) | Value::Json(t) | Value::Other(t) => Ok(mysql_text(t)),
            v => standard_literal(v),
        }
    }

    fn type_name(&self, declared: &str) -> Result<String, DbError> {
        // `enum('S', 'M')` / `set(…)`: their values quoted (checked as text, each one).
        let t = declared.trim();
        let lower = t.to_ascii_lowercase();
        if (lower.starts_with("enum(") || lower.starts_with("set(")) && t.ends_with(')') {
            let open = t.find('(').unwrap_or(0);
            let inner = &t[open + 1..t.len() - 1];
            check_expression(inner)?;
            return Ok(t.to_owned());
        }
        // `int unsigned`, `varchar(20) binary`: words after the type are its own.
        check_type(declared)
    }

    fn type_suggestions(&self) -> &'static [&'static str] {
        &[
            "int",
            "bigint",
            "smallint",
            "tinyint",
            "int unsigned",
            "bigint unsigned",
            "varchar(255)",
            "char(36)",
            "text",
            "mediumtext",
            "longtext",
            "boolean",
            "decimal(10, 2)",
            "float",
            "double",
            "date",
            "time",
            "datetime",
            "timestamp",
            "year",
            "json",
            "blob",
            "varbinary(255)",
            "enum('a', 'b')",
        ]
    }

    fn collations(&self) -> &'static [&'static str] {
        &[
            "utf8mb4_general_ci",
            "utf8mb4_unicode_ci",
            "utf8mb4_bin",
            "utf8mb4_0900_ai_ci",
            "utf8mb4_0900_bin",
            "latin1_swedish_ci",
            "binary",
        ]
    }

    fn auto_increment(&self) -> &'static str {
        "AUTO_INCREMENT"
    }

    fn default_value(&self, default: &DefaultValue) -> Result<String, DbError> {
        match default {
            DefaultValue::Value(v) => self.literal(v),
            DefaultValue::CurrentTimestamp => Ok("CURRENT_TIMESTAMP".to_owned()),
            DefaultValue::CurrentDate if self.has(Feature::DefaultExpression) => {
                Ok("(CURRENT_DATE)".to_owned())
            }
            DefaultValue::CurrentTime if self.has(Feature::DefaultExpression) => {
                Ok("(CURRENT_TIME)".to_owned())
            }
            DefaultValue::Expression(e) => {
                let e = check_expression(e)?;
                if literal_default(&e) {
                    Ok(e)
                } else if self.has(Feature::DefaultExpression) {
                    Ok(format!("({e})"))
                } else {
                    Err(DbError::Unsupported(format!(
                        "the default `{e}` (an expression: MySQL 8.0.13 or MariaDB 10.2 or newer)"
                    )))
                }
            }
            DefaultValue::CurrentDate | DefaultValue::CurrentTime => Err(DbError::Unsupported(
                "a date or time default (MySQL 8.0.13 or MariaDB 10.2 or newer)".to_owned(),
            )),
        }
    }

    /// `` `name` type [COLLATE c] [GENERATED ALWAYS AS (…) STORED|VIRTUAL] [NOT NULL]
    /// [DEFAULT …] [AUTO_INCREMENT] ``: the order every version takes.
    fn column_definition(&self, column: &ColumnDef) -> Result<String, DbError> {
        if !column.auto_increment {
            return standard_column_definition(self, column);
        }
        let mut plain = column.clone();
        plain.auto_increment = false;
        let mut sql = standard_column_definition(self, &plain)?;
        sql.push_str(" AUTO_INCREMENT");
        Ok(sql)
    }

    fn contains_condition(&self, column: &str, text: &str) -> Result<String, DbError> {
        Ok(format!(
            "LOWER(CAST({} AS CHAR)) LIKE {} ESCAPE '!'",
            self.quote_identifier(column)?,
            mysql_text(&contains_pattern(text))
        ))
    }

    fn returning(&self, key: &KeyKind, _row_id: Option<&str>) -> Result<Option<String>, DbError> {
        Ok(match key {
            KeyKind::Primary(columns) if self.has(Feature::Returning) => {
                let list = self.name_list(columns)?;
                Some(format!(" RETURNING {}", &list[1..list.len() - 1]))
            }
            _ => None,
        })
    }

    /// An index's name: its own (indexes belong to their table, never to a schema).
    fn index_name(&self, _table: &TableRef, name: &str) -> Result<String, DbError> {
        self.quote_identifier(name)
    }

    fn index_predicate(&self, index: &IndexInfo) -> Result<String, DbError> {
        match index.predicate.as_deref().map(str::trim) {
            Some(p) if !p.is_empty() => {
                Err(DbError::Unsupported("a partial index (MySQL and MariaDB have none)".into()))
            }
            _ => Ok(String::new()),
        }
    }

    fn schema(&self, change: &SchemaChange) -> Result<Vec<String>, DbError> {
        let alter = |table: &TableRef| self.table_name(table).map(|t| format!("ALTER TABLE {t}"));
        Ok(match change {
            // Its new name in its own schema (unqualified, it would move to the session's).
            SchemaChange::RenameTable { table, to } => {
                let target = TableRef { schema: table.schema.clone(), name: to.clone() };
                vec![format!("{} RENAME TO {}", alter(table)?, self.table_name(&target)?)]
            }
            SchemaChange::RenameColumn { table, from, to } => {
                if !self.has(Feature::RenameColumn) {
                    return Err(DbError::Unsupported(format!(
                        "renaming a column on {} {} without its definition \
                         (MySQL 8.0 or MariaDB 10.5.2 or newer)",
                        self.version.engine(),
                        self.version.text()
                    )));
                }
                vec![format!(
                    "{} RENAME COLUMN {} TO {}",
                    alter(table)?,
                    self.quote_identifier(from)?,
                    self.quote_identifier(to)?
                )]
            }
            // The whole definition at once (and its new name).
            SchemaChange::AlterColumn { table, column, to } => vec![format!(
                "{} CHANGE COLUMN {} {}",
                alter(table)?,
                self.quote_identifier(column)?,
                self.column_definition(to)?
            )],
            SchemaChange::DropIndex { table, name } => vec![format!(
                "DROP INDEX {} ON {}",
                self.quote_identifier(name)?,
                self.table_name(table)?
            )],
            SchemaChange::DropForeignKey { table, key } => match &key.name {
                Some(name) => vec![format!(
                    "{} DROP FOREIGN KEY {}",
                    alter(table)?,
                    self.quote_identifier(name)?
                )],
                None => {
                    return Err(DbError::Unsupported(
                        "dropping a foreign key without a name".to_owned(),
                    ));
                }
            },
            other => crate::dialect::standard_schema(self, other)?,
        })
    }

    /// On a server without `RENAME COLUMN`, a rename is the column `CHANGE`d with its
    /// definition as read.
    fn simplified(&self, change: &SchemaChange, table: &Structure) -> SchemaChange {
        match change {
            SchemaChange::RenameColumn { table: t, from, to }
                if !self.has(Feature::RenameColumn) =>
            {
                match table.column(from) {
                    Some(info) => {
                        let mut def = ColumnDef::from_info(info);
                        def.name = to.clone();
                        SchemaChange::AlterColumn {
                            table: t.clone(),
                            column: from.clone(),
                            to: def,
                        }
                    }
                    None => change.clone(),
                }
            }
            other => other.clone(),
        }
    }

    fn begin(&self) -> &'static str {
        "START TRANSACTION"
    }
}

#[cfg(test)]
#[path = "../tests/unit/mysql.rs"]
mod tests;
