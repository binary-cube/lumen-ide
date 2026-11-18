use super::*;

/// A day starts at local midnight and ends at the next one, whatever the zone; the day
/// shown for a moment is its local day.
#[test]
fn days_are_local() {
    let day = ui_components::Date::new(2026, 3, 29).unwrap();
    let start = date_start(day).unwrap();
    let end = date_start(day.add_days(1)).unwrap();
    // 23, 24 or 25 hours (a clock change that day).
    assert!((23 * 3600..=25 * 3600).contains(&(end - start)), "{}", end - start);
    assert_eq!(date(start), (2026, 3, 29));
    assert_eq!(date(end - 1), (2026, 3, 29));
    assert_eq!(date(end), (2026, 3, 30));
    let local = Local.timestamp_opt(start, 0).unwrap();
    assert_eq!(local.format("%H:%M").to_string(), "00:00");
    let today = today();
    let now = chrono::Local::now().timestamp();
    assert_eq!(date(now), (i64::from(today.year), today.month, today.day));
}
