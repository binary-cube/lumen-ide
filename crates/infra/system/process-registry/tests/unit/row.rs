use super::*;

use crate::record::Record;

/// A row says what it is, its project's name, how long it ran, whether it works.
#[test]
fn rows_come_from_what_was_found() {
    let found = Found {
        record: Record {
            pid: 10,
            started: 1_000,
            kind: "agent".into(),
            program: "claude".into(),
            project: Some("/work/shop".into()),
        },
        tree: vec![(10, 1_000)],
        cpu: 35.0,
    };
    let shown = row(&found, 1_000 + 7_200);
    assert_eq!(shown.kind, Kind::Agent);
    assert_eq!(shown.program, "claude");
    assert_eq!(shown.project.as_deref(), Some("shop"));
    assert_eq!(shown.running_for, Duration::from_secs(7_200));
    assert!(shown.working);

    let idle = Found { cpu: 0.3, ..found.clone() };
    assert!(!row(&idle, 0).working);
    assert_eq!(row(&idle, 0).running_for, Duration::ZERO, "a clock behind: never negative");
    let nowhere = Found { record: Record { project: None, ..found.record.clone() }, ..found };
    assert_eq!(row(&nowhere, 2_000).project, None);
}
