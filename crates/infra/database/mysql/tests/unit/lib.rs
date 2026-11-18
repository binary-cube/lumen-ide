use super::*;
use mysql_async::Column;

fn col(kind: ColumnType) -> Column {
    Column::new(kind).with_character_set(255)
}

#[test]
fn columns_are_named_and_values_typed_by_their_metadata() {
    let int = col(ColumnType::MYSQL_TYPE_LONG);
    assert_eq!(type_name(&int), "int");
    let unsigned = col(ColumnType::MYSQL_TYPE_LONGLONG).with_flags(ColumnFlags::UNSIGNED_FLAG);
    assert_eq!(type_name(&unsigned), "bigint unsigned");
    let blob = Column::new(ColumnType::MYSQL_TYPE_BLOB).with_character_set(63);
    assert_eq!(type_name(&blob), "blob");
    assert_eq!(type_name(&col(ColumnType::MYSQL_TYPE_BLOB)), "text");
    let status = col(ColumnType::MYSQL_TYPE_STRING).with_flags(ColumnFlags::ENUM_FLAG);
    assert_eq!(type_name(&status), "enum");
    assert_eq!(type_name(&col(ColumnType::MYSQL_TYPE_NEWDECIMAL)), "decimal");

    assert_eq!(value("int", Some(b"42")), Value::Int(42));
    assert_eq!(
        value("bigint unsigned", Some(b"18446744073709551615")),
        Value::Other("18446744073709551615".into())
    );
    assert_eq!(value("double", Some(b"1.5")), Value::Float(1.5));
    assert_eq!(value("decimal", Some(b"9.99")), Value::Other("9.99".into()), "kept as written");
    assert_eq!(value("varchar", Some(b"x")), Value::Text("x".into()));
    assert_eq!(value("json", Some(b"{}")), Value::Json("{}".into()));
    assert_eq!(value("blob", Some(&[0, 1])), Value::Bytes(vec![0, 1]));
    assert_eq!(value("bit", Some(&[1, 0])), Value::Int(256));
    assert_eq!(value("enum", Some(b"paid")), Value::Other("paid".into()));
    assert_eq!(
        value("datetime", Some(b"2024-01-01 00:00:00")),
        Value::Other("2024-01-01 00:00:00".into())
    );
    assert_eq!(value("int", None), Value::Null);
}

#[test]
fn the_driver_says_what_it_offers_before_any_server() {
    let driver = MySqlDriver;
    assert_eq!(driver.dialect().map(|d| d.name()), Some("mysql"));
    let caps = driver.capabilities();
    assert!(caps.schemas && caps.transactions && caps.cancel && !caps.file_based);
    assert!(!caps.schema_changes.reorder && caps.schema_changes.renames);
    let spec = driver.connection();
    for field in [
        "host",
        "port",
        "user",
        "password",
        "database",
        "sslmode",
        "sslrootcert",
        "connect_timeout",
    ] {
        assert!(spec.field(field).is_some(), "{field}");
    }
    assert_eq!(factory().default_port, Some(3306));
    assert_eq!((&*factory().id, &*mariadb_factory().id), ("mysql", "mariadb"));
}

#[test]
fn errors_are_said_as_the_domain_says_them() {
    use mysql_async::{Error, ServerError};
    let server = |code: u16, state: &str| {
        Error::Server(ServerError { code, state: state.into(), message: "m".into() })
    };
    assert!(matches!(error(server(1045, "28000")), DbError::Auth(_)));
    assert!(matches!(error(server(1044, "42000")), DbError::Auth(_)));
    assert_eq!(error(server(1317, "70100")), DbError::Cancelled);
    let sql = error(server(1146, "42S02"));
    assert_eq!(sql.sql_error().and_then(|e| e.sqlstate.as_deref()), Some("42S02"));
    assert_eq!(sql.sql_error().and_then(|e| e.detail.as_deref()), Some("error 1146"));
    // HY000 says nothing more than "an error".
    assert_eq!(error(server(1105, "HY000")).sql_error().and_then(|e| e.sqlstate.clone()), None);
}
