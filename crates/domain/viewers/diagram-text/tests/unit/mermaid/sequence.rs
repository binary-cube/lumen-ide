use super::super::{Diagram, ErrorKind, parse as parse_any};
use super::*;

fn seq(source: &str) -> Sequence {
    match parse_any(source).expect("parsed").diagram {
        Diagram::Sequence(s) => s,
        other => panic!("{other:?}"),
    }
}

#[test]
fn participants_messages_and_activations() {
    let s = seq("sequenceDiagram\n  autonumber\n  participant A as Alice\n  actor B as Bob\n\
         A->>+B: Hello\n  B-->>-A: Hi\n  A-xB: lost\n  A-)B: async\n  A->C: plain\n\
         A<<->>B: both\n  activate C\n  deactivate C");
    assert!(s.autonumber);
    assert_eq!(s.participants.len(), 3);
    assert_eq!(s.participants[0].label, "Alice");
    assert!(s.participants[1].actor);
    let messages: Vec<_> = s
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Message { from, to, tip, dotted, activate, both, text, .. } => {
                Some((*from, *to, *tip, *dotted, *activate, *both, text.as_str()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(messages[0], (0, 1, Tip::Filled, false, Some(true), false, "Hello"));
    assert_eq!(messages[1], (1, 0, Tip::Filled, true, Some(false), false, "Hi"));
    assert_eq!(messages[2].2, Tip::Cross);
    assert_eq!(messages[3].2, Tip::Open);
    assert_eq!(messages[4].2, Tip::None);
    assert!(messages[5].5);
}

#[test]
fn notes_blocks_and_boxes() {
    let s = seq("sequenceDiagram\nbox Aqua Team\nparticipant A\nparticipant B\nend\n\
         Note right of A: right\nNote over A,B: both\n\
         loop Every minute\n A->>B: ping\nend\n\
         alt ok\n A->>B: yes\nelse fail\n A->>B: no\nend\n\
         rect rgb(200, 220, 255)\n A->>B: in rect\nend");
    assert_eq!(s.boxes[0].members, vec![0, 1]);
    assert!(s.boxes[0].color.is_some());
    assert!(s.items.contains(&Item::Note { at: Placement::Over(0, 1), text: "both".into() }));
    assert!(s.items.iter().any(|i| matches!(i, Item::Part { label } if label == "fail")));
    assert!(
        s.items
            .iter()
            .any(|i| matches!(i, Item::Start { kind: BlockKind::Rect, color: Some(_), .. }))
    );
}

#[test]
fn errors() {
    let e = parse_any("sequenceDiagram\nloop x\nA->>B: y\n").unwrap_err();
    assert_eq!((e.line, e.kind), (2, ErrorKind::Unclosed("end")));
    let e = parse_any("sequenceDiagram\nA->>B: ok\nwhat is this\n").unwrap_err();
    assert_eq!(e.line, 3);
}
