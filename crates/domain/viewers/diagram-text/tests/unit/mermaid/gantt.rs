use super::super::{Diagram, parse as parse_any};
use super::*;

fn day(y: i64, m: i64, d: i64) -> f64 {
    days_from_civil(y, m, d) as f64 * DAY
}

#[test]
fn tasks_dates_durations_and_dependencies() {
    let p = parse_any(
        "gantt\n  title Plan\n  dateFormat YYYY-MM-DD\n  section A\n  First :a1, 2024-01-01, 30d\n  Second :after a1, 20d\n\
         section B\n  Third :done, 2024-01-12, 2024-01-15\n  Fourth :crit, active, 24h\n  Gate :milestone, m1, 2024-03-01, 0d\n  Last :until m1",
    )
    .expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("Plan"));
    let Diagram::Gantt(g) = p.diagram else { panic!() };
    assert_eq!(g.sections, vec!["A", "B"]);
    let t = &g.tasks;
    assert_eq!((t[0].start, t[0].end), (day(2024, 1, 1), day(2024, 1, 31)));
    assert_eq!((t[1].start, t[1].end), (day(2024, 1, 31), day(2024, 2, 20)));
    assert!(t[2].done && t[2].section == Some(1));
    assert_eq!(t[3].start, t[2].end);
    assert_eq!(t[3].end - t[3].start, DAY);
    assert!(t[3].crit && t[3].active);
    assert!(t[4].milestone && t[4].start == t[4].end);
    assert_eq!(t[5].end, day(2024, 3, 1));
}

#[test]
fn weekends_formats_and_errors() {
    let p =
        parse_any("gantt\n dateFormat DD/MM/YYYY\n excludes weekends\n Work :w, 05/01/2024, 2d")
            .expect("parsed");
    let Diagram::Gantt(g) = p.diagram else { panic!() };
    // Friday 5 January + 2 working days: Tuesday 9 January.
    assert_eq!(g.tasks[0].end, day(2024, 1, 9));
    assert_eq!(format_time(day(2024, 1, 9), "%Y-%m-%d %a %b"), "2024-01-09 Tue Jan");
    assert_eq!(civil_from_days(days_from_civil(1999, 12, 31)), (1999, 12, 31));
    assert_eq!(parse_any("gantt\n Task :2024-13-01, 3d").unwrap_err().line, 2);
}
