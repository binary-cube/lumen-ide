use code_intelligence::servers::{ChangeEdit, FileChange, WorkspaceChange};

use super::*;

#[test]
fn a_change_becomes_a_plan_with_every_place_chosen() {
    let change = WorkspaceChange {
        files: vec![
            FileChange {
                path: "/p/a.ts".into(),
                edits: vec![ChangeEdit {
                    range: 3..8,
                    original: "total".into(),
                    text: "sum".into(),
                    line: 2,
                }],
            },
            FileChange { path: "/p/empty.ts".into(), edits: Vec::new() },
        ],
        renames: vec![("/p/a.ts".into(), "/p/b.ts".into())],
    };
    let plan = plan(&change);
    assert_eq!(plan.files.len(), 1);
    let o = &plan.files[0].occurrences[0];
    assert_eq!(
        (o.range.clone(), o.original.as_str(), o.replacement.as_str(), o.chosen, o.line),
        (3..8, "total", "sum", true, 2)
    );
    assert_eq!(plan.chosen(), (1, 1));
    assert!(plan.renames[0].chosen);
    assert_eq!(plan.edits()[0].edits[0].text, "sum");
}
