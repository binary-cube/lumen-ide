//! The PostgreSQL driver, as a module: it contributes the driver's [`DriverFactory`] (id
//! `postgres`: the data sources find it by that, the explorer never names this module).
//! One driver for every server from 9.2 to the newest (what differs is read from the
//! server's version when it connects), its TLS by `sslmode` (`prefer` by default).
//!
//! Lazy: until a source connects, nothing of PostgreSQL runs (no runtime, no connection).

use database::DriverFactory;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// The module's id.
pub const MODULE_ID: &str = "database.postgres";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "PostgreSQL",
    description: "PostgreSQL servers (9.2 to the newest) as data sources: their schemas, tables, structure and rows, changed in transactions, over TLS by sslmode",
    depends_on: &[],
};

/// The driver (created only when a source connects).
pub fn factory() -> DriverFactory {
    db_postgres::factory()
}

#[derive(Debug, Default)]
pub struct DatabasePostgresModule;

impl Module for DatabasePostgresModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute(factory());
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
