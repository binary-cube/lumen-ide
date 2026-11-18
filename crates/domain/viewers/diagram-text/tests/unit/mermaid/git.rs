use super::super::{Diagram, parse as parse_any};
use super::*;

#[test]
fn commits_branches_merges_and_picks() {
    let p = parse_any(
        "gitGraph\n  commit\n  commit id: \"Alpha\" tag: \"v1.0\"\n  branch develop\n  checkout develop\n  commit type: HIGHLIGHT\n\
         checkout main\n  merge develop\n  branch feature order: 3\n  commit\n  checkout main\n  cherry-pick id: \"Alpha\"",
    )
    .expect("parsed");
    let Diagram::Git(g) = p.diagram else { panic!() };
    assert_eq!(
        g.branches.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["main", "develop", "feature"]
    );
    assert_eq!(g.branches[2].order, Some(3));
    assert_eq!(g.commits.len(), 6);
    assert_eq!((g.commits[1].id.as_str(), g.commits[1].tag.as_deref()), ("Alpha", Some("v1.0")));
    assert_eq!((g.commits[2].branch, g.commits[2].kind), (1, Kind::Highlight));
    assert_eq!((g.commits[3].kind, g.commits[3].parents.clone()), (Kind::Merge, vec![1, 2]));
    assert_eq!(g.commits[4].parents, vec![3]);
    assert_eq!(g.commits[5].kind, Kind::CherryPick);
    assert!(parse_any("gitGraph\ncheckout nowhere").is_err());
}
