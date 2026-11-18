use std::io::Cursor;

use super::*;

#[test]
fn frames_are_read_back_one_after_another_and_the_end_is_said() {
    let mut out = Vec::new();
    write_frame(&mut out, b"{\"a\":1}").unwrap();
    write_frame(&mut out, "{\"b\":\"é\"}".as_bytes()).unwrap();
    assert!(out.starts_with(b"Content-Length: 7\r\n\r\n{\"a\":1}"));
    let mut input = Cursor::new(out);
    assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"{\"a\":1}");
    assert_eq!(read_frame(&mut input).unwrap().unwrap(), "{\"b\":\"é\"}".as_bytes());
    assert_eq!(read_frame(&mut input).unwrap(), None, "the end, between frames");
}

#[test]
fn other_headers_are_ignored_and_broken_frames_refused() {
    let framed = b"Content-Type: application/json\r\ncontent-length: 2\r\n\r\n{}";
    assert_eq!(read_frame(&mut Cursor::new(framed.to_vec())).unwrap().unwrap(), b"{}");
    for broken in [
        &b"Content-Length: x\r\n\r\n"[..],
        b"no colon\r\n\r\n",
        b"Content-Type: a\r\n\r\n{}",
        b"Content-Length: 10\r\n\r\n{}",
        b"Content-Length: 2\r\n",
    ] {
        assert!(read_frame(&mut Cursor::new(broken.to_vec())).is_err(), "{broken:?}");
    }
    let huge = format!("Content-Length: {}\r\n\r\n", MAX_FRAME + 1);
    assert!(read_frame(&mut Cursor::new(huge.into_bytes())).is_err(), "too large");
}

#[test]
fn bytes_are_base64_and_floats_json_cant_hold_are_text() {
    for (bytes, text) in [
        (&b""[..], ""),
        (b"f", "Zg=="),
        (b"fo", "Zm8="),
        (b"foo", "Zm9v"),
        (b"foobar", "Zm9vYmFy"),
        (&[0, 255, 128, 7][..], "AP+ABw=="),
    ] {
        assert_eq!(base64::encode(bytes), text);
        assert_eq!(base64::decode(text).unwrap(), bytes);
    }
    assert!(base64::decode("Z").is_err());
    assert!(base64::decode("Z!==").is_err());
    let values = vec![
        crate::dto::Value::Float(f64::INFINITY),
        crate::dto::Value::Float(-1.5),
        crate::dto::Value::Bytes(vec![1, 2, 3]),
    ];
    let json = serde_json::to_string(&values).unwrap();
    assert_eq!(json, r#"[{"float":"Infinity"},{"float":-1.5},{"bytes":"AQID"}]"#);
    let back: Vec<crate::dto::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, values);
    let nan: crate::dto::Value = serde_json::from_str(r#"{"float":"NaN"}"#).unwrap();
    assert!(matches!(nan, crate::dto::Value::Float(f) if f.is_nan()));
}

#[test]
fn errors_cross_both_ways_and_a_protocol_refusal_says_the_drivers_versions() {
    for error in [
        DbError::Connect("down".into()),
        DbError::Auth("no".into()),
        DbError::Query("bad".into()),
        DbError::Cancelled,
        DbError::Unsupported("that".into()),
        DbError::Conflict("changed".into()),
        DbError::ReadOnly("ro".into()),
        DbError::InUse("held".into()),
    ] {
        assert_eq!(ErrorObject::from_db(&error, 2).to_db("Toy", (1, 2)), error);
        assert_eq!(ErrorObject::from_db(&error, 1).to_db("Toy", (1, 2)), error);
    }
    let mut refused = ErrorObject::new(code::PROTOCOL, "no");
    refused.data = Some(serde_json::json!({"min": 3, "max": 4}));
    assert_eq!(
        refused.to_db("Toy", (1, 2)),
        DbError::Driver(DriverFailure::Protocol {
            driver: "Toy".into(),
            theirs: (3, 4),
            ours: (1, 2)
        })
    );
    assert!(matches!(
        ErrorObject::new(code::METHOD_NOT_FOUND, "x").to_db("Toy", (1, 1)),
        DbError::Unsupported(_)
    ));
    assert!(matches!(
        ErrorObject::new(code::INTERNAL, "boom").to_db("Toy", (1, 1)),
        DbError::Query(m) if m.contains("boom")
    ));
}

#[test]
fn a_refused_statement_brings_its_place_code_detail_and_hint_from_protocol_2() {
    let refused = DbError::sql(database::SqlError {
        message: "column \"nope\" does not exist".into(),
        sqlstate: Some("42703".into()),
        position: Some(database::sql::EnginePosition::Character(8)),
        detail: Some("in users".into()),
        hint: Some("perhaps name".into()),
    });
    let object = ErrorObject::from_db(&refused, 2);
    assert_eq!(object.code, code::QUERY);
    assert_eq!(
        object.data,
        Some(serde_json::json!({
            "sqlstate": "42703", "position": 8, "detail": "in users", "hint": "perhaps name"
        }))
    );
    let text = String::from_utf8(answer::<()>(Some(4), Err(&object))).unwrap();
    let read: Incoming<'_> = serde_json::from_str(&text).unwrap();
    assert_eq!(read.error.unwrap().to_db("Toy", (1, 2)), refused, "back as it was");
    // A byte offset (SQLite's) crosses too.
    let byte = DbError::sql(database::SqlError {
        position: Some(database::sql::EnginePosition::Byte(0)),
        ..database::SqlError::new("syntax error")
    });
    assert_eq!(ErrorObject::from_db(&byte, 2).to_db("Toy", (1, 2)), byte);
    // Protocol 1 has no `data` for it: the message with its detail, as a query error.
    let old = ErrorObject::from_db(&refused, 1);
    assert_eq!(old.data, None);
    assert_eq!(
        old.to_db("Toy", (1, 2)),
        DbError::Query("column \"nope\" does not exist (in users)".into())
    );
}

#[test]
fn requests_and_answers_are_json_rpc() {
    let body = request(Some(3), method::TABLES, &crate::dto::PathParams::default());
    let text = String::from_utf8(body).unwrap();
    assert_eq!(text, r#"{"jsonrpc":"2.0","id":3,"method":"tables","params":{"path":[]}}"#);
    let note =
        String::from_utf8(request(None, method::CANCEL, &crate::dto::CancelParams { id: 3 }))
            .unwrap();
    assert_eq!(note, r#"{"jsonrpc":"2.0","method":"cancel","params":{"id":3}}"#);
    let ok = String::from_utf8(answer(Some(3), Ok(&5u64))).unwrap();
    assert_eq!(ok, r#"{"jsonrpc":"2.0","id":3,"result":5}"#);
    let failed = ErrorObject::new(code::QUERY, "bad");
    let err = String::from_utf8(answer::<()>(Some(3), Err(&failed))).unwrap();
    assert_eq!(err, r#"{"jsonrpc":"2.0","id":3,"error":{"code":3,"message":"bad"}}"#);
    let read: Incoming<'_> = serde_json::from_str(&err).unwrap();
    assert_eq!((read.id, read.error), (Some(3), Some(failed)));
}
