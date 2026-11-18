//! The variables a `{{` may take: the file's own, then the environments'.

use super::*;
use http_requests::env::Source;

#[test]
fn the_file_s_variables_then_the_environments_with_where_they_are() {
    let environments = Environments::read(&[
        (Source::Public, Some(r#"{"dev": {"host": "localhost"}, "prod": {"host": "e.com"}}"#)),
        (Source::Private, Some(r#"{"dev": {"token": "t"}}"#)),
    ]);
    let names = variables("@base = x\n@host = mine\nGET {{base}}\n", &environments);
    assert_eq!(
        names,
        [
            ("base".to_owned(), "@ this file".to_owned()),
            ("host".to_owned(), "@ this file".to_owned()),
            ("token".to_owned(), "dev · private".to_owned()),
        ],
        "the file's own over the environments'"
    );
}
