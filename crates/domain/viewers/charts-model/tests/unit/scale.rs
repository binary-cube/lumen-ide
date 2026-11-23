use super::*;

/// Steps are 1, 2 or 5 × 10ⁿ; ticks cover the range at whole steps.
#[test]
fn ticks_fall_on_nice_steps() {
    assert_eq!(nice_step(100.0, 5), 20.0);
    assert_eq!(nice_step(7.0, 5), 1.0);
    assert_eq!(nice_step(0.9, 5), 0.2);
    let t = number_ticks(3.0, 97.0, true);
    assert_eq!(t.values, [0.0, 20.0, 40.0, 60.0, 80.0, 100.0]);
    assert_eq!(t.labels, ["0", "20", "40", "60", "80", "100"]);
    assert_eq!((t.scale.lo, t.scale.hi), (0.0, 100.0));
}

/// A label has the decimals its step needs, and thousands as k, millions as M.
#[test]
fn labels_are_as_short_as_their_step_lets_them() {
    assert_eq!(number_label(0.3, 0.1), "0.3");
    assert_eq!(number_label(0.25, 0.05), "0.25");
    assert_eq!(number_label(2.5, 2.5), "2.5");
    assert_eq!(number_label(20_000.0, 5_000.0), "20k");
    assert_eq!(number_label(1_500_000.0, 500_000.0), "1.5M");
    assert_eq!(number_label(-40.0, 20.0), "-40");
}

/// One value alone gets room around it; a log scale's ticks are powers of ten.
#[test]
fn a_single_value_and_a_log_scale() {
    let t = number_ticks(5.0, 5.0, false);
    assert!(t.scale.lo < 5.0 && t.scale.hi > 5.0);
    let t = log_ticks(3.0, 4_000.0);
    assert_eq!(t.values, [1.0, 10.0, 100.0, 1_000.0, 10_000.0]);
    assert!((t.scale.unit(100.0) - 0.5).abs() < 1e-9);
}

/// Time ticks at round moments, said as their step needs: hours, days, months.
#[test]
fn time_ticks_fall_on_round_moments() {
    // 2026-11-23 00:00 UTC.
    let day = days_from_civil(2026, 11, 23) as f64 * DAY;
    let t = time_ticks(day, day + 6.0 * 3600.0);
    assert_eq!(t.labels.first().map(String::as_str), Some("2026-11-23"));
    assert!(t.labels.iter().any(|l| l == "03:00"));
    let t = time_ticks(day, day + 400.0 * DAY);
    assert!(t.labels.iter().all(|l| l.len() == 7), "months: {:?}", t.labels);
    assert_eq!(civil(days_from_civil(2024, 2, 29)), (2024, 2, 29));
    assert_eq!(moment_label(day + 3723.0), "2026-11-23 01:02:03");
}

/// Ticks within a range given: never past it, at 1, 2, 2.5 or 5 × 10ⁿ.
#[test]
fn ticks_within_keep_the_range() {
    assert_eq!(ticks_within(0.0, 10.0, 4), [0.0, 2.5, 5.0, 7.5, 10.0]);
    assert_eq!(ticks_within(3.0, 97.0, 6), [20.0, 40.0, 60.0, 80.0]);
    assert!(ticks_within(0.0, 1e300, 6).len() <= 64);
}

/// Values at the float's ends (a span past its range) still give a few finite ticks, never
/// a panic or an endless list.
#[test]
fn ticks_hold_at_the_float_s_ends() {
    for (lo, hi) in [(-1.7e308, 1.7e308), (f64::MIN, f64::MAX), (f64::MAX, f64::MAX)] {
        let ticks = number_ticks(lo, hi, false);
        assert!(!ticks.values.is_empty() && ticks.values.len() <= 65, "{lo} {hi}");
        assert!(ticks.values.iter().all(|v| v.is_finite()), "{lo} {hi}: {:?}", ticks.values);
        assert!(ticks.scale.lo.is_finite() && ticks.scale.hi.is_finite());
    }
}
