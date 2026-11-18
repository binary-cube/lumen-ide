use extensions::{ModuleRegistry, ModuleSelection};

use super::*;

#[test]
fn the_module_contributes_the_driver_and_nothing_runs() {
    let mut registry = ModuleRegistry::default();
    registry.register(Box::new(DatabasePostgresModule));
    let (mut contributions, reports) = registry.activate(&ModuleSelection::default());
    assert_eq!(reports.len(), 1);
    let factories = contributions.take::<DriverFactory>();
    assert_eq!(factories.iter().map(|f| &*f.id).collect::<Vec<_>>(), ["postgres"]);
    assert_eq!(factories[0].default_port, Some(5432));
    // Made, not connected: its fields and dialect are there before any server.
    let driver = (factories[0].create)();
    assert!(driver.connection().field("sslmode").is_some());
    assert_eq!(driver.dialect().map(|d| d.name()), Some("postgres"));
}
