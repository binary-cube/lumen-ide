use super::super::{Diagram, parse as parse_any};

#[test]
fn sections_periods_and_events() {
    let p = parse_any("timeline\n  title Social\n  section Early\n  2002 : LinkedIn\n  2004 : Facebook : Google\n       : More\n  section Later\n  2006 : Twitter").expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("Social"));
    let Diagram::Timeline(t) = p.diagram else { panic!() };
    assert_eq!(t.sections, vec!["Early", "Later"]);
    assert_eq!(t.periods.len(), 3);
    assert_eq!(t.periods[1].events, vec!["Facebook", "Google", "More"]);
    assert_eq!(t.periods[2].section, Some(1));
}
