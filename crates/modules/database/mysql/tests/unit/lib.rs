use extensions::{ModuleRegistry, ModuleSelection};

use super::*;

#[test]
fn the_module_contributes_both_drivers_and_nothing_runs() {
    let mut registry = ModuleRegistry::default();
    registry.register(Box::new(DatabaseMySqlModule));
    let (mut contributions, reports) = registry.activate(&ModuleSelection::default());
    assert_eq!(reports.len(), 1);
    let factories = contributions.take::<DriverFactory>();
    assert_eq!(factories.iter().map(|f| &*f.id).collect::<Vec<_>>(), ["mysql", "mariadb"]);
    assert!(factories.iter().all(|f| f.default_port == Some(3306)));
    // Made, not connected: its fields and dialect are there before any server.
    for factory in &factories {
        let driver = (factory.create)();
        assert!(driver.connection().field("sslmode").is_some());
        assert_eq!(driver.dialect().map(|d| d.name()), Some("mysql"));
    }
}
