//! The SQLite driver, as a module: it contributes the driver's [`DriverFactory`] (id
//! `sqlite`: the data sources find it by that, the explorer never names this module) and
//! the files it opens by themselves ([`DataFiles`]): `.sqlite`, `.sqlite3`, `.db` and
//! `.db3`, confirmed by their 16-byte header (`SQLite format 3\0`) — another program's
//! `.db` isn't taken for one. Only a file's first bytes are read to tell.
//!
//! Lazy: until a source opens, nothing of SQLite runs (no connection, no thread).

use std::path::Path;

use database::{DataFiles, DriverFactory};
use document::DocumentStorage;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// The module's id.
pub const MODULE_ID: &str = "database.sqlite";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "SQLite",
    description: "SQLite databases as data sources: their tables, structure and rows, changed in transactions",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "database", ..extensions::ModuleInfo::BUILT_IN },
};

/// The SQLite files' extensions (lowercase).
pub const EXTENSIONS: &[&str] = &["sqlite", "sqlite3", "db", "db3"];

/// The files the driver opens: by extension, then by header.
pub fn files() -> DataFiles {
    DataFiles::new("sqlite", EXTENSIONS, db_sqlite::HEADER.len(), db_sqlite::is_database, false)
}

/// The driver (created only when a source opens).
pub fn factory() -> DriverFactory {
    db_sqlite::factory()
}

/// `path` is a SQLite database: one of its extensions, and its header (only its first 16
/// bytes read, through `storage`).
pub fn is_sqlite_file(path: &Path, storage: &dyn DocumentStorage) -> bool {
    let files = files();
    files.has_extension(path)
        && storage.head(path, files.head).is_ok_and(|(_, head)| (files.matches)(&head))
}

/// [`is_sqlite_file`] on the local disk.
pub fn is_local_sqlite_file(path: &Path) -> bool {
    is_sqlite_file(path, &fs_local::LocalFileSystem)
}

#[derive(Debug, Default)]
pub struct DatabaseSqliteModule;

impl Module for DatabaseSqliteModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(factory());
        ctx.contribute(files());
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
