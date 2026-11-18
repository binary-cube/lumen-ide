use super::*;

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn a_straight_history_stays_in_one_lane() {
    let mut g = GraphBuilder::default();
    let a = g.push("c3", &ids(&["c2"]));
    let b = g.push("c2", &ids(&["c1"]));
    let c = g.push("c1", &[]);
    assert_eq!((a.column, b.column, c.column), (0, 0, 0));
    assert_eq!(a.from_above, Vec::<usize>::new());
    assert_eq!(b.from_above, [0]);
    assert_eq!(b.to_below, [0]);
    assert!(c.to_below.is_empty());
    assert_eq!(c.width, 1);
}

#[test]
fn a_merge_opens_a_lane_and_the_branch_point_closes_it() {
    // m merges f into main: m → (a, f); f → a; a is the branch point.
    let mut g = GraphBuilder::default();
    let m = g.push("m", &ids(&["a", "f"]));
    assert_eq!((m.column, m.to_below), (0, vec![0, 1]));
    let f = g.push("f", &ids(&["a"]));
    assert_eq!(f.column, 1);
    assert_eq!(f.through, [0]);
    // Both lanes wait for `a`: they join it.
    let a = g.push("a", &[]);
    assert_eq!(a.column, 0);
    assert_eq!(a.from_above, [0, 1]);
    assert_eq!(a.width, 2);
}

#[test]
fn pages_continue_the_same_graph() {
    let mut whole = GraphBuilder::default();
    let mut paged = GraphBuilder::default();
    let commits = [("d", vec!["c", "x"]), ("x", vec!["c"]), ("c", vec!["b"]), ("b", vec![])];
    let all: Vec<_> = commits.iter().map(|(id, p)| whole.push(id, &ids(p))).collect();
    let first: Vec<_> = commits[..2].iter().map(|(id, p)| paged.push(id, &ids(p))).collect();
    let rest: Vec<_> = commits[2..].iter().map(|(id, p)| paged.push(id, &ids(p))).collect();
    assert_eq!(all, [first, rest].concat());
}
