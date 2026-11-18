use super::super::{Diagram, parse as parse_any};

#[test]
fn the_documentation_example() {
    let p = parse_any("radar-beta\n  title Grades\n  axis m[\"Math\"], s[\"Science\"], e[\"English\"]\n  axis h[\"History\"], g[\"Geography\"], a[\"Art\"]\n  curve a[\"Alice\"]{85, 90, 80, 70, 75, 90}\n  curve b[\"Bob\"]{70, 75, 85, 80, 90, 85}\n  max 100\n  min 0").expect("parsed");
    let Diagram::Radar(r) = p.diagram else { panic!() };
    assert_eq!(r.axes.len(), 6);
    assert_eq!(r.axes[0], ("m".into(), "Math".into()));
    assert_eq!(r.curves[1].0, "Bob");
    assert_eq!(r.curves[0].1[5], 90.0);
    assert_eq!(r.max, Some(100.0));
    let named = parse_any("radar-beta\n axis a, b\n curve c{b: 3, a: 1}").expect("parsed");
    let Diagram::Radar(n) = named.diagram else { panic!() };
    assert_eq!(n.curves[0].1, vec![1.0, 3.0]);
}
