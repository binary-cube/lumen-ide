//! The query consoles, from the explorer's side: the domain's
//! [`database::console::ConsoleAccess`] over a project's sources ([`ExplorerConsoles`]) —
//! the sources as a console sees them, a connection of the console's own made as the
//! explorer makes its (the same password read in the background, the same startup script,
//! the same read-only), a table's data opened where the explorer opens it, the "Database"
//! window's selection. The consoles themselves are another module's: only the domain's port
//! is shared, never a crate.

use std::sync::{Arc, Weak};
use std::time::Duration;

use database::console::{Connector, ConsoleAccess, JumpRequest, Overrides, SourceBrief};
use database::diagram::{FileTable, Opening};
use database::relations::RelationNotes;
use database::{DataSource, DataSourceId, DbError, TableRef};

use crate::sessions::Sessions;

/// A project's sources, as its consoles reach them.
#[derive(Debug)]
pub struct ExplorerConsoles {
    sessions: Weak<Sessions>,
}

impl ExplorerConsoles {
    pub fn new(sessions: &Arc<Sessions>) -> Self {
        Self { sessions: Arc::downgrade(sessions) }
    }

    /// `table` of source `id` opened: its data, or its `structure` (a file's own view, else
    /// the source's view through its command).
    fn open(
        &self,
        id: DataSourceId,
        table: &TableRef,
        structure: bool,
        column: Option<&str>,
        filter: Option<&str>,
    ) -> Option<Opening> {
        let sessions = self.sessions.upgrade()?;
        let source = sessions.catalog().get(id)?;
        let column = column.map(str::to_owned);
        let filter = filter.map(str::to_owned);
        if let Some(file) = source.file_in(sessions.root()) {
            let asked = FileTable { table: table.clone(), structure, column, filter };
            return Some(Opening::File(file, asked));
        }
        let open = crate::view::OpenSource {
            project: sessions.project(),
            source: id,
            name: source.name,
            table: Some(table.clone()),
            structure,
            column,
            filter,
        };
        Some(open.into_opening())
    }
}

/// `source` as a console sees it.
pub fn brief(sessions: &Sessions, source: &DataSource) -> SourceBrief {
    let driver = sessions.driver(&source.driver);
    let capabilities = driver.as_ref().map(|d| d.capabilities()).unwrap_or_default();
    let dialect = driver.as_ref().and_then(|d| d.dialect());
    let file_based = capabilities.file_based;
    let database =
        (!file_based && !source.target.database.is_empty()).then(|| source.target.database.clone());
    SourceBrief {
        id: source.id,
        name: source.name.clone(),
        driver: source.driver.clone(),
        icon: crate::window::driver_icon(sessions, &source.driver),
        color: sessions.catalog().color_of(source.id),
        production: sessions.catalog().production_of(source.id),
        read_only: source.read_only,
        dialect: dialect.is_some(),
        file_based,
        database,
        flavor: dialect.map(|d| d.flavor()).unwrap_or_default(),
        capabilities,
    }
}

impl ConsoleAccess for ExplorerConsoles {
    fn sources(&self) -> Vec<SourceBrief> {
        let Some(sessions) = self.sessions.upgrade() else { return Vec::new() };
        sessions.catalog().ensure_loaded();
        sessions.catalog().all().iter().map(|s| brief(&sessions, s)).collect()
    }

    fn source(&self, id: DataSourceId) -> Option<SourceBrief> {
        let sessions = self.sessions.upgrade()?;
        // Read in the background if it wasn't (said on the bus once it is).
        sessions.catalog().ensure_loaded();
        let source = sessions.catalog().get(id)?;
        Some(brief(&sessions, &source))
    }

    fn connector(&self, id: DataSourceId, overrides: &Overrides) -> Result<Connector, DbError> {
        let sessions = self
            .sessions
            .upgrade()
            .ok_or_else(|| DbError::Unsupported(i18n::t("database.why_project_closed")))?;
        let source = sessions
            .catalog()
            .get(id)
            .ok_or_else(|| DbError::Unsupported(i18n::t("database.why_source_removed")))?;
        let driver = sessions.driver(&source.driver).ok_or_else(|| {
            DbError::Unsupported(i18n::tr!("database.why_driver", driver = source.driver.as_str()))
        })?;
        if driver.dialect().is_none() {
            return Err(DbError::Unsupported(i18n::t("datasources.console_no_sql")));
        }
        let secret = sessions.secret_fn(&source, driver.as_ref());
        Ok(sessions.connect_fn(&source, driver, secret, overrides.database.as_deref()))
    }

    fn open_table(&self, id: DataSourceId, table: &TableRef) -> Option<Opening> {
        self.open(id, table, false, None, None)
    }

    fn open_structure(&self, id: DataSourceId, table: &TableRef) -> Option<Opening> {
        self.open(id, table, true, None, None)
    }

    fn open_column(&self, id: DataSourceId, table: &TableRef, column: &str) -> Option<Opening> {
        self.open(id, table, true, Some(column), None)
    }

    fn open_rows(&self, id: DataSourceId, table: &TableRef, filter: &str) -> Option<Opening> {
        self.open(id, table, false, None, Some(filter))
    }

    fn notes(&self, id: DataSourceId) -> RelationNotes {
        self.sessions
            .upgrade()
            .and_then(|s| s.catalog().get(id))
            .map(|s| s.relations)
            .unwrap_or_default()
    }

    fn selected(&self) -> Option<JumpRequest> {
        let sessions = self.sessions.upgrade()?;
        let selected = sessions.selected()?;
        // A database of a source's server selected: the source's console, there.
        let (source, database) = match sessions.catalog().database_of(selected.source) {
            Some((base, name)) => (base, Some(name)),
            None => (selected.source, None),
        };
        Some(JumpRequest {
            project: sessions.project(),
            source,
            schema: selected.schema,
            table: selected.table,
            new: false,
            list: false,
            database,
        })
    }

    fn idle_close(&self) -> Option<Duration> {
        crate::settings::idle_close()
    }
}

/// Asks the consoles' module for a console of `source` (its schema, its table), opened by
/// the command the caller runs next (`new`: a new one, whatever it has).
pub fn ask(
    sessions: &Sessions,
    source: DataSourceId,
    schema: Option<String>,
    table: Option<TableRef>,
    new: bool,
) -> &'static str {
    ask_listed(sessions, source, schema, table, new, false)
}

/// [`ask`], the source's consoles listed even when it has none (`list`: "Database
/// Consoles…", where they're renamed and deleted).
pub fn ask_listed(
    sessions: &Sessions,
    source: DataSourceId,
    schema: Option<String>,
    table: Option<TableRef>,
    new: bool,
    list: bool,
) -> &'static str {
    ask_in(sessions, source, None, schema, table, new, list)
}

/// [`ask_listed`] on `database` of the source's server (`None`: the source's own).
pub fn ask_in(
    sessions: &Sessions,
    source: DataSourceId,
    database: Option<String>,
    schema: Option<String>,
    table: Option<TableRef>,
    new: bool,
    list: bool,
) -> &'static str {
    database::console::ask_jump(JumpRequest {
        project: sessions.project(),
        source,
        schema,
        table,
        new,
        list,
        database,
    });
    if new { database::console::NEW_COMMAND } else { database::console::JUMP_COMMAND }
}

/// Whether source `source` can have a console (its driver writes SQL).
pub fn has_sql(sessions: &Sessions, source: &DataSource) -> bool {
    sessions.driver(&source.driver).is_some_and(|d| d.dialect().is_some())
}

#[cfg(test)]
#[path = "../tests/unit/console.rs"]
mod tests;
