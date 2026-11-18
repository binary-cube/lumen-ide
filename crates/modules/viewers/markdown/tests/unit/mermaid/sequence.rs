use super::super::tests::{scene_of, texts};

#[test]
fn participants_messages_notes_and_blocks() {
    let scene = scene_of(
        "sequenceDiagram\n  autonumber\n  actor U as User\n  participant S as Server\n\
         U->>+S: Request\n  S-->>-U: Response\n  Note over U,S: A note\n\
         loop Every second\n    U->>S: ping\n  end\n  alt ok\n    S->>U: yes\n  else failed\n    S->>U: no\n  end\n  S->>S: self",
    );
    let t = texts(&scene);
    for word in [
        "User",
        "Server",
        "Request",
        "Response",
        "A note",
        "loop",
        "[Every second]",
        "[failed]",
        "self",
        "1",
    ] {
        assert!(t.contains(&word), "{word}: {t:?}");
    }
    // Each participant twice: at the top and at the bottom.
    assert_eq!(t.iter().filter(|w| **w == "Server").count(), 2);
}
