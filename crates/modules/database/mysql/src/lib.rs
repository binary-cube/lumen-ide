//! The MySQL / MariaDB driver, as a module: it contributes the driver's two
//! [`DriverFactory`]s (ids `mysql` and `mariadb`, the same driver: the data sources find
//! them by those, the explorer never names this module). One driver for every server from
//! MySQL 5.7 and MariaDB 10.2 to the newest (what differs is read from the server's version
//! when it connects), its TLS by `sslmode` (`prefer` by default).
//!
//! Lazy: until a source connects, nothing of MySQL runs (no runtime, no connection).

use database::DriverFactory;
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

/// The module's id.
pub const MODULE_ID: &str = "database.mysql";

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static(MODULE_ID),
    name: "MySQL / MariaDB",
    description: "MySQL (5.7 to the newest) and MariaDB (10.2 to the newest) servers as data sources: their databases, tables, structure and rows, changed in transactions, the console's plans and profiles, over TLS by sslmode",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "database", ..extensions::ModuleInfo::BUILT_IN },
};

/// The drivers (created only when a source connects): MySQL's, then MariaDB's.
pub fn factories() -> [DriverFactory; 2] {
    [db_mysql::factory(), db_mysql::mariadb_factory()]
}

#[derive(Debug, Default)]
pub struct DatabaseMySqlModule;

impl Module for DatabaseMySqlModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        for factory in factories() {
            ctx.contribute(factory);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
