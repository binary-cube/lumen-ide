use super::*;

fn row(id: i64, parent: i64, detail: &str) -> (i64, i64, String) {
    (id, parent, detail.to_owned())
}

#[test]
fn sqlites_plan_rows_are_a_tree() {
    let rows = [
        row(2, 0, "SCAN users"),
        row(5, 0, "SEARCH orders USING INDEX orders_user (user_id=?)"),
        row(9, 0, "CORRELATED SCALAR SUBQUERY 1"),
        row(12, 9, "SCAN teams"),
        row(20, 0, "USE TEMP B-TREE FOR ORDER BY"),
    ];
    let plan = from_query_plan(&rows);
    let tops: Vec<&str> = plan.iter().map(|n| n.operation.as_str()).collect();
    assert_eq!(
        tops,
        [
            "SCAN users",
            "SEARCH orders USING INDEX orders_user (user_id=?)",
            "CORRELATED SCALAR SUBQUERY 1",
            "USE TEMP B-TREE FOR ORDER BY"
        ]
    );
    assert_eq!(plan[0].object.as_deref(), Some("users"));
    assert_eq!(plan[1].object.as_deref(), Some("orders"));
    assert_eq!(plan[1].detail.as_deref(), Some("USING INDEX orders_user (user_id=?)"));
    assert_eq!(plan[2].children.len(), 1);
    assert_eq!(plan[2].children[0].object.as_deref(), Some("teams"));
    assert_eq!(plan[3].object, None);
    assert_eq!(plan.iter().map(|n| n.walk().len()).sum::<usize>(), rows.len(), "none lost");
}

#[test]
fn an_older_sqlites_rows_and_broken_parents_lose_nothing() {
    let old = from_query_plan(&[row(1, 0, "SCAN TABLE users AS u")]);
    assert_eq!(old[0].object.as_deref(), Some("users"));
    // A parent not there, a row its own parent, two rows each other's parent.
    let rows = [row(1, 7, "orphan"), row(2, 2, "self"), row(3, 4, "a"), row(4, 3, "b")];
    let plan = from_query_plan(&rows);
    let count: usize = plan.iter().map(|n| n.walk().len()).sum();
    assert_eq!(count, rows.len());
    assert!(from_query_plan(&[]).is_empty());
}

fn node(op: &str, cost: Option<f64>, time: Option<f64>, children: Vec<PlanNode>) -> PlanNode {
    PlanNode { total_cost: cost, actual_time_ms: time, children, ..PlanNode::new(op) }
}

#[test]
fn a_plan_flattens_parents_first() {
    let plan = vec![node(
        "A",
        None,
        None,
        vec![
            node("B", None, None, vec![node("C", None, None, vec![])]),
            node("D", None, None, vec![]),
        ],
    )];
    let flat = flatten(&plan);
    let seen: Vec<(&str, usize, Option<usize>)> =
        flat.iter().map(|f| (f.node.operation.as_str(), f.depth, f.parent)).collect();
    assert_eq!(seen, [("A", 0, None), ("B", 1, Some(0)), ("C", 2, Some(1)), ("D", 1, Some(0))]);
}

#[test]
fn the_costliest_branch_follows_the_heaviest_child() {
    let plan = vec![node(
        "Join",
        Some(100.0),
        None,
        vec![
            node("Light", Some(10.0), None, vec![]),
            node("Heavy", Some(80.0), None, vec![node("Leaf", Some(70.0), None, vec![])]),
        ],
    )];
    assert_eq!(hot_path(&plan), [0, 2, 3]);
    // Time wins over cost when it ran; loops multiply it.
    let mut light = node("Light", Some(90.0), Some(1.0), vec![]);
    light.loops = Some(50.0);
    let timed = vec![node(
        "Top",
        Some(100.0),
        Some(60.0),
        vec![light, node("Heavy", Some(5.0), Some(20.0), vec![])],
    )];
    assert_eq!(hot_path(&timed), [0, 1]);
    assert_eq!(timed[0].children[0].inclusive_ms(), Some(50.0));
    // Nothing measured (SQLite's plan): no branch.
    assert!(hot_path(&from_query_plan(&[row(1, 0, "SCAN t")])).is_empty());
}

#[test]
fn the_columns_are_what_some_step_has() {
    let sqlite = from_query_plan(&[row(1, 0, "SEARCH t USING INDEX i (a=?)")]);
    assert_eq!(PlanColumns::of(&sqlite), PlanColumns::default(), "its detail is in its words");
    assert_eq!(sqlite[0].own_detail(), None);
    let mut n = node("Seq Scan", Some(1.0), Some(0.5), vec![]);
    n.detail = Some("Filter: (a > 1)".into());
    n.plan_rows = Some(3.0);
    n.actual_rows = Some(2.0);
    n.loops = Some(2.0);
    let c = PlanColumns::of(&[n.clone()]);
    assert!(c.detail && c.cost && c.rows && c.actual_rows && c.time && c.loops);
    assert_eq!(n.total_rows(), Some(4.0));
    assert_eq!(
        line(&n),
        "Seq Scan (Filter: (a > 1))  (cost=1.00 rows=3 actual time=0.500 ms actual rows=2 loops=2)"
    );
}
