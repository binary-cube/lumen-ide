use super::super::tests::{scene_of, texts};

#[test]
fn states_composites_and_notes() {
    let scene = scene_of(
        "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Running : start\n  state Running {\n    [*] --> Fetch\n    Fetch --> Parse\n    --\n    [*] --> Log\n  }\n\
         Running --> Done\n  state check <<choice>>\n  Done --> check\n  check --> [*]\n  state fork <<fork>>\n  Idle --> fork\n\
         Idle : waits\n  note right of Idle : a note",
    );
    let t = texts(&scene);
    for word in ["Idle", "waits", "start", "Running", "Fetch", "Parse", "Log", "Done", "a note"] {
        assert!(t.contains(&word), "{word}: {t:?}");
    }
}
