use super::super::{Diagram, parse as parse_any};

#[test]
fn the_documentation_example() {
    let p = parse_any("quadrantChart\n    title Reach and engagement of campaigns\n    x-axis Low Reach --> High Reach\n    y-axis Low Engagement --> High Engagement\n    quadrant-1 We should expand\n    quadrant-2 Need to promote\n    quadrant-3 Re-evaluate\n    quadrant-4 May be improved\n    Campaign A: [0.3, 0.6]\n    Campaign B: [0.45, 0.23]").expect("parsed");
    let Diagram::Quadrant(q) = p.diagram else { panic!() };
    assert_eq!(q.x, ("Low Reach".into(), "High Reach".into()));
    assert_eq!(q.quadrants[0], "We should expand");
    assert_eq!(q.points[1], ("Campaign B".into(), 0.45, 0.23));
}
