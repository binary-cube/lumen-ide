use super::*;
use crate::client::{Connection, Transport};
use crate::driver::VERSIONS;
use std::time::Duration;

fn served() -> Connection {
    let (editor_reads, driver_writes) = std::io::pipe().unwrap();
    let (driver_reads, editor_writes) = std::io::pipe().unwrap();
    std::thread::spawn(move || {
        let toy: Arc<dyn Driver> = Arc::new(crate::toy::Toy::default());
        serve(toy, crate::toy::description(), VERSIONS, driver_reads, driver_writes)
    });
    Connection::new(
        "Toy",
        Transport {
            input: Box::new(editor_reads),
            output: Box::new(editor_writes),
            errors: None,
            process: None,
        },
    )
    .unwrap()
}

fn call<R: DeserializeOwned>(
    c: &Connection,
    name: &str,
    params: impl Serialize,
) -> Result<R, DbError> {
    c.call(name, &params, None, Duration::from_secs(5), (1, 1))
}

#[test]
fn the_handshake_describes_the_driver_and_nothing_is_asked_before_connect() {
    let c = served();
    let tables: Result<Vec<dto::TableInfo>, DbError> =
        call(&c, method::TABLES, dto::PathParams::default());
    assert!(matches!(tables, Err(DbError::Connect(m)) if m.contains("not connected")));
    let hello: dto::Initialized =
        call(&c, method::INITIALIZE, dto::Initialize { protocol: VERSIONS, editor: "test".into() })
            .unwrap();
    assert_eq!((hello.protocol, hello.driver.id.as_str()), (VERSIONS.max, "toy"));
    let connect =
        dto::Connect { target: dto::Target::default(), password: crate::toy::PASSWORD.into() };
    let connected: dto::Connected = call(&c, method::CONNECT, &connect).unwrap();
    assert_eq!(connected.server, "Toy 1.0");
    let again: Result<dto::Connected, DbError> = call(&c, method::CONNECT, &connect);
    assert!(again.is_err(), "one session per driver program");
    let unknown: Result<(), DbError> = call(&c, "teleport", ());
    assert!(matches!(unknown, Err(DbError::Unsupported(_))));
    let bad: Result<dto::Structure, DbError> = call(&c, method::STRUCTURE, 5);
    assert!(matches!(bad, Err(DbError::Query(m)) if m.contains("failed")), "invalid params");
    let structure: dto::Structure = call(
        &c,
        method::STRUCTURE,
        dto::TableParams { table: dto::TableRef { schema: None, name: "users".into() } },
    )
    .unwrap();
    assert_eq!(structure.columns.len(), 3);
}
