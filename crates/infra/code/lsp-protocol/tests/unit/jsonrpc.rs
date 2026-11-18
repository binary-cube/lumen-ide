use std::io::Cursor;

use serde_json::{Value, json};

use super::*;

fn framed(body: &str) -> Vec<u8> {
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

#[test]
fn messages_are_written_and_read_back() {
    let messages = [
        Message::Request {
            id: Id::Number(1),
            method: "initialize".into(),
            params: json!({"a": 1}),
        },
        Message::Notification { method: "initialized".into(), params: json!({}) },
        Message::Response { id: Id::Text("x".into()), result: Ok(json!([1, 2])) },
        Message::Response {
            id: Id::Number(3),
            result: Err(ResponseError { code: codes::REQUEST_CANCELLED, message: "gone".into() }),
        },
    ];
    let mut out = Vec::new();
    for m in &messages {
        write_message(&mut out, &m.to_value()).unwrap();
    }
    let mut reader = Cursor::new(out);
    for m in &messages {
        assert_eq!(read_message(&mut reader).unwrap().as_ref(), Some(m));
    }
    assert_eq!(read_message(&mut reader).unwrap(), None);
}

#[test]
fn a_header_with_a_content_type_and_odd_case_is_read() {
    let body = r#"{"jsonrpc":"2.0","method":"x"}"#;
    let text = format!(
        "content-length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{body}",
        body.len()
    );
    let message = read_message(&mut Cursor::new(text.into_bytes())).unwrap();
    assert_eq!(message, Some(Message::Notification { method: "x".into(), params: json!(null) }));
}

#[test]
fn a_body_is_counted_in_bytes_not_characters() {
    let body = r#"{"jsonrpc":"2.0","method":"ă😀","params":"ț"}"#;
    let message = read_message(&mut Cursor::new(framed(body))).unwrap();
    assert_eq!(message, Some(Message::Notification { method: "ă😀".into(), params: json!("ț") }));
}

#[test]
fn blank_lines_between_messages_are_skipped() {
    let mut bytes = b"\r\n".to_vec();
    bytes.extend(framed(r#"{"jsonrpc":"2.0","method":"a"}"#));
    bytes.extend(b"\r\n");
    bytes.extend(framed(r#"{"jsonrpc":"2.0","method":"b"}"#));
    let mut reader = Cursor::new(bytes);
    for name in ["a", "b"] {
        match read_message(&mut reader).unwrap() {
            Some(Message::Notification { method, .. }) => assert_eq!(method, name),
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_stream_cut_mid_message_is_an_error() {
    let mut bytes = framed(r#"{"jsonrpc":"2.0","method":"a"}"#);
    bytes.truncate(bytes.len() - 3);
    assert!(read_message(&mut Cursor::new(bytes)).is_err());
    assert!(read_message(&mut Cursor::new(b"Content-Length: 4\r\n".to_vec())).is_err());
}

#[test]
fn bad_frames_are_errors() {
    assert!(read_message(&mut Cursor::new(b"Content-Length: x\r\n\r\n".to_vec())).is_err());
    assert!(read_message(&mut Cursor::new(framed("not json"))).is_err());
    assert!(read_message(&mut Cursor::new(framed(r#"{"jsonrpc":"2.0"}"#))).is_err());
    let huge = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE + 1);
    assert!(read_message(&mut Cursor::new(huge.into_bytes())).is_err());
}

#[test]
fn a_response_with_a_null_error_is_a_result() {
    let body = r#"{"jsonrpc":"2.0","id":2,"result":null,"error":null}"#;
    let message = read_message(&mut Cursor::new(framed(body))).unwrap();
    assert_eq!(message, Some(Message::Response { id: Id::Number(2), result: Ok(json!(null)) }));
}

#[test]
fn a_notification_without_parameters_says_none() {
    let bare =
        Message::Notification { method: "yaml/registerContentRequest".into(), params: Value::Null };
    assert_eq!(bare.to_value(), json!({"jsonrpc": "2.0", "method": "yaml/registerContentRequest"}));
    let given = Message::Notification { method: "initialized".into(), params: json!({}) };
    assert_eq!(given.to_value()["params"], json!({}));
}
