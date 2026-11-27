//! How an SQL engine is written to: its identifiers and literals quoted, its pages, and the
//! statements for a [`ChangeSet`] or a [`SchemaChange`] — built only from the model, every
//! name quoted and every value a literal, so the preview shows exactly what runs. The
//! defaults are standard SQL ([`GenericSql`]); an engine overrides what it writes otherwise
//! ([`crate::sqlite::Sqlite`]).

use crate::sql::Flavor;
use crate::{
    Change, ChangeSet, ColumnDef, DbError, DefaultValue, Filter, FkAction, ForeignKey, IndexInfo,
    KeyKind, Order, Page, RowKey, SchemaChange, Structure, TableDef, TableRef, Value,
};

/// Refused: the model can't be written safely (`what`, why).
fn refused(what: impl Into<String>) -> DbError {
    DbError::Unsupported(what.into())
}

/// `name` between double quotes, its own doubled (standard SQL, SQLite, PostgreSQL).
pub fn quote_ansi(name: &str) -> Result<String, DbError> {
    if name.is_empty() {
        return Err(refused("an empty name"));
    }
    if name.contains('\0') {
        return Err(refused("a name with a NUL character"));
    }
    Ok(format!("\"{}\"", name.replace('"', "\"\"")))
}

/// `text` between single quotes, its own doubled.
pub fn quote_text(text: &str) -> Result<String, DbError> {
    if text.contains('\0') {
        return Err(refused("text with a NUL character"));
    }
    Ok(format!("'{}'", text.replace('\'', "''")))
}

/// `declared` if it's only a type's name and its arguments (`VARCHAR(255)`,
/// `numeric(10, 2)`, `timestamp with time zone`, `int[]`): letters, digits, `_`, spaces,
/// commas, dots, balanced parentheses and brackets — nothing that ends or comments a
/// statement.
pub fn check_type(declared: &str) -> Result<String, DbError> {
    let t = declared.trim();
    let bad = || refused(format!("the type `{declared}`"));
    if !t.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return Err(bad());
    }
    let mut depth = 0i32;
    for c in t.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => {
                depth -= 1;
                if depth < 0 {
                    return Err(bad());
                }
            }
            c if c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | ',' | '.') => {}
            _ => return Err(bad()),
        }
    }
    if depth != 0 {
        return Err(bad());
    }
    Ok(t.to_owned())
}

/// `expr` if it's one expression (an engine's own default): its quotes and parentheses
/// balanced, nothing outside quotes that ends or comments a statement.
pub fn check_expression(expr: &str) -> Result<String, DbError> {
    let bad = || refused(format!("the expression `{expr}`"));
    let e = expr.trim();
    if e.is_empty() || e.contains('\0') {
        return Err(bad());
    }
    let mut quote: Option<char> = None;
    let mut depth = 0i32;
    let mut prev = '\0';
    for c in e.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '\'' | '"' | '`' => quote = Some(c),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth < 0 {
                        return Err(bad());
                    }
                }
                ';' => return Err(bad()),
                '-' if prev == '-' => return Err(bad()),
                '*' if prev == '/' => return Err(bad()),
                _ => {}
            },
        }
        prev = if quote.is_some() { '\0' } else { c };
    }
    if quote.is_some() || depth != 0 {
        return Err(bad());
    }
    Ok(e.to_owned())
}

/// A literal `LIKE` pattern matching `text` anywhere (`%`, `_` and `\` escaped by `\`).
fn contains_pattern(text: &str) -> String {
    let mut pattern = String::from("%");
    for c in text.to_lowercase().chars() {
        if matches!(c, '%' | '_' | '\\') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

/// An SQL engine's way of writing (pure: no connection). Every default is standard SQL.
pub trait Dialect: Send + Sync {
    /// Its name (`sql`, `sqlite`, `postgres`).
    fn name(&self) -> &'static str;

    /// How its SQL is read (strings, comments, bodies, parameters): what the console lexes
    /// and splits statements by.
    fn flavor(&self) -> Flavor {
        Flavor::Generic
    }

    /// A name (a table's, a column's, an index's), quoted.
    fn quote_identifier(&self, name: &str) -> Result<String, DbError> {
        quote_ansi(name)
    }

    /// A value, as a literal.
    fn literal(&self, value: &Value) -> Result<String, DbError> {
        standard_literal(value)
    }

    /// A table's name, with its schema.
    fn table_name(&self, table: &TableRef) -> Result<String, DbError> {
        let name = self.quote_identifier(&table.name)?;
        Ok(match &table.schema {
            Some(schema) => format!("{}.{name}", self.quote_identifier(schema)?),
            None => name,
        })
    }

    /// A declared type, checked ([`check_type`]).
    fn type_name(&self, declared: &str) -> Result<String, DbError> {
        check_type(declared)
    }

    /// The types a column is usually declared with, as the UI suggests them (any other
    /// name of a type is accepted, checked by [`Self::type_name`]).
    fn type_suggestions(&self) -> &'static [&'static str] {
        &[
            "INTEGER",
            "BIGINT",
            "SMALLINT",
            "VARCHAR(255)",
            "TEXT",
            "BOOLEAN",
            "NUMERIC(10, 2)",
            "REAL",
            "DOUBLE PRECISION",
            "DATE",
            "TIME",
            "TIMESTAMP",
            "JSON",
            "UUID",
        ]
    }

    /// The collations a column may be declared with (none: the engine's own names aren't
    /// known; any is accepted).
    fn collations(&self) -> &'static [&'static str] {
        &[]
    }

    /// The clause that picks a page.
    fn pagination(&self, limit: u32, offset: u64) -> String {
        if offset == 0 {
            format!("LIMIT {limit}")
        } else {
            format!("LIMIT {limit} OFFSET {offset}")
        }
    }

    /// The engine's row id, if its rows have one (`rowid`).
    fn row_id(&self) -> Option<&'static str> {
        None
    }

    /// What numbers a column by itself.
    fn auto_increment(&self) -> &'static str {
        "GENERATED BY DEFAULT AS IDENTITY"
    }

    fn default_value(&self, default: &DefaultValue) -> Result<String, DbError> {
        standard_default(self, default)
    }

    /// A column's definition (`"name" TYPE COLLATE "c" GENERATED ALWAYS AS (…) STORED NOT
    /// NULL DEFAULT …`).
    fn column_definition(&self, column: &ColumnDef) -> Result<String, DbError> {
        standard_column_definition(self, column)
    }

    /// A list of names, quoted, between parentheses.
    fn name_list(&self, names: &[String]) -> Result<String, DbError> {
        let names: Vec<String> =
            names.iter().map(|n| self.quote_identifier(n)).collect::<Result<_, _>>()?;
        Ok(format!("({})", names.join(", ")))
    }

    /// The condition finding a row (its row id by the engine's own name).
    fn key_condition(&self, key: &RowKey) -> Result<String, DbError> {
        self.key_condition_by(key, self.row_id())
    }

    /// The condition finding a row, its row id read by `row_id` (none: a table whose rows
    /// can't be found by one).
    fn key_condition_by(&self, key: &RowKey, row_id: Option<&str>) -> Result<String, DbError> {
        match key {
            RowKey::Primary(columns) if !columns.is_empty() => {
                let parts: Vec<String> =
                    columns.iter().map(|(c, v)| self.equals(c, v)).collect::<Result<_, _>>()?;
                Ok(parts.join(" AND "))
            }
            RowKey::Primary(_) => Err(refused("a row without a key")),
            RowKey::RowId(id) => match row_id {
                Some(rowid) => Ok(format!("{rowid} = {id}")),
                None => Err(refused("a row id")),
            },
            RowKey::Line { .. } => Err(refused("a row found by its line")),
        }
    }

    /// `column` holds `value` (`IS NULL` for null).
    fn equals(&self, column: &str, value: &Value) -> Result<String, DbError> {
        let column = self.quote_identifier(column)?;
        Ok(match value {
            Value::Null => format!("{column} IS NULL"),
            v => format!("{column} = {}", self.literal(v)?),
        })
    }

    fn filter(&self, filter: &Filter) -> Result<String, DbError> {
        let group = |parts: &[Filter], join: &str, empty: &str| -> Result<String, DbError> {
            if parts.is_empty() {
                return Ok(empty.to_owned());
            }
            let parts: Vec<String> = parts
                .iter()
                .map(|p| self.filter(p).map(|s| format!("({s})")))
                .collect::<Result<_, _>>()?;
            Ok(parts.join(join))
        };
        Ok(match filter {
            Filter::Compare { column, op, value } => {
                if matches!(value, Value::Null) {
                    return Err(refused("a comparison with null (use IsNull / NotNull)"));
                }
                format!("{} {} {}", self.quote_identifier(column)?, op.sql(), self.literal(value)?)
            }
            Filter::IsNull(column) => format!("{} IS NULL", self.quote_identifier(column)?),
            Filter::NotNull(column) => format!("{} IS NOT NULL", self.quote_identifier(column)?),
            Filter::Contains { column, text } => self.contains_condition(column, text)?,
            Filter::And(parts) => group(parts, " AND ", "1 = 1")?,
            Filter::Or(parts) => group(parts, " OR ", "1 = 0")?,
            Filter::Not(inner) => format!("NOT ({})", self.filter(inner)?),
        })
    }

    /// `column`'s value, as text, holds `text` anywhere (any case): `LOWER(CAST(… AS TEXT))
    /// LIKE '%…%' ESCAPE '\'`.
    fn contains_condition(&self, column: &str, text: &str) -> Result<String, DbError> {
        Ok(format!(
            "LOWER(CAST({} AS TEXT)) LIKE {} ESCAPE '\\'",
            self.quote_identifier(column)?,
            quote_text(&contains_pattern(text))?
        ))
    }

    /// A page of `table`'s `columns` (none: all of them).
    fn select(&self, table: &TableRef, columns: &[String], page: &Page) -> Result<String, DbError> {
        let what = if columns.is_empty() {
            "*".to_owned()
        } else {
            let names: Vec<String> =
                columns.iter().map(|c| self.quote_identifier(c)).collect::<Result<_, _>>()?;
            names.join(", ")
        };
        let mut sql = format!("SELECT {what} FROM {}", self.table_name(table)?);
        if let Some(filter) = &page.filter {
            sql.push_str(" WHERE ");
            sql.push_str(&self.filter(filter)?);
        }
        if !page.order.is_empty() {
            let order: Vec<String> = page
                .order
                .iter()
                .map(|o| {
                    let c = self.quote_identifier(&o.column)?;
                    Ok(if o.descending { format!("{c} DESC") } else { format!("{c} ASC") })
                })
                .collect::<Result<_, DbError>>()?;
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        sql.push(' ');
        sql.push_str(&self.pagination(page.limit, page.offset));
        Ok(sql)
    }

    /// What `query` (a statement that only reads: [`crate::sql::rerunnable`]) gives, filtered
    /// and ordered over its result, from row `offset` (`limit`: at most that many; `None`:
    /// all of them): `SELECT * FROM (<query>) AS console_result WHERE … ORDER BY … LIMIT …`.
    /// A query console's next page, or its result sorted and filtered by the engine.
    fn select_over(
        &self,
        query: &str,
        filter: Option<&Filter>,
        order: &[Order],
        limit: Option<u32>,
        offset: u64,
    ) -> Result<String, DbError> {
        let mut inner = query.trim();
        while let Some(rest) = inner.strip_suffix(';') {
            inner = rest.trim_end();
        }
        // On lines of their own: a line comment ending the query never hides the `)`.
        let mut sql = format!("SELECT * FROM (\n{inner}\n) AS console_result");
        if let Some(filter) = filter {
            sql.push_str(" WHERE ");
            sql.push_str(&self.filter(filter)?);
        }
        if !order.is_empty() {
            let order: Vec<String> = order
                .iter()
                .map(|o| {
                    let c = self.quote_identifier(&o.column)?;
                    Ok(if o.descending { format!("{c} DESC") } else { format!("{c} ASC") })
                })
                .collect::<Result<_, DbError>>()?;
            sql.push_str(" ORDER BY ");
            sql.push_str(&order.join(", "));
        }
        match (limit, offset) {
            (None, 0) => {}
            (limit, offset) => {
                sql.push(' ');
                sql.push_str(&self.pagination(limit.unwrap_or(u32::MAX), offset));
            }
        }
        Ok(sql)
    }

    /// How many of `table`'s rows `filter` keeps.
    fn count(&self, table: &TableRef, filter: Option<&Filter>) -> Result<String, DbError> {
        let mut sql = format!("SELECT COUNT(*) FROM {}", self.table_name(table)?);
        if let Some(filter) = filter {
            sql.push_str(" WHERE ");
            sql.push_str(&self.filter(filter)?);
        }
        Ok(sql)
    }

    /// What an insert appends to return its new row's key (`RETURNING "id"`), for a table
    /// whose rows are found by `key` (its row id read by `row_id`): none, by default.
    fn returning(&self, key: &KeyKind, row_id: Option<&str>) -> Result<Option<String>, DbError> {
        let _ = (key, row_id);
        Ok(None)
    }

    /// The statements applying `set`, in order, one per change (see [`Self::changes_of`]),
    /// without knowing the table's structure.
    fn changes(&self, set: &ChangeSet) -> Result<Vec<String>, DbError> {
        self.changes_of(set, None)
    }

    /// The statements applying `set` to the table `structure` describes (when known), in
    /// order, one per change. Each must change exactly one row: one that changes none found
    /// its row changed or gone since it was read (an update checks the old values it can
    /// compare: null, booleans, integers, text and bytes), a conflict, and the whole set is
    /// rolled back. With the structure, rows are found by the row id's name no column hides,
    /// a generated column is refused (it's never written), and an insert returns its row's
    /// key when the engine can ([`Self::returning`]).
    fn changes_of(
        &self,
        set: &ChangeSet,
        structure: Option<&Structure>,
    ) -> Result<Vec<String>, DbError> {
        let table = self.table_name(&set.table)?;
        let row_id = match structure {
            Some(s) => s.row_id_name(),
            None => self.row_id(),
        };
        let returning = match structure {
            Some(s) => self.returning(&s.key_kind(), row_id)?.unwrap_or_default(),
            None => String::new(),
        };
        let writable = |column: &str| -> Result<(), DbError> {
            match structure.and_then(|s| s.column(column)) {
                Some(c) if c.generated.is_some() => {
                    Err(refused(format!("writing the generated column `{column}`")))
                }
                _ => Ok(()),
            }
        };
        set.changes
            .iter()
            .map(|change| match change {
                Change::Insert { values } if values.is_empty() => {
                    Ok(format!("INSERT INTO {table} DEFAULT VALUES{returning}"))
                }
                Change::Insert { values } => {
                    for (column, _) in values {
                        writable(column)?;
                    }
                    let columns: Vec<String> = values.iter().map(|(c, _)| c.clone()).collect();
                    let literals: Vec<String> =
                        values.iter().map(|(_, v)| self.literal(v)).collect::<Result<_, _>>()?;
                    Ok(format!(
                        "INSERT INTO {table} {} VALUES ({}){returning}",
                        self.name_list(&columns)?,
                        literals.join(", ")
                    ))
                }
                Change::Update { cells, .. } if cells.is_empty() => {
                    Err(refused("an update without a change"))
                }
                Change::Update { key, cells } => {
                    for cell in cells {
                        writable(&cell.column)?;
                    }
                    let set: Vec<String> = cells
                        .iter()
                        .map(|c| {
                            Ok(format!(
                                "{} = {}",
                                self.quote_identifier(&c.column)?,
                                self.literal(&c.new)?
                            ))
                        })
                        .collect::<Result<_, DbError>>()?;
                    let mut condition = self.key_condition_by(key, row_id)?;
                    for cell in cells {
                        if matches!(
                            cell.old,
                            Value::Null
                                | Value::Bool(_)
                                | Value::Int(_)
                                | Value::Text(_)
                                | Value::Bytes(_)
                        ) {
                            condition.push_str(" AND ");
                            condition.push_str(&self.equals(&cell.column, &cell.old)?);
                        }
                    }
                    Ok(format!("UPDATE {table} SET {} WHERE {condition}", set.join(", ")))
                }
                Change::Delete { key } => {
                    Ok(format!("DELETE FROM {table} WHERE {}", self.key_condition_by(key, row_id)?))
                }
            })
            .collect()
    }

    /// `CREATE TABLE`'s statement.
    fn create_table(&self, def: &TableDef) -> Result<String, DbError> {
        if def.without_rowid {
            return Err(refused("a table without row ids"));
        }
        standard_create_table(self, def)
    }

    /// A table's foreign key constraint.
    fn foreign_key(&self, fk: &ForeignKey) -> Result<String, DbError> {
        let mut sql = String::new();
        if let Some(name) = &fk.name {
            sql.push_str(&format!("CONSTRAINT {} ", self.quote_identifier(name)?));
        }
        sql.push_str(&format!(
            "FOREIGN KEY {} REFERENCES {} {}",
            self.name_list(&fk.columns)?,
            self.table_name(&fk.table)?,
            self.name_list(&fk.references)?
        ));
        if fk.on_update != FkAction::NoAction {
            sql.push_str(&format!(" ON UPDATE {}", fk.on_update.sql()));
        }
        if fk.on_delete != FkAction::NoAction {
            sql.push_str(&format!(" ON DELETE {}", fk.on_delete.sql()));
        }
        Ok(sql)
    }

    /// An index's name in `CREATE INDEX` / `DROP INDEX` (its table's schema, if any).
    fn index_name(&self, table: &TableRef, name: &str) -> Result<String, DbError> {
        self.table_name(&TableRef { schema: table.schema.clone(), name: name.to_owned() })
    }

    /// `CREATE INDEX`'s statement.
    fn create_index(&self, table: &TableRef, index: &IndexInfo) -> Result<String, DbError> {
        Ok(format!(
            "CREATE {}INDEX {} ON {} {}{}",
            if index.unique { "UNIQUE " } else { "" },
            self.quote_identifier(&index.name)?,
            self.table_name(table)?,
            self.index_columns(index)?,
            self.index_predicate(index)?
        ))
    }

    /// An index's columns, each with its order, between parentheses.
    fn index_columns(&self, index: &IndexInfo) -> Result<String, DbError> {
        if index.columns.is_empty() {
            return Err(refused("an index without columns"));
        }
        let columns: Vec<String> = index
            .columns
            .iter()
            .enumerate()
            .map(|(at, c)| {
                let name = self.quote_identifier(c)?;
                Ok(if index.is_descending(at) { format!("{name} DESC") } else { name })
            })
            .collect::<Result<_, DbError>>()?;
        Ok(format!("({})", columns.join(", ")))
    }

    /// A partial index's ` WHERE …` (checked to be one expression), or nothing.
    fn index_predicate(&self, index: &IndexInfo) -> Result<String, DbError> {
        Ok(match index.predicate.as_deref().map(str::trim) {
            Some(p) if !p.is_empty() => format!(" WHERE {}", check_expression(p)?),
            _ => String::new(),
        })
    }

    /// The DDL for `change`, in order.
    fn schema(&self, change: &SchemaChange) -> Result<Vec<String>, DbError> {
        standard_schema(self, change)
    }

    /// `change` can't be made by `ALTER TABLE` on `table`: the table is rebuilt instead
    /// (SQLite's procedure; no other engine needs one).
    fn needs_rebuild(&self, change: &SchemaChange, table: &Structure) -> bool {
        let _ = (change, table);
        false
    }

    /// `change` as this engine makes it most simply on `table` (SQLite: a column "changed"
    /// only in its name is renamed): itself, by default.
    fn simplified(&self, change: &SchemaChange, table: &Structure) -> SchemaChange {
        let _ = table;
        change.clone()
    }

    /// `CREATE DATABASE` for `name`.
    fn create_database(&self, name: &str) -> Result<String, DbError> {
        Ok(format!("CREATE DATABASE {}", self.quote_identifier(name)?))
    }

    /// `CREATE SCHEMA` for `name`.
    fn create_schema(&self, name: &str) -> Result<String, DbError> {
        Ok(format!("CREATE SCHEMA {}", self.quote_identifier(name)?))
    }

    fn begin(&self) -> &'static str {
        "BEGIN"
    }

    fn commit(&self) -> &'static str {
        "COMMIT"
    }

    fn rollback(&self) -> &'static str {
        "ROLLBACK"
    }
}

/// Standard SQL's literal for `value` (an engine's own starts from it).
pub fn standard_literal(value: &Value) -> Result<String, DbError> {
    Ok(match value {
        Value::Null => "NULL".to_owned(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_owned(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) if f.is_finite() => format!("{f:?}"),
        Value::Float(f) => return Err(refused(format!("the number {f}"))),
        Value::Text(t) | Value::Json(t) | Value::Other(t) => quote_text(t)?,
        Value::Bytes(b) => {
            let hex: String = b.iter().map(|b| format!("{b:02X}")).collect();
            format!("X'{hex}'")
        }
    })
}

/// Standard SQL's column definition, written by `d` (an engine changing one part of it
/// starts from it).
pub fn standard_column_definition<D: Dialect + ?Sized>(
    d: &D,
    column: &ColumnDef,
) -> Result<String, DbError> {
    let mut sql = d.quote_identifier(&column.name)?;
    // No type: a column that holds anything (SQLite's).
    let type_name = d.type_name(&column.declared_type)?;
    if !type_name.is_empty() {
        sql.push(' ');
        sql.push_str(&type_name);
    }
    if let Some(collation) = &column.collation {
        sql.push_str(" COLLATE ");
        sql.push_str(&d.quote_identifier(collation)?);
    }
    if let Some(generated) = &column.generated {
        if column.default.is_some() || column.auto_increment {
            return Err(refused(format!(
                "a generated column with a default or numbered (`{}`)",
                column.name
            )));
        }
        sql.push_str(&format!(
            " GENERATED ALWAYS AS ({}) {}",
            check_expression(&generated.expression)?,
            if generated.stored { "STORED" } else { "VIRTUAL" }
        ));
    }
    if column.auto_increment {
        sql.push(' ');
        sql.push_str(d.auto_increment());
    }
    if !column.nullable {
        sql.push_str(" NOT NULL");
    }
    if let Some(default) = &column.default {
        sql.push_str(" DEFAULT ");
        sql.push_str(&d.default_value(default)?);
    }
    Ok(sql)
}

/// Standard SQL's default, written by `d` (its literals).
pub fn standard_default<D: Dialect + ?Sized>(
    d: &D,
    default: &DefaultValue,
) -> Result<String, DbError> {
    Ok(match default {
        DefaultValue::Value(v) => d.literal(v)?,
        DefaultValue::CurrentTimestamp => "CURRENT_TIMESTAMP".to_owned(),
        DefaultValue::CurrentDate => "CURRENT_DATE".to_owned(),
        DefaultValue::CurrentTime => "CURRENT_TIME".to_owned(),
        DefaultValue::Expression(e) => check_expression(e)?,
    })
}

/// Standard SQL's `CREATE TABLE`, written by `d`: its columns, then its primary key,
/// foreign keys, `UNIQUE` and `CHECK` constraints (a dialect adds what follows the
/// parentheses).
pub fn standard_create_table<D: Dialect + ?Sized>(
    d: &D,
    def: &TableDef,
) -> Result<String, DbError> {
    if def.columns.is_empty() {
        return Err(refused("a table without columns"));
    }
    let mut parts: Vec<String> =
        def.columns.iter().map(|c| d.column_definition(c)).collect::<Result<_, _>>()?;
    if !def.primary_key.is_empty() {
        parts.push(format!("PRIMARY KEY {}", d.name_list(&def.primary_key)?));
    }
    for fk in &def.foreign_keys {
        parts.push(d.foreign_key(fk)?);
    }
    for unique in &def.uniques {
        if unique.is_empty() {
            return Err(refused("a unique constraint without columns"));
        }
        parts.push(format!("UNIQUE {}", d.name_list(unique)?));
    }
    for check in &def.checks {
        parts.push(format!("CHECK ({})", check_expression(check)?));
    }
    Ok(format!("CREATE TABLE {} ({})", d.table_name(&def.table)?, parts.join(", ")))
}

/// Standard SQL's DDL for `change`, written by `d`.
pub fn standard_schema<D: Dialect + ?Sized>(
    d: &D,
    change: &SchemaChange,
) -> Result<Vec<String>, DbError> {
    let alter = |table: &TableRef| d.table_name(table).map(|t| format!("ALTER TABLE {t}"));
    Ok(match change {
        SchemaChange::CreateTable(def) => vec![d.create_table(def)?],
        SchemaChange::RenameTable { table, to } => {
            vec![format!("{} RENAME TO {}", alter(table)?, d.quote_identifier(to)?)]
        }
        SchemaChange::DropTable { table } => vec![format!("DROP TABLE {}", d.table_name(table)?)],
        SchemaChange::AddColumn { table, column } => {
            vec![format!("{} ADD COLUMN {}", alter(table)?, d.column_definition(column)?)]
        }
        SchemaChange::RenameColumn { table, from, to } => vec![format!(
            "{} RENAME COLUMN {} TO {}",
            alter(table)?,
            d.quote_identifier(from)?,
            d.quote_identifier(to)?
        )],
        SchemaChange::AlterColumn { table, column, to } => {
            let head = alter(table)?;
            let c = d.quote_identifier(column)?;
            let mut sql = vec![
                format!("{head} ALTER COLUMN {c} TYPE {}", d.type_name(&to.declared_type)?),
                format!(
                    "{head} ALTER COLUMN {c} {} NOT NULL",
                    if to.nullable { "DROP" } else { "SET" }
                ),
                match &to.default {
                    Some(default) => {
                        format!("{head} ALTER COLUMN {c} SET DEFAULT {}", d.default_value(default)?)
                    }
                    None => format!("{head} ALTER COLUMN {c} DROP DEFAULT"),
                },
            ];
            if to.name != *column {
                sql.push(format!("{head} RENAME COLUMN {c} TO {}", d.quote_identifier(&to.name)?));
            }
            sql
        }
        SchemaChange::DropColumn { table, column } => {
            vec![format!("{} DROP COLUMN {}", alter(table)?, d.quote_identifier(column)?)]
        }
        SchemaChange::CreateIndex { table, index } => vec![d.create_index(table, index)?],
        SchemaChange::DropIndex { table, name } => {
            vec![format!("DROP INDEX {}", d.index_name(table, name)?)]
        }
        SchemaChange::AddForeignKey { table, key } => {
            vec![format!("{} ADD {}", alter(table)?, d.foreign_key(key)?)]
        }
        SchemaChange::DropForeignKey { table, key } => match &key.name {
            Some(name) => {
                vec![format!("{} DROP CONSTRAINT {}", alter(table)?, d.quote_identifier(name)?)]
            }
            None => return Err(refused("dropping a foreign key without a name")),
        },
        SchemaChange::MoveColumn { .. } => return Err(refused("moving a column")),
    })
}

/// Standard SQL: every default.
#[derive(Debug, Clone, Copy, Default)]
pub struct GenericSql;

impl Dialect for GenericSql {
    fn name(&self) -> &'static str {
        "sql"
    }
}

#[cfg(test)]
#[path = "../tests/unit/dialect.rs"]
mod tests;
