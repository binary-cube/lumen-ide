use super::super::{Diagram, parse as parse_any};

#[test]
fn the_documentation_example() {
    let p = parse_any("journey\n    title My working day\n    section Go to work\n      Make tea: 5: Me\n      Go upstairs: 3: Me\n      Do work: 1: Me, Cat\n    section Go home\n      Go downstairs: 5: Me\n      Sit down: 5: Me").expect("parsed");
    assert_eq!(p.meta.title.as_deref(), Some("My working day"));
    let Diagram::Journey(j) = p.diagram else { panic!() };
    assert_eq!(j.sections, vec!["Go to work", "Go home"]);
    assert_eq!(j.tasks.len(), 5);
    assert_eq!(j.tasks[2].actors, vec!["Me", "Cat"]);
    assert_eq!(j.actors, vec!["Me", "Cat"]);
    assert_eq!(j.tasks[4].section, Some(1));
}
