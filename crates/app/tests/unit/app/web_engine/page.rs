//! A page of the engine before it's open: what's sent meanwhile kept, bounded.

use super::*;

/// The messages sent before a page opens are kept in order, the oldest let go past the
/// count or the bytes allowed.
#[test]
fn messages_waiting_for_a_page_are_bounded() {
    let mut waiting = Waiting::default();
    for i in 0..WAITING_MESSAGES + 10 {
        waiting.push(i.to_string());
    }
    let kept = waiting.take();
    assert_eq!(kept.len(), WAITING_MESSAGES);
    assert_eq!(kept.front().map(String::as_str), Some("10"), "the oldest let go");
    assert_eq!(kept.back(), Some(&(WAITING_MESSAGES + 9).to_string()), "in order");
    let mut waiting = Waiting::default();
    let half = "x".repeat(WAITING_BYTES / 2 + 1);
    waiting.push(half.clone());
    waiting.push(half);
    assert_eq!(waiting.messages.len(), 1, "past the bytes: the oldest let go");
    assert!(waiting.bytes <= WAITING_BYTES);
}
