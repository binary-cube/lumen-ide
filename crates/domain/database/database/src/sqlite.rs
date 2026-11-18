//! SQLite's dialect, and the rebuild of a table for the changes its `ALTER TABLE` can't make
//! (a column's type, nullability or default; dropping a column that's in a key, an index or
//! a foreign key; adding one it refuses): the 12 steps of SQLite's own procedure
//! (<https://sqlite.org/lang_altertable.html#otheralter>), as statements.
//!
//! Columns renamed along a rebuild keep their old names while the table is rebuilt (its
//! indexes, triggers and views made again as they were written) and are renamed last, by
//! `ALTER TABLE … RENAME COLUMN`: SQLite rewrites whatever names them. What names a column
//! dropped can't be made again: it's left out, and said ([`Stale`]) for the user to confirm.

use crate::dialect::{
    Dialect, check_expression, check_type, standard_create_table, standard_default,
    standard_literal, standard_schema,
};
use crate::sql::Flavor;
use crate::{
    ColumnDef, ColumnInfo, DbError, DefaultValue, ForeignKey, IndexInfo, KeyKind, SchemaChange,
    SchemaObject, Structure, TableDef, TableRef, Value,
};

/// SQLite: booleans are 0 / 1, rows have a `rowid`, transactions take the write lock at
/// once (`BEGIN IMMEDIATE`: no deadlock between two writers).
#[derive(Debug, Clone, Copy, Default)]
pub struct Sqlite;

impl Dialect for Sqlite {
    fn name(&self) -> &'static str {
        "sqlite"
    }

    fn flavor(&self) -> Flavor {
        Flavor::Sqlite
    }

    fn literal(&self, value: &Value) -> Result<String, DbError> {
        match value {
            Value::Bool(b) => Ok(if *b { "1" } else { "0" }.to_owned()),
            v => standard_literal(v),
        }
    }

    fn row_id(&self) -> Option<&'static str> {
        Some("rowid")
    }

    fn type_name(&self, declared: &str) -> Result<String, DbError> {
        // A column may have no type (it holds anything): written without one.
        if declared.trim().is_empty() { Ok(String::new()) } else { check_type(declared) }
    }

    fn auto_increment(&self) -> &'static str {
        "PRIMARY KEY AUTOINCREMENT"
    }

    fn type_suggestions(&self) -> &'static [&'static str] {
        // Its five affinities first, then the names it reads by them.
        &[
            "INTEGER",
            "TEXT",
            "REAL",
            "NUMERIC",
            "BLOB",
            "BOOLEAN",
            "VARCHAR(255)",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "DECIMAL(10, 2)",
            "JSON",
        ]
    }

    fn collations(&self) -> &'static [&'static str] {
        &["BINARY", "NOCASE", "RTRIM"]
    }

    fn default_value(&self, default: &DefaultValue) -> Result<String, DbError> {
        match default {
            // Any expression, in parentheses (literals too: always valid).
            DefaultValue::Expression(e) => Ok(format!("({})", check_expression(e)?)),
            d => standard_default(self, d),
        }
    }

    /// `RETURNING` the primary key's columns, or the row id (by the name no column hides).
    fn returning(&self, key: &KeyKind, row_id: Option<&str>) -> Result<Option<String>, DbError> {
        Ok(match (key, row_id) {
            (KeyKind::Primary(columns), _) => {
                let list = self.name_list(columns)?;
                Some(format!(" RETURNING {}", &list[1..list.len() - 1]))
            }
            (KeyKind::RowId, Some(row_id)) => Some(format!(" RETURNING {row_id}")),
            _ => None,
        })
    }

    fn create_table(&self, def: &TableDef) -> Result<String, DbError> {
        // An auto-incremented column is the primary key, written on it (`INTEGER PRIMARY KEY
        // AUTOINCREMENT`): the table's own clause would be a second one.
        let numbered: Vec<&ColumnDef> = def.columns.iter().filter(|c| c.auto_increment).collect();
        let primary_key = match numbered.as_slice() {
            [] => def.primary_key.clone(),
            [c] if def.primary_key.is_empty()
                || (def.primary_key.len() == 1 && def.primary_key[0] == c.name) =>
            {
                if !c.declared_type.trim().eq_ignore_ascii_case("integer") {
                    return Err(DbError::Unsupported(format!(
                        "an auto-incremented column of type `{}` (SQLite numbers only INTEGER)",
                        c.declared_type
                    )));
                }
                if def.without_rowid {
                    return Err(DbError::Unsupported(
                        "auto-increment in a table without row ids".to_owned(),
                    ));
                }
                Vec::new()
            }
            _ => {
                return Err(DbError::Unsupported(
                    "auto-increment on a column that isn't the whole primary key".to_owned(),
                ));
            }
        };
        if def.without_rowid && primary_key.is_empty() {
            return Err(DbError::Unsupported(
                "a table without row ids and without a primary key".to_owned(),
            ));
        }
        let mut sql = standard_create_table(self, &TableDef { primary_key, ..def.clone() })?;
        if def.without_rowid {
            sql.push_str(" WITHOUT ROWID");
        }
        Ok(sql)
    }

    fn create_index(&self, table: &TableRef, index: &IndexInfo) -> Result<String, DbError> {
        Ok(format!(
            "CREATE {}INDEX {} ON {} {}{}",
            if index.unique { "UNIQUE " } else { "" },
            // The schema (an attached database) goes on the index; its table is unqualified.
            self.index_name(table, &index.name)?,
            self.quote_identifier(&table.name)?,
            self.index_columns(index)?,
            self.index_predicate(index)?
        ))
    }

    fn schema(&self, change: &SchemaChange) -> Result<Vec<String>, DbError> {
        match change {
            SchemaChange::AlterColumn { .. }
            | SchemaChange::AddForeignKey { .. }
            | SchemaChange::DropForeignKey { .. }
            | SchemaChange::MoveColumn { .. } => Err(DbError::Unsupported(
                "SQLite changes a column, a foreign key or the columns' order only by rebuilding \
                 its table"
                    .to_owned(),
            )),
            c => standard_schema(self, c),
        }
    }

    fn begin(&self) -> &'static str {
        "BEGIN IMMEDIATE"
    }

    fn needs_rebuild(&self, change: &SchemaChange, table: &Structure) -> bool {
        needs_rebuild(change, table)
    }

    fn simplified(&self, change: &SchemaChange, table: &Structure) -> SchemaChange {
        simplified(change, table)
    }
}

/// A SQLite table, as a rebuild needs it: its structure, its `CHECK` constraints (their
/// expressions, from its definition), and what goes with it as its own SQL (from
/// `sqlite_schema`): its triggers, its indexes made on expressions or with a `WHERE`
/// (their columns alone would lose what they index), and the views naming it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SqliteTable {
    pub structure: Structure,
    pub checks: Vec<String>,
    pub triggers: Vec<SchemaObject>,
    /// The indexes made again from their own SQL (not in the structure's indexes).
    pub written_indexes: Vec<SchemaObject>,
    pub views: Vec<SchemaObject>,
}

/// What a rebuild reads from the database before it's planned: the table, and whether
/// foreign keys are on (`PRAGMA foreign_keys`: turned off around the rebuild, then checked).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebuildSource {
    pub table: SqliteTable,
    pub foreign_keys: bool,
}

/// `change` needs the table rebuilt (SQLite's `ALTER TABLE` can't make it on `table`).
pub fn needs_rebuild(change: &SchemaChange, table: &Structure) -> bool {
    match simplified(change, table) {
        SchemaChange::AlterColumn { .. }
        | SchemaChange::AddForeignKey { .. }
        | SchemaChange::DropForeignKey { .. }
        | SchemaChange::MoveColumn { .. } => true,
        SchemaChange::DropColumn { column, .. } => {
            table.column(&column).is_some_and(|c| c.in_key)
                || table.indexes.iter().any(|i| i.columns.contains(&column))
                || table.foreign_keys.iter().any(|f| f.columns.contains(&column))
        }
        SchemaChange::AddColumn { column, .. } => {
            let now = matches!(
                column.default,
                Some(
                    DefaultValue::CurrentTimestamp
                        | DefaultValue::CurrentDate
                        | DefaultValue::CurrentTime
                        | DefaultValue::Expression(_)
                )
            );
            // A stored generated column can't be added (a virtual one can).
            let stored = column.generated.as_ref().is_some_and(|g| g.stored);
            column.auto_increment || now || stored || (!column.nullable && column.default.is_none())
        }
        _ => false,
    }
}

/// `text` without one pair of parentheses around it (`('x')` is `'x'`).
fn bare(text: &str) -> &str {
    let t = text.trim();
    match t.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
        Some(inner) => inner.trim(),
        None => t,
    }
}

/// `to` is `old` as it is, but for its name (its type, nullability, default, collation and
/// generation the same).
fn same_but_name(old: &ColumnInfo, to: &ColumnDef) -> bool {
    let norm = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
    let default = match (&old.default, &to.default) {
        (None, None) => true,
        (Some(old), Some(to)) => {
            Sqlite.default_value(to).is_ok_and(|to| bare(&to).eq_ignore_ascii_case(bare(old)))
        }
        _ => false,
    };
    let collation = match (&old.collation, &to.collation) {
        (None, None) => true,
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
    };
    norm(&old.declared_type) == norm(&to.declared_type)
        && old.nullable == to.nullable
        && old.auto_increment == to.auto_increment
        && old.generated == to.generated
        && collation
        && default
}

/// `change` as `ALTER TABLE` can make it, when it can: a column "changed" only in its name
/// is renamed (no rebuild). Anything else is itself.
pub fn simplified(change: &SchemaChange, table: &Structure) -> SchemaChange {
    if let SchemaChange::AlterColumn { table: t, column, to } = change
        && let Some(old) = table.column(column)
        && same_but_name(old, to)
        && to.name != *column
    {
        return SchemaChange::RenameColumn {
            table: t.clone(),
            from: column.clone(),
            to: to.name.clone(),
        };
    }
    change.clone()
}

/// What an object kept as its own SQL is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Index,
    Trigger,
    View,
}

/// An object a rebuild drops and doesn't make again: it names a column dropped (or a view
/// that goes), so made again as written it would fail. The user confirms losing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stale {
    /// The table rebuilt.
    pub table: TableRef,
    pub kind: ObjectKind,
    pub name: String,
}

/// The statements of a table's rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebuildPlan {
    /// Run in one transaction, in order (steps 3 to 9).
    pub statements: Vec<String>,
    /// What's dropped and not made again (in the order found).
    pub stale: Vec<Stale>,
}

/// Turns foreign keys off, before the transaction (step 1; a no-op inside one).
pub const FOREIGN_KEYS_OFF: &str = "PRAGMA foreign_keys = OFF";
/// Lists the rows breaking a foreign key (step 10): read before the changes and again
/// before committing — a row breaking one that didn't before, and the transaction is
/// rolled back.
pub const FOREIGN_KEY_CHECK: &str = "PRAGMA foreign_key_check";
/// Turns foreign keys back on, after the transaction (step 12).
pub const FOREIGN_KEYS_ON: &str = "PRAGMA foreign_keys = ON";

/// [`FOREIGN_KEY_CHECK`] of the tables of `schema` (none: the main database).
pub fn foreign_key_check(schema: Option<&str>) -> Result<String, DbError> {
    Ok(match schema {
        None => FOREIGN_KEY_CHECK.to_owned(),
        Some(schema) => format!("PRAGMA {}.foreign_key_check", Sqlite.quote_identifier(schema)?),
    })
}

/// A token of SQL text, as far as the rebuild reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    /// A keyword or a bare name.
    Word,
    /// A quoted name (`"…"`, `` `…` ``, `[…]`).
    Quoted,
    /// Anything else (strings, numbers, punctuation): never a name.
    Other,
}

/// `sql`'s tokens with their byte ranges (spaces and comments left out, strings one token).
fn tokens(sql: &str) -> Vec<(Token, usize, usize)> {
    let b = sql.as_bytes();
    let closing = |from: usize, quote: u8| -> usize {
        let mut i = from;
        while i < b.len() {
            if b[i] == quote {
                // A quote doubled is one quote, inside.
                if b.get(i + 1) == Some(&quote) {
                    i += 2;
                    continue;
                }
                return i + 1;
            }
            i += 1;
        }
        b.len()
    };
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c >= 0x80;
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        match c {
            b'\'' => {
                i = closing(i + 1, c);
                out.push((Token::Other, start, i));
            }
            b'"' | b'`' => {
                i = closing(i + 1, c);
                out.push((Token::Quoted, start, i));
            }
            b'[' => {
                i = b[i..].iter().position(|&c| c == b']').map_or(b.len(), |at| i + at + 1);
                out.push((Token::Quoted, start, i));
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                i = b[i..].iter().position(|&c| c == b'\n').map_or(b.len(), |at| i + at + 1);
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = sql[i + 2..].find("*/").map_or(b.len(), |at| i + 2 + at + 2);
            }
            c if c.is_ascii_whitespace() => i += 1,
            c if word(c) => {
                while i < b.len() && word(b[i]) {
                    i += 1;
                }
                out.push((Token::Word, start, i));
            }
            _ => {
                i += 1;
                out.push((Token::Other, start, i));
            }
        }
    }
    out
}

/// A name token's text, unquoted.
fn unquoted(sql: &str, (t, s, e): (Token, usize, usize)) -> Option<String> {
    match t {
        Token::Word => Some(sql[s..e].to_owned()),
        Token::Quoted if e - s >= 2 => {
            let inner = &sql[s + 1..e - 1];
            Some(match sql.as_bytes()[s] {
                b'"' => inner.replace("\"\"", "\""),
                b'`' => inner.replace("``", "`"),
                _ => inner.to_owned(),
            })
        }
        _ => None,
    }
}

/// `sql` names `name` (a word or a quoted name, any case; never inside a string).
pub fn names(sql: &str, name: &str) -> bool {
    tokens(sql).into_iter().filter_map(|t| unquoted(sql, t)).any(|n| n.eq_ignore_ascii_case(name))
}

/// `sql` (a `CREATE INDEX`, `CREATE TRIGGER` or `CREATE VIEW` as `sqlite_schema` keeps it,
/// its schema left out) made in `schema`: its object's name qualified. Unchanged for the
/// main database.
pub fn qualified(sql: &str, schema: Option<&str>) -> Result<String, DbError> {
    let Some(schema) = schema else { return Ok(sql.to_owned()) };
    let refused = || DbError::Unsupported(format!("making `{sql}` in {schema}"));
    let tokens = tokens(sql);
    let word = |at: usize, w: &str| {
        tokens
            .get(at)
            .is_some_and(|&(t, s, e)| t == Token::Word && sql[s..e].eq_ignore_ascii_case(w))
    };
    if !word(0, "CREATE") {
        return Err(refused());
    }
    let mut at = 1;
    if word(at, "UNIQUE") {
        at += 1;
    }
    if !(word(at, "INDEX") || word(at, "TRIGGER") || word(at, "VIEW")) {
        return Err(refused());
    }
    at += 1;
    if word(at, "IF") && word(at + 1, "NOT") && word(at + 2, "EXISTS") {
        at += 3;
    }
    let Some(&(t, start, _)) = tokens.get(at) else { return Err(refused()) };
    // Already qualified (`name` followed by a dot): as it is.
    if t == Token::Other
        || tokens.get(at + 1).is_some_and(|&(t, s, e)| t == Token::Other && &sql[s..e] == ".")
    {
        return Err(refused());
    }
    Ok(format!("{}{}.{}", &sql[..start], Sqlite.quote_identifier(schema)?, &sql[start..]))
}

/// A column of the rebuilt table.
#[derive(Debug, Clone)]
struct NewColumn {
    /// Its name before the changes, where its values come from (none: a new column).
    from: Option<String>,
    /// Its name once every change is made.
    name: String,
    /// Its definition in the rebuilt table (named as it's built: the old name of a column
    /// renamed last).
    def: ColumnDef,
}

fn rename_in(names: &mut [String], from: &str, to: &str) {
    for name in names.iter_mut().filter(|n| *n == from) {
        *name = to.to_owned();
    }
}

/// Why a draft failed: renaming last can't be (a name taken by another column on the
/// way), or the changes are wrong.
enum Failed {
    Clash,
    Error(DbError),
}

impl From<DbError> for Failed {
    fn from(e: DbError) -> Self {
        Self::Error(e)
    }
}

/// The rebuilt table, as the changes shape it (keys, constraints and indexes by the
/// columns' names as built).
struct Draft {
    columns: Vec<NewColumn>,
    primary_key: Vec<String>,
    foreign_keys: Vec<ForeignKey>,
    /// `UNIQUE` constraints (their implicit indexes, made again with the table).
    uniques: Vec<Vec<String>>,
    /// The indexes made by `CREATE INDEX` on columns.
    indexes: Vec<IndexInfo>,
    /// The indexes made again from their SQL.
    written: Vec<SchemaObject>,
    /// The old names of the columns whose old name nothing may name any more (dropped;
    /// renamed in the table, when renaming last can't be).
    gone: Vec<String>,
    /// Columns are renamed last (`ALTER TABLE … RENAME COLUMN`), not in the rebuilt table.
    rename_last: bool,
}

impl Draft {
    fn new(table: &SqliteTable, rename_last: bool) -> Self {
        let s = &table.structure;
        let primary_key = s.primary_key();
        Self {
            columns: s
                .columns
                .iter()
                .map(|c| NewColumn {
                    from: Some(c.name.clone()),
                    name: c.name.clone(),
                    def: ColumnDef::from_info(c),
                })
                .collect(),
            // A primary key's own index is the key; a `UNIQUE` one becomes a constraint again.
            uniques: s
                .indexes
                .iter()
                .filter(|i| i.implicit && i.unique && i.columns != primary_key)
                .map(|i| i.columns.clone())
                .collect(),
            indexes: s.indexes.iter().filter(|i| !i.implicit).cloned().collect(),
            written: table.written_indexes.clone(),
            primary_key,
            foreign_keys: s.foreign_keys.clone(),
            gone: Vec::new(),
            rename_last,
        }
    }

    fn position(&self, column: &str, table: &TableRef) -> Result<usize, DbError> {
        self.columns
            .iter()
            .position(|c| c.name == column)
            .ok_or_else(|| DbError::Query(format!("no column `{column}` in {table}")))
    }

    fn taken(&self, name: &str, but: Option<usize>, table: &TableRef) -> Result<(), Failed> {
        let others = || self.columns.iter().enumerate().filter(|(i, _)| Some(*i) != but);
        if others().any(|(_, c)| c.name == name) {
            return Err(DbError::Query(format!("column `{name}` already exists in {table}")).into());
        }
        if others().any(|(_, c)| c.def.name == name) {
            return Err(Failed::Clash);
        }
        Ok(())
    }

    /// The column at `at` is named `to` in the rebuilt table, everywhere it's named.
    fn rename_built(&mut self, at: usize, to: &str) {
        let from = std::mem::replace(&mut self.columns[at].def.name, to.to_owned());
        rename_in(&mut self.primary_key, &from, to);
        for fk in &mut self.foreign_keys {
            rename_in(&mut fk.columns, &from, to);
        }
        for unique in &mut self.uniques {
            rename_in(unique, &from, to);
        }
        for index in &mut self.indexes {
            rename_in(&mut index.columns, &from, to);
        }
        if let Some(old) = self.columns[at].from.clone() {
            self.gone.push(old);
        }
    }

    /// The column named `from` is named `to`.
    fn rename(&mut self, from: &str, to: &str, table: &TableRef) -> Result<(), Failed> {
        let at = self.position(from, table)?;
        if from == to {
            return Ok(());
        }
        self.taken(to, Some(at), table)?;
        self.columns[at].name = to.to_owned();
        if !self.rename_last {
            self.rename_built(at, to);
        }
        Ok(())
    }

    /// Column `column` goes, with what can't be without it.
    fn drop(&mut self, column: &str, table: &TableRef) -> Result<(), DbError> {
        let at = self.position(column, table)?;
        let gone = self.columns.remove(at);
        let built = gone.def.name;
        self.primary_key.retain(|c| *c != built);
        self.foreign_keys.retain(|f| !f.columns.contains(&built));
        self.uniques.retain(|u| !u.contains(&built));
        // An index on it goes (said at the end, with what else names it).
        self.indexes.retain(|i| !i.columns.contains(&built));
        if let Some(old) = gone.from {
            self.gone.push(old);
        }
        Ok(())
    }

    /// `columns` (by their names now) as they're built.
    fn built(&self, columns: &[String]) -> Vec<String> {
        columns
            .iter()
            .map(|c| {
                self.columns
                    .iter()
                    .find(|n| n.name == *c)
                    .map_or_else(|| c.clone(), |n| n.def.name.clone())
            })
            .collect()
    }

    fn apply(&mut self, change: &SchemaChange, table: &TableRef) -> Result<(), Failed> {
        if change.table() != table {
            return Err(DbError::Unsupported(format!(
                "a rebuild of {table} changing {}",
                change.table()
            ))
            .into());
        }
        match change {
            SchemaChange::AddColumn { column, .. } => {
                self.taken(&column.name, None, table)?;
                self.columns.push(NewColumn {
                    from: None,
                    name: column.name.clone(),
                    def: column.clone(),
                });
            }
            SchemaChange::RenameColumn { from, to, .. } => self.rename(from, to, table)?,
            SchemaChange::AlterColumn { column, to, .. } => {
                self.rename(column, &to.name, table)?;
                let at = self.position(&to.name, table)?;
                let built = self.columns[at].def.name.clone();
                self.columns[at].def = ColumnDef { name: built, ..to.clone() };
            }
            SchemaChange::DropColumn { column, .. } => self.drop(column, table)?,
            SchemaChange::CreateIndex { index, .. } => {
                let columns = self.built(&index.columns);
                self.indexes.push(IndexInfo { columns, ..index.clone() });
            }
            SchemaChange::DropIndex { name, .. } => {
                self.indexes.retain(|i| i.name != *name);
                // Never made again: an index dropped stays dropped, whatever made it.
                self.written.retain(|i| i.name != *name);
            }
            SchemaChange::AddForeignKey { key, .. } => {
                for column in &key.columns {
                    self.position(column, table)?;
                }
                let columns = self.built(&key.columns);
                self.foreign_keys.push(ForeignKey { columns, ..key.clone() });
            }
            SchemaChange::DropForeignKey { key, .. } => {
                let columns = self.built(&key.columns);
                let before = self.foreign_keys.len();
                self.foreign_keys.retain(|f| {
                    !(f.columns == columns
                        && f.table.name.eq_ignore_ascii_case(&key.table.name)
                        && f.references == key.references)
                });
                if self.foreign_keys.len() == before {
                    return Err(DbError::Query(format!("no such foreign key in {table}")).into());
                }
            }
            SchemaChange::MoveColumn { column, to, .. } => {
                let at = self.position(column, table)?;
                let moved = self.columns.remove(at);
                let to = (*to).min(self.columns.len());
                self.columns.insert(to, moved);
            }
            other => {
                return Err(DbError::Unsupported(format!("{other:?} as part of a rebuild")).into());
            }
        }
        Ok(())
    }

    /// The renames made last, in order, each to a name no column has by then.
    fn renames(&self) -> Result<Vec<(String, String)>, Failed> {
        let mut now: Vec<String> = self.columns.iter().map(|c| c.def.name.clone()).collect();
        let mut renames = Vec::new();
        for (at, column) in self.columns.iter().enumerate() {
            if column.def.name == column.name {
                continue;
            }
            if now.contains(&column.name) {
                return Err(Failed::Clash);
            }
            now[at] = column.name.clone();
            renames.push((column.def.name.clone(), column.name.clone()));
        }
        Ok(renames)
    }
}

/// The changes drafted on `table`, renaming last when `rename_last`.
fn draft(
    table: &SqliteTable,
    changes: &[SchemaChange],
    rename_last: bool,
) -> Result<(Draft, Vec<(String, String)>), Failed> {
    let mut draft = Draft::new(table, rename_last);
    for change in changes {
        draft.apply(change, &table.structure.table)?;
    }
    let renames = if rename_last { draft.renames()? } else { Vec::new() };
    Ok((draft, renames))
}

/// The rebuild of `table` making `changes` (all on it: columns and indexes; renaming or
/// dropping the table itself isn't a rebuild). Foreign keys off and on, and their check,
/// are [`crate::transaction::rebuild_script`]'s.
///
/// Columns renamed are renamed last (see the module's doc) — unless a name is taken on the
/// way (two columns swapping names, a new column taking a renamed one's old name): then
/// they're renamed in the rebuilt table, and what names their old names is stale too.
pub fn rebuild(table: &SqliteTable, changes: &[SchemaChange]) -> Result<RebuildPlan, DbError> {
    let (draft, renames) = match draft(table, changes, true) {
        Ok(drafted) => drafted,
        Err(Failed::Clash) => match draft(table, changes, false) {
            Ok(drafted) => drafted,
            Err(Failed::Error(e)) => return Err(e),
            Err(Failed::Clash) => {
                return Err(DbError::Query("a column's name is taken twice".to_owned()));
            }
        },
        Err(Failed::Error(e)) => return Err(e),
    };
    let s = &table.structure;
    let schema = s.table.schema.as_deref();
    let Draft { columns, primary_key, foreign_keys, uniques, indexes, written, gone, .. } = draft;
    if columns.is_empty() {
        return Err(DbError::Unsupported("a table without columns".to_owned()));
    }

    let d = Sqlite;
    let temp = TableRef { schema: s.table.schema.clone(), name: format!("new_{}", s.table.name) };
    let defs: Vec<ColumnDef> = columns.iter().map(|c| c.def.clone()).collect();
    let create = d.create_table(&TableDef {
        table: temp.clone(),
        columns: defs,
        primary_key,
        foreign_keys,
        uniques,
        checks: table.checks.clone(),
        without_rowid: !s.rowid,
    })?;

    let mut plan = RebuildPlan::default();
    let mut stale = |kind: ObjectKind, name: &str| {
        plan.stale.push(Stale { table: s.table.clone(), kind, name: name.to_owned() });
    };
    // The indexes on a column dropped went with it.
    for index in s.indexes.iter().filter(|i| !i.implicit) {
        let kept = indexes.iter().any(|k| k.name == index.name);
        let asked = changes
            .iter()
            .any(|c| matches!(c, SchemaChange::DropIndex { name, .. } if *name == index.name));
        if !kept && !asked {
            stale(ObjectKind::Index, &index.name);
        }
    }
    // What names a column gone (or a view that goes) isn't made again. Views before
    // triggers: a trigger may name a view that goes.
    let mut gone = gone;
    let mut made: Vec<&SchemaObject> = Vec::new();
    let kinds = [ObjectKind::Index, ObjectKind::View, ObjectKind::Trigger];
    let lists = [
        written.iter().collect::<Vec<_>>(),
        table.views.iter().collect(),
        table.triggers.iter().collect(),
    ];
    let mut by_kind: Vec<Vec<&SchemaObject>> = Vec::new();
    for (kind, list) in kinds.into_iter().zip(lists) {
        let mut kept = Vec::new();
        for object in list {
            if gone.iter().any(|g| names(&object.sql, g)) {
                stale(kind, &object.name);
                gone.push(object.name.clone());
            } else {
                kept.push(object);
            }
        }
        by_kind.push(kept);
    }
    // Made again in this order: indexes, triggers, views (each view after what it reads).
    made.extend(&by_kind[0]);
    made.extend(&by_kind[2]);
    made.extend(&by_kind[1]);

    // Views naming the table would break the rename (SQLite checks the schema): dropped
    // first, made again last (those still valid).
    for view in &table.views {
        let name = TableRef { schema: s.table.schema.clone(), name: view.name.clone() };
        plan.statements.push(format!("DROP VIEW {}", d.table_name(&name)?));
    }
    plan.statements.push(create);
    // Generated columns are computed, never copied.
    let copied: Vec<&NewColumn> =
        columns.iter().filter(|c| c.from.is_some() && c.def.generated.is_none()).collect();
    if !copied.is_empty() {
        let to: Vec<String> = copied.iter().map(|c| c.def.name.clone()).collect();
        let from: Vec<String> = copied.iter().filter_map(|c| c.from.clone()).collect();
        let from = d.name_list(&from)?;
        plan.statements.push(format!(
            "INSERT INTO {} {} SELECT {} FROM {}",
            d.table_name(&temp)?,
            d.name_list(&to)?,
            &from[1..from.len() - 1],
            d.table_name(&s.table)?
        ));
    }
    plan.statements.push(format!("DROP TABLE {}", d.table_name(&s.table)?));
    plan.statements.push(format!(
        "ALTER TABLE {} RENAME TO {}",
        d.table_name(&temp)?,
        d.quote_identifier(&s.table.name)?
    ));
    for index in &indexes {
        plan.statements.push(d.create_index(&s.table, index)?);
    }
    for object in made {
        plan.statements.push(qualified(&object.sql, schema)?);
    }
    for (from, to) in renames {
        plan.statements.push(format!(
            "ALTER TABLE {} RENAME COLUMN {} TO {}",
            d.table_name(&s.table)?,
            d.quote_identifier(&from)?,
            d.quote_identifier(&to)?
        ));
    }
    Ok(plan)
}

/// A partial index's condition, read from its `CREATE INDEX` (what follows its `WHERE`,
/// outside parentheses and strings): none for an index of every row.
pub fn index_predicate(sql: &str) -> Option<String> {
    let mut depth = 0i32;
    for (t, start, end) in tokens(sql) {
        match (t, &sql[start..end]) {
            (Token::Other, "(") => depth += 1,
            (Token::Other, ")") => depth -= 1,
            (Token::Word, w) if depth == 0 && w.eq_ignore_ascii_case("WHERE") => {
                let rest = sql[end..].trim().trim_end_matches(';').trim();
                return (!rest.is_empty()).then(|| rest.to_owned());
            }
            _ => {}
        }
    }
    None
}

/// `text` with every name `from` (a word or a quoted name, any case; never inside a
/// string) written `to` instead (quoted when it must be): a filter following a column
/// renamed.
pub fn rename_name(text: &str, from: &str, to: &str) -> String {
    let bare = to.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && to.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let written = if bare { to.to_owned() } else { format!("\"{}\"", to.replace('"', "\"\"")) };
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for token in tokens(text) {
        let (_, start, end) = token;
        if unquoted(text, token).is_some_and(|n| n.eq_ignore_ascii_case(from)) {
            out.push_str(&text[at..start]);
            out.push_str(&written);
            at = end;
        }
    }
    out.push_str(&text[at..]);
    out
}

/// An object SQLite rewrites when a name it uses changes (a column or a table renamed by
/// `ALTER TABLE … RENAME`): shown to the user before, for information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub kind: ObjectKind,
    pub name: String,
}

/// What renaming in `changes` makes SQLite rewrite among `table`'s triggers, its indexes
/// kept as written and the views naming it: the objects naming a column renamed (or the
/// table renamed), but those `stale` (dropped by a rebuild, never made again).
pub fn rewritten(table: &SqliteTable, changes: &[SchemaChange], stale: &[Stale]) -> Vec<Rewritten> {
    let me = &table.structure.table;
    let mut old_names: Vec<&str> = Vec::new();
    for change in changes.iter().filter(|c| c.table() == me) {
        match change {
            SchemaChange::RenameColumn { from, .. } => old_names.push(from),
            SchemaChange::AlterColumn { column, to, .. } if to.name != *column => {
                old_names.push(column);
            }
            SchemaChange::RenameTable { table, .. } => old_names.push(&table.name),
            _ => {}
        }
    }
    let mut found = Vec::new();
    if old_names.is_empty() {
        return found;
    }
    let lists = [
        (ObjectKind::Index, &table.written_indexes),
        (ObjectKind::Trigger, &table.triggers),
        (ObjectKind::View, &table.views),
    ];
    for (kind, list) in lists {
        for object in list {
            let gone = stale.iter().any(|s| s.kind == kind && s.name == object.name);
            if !gone && old_names.iter().any(|n| names(&object.sql, n)) {
                found.push(Rewritten { kind, name: object.name.clone() });
            }
        }
    }
    found
}

#[cfg(test)]
#[path = "../tests/unit/sqlite.rs"]
mod tests;
