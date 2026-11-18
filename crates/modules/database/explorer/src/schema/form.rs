//! The structure's forms as state, apart from how they're drawn: what each asks (a new
//! table's columns and keys, a column's definition, an index's columns…), its fields
//! (`ui_kit` text inputs and plain choices), and what it becomes — the
//! [`SchemaChange`]s it asks for, or why it can't yet (a localized reason). Nothing here
//! writes SQL: the dialect writes it from the changes, and that is what the modal shows.

use std::sync::Arc;

use database::dialect::{check_expression, check_type};
use database::{
    ColumnDef, ColumnInfo, DefaultValue, Dialect, FkAction, ForeignKey, Generated, IndexInfo,
    SchemaChange, SchemaChanges, Structure, TableDef, TableInfo, TableKind, TableRef, TypeClass,
    Value,
};
use i18n::{t, tr};
use ui_kit::TextInput;

/// What the modal does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    CreateTable,
    RenameTable,
    DropTable,
    AddColumn,
    /// Change a column (its name too; only its name for a source that can't change more).
    AlterColumn(String),
    DropColumn(String),
    MoveColumn(String),
    CreateIndex,
    DropIndex(String),
    AddForeignKey,
    DropForeignKey(ForeignKey),
}

impl Kind {
    /// The modal's title.
    pub fn title(&self) -> String {
        t(match self {
            Self::CreateTable => "schema.title_create_table",
            Self::RenameTable => "schema.title_rename_table",
            Self::DropTable => "schema.title_drop_table",
            Self::AddColumn => "schema.title_add_column",
            Self::AlterColumn(_) => "schema.title_alter_column",
            Self::DropColumn(_) => "schema.title_drop_column",
            Self::MoveColumn(_) => "schema.title_move_column",
            Self::CreateIndex => "schema.title_create_index",
            Self::DropIndex(_) => "schema.title_drop_index",
            Self::AddForeignKey => "schema.title_add_key",
            Self::DropForeignKey(_) => "schema.title_drop_key",
        })
    }

    /// It only takes something away (asked to confirm, its button in the danger color).
    pub fn drops(&self) -> bool {
        matches!(
            self,
            Self::DropTable | Self::DropColumn(_) | Self::DropIndex(_) | Self::DropForeignKey(_)
        )
    }
}

/// What a form knows of the source: the table it acts on (none: a table being created),
/// its structure, the source's tables (names taken, what a key points to) and their rows
/// when counted, what the driver can do and how it writes SQL (none: a source without).
#[derive(Clone, Default)]
pub struct Context {
    pub table: Option<TableRef>,
    /// Where a new table goes (an attached schema), when not the main one.
    pub schema: Option<String>,
    pub structure: Option<Structure>,
    pub tables: Vec<TableInfo>,
    pub rows: Vec<(TableRef, u64)>,
    pub can: SchemaChanges,
    pub dialect: Option<Arc<dyn Dialect>>,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context").field("table", &self.table).finish_non_exhaustive()
    }
}

impl Context {
    /// The rows of `table`, when counted.
    pub fn rows_of(&self, table: &TableRef) -> Option<u64> {
        self.rows.iter().find(|(t, _)| t == table).map(|(_, n)| *n)
    }

    /// The source writes SQL (else its changes are summed up, a CSV file's).
    pub fn sql(&self) -> bool {
        self.dialect.is_some()
    }

    /// The types suggested for a column.
    pub fn types(&self) -> Vec<&'static str> {
        self.dialect.as_ref().map(|d| d.type_suggestions().to_vec()).unwrap_or_default()
    }

    pub fn collations(&self) -> Vec<&'static str> {
        self.dialect.as_ref().map(|d| d.collations().to_vec()).unwrap_or_default()
    }

    fn columns(&self) -> &[ColumnInfo] {
        self.structure.as_ref().map_or(&[], |s| s.columns.as_slice())
    }
}

/// A text field of a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Text {
    /// A table's name (created, renamed).
    TableName,
    /// A column's (row `i` of the columns).
    Name(usize),
    Type(usize),
    Default(usize),
    Check(usize),
    Collate(usize),
    Generated(usize),
    IndexName,
    /// A partial index's condition.
    Where,
    /// A foreign key's name (row `i` of the keys).
    KeyName(usize),
}

/// A column being made or changed.
#[derive(Debug, Default)]
pub struct ColumnRow {
    pub name: TextInput,
    pub ty: TextInput,
    pub default: TextInput,
    pub check: TextInput,
    pub collate: TextInput,
    pub generated: TextInput,
    pub not_null: bool,
    pub primary: bool,
    pub auto: bool,
    pub unique: bool,
    /// A generated column kept in the row (else computed when read).
    pub stored: bool,
    /// The column as it is (a column changed).
    pub original: Option<ColumnInfo>,
}

impl ColumnRow {
    pub fn new(name: &str, ty: &str) -> Self {
        Self { name: TextInput::new(name), ty: TextInput::new(ty), ..Self::default() }
    }

    /// The row of a column as it is.
    pub fn of(column: &ColumnInfo) -> Self {
        let generated = column.generated.as_ref();
        Self {
            name: TextInput::new(&column.name),
            ty: TextInput::new(&column.declared_type),
            default: TextInput::new(column.default.as_deref().unwrap_or("")),
            collate: TextInput::new(column.collation.as_deref().unwrap_or("")),
            generated: TextInput::new(generated.map_or("", |g| g.expression.as_str())),
            stored: generated.is_some_and(|g| g.stored),
            not_null: !column.nullable,
            primary: column.in_key,
            auto: column.auto_increment,
            original: Some(column.clone()),
            ..Self::default()
        }
    }

    /// Nothing typed in it (a spare row of a new table).
    pub fn is_blank(&self) -> bool {
        self.name.text().trim().is_empty() && self.ty.text().trim().is_empty()
    }

    fn input(&self, which: Text) -> Option<&TextInput> {
        Some(match which {
            Text::Name(_) => &self.name,
            Text::Type(_) => &self.ty,
            Text::Default(_) => &self.default,
            Text::Check(_) => &self.check,
            Text::Collate(_) => &self.collate,
            Text::Generated(_) => &self.generated,
            _ => return None,
        })
    }

    fn input_mut(&mut self, which: Text) -> Option<&mut TextInput> {
        Some(match which {
            Text::Name(_) => &mut self.name,
            Text::Type(_) => &mut self.ty,
            Text::Default(_) => &mut self.default,
            Text::Check(_) => &mut self.check,
            Text::Collate(_) => &mut self.collate,
            Text::Generated(_) => &mut self.generated,
            _ => return None,
        })
    }

    /// The column it defines (its default read by its type; what's unchanged of a column
    /// changed kept exactly as the catalog wrote it).
    pub fn definition(&self) -> Result<ColumnDef, String> {
        let name = self.name.text().trim().to_owned();
        if name.is_empty() {
            return Err(t("schema.invalid_column_name"));
        }
        let declared = self.ty.text().trim().to_owned();
        if !declared.is_empty() && check_type(&declared).is_err() {
            return Err(tr!("schema.invalid_type", column = name.as_str(), ty = declared.as_str()));
        }
        let original = self.original.as_ref();
        let typed_default = self.default.text().trim();
        let default = match original.and_then(|o| o.default.as_deref()) {
            // Unchanged: as the catalog wrote it.
            Some(old) if old.trim() == typed_default => Some(DefaultValue::Expression(old.into())),
            _ => parse_default(typed_default, &declared)
                .map_err(|_| tr!("schema.invalid_default", column = name.as_str()))?,
        };
        let collate = self.collate.text().trim();
        let generated = self.generated.text().trim();
        let generated = match original.and_then(|o| o.generated.clone()) {
            Some(old) if old.expression.trim() == generated && old.stored == self.stored => {
                Some(old)
            }
            _ if generated.is_empty() => None,
            _ => {
                if check_expression(generated).is_err() {
                    return Err(tr!("schema.invalid_generated", column = name.as_str()));
                }
                Some(Generated { expression: generated.to_owned(), stored: self.stored })
            }
        };
        if !self.check.text().trim().is_empty() && check_expression(self.check.text()).is_err() {
            return Err(tr!("schema.invalid_check", column = name.as_str()));
        }
        Ok(ColumnDef {
            name,
            declared_type: declared,
            nullable: !self.not_null,
            default,
            auto_increment: self.auto,
            collation: (!collate.is_empty()).then(|| collate.to_owned()),
            generated,
        })
    }
}

/// A default that can't be read as typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadDefault;

/// A column's default as typed: none (empty, or `NULL`), the time of the insert
/// (`CURRENT_TIMESTAMP`…), an expression (in parentheses, or a function's call:
/// `datetime('now')`), else a value read by the column's type (a number, a boolean, text —
/// quoted or not).
pub fn parse_default(typed: &str, declared: &str) -> Result<Option<DefaultValue>, BadDefault> {
    let typed = typed.trim();
    if typed.is_empty() || typed.eq_ignore_ascii_case("null") {
        return Ok(None);
    }
    let upper = typed.to_ascii_uppercase();
    match upper.as_str() {
        "CURRENT_TIMESTAMP" => return Ok(Some(DefaultValue::CurrentTimestamp)),
        "CURRENT_DATE" => return Ok(Some(DefaultValue::CurrentDate)),
        "CURRENT_TIME" => return Ok(Some(DefaultValue::CurrentTime)),
        _ => {}
    }
    let call = typed.find('(').is_some_and(|at| {
        let head = typed[..at].trim();
        !head.is_empty()
            && head.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && head.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && typed.ends_with(')')
    });
    if typed.starts_with('(') || call {
        return check_expression(typed)
            .map(|e| Some(DefaultValue::Expression(e)))
            .map_err(|_| BadDefault);
    }
    if let Some(inner) = typed.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')) {
        return Ok(Some(DefaultValue::Value(Value::Text(inner.replace("''", "'")))));
    }
    let value = match TypeClass::of(declared) {
        TypeClass::Integer => typed.parse().map(Value::Int).map_err(|_| BadDefault)?,
        TypeClass::Decimal => match typed.parse::<i64>() {
            Ok(i) => Value::Int(i),
            Err(_) => typed.parse().map(Value::Float).map_err(|_| BadDefault)?,
        },
        TypeClass::Bool => match upper.as_str() {
            "TRUE" | "1" => Value::Bool(true),
            "FALSE" | "0" => Value::Bool(false),
            _ => return Err(BadDefault),
        },
        // No type (SQLite's "anything"), or another: a number as a number, else text.
        TypeClass::Other => match (typed.parse::<i64>(), typed.parse::<f64>()) {
            (Ok(i), _) => Value::Int(i),
            (_, Ok(f)) if f.is_finite() => Value::Float(f),
            _ => Value::Text(typed.to_owned()),
        },
        _ => Value::Text(typed.to_owned()),
    };
    Ok(Some(DefaultValue::Value(value)))
}

/// A foreign key being made.
#[derive(Debug, Default)]
pub struct KeyRow {
    pub name: TextInput,
    /// Its columns, in the order chosen.
    pub columns: Vec<String>,
    pub target: Option<TableRef>,
    /// The columns pointed to, in order.
    pub references: Vec<String>,
    pub on_delete: FkAction,
    pub on_update: FkAction,
}

impl KeyRow {
    pub fn key(&self) -> Result<ForeignKey, String> {
        let Some(target) = self.target.clone() else { return Err(t("schema.invalid_key_target")) };
        if self.columns.is_empty() || self.references.is_empty() {
            return Err(t("schema.invalid_key_columns"));
        }
        if self.columns.len() != self.references.len() {
            return Err(t("schema.invalid_key_count"));
        }
        let name = self.name.text().trim();
        Ok(ForeignKey {
            name: (!name.is_empty()).then(|| name.to_owned()),
            columns: self.columns.clone(),
            table: target,
            references: self.references.clone(),
            on_update: self.on_update,
            on_delete: self.on_delete,
        })
    }
}

/// The actions a foreign key may take, in the order offered.
pub const ACTIONS: [FkAction; 5] = [
    FkAction::NoAction,
    FkAction::Restrict,
    FkAction::Cascade,
    FkAction::SetNull,
    FkAction::SetDefault,
];

/// A form's state (see the module's doc).
#[derive(Debug)]
pub struct Form {
    pub kind: Kind,
    pub ctx: Context,
    pub table_name: TextInput,
    /// A new table's columns, or the one column added / changed.
    pub columns: Vec<ColumnRow>,
    pub without_rowid: bool,
    /// A new table's foreign keys, or the one added.
    pub keys: Vec<KeyRow>,
    pub index_name: TextInput,
    /// The index's name was typed (else it follows its columns).
    pub index_named: bool,
    /// Its columns, in order, each descending or not.
    pub index_columns: Vec<(String, bool)>,
    pub unique: bool,
    pub predicate: TextInput,
    /// Where a column moves to (0-based).
    pub position: usize,
}

impl Form {
    pub fn new(kind: Kind, ctx: Context) -> Self {
        let mut form = Self {
            kind,
            ctx,
            table_name: TextInput::default(),
            columns: Vec::new(),
            without_rowid: false,
            keys: Vec::new(),
            index_name: TextInput::default(),
            index_named: false,
            index_columns: Vec::new(),
            unique: false,
            predicate: TextInput::default(),
            position: 0,
        };
        match form.kind.clone() {
            Kind::CreateTable => {
                // A numbered key to start with, and a row to type the next column in.
                let mut id = ColumnRow::new("id", if form.ctx.sql() { "INTEGER" } else { "" });
                id.primary = true;
                id.not_null = true;
                id.auto = form.ctx.dialect.as_ref().is_some_and(|d| d.name() == "sqlite");
                form.columns = vec![id, ColumnRow::default()];
            }
            Kind::RenameTable => {
                let name = form.ctx.table.as_ref().map(|t| t.name.clone()).unwrap_or_default();
                form.table_name = TextInput::new(&name);
                form.table_name.select_all();
            }
            Kind::AddColumn => form.columns = vec![ColumnRow::default()],
            Kind::AlterColumn(name) => {
                if let Some(column) = form.ctx.columns().iter().find(|c| c.name == name) {
                    form.columns = vec![ColumnRow::of(column)];
                }
            }
            Kind::MoveColumn(name) => {
                form.position = form.ctx.columns().iter().position(|c| c.name == name).unwrap_or(0);
            }
            Kind::AddForeignKey => form.keys = vec![KeyRow::default()],
            Kind::CreateIndex => form.follow_index_name(),
            _ => {}
        }
        form
    }

    /// The table it acts on.
    pub fn table(&self) -> Option<&TableRef> {
        self.ctx.table.as_ref()
    }

    /// The text fields shown, in the order Tab goes through them.
    pub fn texts(&self) -> Vec<Text> {
        let alter_more = self.ctx.can.alter_columns || self.kind == Kind::AddColumn;
        match &self.kind {
            Kind::CreateTable => {
                let mut all = vec![Text::TableName];
                for i in 0..self.columns.len() {
                    all.extend([Text::Name(i), Text::Type(i), Text::Default(i)]);
                }
                all
            }
            Kind::RenameTable => vec![Text::TableName],
            Kind::AddColumn | Kind::AlterColumn(_) if !self.ctx.sql() => vec![Text::Name(0)],
            Kind::AddColumn | Kind::AlterColumn(_) if alter_more => vec![
                Text::Name(0),
                Text::Type(0),
                Text::Default(0),
                Text::Collate(0),
                Text::Generated(0),
            ],
            Kind::AlterColumn(_) => vec![Text::Name(0)],
            Kind::CreateIndex => vec![Text::IndexName, Text::Where],
            Kind::AddForeignKey => vec![Text::KeyName(0)],
            _ => Vec::new(),
        }
    }

    pub fn input(&self, which: Text) -> Option<&TextInput> {
        match which {
            Text::TableName => Some(&self.table_name),
            Text::IndexName => Some(&self.index_name),
            Text::Where => Some(&self.predicate),
            Text::KeyName(i) => self.keys.get(i).map(|k| &k.name),
            Text::Name(i)
            | Text::Type(i)
            | Text::Default(i)
            | Text::Check(i)
            | Text::Collate(i)
            | Text::Generated(i) => self.columns.get(i)?.input(which),
        }
    }

    pub fn input_mut(&mut self, which: Text) -> Option<&mut TextInput> {
        match which {
            Text::TableName => Some(&mut self.table_name),
            Text::IndexName => Some(&mut self.index_name),
            Text::Where => Some(&mut self.predicate),
            Text::KeyName(i) => self.keys.get_mut(i).map(|k| &mut k.name),
            Text::Name(i)
            | Text::Type(i)
            | Text::Default(i)
            | Text::Check(i)
            | Text::Collate(i)
            | Text::Generated(i) => self.columns.get_mut(i)?.input_mut(which),
        }
    }

    /// `which` was edited: what follows it (a new table's last row typed in: a spare one
    /// after it; the index's name, until typed).
    pub fn edited(&mut self, which: Text) {
        match which {
            Text::Name(i) | Text::Type(i)
                if self.kind == Kind::CreateTable && i + 1 == self.columns.len() =>
            {
                if !self.columns[i].is_blank() {
                    self.columns.push(ColumnRow::default());
                }
            }
            Text::IndexName => self.index_named = !self.index_name.text().trim().is_empty(),
            _ => {}
        }
    }

    /// The columns a key, an index or a move chooses from: a new table's (as typed), else
    /// the table's.
    pub fn own_columns(&self) -> Vec<String> {
        if self.kind == Kind::CreateTable {
            return self
                .columns
                .iter()
                .map(|c| c.name.text().trim().to_owned())
                .filter(|n| !n.is_empty())
                .collect();
        }
        self.ctx.columns().iter().map(|c| c.name.clone()).collect()
    }

    /// The tables a key may point to (a new table: itself too, by its name as typed).
    pub fn targets(&self) -> Vec<TableRef> {
        let mut all: Vec<TableRef> = self
            .ctx
            .tables
            .iter()
            .filter(|t| t.kind == TableKind::Table)
            .map(|t| t.table.clone())
            .collect();
        if self.kind == Kind::CreateTable {
            let name = self.table_name.text().trim();
            if !name.is_empty() && !all.iter().any(|t| t.name == name) {
                all.push(TableRef { schema: self.ctx.schema.clone(), name: name.to_owned() });
            }
        }
        all
    }

    /// An index's column picked or let go (added last; taken out where it was).
    pub fn toggle_index_column(&mut self, name: &str) {
        match self.index_columns.iter().position(|(c, _)| c == name) {
            Some(at) => {
                self.index_columns.remove(at);
            }
            None => self.index_columns.push((name.to_owned(), false)),
        }
        self.follow_index_name();
    }

    /// An index's column's order flipped.
    pub fn flip_index_order(&mut self, name: &str) {
        if let Some((_, desc)) = self.index_columns.iter_mut().find(|(c, _)| c == name) {
            *desc = !*desc;
        }
    }

    /// The index's name, following its table and columns until one is typed.
    fn follow_index_name(&mut self) {
        if self.index_named {
            return;
        }
        let table = self.table().map(|t| t.name.clone()).unwrap_or_default();
        let mut name = table;
        for (c, _) in &self.index_columns {
            name.push('_');
            name.push_str(c);
        }
        name.push_str(if self.unique { "_unique" } else { "_index" });
        self.index_name.set_text(&name);
    }

    pub fn set_unique(&mut self, unique: bool) {
        self.unique = unique;
        self.follow_index_name();
    }

    /// A table named `name` is there already (another than the one acted on).
    fn table_taken(&self, name: &str) -> bool {
        self.ctx.tables.iter().any(|t| {
            t.table.name.eq_ignore_ascii_case(name)
                && t.table.schema == self.ctx.schema
                && Some(&t.table) != self.ctx.table.as_ref()
        })
    }

    fn column_taken(&self, name: &str, but: Option<&str>) -> bool {
        self.ctx
            .columns()
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(name) && Some(c.name.as_str()) != but)
    }

    fn acted_on(&self) -> Result<TableRef, String> {
        self.ctx.table.clone().ok_or_else(|| t("schema.invalid_no_table"))
    }

    /// What the form asks for, or why it can't yet.
    pub fn changes(&self) -> Result<Vec<SchemaChange>, String> {
        let changes = self.draft()?;
        // The dialect's own rules (an auto-increment, a key, a type, an expression): refused
        // before anything is asked of the source.
        if let Some(d) = &self.ctx.dialect {
            for change in &changes {
                let checked = match change {
                    SchemaChange::CreateTable(def) => d.create_table(def).map(drop),
                    SchemaChange::AddColumn { column: c, .. }
                    | SchemaChange::AlterColumn { to: c, .. } => d.column_definition(c).map(drop),
                    SchemaChange::CreateIndex { table, index } => {
                        d.create_index(table, index).map(drop)
                    }
                    SchemaChange::AddForeignKey { key, .. } => d.foreign_key(key).map(drop),
                    _ => Ok(()),
                };
                if let Err(e) = checked {
                    return Err(tr!(
                        "schema.invalid_refused",
                        why = crate::grid_source::message(&e)
                    ));
                }
            }
        }
        Ok(changes)
    }

    fn draft(&self) -> Result<Vec<SchemaChange>, String> {
        Ok(match &self.kind {
            Kind::CreateTable => vec![SchemaChange::CreateTable(self.table_def()?)],
            Kind::RenameTable => {
                let table = self.acted_on()?;
                let to = self.table_name.text().trim().to_owned();
                if to.is_empty() {
                    return Err(t("schema.invalid_table_name"));
                }
                if to == table.name {
                    return Err(t("schema.invalid_unchanged"));
                }
                if self.table_taken(&to) {
                    return Err(tr!("schema.invalid_table_taken", name = to.as_str()));
                }
                vec![SchemaChange::RenameTable { table, to }]
            }
            Kind::DropTable => vec![SchemaChange::DropTable { table: self.acted_on()? }],
            Kind::AddColumn => {
                let table = self.acted_on()?;
                let row = self.columns.first().ok_or_else(|| t("schema.invalid_column_name"))?;
                let column = row.definition()?;
                if self.column_taken(&column.name, None) {
                    return Err(tr!("schema.invalid_column_taken", name = column.name.as_str()));
                }
                vec![SchemaChange::AddColumn { table, column }]
            }
            Kind::AlterColumn(old) => {
                let table = self.acted_on()?;
                let row = self.columns.first().ok_or_else(|| t("schema.invalid_column_name"))?;
                let to = row.definition()?;
                if to.name != *old && self.column_taken(&to.name, Some(old)) {
                    return Err(tr!("schema.invalid_column_taken", name = to.name.as_str()));
                }
                let same = row.original.as_ref().is_some_and(|o| ColumnDef::from_info(o) == to);
                if same {
                    return Err(t("schema.invalid_unchanged"));
                }
                // Only its name, where that's all the source changes.
                if !self.ctx.can.alter_columns || !self.ctx.sql() {
                    return Ok(vec![SchemaChange::RenameColumn {
                        table,
                        from: old.clone(),
                        to: to.name,
                    }]);
                }
                vec![SchemaChange::AlterColumn { table, column: old.clone(), to }]
            }
            Kind::DropColumn(column) => {
                if self.ctx.columns().len() <= 1 {
                    return Err(t("schema.invalid_last_column"));
                }
                vec![SchemaChange::DropColumn { table: self.acted_on()?, column: column.clone() }]
            }
            Kind::MoveColumn(column) => {
                let now = self.ctx.columns().iter().position(|c| c.name == *column);
                if now == Some(self.position) {
                    return Err(t("schema.invalid_unchanged"));
                }
                vec![SchemaChange::MoveColumn {
                    table: self.acted_on()?,
                    column: column.clone(),
                    to: self.position,
                }]
            }
            Kind::CreateIndex => {
                let table = self.acted_on()?;
                let name = self.index_name.text().trim().to_owned();
                if name.is_empty() {
                    return Err(t("schema.invalid_index_name"));
                }
                let taken =
                    self.ctx.structure.as_ref().is_some_and(|s| {
                        s.indexes.iter().any(|i| i.name.eq_ignore_ascii_case(&name))
                    });
                if taken {
                    return Err(tr!("schema.invalid_index_taken", name = name.as_str()));
                }
                if self.index_columns.is_empty() {
                    return Err(t("schema.invalid_index_columns"));
                }
                let predicate = self.predicate.text().trim();
                if !predicate.is_empty() && check_expression(predicate).is_err() {
                    return Err(t("schema.invalid_where"));
                }
                let descending: Vec<bool> = self.index_columns.iter().map(|(_, d)| *d).collect();
                let index = IndexInfo {
                    name,
                    columns: self.index_columns.iter().map(|(c, _)| c.clone()).collect(),
                    unique: self.unique,
                    implicit: false,
                    descending: if descending.contains(&true) { descending } else { Vec::new() },
                    predicate: (!predicate.is_empty()).then(|| predicate.to_owned()),
                };
                vec![SchemaChange::CreateIndex { table, index }]
            }
            Kind::DropIndex(name) => {
                vec![SchemaChange::DropIndex { table: self.acted_on()?, name: name.clone() }]
            }
            Kind::AddForeignKey => {
                let table = self.acted_on()?;
                let key = self.keys.first().ok_or_else(|| t("schema.invalid_key_target"))?.key()?;
                vec![SchemaChange::AddForeignKey { table, key }]
            }
            Kind::DropForeignKey(key) => {
                vec![SchemaChange::DropForeignKey { table: self.acted_on()?, key: key.clone() }]
            }
        })
    }

    /// A new table, as typed (its blank rows left out).
    fn table_def(&self) -> Result<TableDef, String> {
        let name = self.table_name.text().trim().to_owned();
        if name.is_empty() {
            return Err(t("schema.invalid_table_name"));
        }
        if self.table_taken(&name) {
            return Err(tr!("schema.invalid_table_taken", name = name.as_str()));
        }
        let rows: Vec<&ColumnRow> = self.columns.iter().filter(|c| !c.is_blank()).collect();
        if rows.is_empty() {
            return Err(t("schema.invalid_no_columns"));
        }
        let mut columns: Vec<ColumnDef> = Vec::new();
        for row in &rows {
            let column = row.definition()?;
            if columns.iter().any(|c| c.name.eq_ignore_ascii_case(&column.name)) {
                return Err(tr!("schema.invalid_column_taken", name = column.name.as_str()));
            }
            columns.push(column);
        }
        let primary_key: Vec<String> =
            rows.iter().filter(|r| r.primary).map(|r| r.name.text().trim().to_owned()).collect();
        if self.without_rowid && primary_key.is_empty() {
            return Err(t("schema.invalid_without_rowid"));
        }
        let uniques = rows
            .iter()
            .filter(|r| r.unique && !(r.primary && primary_key.len() == 1))
            .map(|r| vec![r.name.text().trim().to_owned()])
            .collect();
        let checks = rows
            .iter()
            .map(|r| r.check.text().trim().to_owned())
            .filter(|c| !c.is_empty())
            .collect();
        let names: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        let mut foreign_keys = Vec::new();
        for row in &self.keys {
            let key = row.key()?;
            if let Some(missing) = key.columns.iter().find(|c| !names.contains(&c.as_str())) {
                return Err(tr!("schema.invalid_key_missing", column = missing.as_str()));
            }
            foreign_keys.push(key);
        }
        Ok(TableDef {
            table: TableRef { schema: self.ctx.schema.clone(), name },
            columns,
            primary_key,
            foreign_keys,
            uniques,
            checks,
            without_rowid: self.without_rowid,
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/schema/form.rs"]
mod tests;
