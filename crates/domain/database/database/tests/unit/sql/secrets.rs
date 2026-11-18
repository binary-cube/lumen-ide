use super::*;

const PG: Flavor = Flavor::Postgres { version: None };

fn masked(text: &str, flavor: Flavor) -> String {
    mask_secrets(text, flavor).into_owned()
}

#[test]
fn passwords_are_hidden() {
    let cases = [
        ("CREATE USER bob WITH PASSWORD 's3cret'", "CREATE USER bob WITH PASSWORD '••••'"),
        ("alter role bob encrypted password 'x''y'", "alter role bob encrypted password '••••'"),
        (
            "ALTER USER bob PASSWORD $$p;w$$ VALID UNTIL '2030-01-01'",
            "ALTER USER bob PASSWORD '••••' VALID UNTIL '2030-01-01'",
        ),
        (
            "CREATE USER MAPPING FOR bob SERVER s OPTIONS (user 'bob', password 'pw')",
            "CREATE USER MAPPING FOR bob SERVER s OPTIONS (user 'bob', password '••••')",
        ),
        ("create role r password E'a\\'b' login", "create role r password '••••' login"),
    ];
    for (text, expected) in cases {
        assert_eq!(masked(text, PG), expected);
    }
    let mysql = [
        ("CREATE USER 'u'@'%' IDENTIFIED BY 'pw'", "CREATE USER 'u'@'%' IDENTIFIED BY '••••'"),
        (
            "ALTER USER u IDENTIFIED WITH mysql_native_password BY \"pw\"",
            "ALTER USER u IDENTIFIED WITH mysql_native_password BY '••••'",
        ),
        ("SET PASSWORD = 'pw'", "SET PASSWORD = '••••'"),
        ("SET PASSWORD FOR u = PASSWORD('pw')", "SET PASSWORD FOR u = PASSWORD('••••')"),
        (
            "GRANT ALL ON *.* TO u IDENTIFIED BY PASSWORD '*AB12'",
            "GRANT ALL ON *.* TO u IDENTIFIED BY PASSWORD '••••'",
        ),
    ];
    for (text, expected) in mysql {
        assert_eq!(masked(text, Flavor::MySql), expected);
    }
}

#[test]
fn every_password_of_a_script_and_nothing_else() {
    let text = "create user a password 'one'; select 'password'; create user b password 'two'";
    assert_eq!(
        masked(text, PG),
        "create user a password '••••'; select 'password'; create user b password '••••'"
    );
}

#[test]
fn text_without_secrets_is_left_as_it_is() {
    for text in [
        "select password from users where password = crypt('x', password)",
        "-- password 'x'\nselect 1",
        "select 'PASSWORD ''x'''",
        "alter user bob password null",
        "",
    ] {
        assert!(matches!(mask_secrets(text, PG), Cow::Borrowed(_)), "{text:?}");
    }
}
