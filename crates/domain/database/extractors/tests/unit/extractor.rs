use super::*;

#[test]
fn lines_are_joined_without_a_last_break() {
    let mut lines = Lines::new("\r\n");
    let mut out = String::new();
    assert!(!lines.started());
    lines.push(&mut out, "a");
    lines.push(&mut out, "b");
    assert_eq!(out, "a\r\nb");
}

#[test]
fn groups_have_an_order() {
    assert!(Group::Sql < Group::Delimited && Group::Delimited < Group::Other);
    assert!(Group::Other < Group::Scripted);
}
