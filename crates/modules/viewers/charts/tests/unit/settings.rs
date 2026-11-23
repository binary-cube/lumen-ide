use super::*;

/// The rows fetched follow what's applied, within their bounds; counts are written with
/// their thousands apart.
#[test]
fn the_rows_fetched_follow_the_setting() {
    configure_fetch_rows(50_000);
    assert_eq!(fetch_rows(), 50_000);
    configure_fetch_rows(1);
    assert_eq!(fetch_rows(), 1_000, "at least");
    configure_fetch_rows(FETCH_ROWS.default_value());
    assert_eq!(grouped(200_000), "200\u{202f}000");
    assert_eq!(grouped(1_000), "1\u{202f}000");
    assert_eq!(grouped(500), "500");
}
