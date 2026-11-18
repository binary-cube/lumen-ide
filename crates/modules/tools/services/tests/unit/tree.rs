//! The rows: each group then its nodes under their parents, in order; folded nodes' children
//! hidden; a filter keeping the matching nodes with their parents (everything open); nodes
//! whose parent is unknown at the top; a cycle never looping.

use super::*;

fn node(id: u128, parent: Option<u128>, name: &str) -> ServiceNode {
    let n = ServiceNode::new(ServiceId(id), "database", name);
    match parent {
        Some(p) => n.under(ServiceId(p)),
        None => n,
    }
}

fn labels(rows: &[Row]) -> Vec<(u8, String)> {
    rows.iter().map(|r| (r.depth, r.label.clone())).collect()
}

fn nodes() -> Vec<ServiceNode> {
    vec![
        node(1, None, "shop"),
        node(2, Some(1), "console"),
        node(3, Some(2), "Result 1"),
        node(4, Some(1), "console_1"),
        node(5, None, "analytics"),
        node(6, Some(99), "orphan"),
    ]
}

#[test]
fn groups_then_nodes_under_their_parents() {
    let nodes = nodes();
    let groups = [
        Group { provider: "db", icon: "database", title: "Database", nodes: &nodes },
        Group { provider: "none", icon: "x", title: "Empty", nodes: &[] },
    ];
    let rows = rows(&groups, &HashSet::new(), &ListFilter::new(""));
    assert_eq!(
        labels(&rows),
        [
            (0, "Database".into()),
            (1, "shop".into()),
            (2, "console".into()),
            (3, "Result 1".into()),
            (2, "console_1".into()),
            (1, "analytics".into()),
            (1, "orphan".into()),
        ],
        "a group without nodes isn't shown; an unknown parent: at the top"
    );
    assert!(rows[1].expandable && rows[1].expanded && !rows[5].expandable);
    // Folded: its children hidden; the group folded: only its row.
    let mut collapsed = HashSet::from([Key::Node("db", ServiceId(1))]);
    let folded = rows_of(&groups, &collapsed, "");
    assert_eq!(folded, ["Database", "shop", "analytics", "orphan"]);
    collapsed.insert(Key::Group("db"));
    assert_eq!(rows_of(&groups, &collapsed, ""), ["Database"]);
}

fn rows_of(groups: &[Group<'_>], collapsed: &HashSet<Key>, filter: &str) -> Vec<String> {
    rows(groups, collapsed, &ListFilter::new(filter)).into_iter().map(|r| r.label).collect()
}

#[test]
fn the_filter_keeps_matches_with_their_parents_open() {
    let nodes = nodes();
    let groups = [Group { provider: "db", icon: "database", title: "Database", nodes: &nodes }];
    let collapsed = HashSet::from([Key::Group("db"), Key::Node("db", ServiceId(1))]);
    assert_eq!(rows_of(&groups, &collapsed, "result"), ["Database", "shop", "console", "Result 1"]);
    assert!(rows_of(&groups, &collapsed, "nothing like it").is_empty());
    // The group's own name: all of it.
    assert_eq!(rows_of(&groups, &HashSet::new(), "datab").len(), 7);
}

#[test]
fn a_cycle_never_loops() {
    let nodes = vec![node(1, Some(2), "a"), node(2, Some(1), "b"), node(3, Some(3), "self")];
    let groups = [Group { provider: "db", icon: "database", title: "Database", nodes: &nodes }];
    let rows = rows_of(&groups, &HashSet::new(), "");
    // Every node at most once; a node its own parent at the top.
    assert!(rows.len() <= 4 && rows.contains(&"self".to_owned()));
    assert_eq!(
        rows_of(&groups, &HashSet::new(), "b").first().map(String::as_str),
        Some("Database")
    );
}
