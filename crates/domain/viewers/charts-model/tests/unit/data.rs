use super::*;

/// Moments as databases write them, with or without a time, a fraction, an offset.
#[test]
fn moments_are_read_as_written() {
    let day = 20_411.0 * 86_400.0; // 2025-11-19
    assert_eq!(moment("2025-11-19"), Some(day));
    assert_eq!(moment("2025-11-19 09:09:11"), Some(day + 32_951.0));
    let frac = moment("2025-11-19T09:09:11.5Z").expect("a moment");
    assert!((frac - (day + 32_951.5)).abs() < 1e-6);
    assert_eq!(moment("2025-11-19 10:09:11+01:00"), Some(day + 32_951.0));
    assert_eq!(moment("2025-11-19 08:09:11-01"), Some(day + 32_951.0));
    assert_eq!(moment("not a date"), None);
    assert_eq!(moment("2025-13-01"), None);
}
