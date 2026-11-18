use super::super::{Diagram, parse as parse_any};
use super::*;

fn diagram(source: &str) -> StateDiagram {
    match parse_any(source).expect("parsed").diagram {
        Diagram::State(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn states_transitions_and_ends() {
    let d = diagram(
        "stateDiagram-v2\n  [*] --> Still\n  Still --> [*]\n  Still --> Moving : push\n  Moving --> Crash\n  Crash --> [*]\n\
         state \"A long name\" as Long\n  Long : described",
    );
    let start = d.states.iter().filter(|s| s.kind == Kind::Start).count();
    let end = d.states.iter().filter(|s| s.kind == Kind::End).count();
    assert_eq!((start, end), (1, 1));
    assert_eq!(d.transitions.len(), 5);
    assert_eq!(d.transitions[2].label.as_deref(), Some("push"));
    let long = d.states.iter().find(|s| s.id == "Long").unwrap();
    assert_eq!(
        (long.label.as_str(), long.descriptions.as_slice()),
        ("A long name", &["described".to_owned()][..])
    );
}

#[test]
fn composites_regions_forks_choices_notes() {
    let d = diagram(
        "stateDiagram-v2\n  state Active {\n    [*] --> Idle\n    Idle --> Busy\n    --\n    [*] --> Listening\n  }\n\
         state fork_state <<fork>>\n  state join_state <<join>>\n  state if_state <<choice>>\n\
         note right of Active : a note\n  note left of Idle\n    two\n    lines\n  end note\n  [*] --> Active",
    );
    let active = d.states.iter().position(|s| s.id == "Active").unwrap();
    assert!(d.states[active].composite);
    assert_eq!(d.states[active].regions, 2);
    let listening = d.states.iter().find(|s| s.id == "Listening").unwrap();
    assert_eq!((listening.parent, listening.region), (Some(active), 1));
    // The composite's own start in each region, and the top one.
    assert_eq!(d.states.iter().filter(|s| s.kind == Kind::Start).count(), 3);
    assert!(d.states.iter().any(|s| s.kind == Kind::Fork));
    assert!(d.states.iter().any(|s| s.kind == Kind::Join));
    assert!(d.states.iter().any(|s| s.kind == Kind::Choice));
    assert_eq!(d.notes.len(), 2);
    assert_eq!(d.notes[1].text, "two\nlines");
    assert!(d.notes[1].left);
    let e = parse_any("stateDiagram-v2\nstate X {\nA --> B\n").unwrap_err();
    assert_eq!(e.line, 2);
}
