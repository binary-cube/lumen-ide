use super::*;

#[test]
fn never_printed_and_read_only_when_exposed() {
    let secret = Secret::new("s3cret");
    assert_eq!(format!("{secret:?}"), "Secret(…)");
    assert_eq!(format!("{:?}", Secret::default()), "Secret(empty)");
    assert_eq!(secret.expose(), "s3cret");
    assert!(!secret.is_empty());
}
