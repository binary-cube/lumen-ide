//! The CSV driver, as a module: it contributes the driver's [`DriverFactory`] (id `csv`: the
//! data sources find it by that, the explorer never names this module) and the files it
//! opens by themselves ([`DataFiles`]): `.csv`, `.tsv` and `.tab`, confirmed to start as
//! text (no NUL byte in their first kilobyte: not a binary file, nor UTF-16). Only a
//! file's first bytes are read to tell.
//!
//! Lazy: until a source opens, nothing of it runs (no file read, no index).

use std::path::Path;

use database::{DataFiles, DriverFactory};
use document::DocumentStorage;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// The module's id.
pub const MODULE_ID: &str = "database.csv";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "CSV",
    description: "CSV and TSV files as data sources: their rows read a page at a time, sorted, filtered and changed in one commit",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "table", ..extensions::ModuleInfo::BUILT_IN },
};

/// The files the driver opens: by extension, then by a text start.
pub fn files() -> DataFiles {
    DataFiles::new("csv", db_csv::EXTENSIONS, db_csv::HEAD, db_csv::is_text, true)
}

/// The driver (created only when a source opens).
pub fn factory() -> DriverFactory {
    db_csv::factory()
}

/// `path` is a CSV file: one of its extensions, and a text start (only its first bytes
/// read, through `storage`).
pub fn is_csv_file(path: &Path, storage: &dyn DocumentStorage) -> bool {
    let files = files();
    files.has_extension(path)
        && storage.head(path, files.head).is_ok_and(|(_, head)| (files.matches)(&head))
}

/// [`is_csv_file`] on the local disk.
pub fn is_local_csv_file(path: &Path) -> bool {
    is_csv_file(path, &fs_local::LocalFileSystem)
}

#[derive(Debug, Default)]
pub struct DatabaseCsvModule;

impl Module for DatabaseCsvModule {
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
