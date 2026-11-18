use super::super::{Diagram, parse as parse_any};
use super::*;

#[test]
fn the_documentation_example() {
    let p = parse_any("xychart-beta\n    title \"Sales Revenue\"\n    x-axis [jan, feb, mar, apr, may, jun, jul, aug, sep, oct, nov, dec]\n    y-axis \"Revenue (in $)\" 4000 --> 11000\n    bar [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]\n    line [5000, 6000, 7500, 8200, 9500, 10500, 11000, 10200, 9200, 8500, 7000, 6000]").expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("Sales Revenue"));
    let Diagram::XyChart(c) = p.diagram else { panic!() };
    assert_eq!(c.categories.len(), 12);
    assert_eq!(
        (c.y_title.as_deref(), c.y_range),
        (Some("Revenue (in $)"), Some((4000.0, 11000.0)))
    );
    assert!(matches!(&c.series[0], Series::Bar(v) if v.len() == 12));
    assert!(matches!(&c.series[1], Series::Line(_)));
    let h = parse_any("xychart-beta horizontal\n x-axis \"Days\" 1 --> 7\n bar [1, 2]")
        .expect("parsed");
    let Diagram::XyChart(h) = h.diagram else { panic!() };
    assert!(h.horizontal && h.x_range == Some((1.0, 7.0)));
}
