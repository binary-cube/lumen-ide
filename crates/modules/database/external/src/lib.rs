//! Drivers installed apart (plan 37i), as a module: a driver of any language, or a bridge
//! to JDBC, installed in a folder of its own under the config folder
//! (`drivers/<name>/driver.toml`, its program beside it) speaks the drivers' protocol
//! (`db-protocol`) and is one more driver of the data sources — the explorer never knows
//! it's external, but for the mark the `+` menu shows.
//!
//! Lazy: the manifests are read the first time a driver is asked for
//! ([`database::drivers::register_discovery`]), nothing is started then; a driver's
//! program runs only while a session of it is open. A manifest naming one of the app's own
//! drivers is left out (it never takes its place), one that can't be read is said in the
//! log.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use db_protocol::dto;
use db_protocol::manifest::{self, ProgramLaunch};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use ui_components::data_grid::CellClass;
use ui_components::data_grid::value_editor::{self, ValueEditorSpec, ValueFormat};

/// The module's id.
pub const MODULE_ID: &str = "database.external";

/// The config folder's folder of drivers.
pub const DRIVERS_DIR: &str = "drivers";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "External Drivers",
    description: "Database drivers installed apart (any language, a JDBC bridge), speaking the drivers' protocol: started only while a session of theirs is open",
    depends_on: &[],
};

/// Where the drivers are installed: `<config folder>/drivers`.
pub fn drivers_dir() -> Option<PathBuf> {
    storage::config_dir().map(|d| d.join(DRIVERS_DIR))
}

/// A value editor a manifest declares, as the grid's.
pub fn value_editor(declared: &dto::ValueEditor) -> ValueEditorSpec {
    let format = match &declared.format {
        dto::ValueFormat::Text => ValueFormat::Text,
        dto::ValueFormat::Integer => ValueFormat::Integer,
        dto::ValueFormat::Decimal => ValueFormat::Decimal,
        dto::ValueFormat::Bool => ValueFormat::Bool,
        dto::ValueFormat::DateTime => ValueFormat::DateTime,
        dto::ValueFormat::Date => ValueFormat::Date,
        dto::ValueFormat::Time => ValueFormat::Time,
        dto::ValueFormat::Json => ValueFormat::Json,
        dto::ValueFormat::Choice(values) => ValueFormat::Choice(values.clone()),
        dto::ValueFormat::Raw => ValueFormat::Raw,
    };
    let classes: Vec<CellClass> = declared
        .classes
        .iter()
        .map(|c| match c {
            dto::TypeClass::Text => CellClass::Text,
            dto::TypeClass::Integer => CellClass::Integer,
            dto::TypeClass::Decimal => CellClass::Decimal,
            dto::TypeClass::Bool => CellClass::Bool,
            dto::TypeClass::DateTime => CellClass::DateTime,
            dto::TypeClass::Binary => CellClass::Binary,
            dto::TypeClass::Json => CellClass::Json,
            dto::TypeClass::Other => CellClass::Other,
        })
        .collect();
    let types: Vec<&str> = declared.types.iter().map(String::as_str).collect();
    let mut spec = ValueEditorSpec::new(&declared.id, format)
        .for_classes(&classes)
        .for_types(&types)
        .priority(declared.priority);
    if declared.large {
        spec = spec.in_large_editor();
    }
    spec
}

/// Registers the drivers installed under `dir` (their factories, their files, their
/// value editors): the ids of those registered.
pub fn install(dir: &Path) -> Vec<String> {
    let mut installed = Vec::new();
    for found in manifest::discover(dir) {
        let driver = match found {
            Ok(driver) => driver,
            Err((path, why)) => {
                tracing::warn!(path = %path.display(), "a driver's manifest wasn't read: {why}");
                continue;
            }
        };
        let m = &driver.manifest;
        let id = m.driver.id.clone();
        if database::drivers::is_registered(&id) {
            tracing::warn!(driver = %id, "an installed driver has the id of one already there: left out");
            continue;
        }
        database::drivers::register(manifest::factory(m, Arc::new(ProgramLaunch::of(&driver))));
        if let Some(files) = manifest::files(m) {
            database::drivers::register_files(files);
        }
        for editor in &m.driver.value_editors {
            value_editor::register(Arc::new(value_editor(editor)));
        }
        tracing::debug!(driver = %id, dir = %driver.dir.display(), "an external driver installed");
        installed.push(id);
    }
    installed
}

#[derive(Debug, Default)]
pub struct DatabaseExternalModule;

impl Module for DatabaseExternalModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, _ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        // Read the first time a driver is asked for, not now.
        if let Some(dir) = drivers_dir() {
            database::drivers::register_discovery(move || {
                install(&dir);
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
