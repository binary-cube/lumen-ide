//! A WebSocket request's messages read from its body (its `===` blocks).

use super::*;

#[test]
fn messages_are_read_from_their_blocks() {
    let body = "{\"hello\": 1}\n===\n{\"two\": 2}\n=== wait-for-server\n{\"three\": 3}\n===\n";
    let messages = ws_messages(body);
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0], WsMessage { text: "{\"hello\": 1}".into(), wait_for_server: false });
    assert!(!messages[1].wait_for_server);
    assert!(messages[2].wait_for_server, "after `=== wait-for-server`");
    assert!(ws_messages("").is_empty());
}

#[test]
fn the_request_line_s_version_says_how_http2_is_asked() {
    assert_eq!(Http2::written(None), Http2::Off);
    assert_eq!(Http2::written(Some("HTTP/1.1")), Http2::Off);
    assert_eq!(Http2::written(Some("HTTP/2")), Http2::Negotiated);
    assert_eq!(Http2::written(Some("HTTP/2 (Prior Knowledge)")), Http2::PriorKnowledge);
}
